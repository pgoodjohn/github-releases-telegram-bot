use teloxide::{prelude::*, utils::command::BotCommands};
use teloxide::dispatching::{Dispatcher, UpdateFilterExt};
use teloxide::dptree;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use crate::tracked_releases::repository::TrackedReleasesRepository;

mod db;
mod configuration;
mod logger;
mod tracked_releases;

struct State {
    db: SqlitePool,
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

    let bot = Bot::new(config.teloxide_token);

    let state = Arc::new(State { db: pool });

    let handler = Update::filter_message()
        .filter_command::<Command>()
        .endpoint(answer);

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .build()
        .dispatch()
        .await;

    Ok(())
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "These commands are supported:")]
enum Command {
    #[command(description = "display this text.")]
    Help,
    #[command(description = "handle a username.")]
    Username(String),
    #[command(description = "handle a username and an age.", parse_with = "split")]
    UsernameAndAge { username: String, age: u8 },
    #[command(description = "track a repository: <name> <url>", parse_with = "split")]
    TrackRepo { name: String, url: String },
    #[command(description = "list all tracked repositories")]
    ListRepos,
}

async fn answer(bot: Bot, msg: Message, cmd: Command, state: Arc<State>) -> ResponseResult<()> {
    match cmd {
        Command::Help => {
            bot.send_message(msg.chat.id, Command::descriptions().to_string()).await?;
        }
        Command::Username(username) => {
            bot.send_message(msg.chat.id, format!("Your username is @{username}."))
                .await?;
        }
        Command::UsernameAndAge { username, age } => {
            bot.send_message(msg.chat.id, format!("Your username is @{username} and age is {age}."))
                .await?;
        }
        Command::TrackRepo { name, url } => {
            // Validate URL format first
            let repo_url = match crate::tracked_releases::RepositoryUrl::new(url.clone()) {
                Ok(u) => u,
                Err(err_msg) => {
                    bot.send_message(msg.chat.id, err_msg).await?;
                    return Ok(());
                }
            };

            let repository = crate::tracked_releases::repository::SqliteTrackedReleasesRepository::new(state.db.clone());

            // Try to find existing by URL to avoid UNIQUE constraint violations
            match repository.find_by_repository_url(&repo_url.url()).await {
                Ok(Some(mut existing)) => {
                    existing.repository_name = name.clone();
                    existing.updated_at = chrono::Utc::now();
                    if let Err(e) = repository.save(&mut existing).await {
                        bot.send_message(msg.chat.id, format!("Failed to update tracked repository: {e}")).await?;
                    } else {
                        bot.send_message(msg.chat.id, format!("Updated tracking for {name} ({url}).")).await?;
                    }
                }
                Ok(None) => {
                    let now = chrono::Utc::now();
                    let mut tracked = crate::tracked_releases::TrackedRelease {
                        id: uuid::Uuid::now_v7(),
                        repository_name: name.clone(),
                        repository_url: repo_url,
                        created_at: now,
                        updated_at: now,
                    };
                    if let Err(e) = repository.save(&mut tracked).await {
                        bot.send_message(msg.chat.id, format!("Failed to track repository: {e}")).await?;
                    } else {
                        bot.send_message(msg.chat.id, format!("Now tracking {name} ({url}).")).await?;
                    }
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Failed to query repository: {e}")).await?;
                }
            }
        }
        Command::ListRepos => {
            let repository = crate::tracked_releases::repository::SqliteTrackedReleasesRepository::new(state.db.clone());
            match repository.find_all().await {
                Ok(repos) => {
                    if repos.is_empty() {
                        bot.send_message(msg.chat.id, "No repositories tracked yet.").await?;
                    } else {
                        let mut lines: Vec<String> = Vec::with_capacity(repos.len());
                        for r in repos {
                            lines.push(format!("- {} ({})", r.repository_name, r.repository_url));
                        }
                        let text = format!("Tracked repositories:\n{}", lines.join("\n"));
                        bot.send_message(msg.chat.id, text).await?;
                    }
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Failed to list repositories: {e}")).await?;
                }
            }
        }
    };

    Ok(())
}