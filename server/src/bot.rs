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
use crate::tracked_repositories::tracked_repositories_releases::CachedRepositoryRelease;
use crate::tracked_repositories::tracked_repositories_releases::repository::{
    CachedRepositoryReleasesRepository, SqliteCachedRepositoryReleasesRepository,
};
use crate::utils::html_escape;

pub struct BotState {
    pub db: SqlitePool,
    pub config: configuration::Configuration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleTrackResult {
    AlreadyTracking { message: String },
    Updated { id: uuid::Uuid, message: String },
    Created { id: uuid::Uuid, message: String },
}

pub(crate) async fn handle_track(
    db: &SqlitePool,
    chat_id: i64,
    name: &str,
    url: &str,
) -> Result<HandleTrackResult, String> {
    if name.is_empty() {
        return Err("Please provide a name for the repository.".to_string());
    }

    let repo_url = match crate::tracked_repositories::RepositoryUrl::new(url.to_string()) {
        Ok(u) => u,
        Err(err_msg) => return Err(err_msg),
    };

    let repository = SqliteTrackedRepositoriesRepository::new(db.clone());

    match repository
        .find_by_repository_url(&repo_url.url())
        .await
        .map_err(|e| format!("Failed to query repository: {e}"))?
    {
        Some(mut existing) => {
            if existing.chat_id == chat_id {
                return Ok(HandleTrackResult::AlreadyTracking {
                    message: format!("This chat is already tracking {name} ({url})."),
                });
            }

            existing.repository_name = name.to_string();
            existing.updated_at = chrono::Utc::now();
            // Persist name/update but do not change chat_id here to mirror runtime flow
            TrackedRepositoriesRepository::save(&repository, &mut existing)
                .await
                .map_err(|e| format!("Failed to update tracked repository: {e}"))?;

            Ok(HandleTrackResult::Updated {
                id: existing.id,
                message: format!("Updated tracking for {name} ({url})."),
            })
        }
        None => {
            let now = chrono::Utc::now();
            let mut tracked = crate::tracked_repositories::TrackedRelease {
                id: uuid::Uuid::now_v7(),
                repository_name: name.to_string(),
                repository_url: repo_url,
                chat_id,
                created_at: now,
                updated_at: now,
            };

            TrackedRepositoriesRepository::save(&repository, &mut tracked)
                .await
                .map_err(|e| format!("Failed to track repository: {e}"))?;

            Ok(HandleTrackResult::Created {
                id: tracked.id,
                message: format!("Now tracking {name} ({url})."),
            })
        }
    }
}

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "snake_case",
    description = "These commands are supported:"
)]
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
                bot.send_message(msg.chat.id, "Please provide a name for the repository.")
                    .await?;
                return Ok(());
            }

            match crate::tracked_repositories::RepositoryUrl::new(url.clone()) {
                Ok(u) => u,
                Err(err_msg) => {
                    bot.send_message(msg.chat.id, err_msg).await?;
                    return Ok(());
                }
            };

            match handle_track(&state.db, msg.chat.id.0, &name, &url).await {
                Ok(HandleTrackResult::AlreadyTracking { message }) => {
                    bot.send_message(msg.chat.id, message).await?;
                }
                Ok(HandleTrackResult::Updated { id, message }) => {
                    bot.send_message(msg.chat.id, message).await?;
                    if let Some((owner, repo)) =
                        crate::tracked_repositories::RepositoryUrl::new(url.clone())
                            .ok()
                            .and_then(|u| u.owner_and_repo())
                    {
                        let client = reqwest::Client::new();
                        let token_opt = state.config.github_token.clone();
                        if let Ok(Some(tag)) =
                            fetch_latest_release_tag(&client, &owner, &repo, token_opt.as_deref())
                                .await
                        {
                            let cache_repo =
                                SqliteCachedRepositoryReleasesRepository::new(state.db.clone());
                            let cached = CachedRepositoryRelease {
                                tracked_repository_id: id,
                                tag_name: tag,
                                first_seen_at: chrono::Utc::now(),
                            };
                            let _ = cache_repo.save(&cached).await;
                        }
                    }
                    // After messaging and caching, move the tracking to this chat
                    let repository = SqliteTrackedRepositoriesRepository::new(state.db.clone());
                    if let Ok(Some(mut existing)) = repository.find_by_repository_url(&url).await {
                        existing.chat_id = msg.chat.id.0;
                        let _ = repository.save(&mut existing).await;
                    }
                }
                Ok(HandleTrackResult::Created { id, message }) => {
                    bot.send_message(msg.chat.id, message).await?;
                    if let Some((owner, repo)) =
                        crate::tracked_repositories::RepositoryUrl::new(url.clone())
                            .ok()
                            .and_then(|u| u.owner_and_repo())
                    {
                        let client = reqwest::Client::new();
                        let token_opt = state.config.github_token.clone();
                        if let Ok(Some(tag)) =
                            fetch_latest_release_tag(&client, &owner, &repo, token_opt.as_deref())
                                .await
                        {
                            let cache_repo =
                                SqliteCachedRepositoryReleasesRepository::new(state.db.clone());
                            let cached = CachedRepositoryRelease {
                                tracked_repository_id: id,
                                tag_name: tag,
                                first_seen_at: chrono::Utc::now(),
                            };
                            let _ = cache_repo.save(&cached).await;
                        }
                    }
                }
                Err(err_msg) => {
                    bot.send_message(msg.chat.id, err_msg).await?;
                }
            }
        }
        Command::List => {
            let repository = SqliteTrackedRepositoriesRepository::new(state.db.clone());
            match repository.find_all_by_chat_id(msg.chat.id.0).await {
                Ok(repos) => {
                    if repos.is_empty() {
                        bot.send_message(msg.chat.id, "No repositories tracked yet.")
                            .await?;
                    } else {
                        let mut lines: Vec<String> = Vec::with_capacity(repos.len());
                        let cache_repo =
                            SqliteCachedRepositoryReleasesRepository::new(state.db.clone());

                        for r in repos {
                            let latest_str =
                                match cache_repo.find_by_tracked_release_id(&r.id).await {
                                    Ok(Some(cached)) => format!("latest: {}", cached.tag_name),
                                    _ => "latest: unknown".to_string(),
                                };
                            let url_string = r.repository_url.to_string();
                            let url_escaped = html_escape(&url_string);
                            let name_escaped = html_escape(&r.repository_name);
                            lines.push(format!(
                                "- <a href=\"{}\">{}</a> - {}",
                                url_escaped, name_escaped, latest_str
                            ));
                        }
                        let text = format!("Tracked repositories:\n{}", lines.join("\n"));
                        bot.send_message(msg.chat.id, text)
                            .parse_mode(ParseMode::Html)
                            .await?;
                    }
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Failed to list repositories: {e}"))
                        .await?;
                }
            }
        }
        Command::Help => {
            bot.send_message(msg.chat.id, Command::descriptions().to_string())
                .await?;
        }
    };

    Ok(())
}

async fn fallback(bot: Bot, msg: Message) -> ResponseResult<()> {
    if let Some(text) = msg.text() {
        if text.starts_with('/') {
            bot.send_message(msg.chat.id, Command::descriptions().to_string())
                .await?;
        } else {
            bot.send_message(
                msg.chat.id,
                format!(
                    "Sorry, I only work with commands. \n\n{}",
                    Command::descriptions()
                ),
            )
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("failed to create in-memory sqlite pool");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("failed to run migrations");

        pool
    }

    #[tokio::test]
    async fn handle_track_creates_new_when_not_exists() {
        let db = setup_db().await;
        let res = handle_track(&db, 100, "repo-one", "https://github.com/owner/repo-one")
            .await
            .expect("should succeed");

        match res {
            HandleTrackResult::Created { id: _, message } => {
                assert!(message.contains("Now tracking"));
            }
            _ => panic!("expected Created"),
        }
    }

    #[tokio::test]
    async fn handle_track_reports_already_tracking_in_same_chat() {
        let db = setup_db().await;

        // First, create
        let _ = handle_track(&db, 42, "repo-two", "https://github.com/owner/repo-two")
            .await
            .expect("create should succeed");

        // Second, same chat and same url -> already tracking
        let res = handle_track(&db, 42, "repo-two", "https://github.com/owner/repo-two")
            .await
            .expect("should succeed");

        match res {
            HandleTrackResult::AlreadyTracking { message } => {
                assert!(message.contains("already tracking"));
            }
            _ => panic!("expected AlreadyTracking"),
        }
    }

    #[tokio::test]
    async fn handle_track_updates_when_tracked_in_other_chat() {
        let db = setup_db().await;

        // Create tracked in chat 1
        let _ = handle_track(&db, 1, "repo-three", "https://github.com/owner/repo-three")
            .await
            .expect("create should succeed");

        // Track same url in different chat -> should Update (then outer flow can move chat)
        let res = handle_track(&db, 2, "repo-three", "https://github.com/owner/repo-three")
            .await
            .expect("should succeed");

        match res {
            HandleTrackResult::Updated { id: _, message } => {
                assert!(message.contains("Updated tracking"));
            }
            _ => panic!("expected Updated"),
        }
    }
}
