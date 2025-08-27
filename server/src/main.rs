use teloxide::{prelude::*, utils::command::BotCommands};
use teloxide::dispatching::{Dispatcher, UpdateFilterExt};
use teloxide::dptree;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use crate::tracked_repositories::repository::{
    TrackedRepositoriesRepository,
    SqliteTrackedRepositoriesRepository,
};
use crate::tracked_repositories::tracked_repositories_releases::repository::{
    CachedRepositoryReleasesRepository,
    SqliteCachedRepositoryReleasesRepository,
};
use crate::tracked_repositories::tracked_repositories_releases::CachedRepositoryRelease;
use teloxide::types::ParseMode;

mod db;
mod configuration;
mod logger;
mod tracked_repositories;
mod utils;
mod github;
mod poller;

struct State {
    db: SqlitePool,
    config: configuration::Configuration,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting github release bot");

    if cfg!(debug_assertions) {
        println!("Debug mode - loading .env file.");
        dotenvy::dotenv().expect("Failed to load .env file.");
    }
    logger::init_from_environment();

    log::info!("Starting github release bot...");

    log::debug!("Loading configuration");
    let config = configuration::Configuration::from_env();

    log::debug!("Initializing database");
    let pool = db::initialize_db(config.clone()).await?;

    let bot = Bot::new(config.teloxide_token.clone());

    let state = Arc::new(State { db: pool, config: config.clone() });

    let handler = Update::filter_message()
        .branch(
            dptree::entry()
                .filter_command::<Command>()
                .endpoint(answer)
        )
        .branch(dptree::endpoint(fallback));

    let mut dispatcher = Dispatcher::builder(bot.clone(), handler)
        .dependencies(dptree::deps![state.clone()])
        .build();

    let polling_state = Arc::new(poller::AppState { db: state.db.clone() });
    let polling_bot = bot.clone();
    poller::spawn(polling_state, polling_bot, config.clone()).await;

    dispatcher.dispatch().await;

    Ok(())
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "snake_case", description = "These commands are supported:")]
enum Command {
    #[command(description = "track a repository: <name> <url>", parse_with = "split")]
    TrackRepo { name: String, url: String },
    #[command(description = "list all tracked repositories")]
    ListRepos,
    #[command(description = "display this help message")]
    Help,
}

async fn answer(bot: Bot, msg: Message, cmd: Command, state: Arc<State>) -> ResponseResult<()> {
    match cmd {
        Command::TrackRepo { name, url } => {
            log::info!("Tracking repository: {name} ({url})");

            // validate the name
            if name.is_empty() {
                bot.send_message(msg.chat.id, "Please provide a name for the repository.").await?;
                return Ok(());
            }

            // Validate URL format first
            let repo_url = match crate::tracked_repositories::RepositoryUrl::new(url.clone()) {
                Ok(u) => u,
                Err(err_msg) => {
                    bot.send_message(msg.chat.id, err_msg).await?;
                    return Ok(());
                }
            };

            let repository = SqliteTrackedRepositoriesRepository::new(state.db.clone());

            // Try to find existing by URL to avoid UNIQUE constraint violations
            match repository.find_by_repository_url(&repo_url.url()).await {
                Ok(Some(mut existing)) => {
                    existing.repository_name = name.clone();
                    existing.updated_at = chrono::Utc::now();
                    if let Err(e) = repository.save(&mut existing).await {
                        bot.send_message(msg.chat.id, format!("Failed to update tracked repository: {e}")).await?;
                    } else {
                        bot.send_message(msg.chat.id, format!("Updated tracking for {name} ({url}).")).await?;
                        // Optionally refresh cache for existing repos as well
                        if let Some((owner, repo)) = existing.repository_url.owner_and_repo() {
                            let client = reqwest::Client::new();
                            let token_opt = state.config.github_token.clone();
                            if let Ok(Some(tag)) = fetch_latest_release_tag(&client, &owner, &repo, token_opt.as_deref()).await {
                                let cache_repo = SqliteCachedRepositoryReleasesRepository::new(state.db.clone());
                                let cached = CachedRepositoryRelease {
                                    tracked_repository_id: existing.id,
                                    tag_name: tag,
                                    first_seen_at: chrono::Utc::now(),
                                };
                                let _ = cache_repo.save(&cached).await;
                            }
                        }
                        // chat is implicitly subscribed via chat_id column; update it on change
                        existing.chat_id = msg.chat.id.0;
                        let _ = repository.save(&mut existing).await;
                    }
                }
                Ok(None) => {
                    let now = chrono::Utc::now();
                    let mut tracked = crate::tracked_repositories::TrackedRelease {
                        id: uuid::Uuid::now_v7(),
                        repository_name: name.clone(),
                        repository_url: repo_url,
                        chat_id: msg.chat.id.0,
                        created_at: now,
                        updated_at: now,
                    };
                    if let Err(e) = repository.save(&mut tracked).await {
                        bot.send_message(msg.chat.id, format!("Failed to track repository: {e}")).await?;
                    } else {
                        bot.send_message(msg.chat.id, format!("Now tracking {name} ({url}).")).await?;
                        // Fetch and cache latest release for the new tracked repository
                        if let Some((owner, repo)) = tracked.repository_url.owner_and_repo() {
                            let client = reqwest::Client::new();
                            let token_opt = state.config.github_token.clone();
                            if let Ok(Some(tag)) = fetch_latest_release_tag(&client, &owner, &repo, token_opt.as_deref()).await {
                                let cache_repo = SqliteCachedRepositoryReleasesRepository::new(state.db.clone());
                                let cached = CachedRepositoryRelease {
                                    tracked_repository_id: tracked.id,
                                    tag_name: tag,
                                    first_seen_at: chrono::Utc::now(),
                                };
                                let _ = cache_repo.save(&cached).await;
                            }
                        }
                        // chat is implicitly subscribed via chat_id field
                    }
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Failed to query repository: {e}")).await?;
                }
            }
        }
        Command::ListRepos => {
            let repository = SqliteTrackedRepositoriesRepository::new(state.db.clone());
            match repository.find_all().await {
                Ok(repos) => {
                    if repos.is_empty() {
                        bot.send_message(msg.chat.id, "No repositories tracked yet.").await?;
                    } else {
                        let mut lines: Vec<String> = Vec::with_capacity(repos.len());
                        let cache_repo = SqliteCachedRepositoryReleasesRepository::new(state.db.clone());

                        for r in repos {
                            let latest_str = match cache_repo.find_by_tracked_release_id(&r.id).await {
                                Ok(Some(cached)) => format!("latest: {}", cached.tag_name),
                                _ => "latest: unknown".to_string(),
                            };
                            let url_string = r.repository_url.to_string();
                            let url_escaped = html_escape(&url_string);
                            let name_escaped = html_escape(&r.repository_name);
                            lines.push(format!("- <a href=\"{}\">{}</a> - {}", url_escaped, name_escaped, latest_str));
                        }
                        let text = format!("Tracked repositories:\n{}", lines.join("\n"));
                        bot.send_message(msg.chat.id, text).parse_mode(ParseMode::Html).await?;
                    }
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Failed to list repositories: {e}")).await?;
                }
            }
        }
        Command::Help => {
            bot.send_message(msg.chat.id, Command::descriptions().to_string()).await?;
        }
    };

    Ok(())
}

async fn fallback(bot: Bot, msg: Message) -> ResponseResult<()> {
    if let Some(text) = msg.text() {
        if text.starts_with('/') {
            bot.send_message(msg.chat.id, Command::descriptions().to_string()).await?;
        } else {
            bot.send_message(msg.chat.id, format!("Sorry, I only work with commands. \n\n{}", Command::descriptions().to_string())).await?;
        }
    }
    Ok(())
}

use crate::utils::html_escape;
use crate::github::fetch_latest_release_tag;