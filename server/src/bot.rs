use std::sync::Arc;

use sqlx::sqlite::SqlitePool;
use teloxide::dispatching::{Dispatcher, UpdateFilterExt};
use teloxide::dptree;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use teloxide::utils::command::BotCommands;

use crate::configuration;
use crate::github::fetch_latest_release_tag;
use crate::tracked_repositories::repository::{
    SqliteTrackedRepositoriesRepository, TrackedRepositoriesRepository,
};
use crate::tracked_repositories::tracked_repositories_releases::repository::{
    CachedRepositoryReleasesRepository, SqliteCachedRepositoryReleasesRepository,
};
use crate::tracked_repositories::tracked_repositories_releases::CachedRepositoryRelease;
use crate::utils::html_escape;

pub struct BotState {
    pub db: SqlitePool,
    pub config: configuration::Configuration,
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "snake_case", description = "These commands are supported:")]
pub enum Command {
    #[command(description = "track a repository: <name> <url>", parse_with = "split")]
    Track { name: String, url: String },
    #[command(description = "list all tracked repositories")]
    List,
    #[command(description = "display this help message")]
    Help,
}

pub async fn run(bot: Bot, state: Arc<BotState>) {
    // Register available bot commands with Telegram at startup
    if let Err(e) = bot.set_my_commands(Command::bot_commands()).await {
        log::warn!("Failed to set Telegram bot commands: {}", e);
    }

    let handler = Update::filter_message()
        .branch(dptree::entry().filter_command::<Command>().endpoint(answer))
        .branch(dptree::endpoint(fallback));

    let mut dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .build();

    dispatcher.dispatch().await;
}

async fn answer(bot: Bot, msg: Message, cmd: Command, state: Arc<BotState>) -> ResponseResult<()> {
    match cmd {
        Command::Track { name, url } => {
            log::info!("Tracking repository: {name} ({url})");

            if name.is_empty() {
                bot.send_message(msg.chat.id, "Please provide a name for the repository.").await?;
                return Ok(());
            }

            let repo_url = match crate::tracked_repositories::RepositoryUrl::new(url.clone()) {
                Ok(u) => u,
                Err(err_msg) => {
                    bot.send_message(msg.chat.id, err_msg).await?;
                    return Ok(());
                }
            };

            let repository = SqliteTrackedRepositoriesRepository::new(state.db.clone());

            match repository.find_by_repository_url(&repo_url.url()).await {
                Ok(Some(mut existing)) => {
                    existing.repository_name = name.clone();
                    existing.updated_at = chrono::Utc::now();
                    if let Err(e) = repository.save(&mut existing).await {
                        bot.send_message(msg.chat.id, format!("Failed to update tracked repository: {e}")).await?;
                    } else {
                        bot.send_message(msg.chat.id, format!("Updated tracking for {name} ({url}).")).await?;
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
                    }
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Failed to query repository: {e}")).await?;
                }
            }
        }
        Command::List => {
            let repository = SqliteTrackedRepositoriesRepository::new(state.db.clone());
            match repository.find_all_by_chat_id(msg.chat.id.0).await {
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
            bot.send_message(
                msg.chat.id,
                format!(
                    "Sorry, I only work with commands. \n\n{}",
                    Command::descriptions().to_string()
                ),
            )
            .await?;
        }
    }
    Ok(())
}


