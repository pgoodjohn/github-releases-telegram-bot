use std::sync::Arc;

use teloxide::dispatching::{Dispatcher, UpdateFilterExt};
use teloxide::dptree;
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

use crate::services::tracking::{HandleTrackResult, TrackingService};
use crate::tracked_repositories::repository::SqliteTrackedRepositoriesRepository;
use crate::tracked_repositories::tracked_repositories_releases::repository::SqliteCachedRepositoryReleasesRepository;

pub struct BotState {
    pub tracking_service: Arc<
        TrackingService<
            SqliteTrackedRepositoriesRepository,
            SqliteCachedRepositoryReleasesRepository,
        >,
    >,
}

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "snake_case",
    description = "These commands are supported:"
)]
pub enum Command {
    #[command(description = "track a repository: <name> <url>", parse_with = "split")]
    Track { name: String, url: String },
    #[command(description = "stop tracking a repository: <url>")]
    Untrack { url: String },
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
                Ok(_) => (),
                Err(err_msg) => {
                    bot.send_message(msg.chat.id, err_msg).await?;
                    return Ok(());
                }
            };

            match state
                .tracking_service
                .handle_track(msg.chat.id.0, &name, &url)
                .await
            {
                Ok(track_result) => {
                    let message = match &track_result {
                        HandleTrackResult::AlreadyTracking { message } => message.clone(),
                        HandleTrackResult::Updated { message, .. } => message.clone(),
                        HandleTrackResult::Created { message, .. } => message.clone(),
                    };
                    bot.send_message(msg.chat.id, message).await?;

                    // Cache latest release and update chat
                    let _ = state
                        .tracking_service
                        .cache_latest_release_for_new_track(&track_result, &url)
                        .await;
                    let _ = state
                        .tracking_service
                        .update_chat_for_track(&track_result, msg.chat.id.0, &url)
                        .await;
                }
                Err(err_msg) => {
                    bot.send_message(msg.chat.id, err_msg).await?;
                }
            }
        }
        Command::Untrack { url } => {
            log::info!("Untracking repository: {url}");

            match crate::tracked_repositories::RepositoryUrl::new(url.clone()) {
                Ok(_) => (),
                Err(err_msg) => {
                    bot.send_message(msg.chat.id, err_msg).await?;
                    return Ok(());
                }
            };

            match state
                .tracking_service
                .handle_untrack(msg.chat.id.0, &url)
                .await
            {
                Ok(message) => {
                    bot.send_message(msg.chat.id, message).await?;
                }
                Err(err_msg) => {
                    bot.send_message(msg.chat.id, err_msg).await?;
                }
            }
        }
        Command::List => match state.tracking_service.handle_list(msg.chat.id.0).await {
            Ok(text) => {
                if text == "No repositories tracked yet." {
                    bot.send_message(msg.chat.id, text).await?;
                } else {
                    bot.send_message(msg.chat.id, text)
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                }
            }
            Err(e) => {
                bot.send_message(msg.chat.id, format!("Failed to list repositories: {e}"))
                    .await?;
            }
        },
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
    use std::sync::Arc;

    async fn setup_tracking_service() -> Arc<
        TrackingService<
            SqliteTrackedRepositoriesRepository,
            SqliteCachedRepositoryReleasesRepository,
        >,
    > {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("failed to create in-memory sqlite pool");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("failed to run migrations");

        let tracked_repo = Arc::new(SqliteTrackedRepositoriesRepository::new(pool.clone()));
        let cached_repo = Arc::new(SqliteCachedRepositoryReleasesRepository::new(pool));

        // Mock adapters for tests
        let github_service = Arc::new(MockGitHubService::new());

        Arc::new(TrackingService::new(
            tracked_repo,
            cached_repo,
            github_service,
        ))
    }

    // Mock implementations for testing
    struct MockGitHubService;

    impl MockGitHubService {
        fn new() -> Self {
            Self
        }
    }

    #[async_trait::async_trait]
    impl crate::ports::GitHubService for MockGitHubService {
        async fn fetch_latest_release_tag(
            &self,
            _owner: &str,
            _repo: &str,
        ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Some("v1.0.0".to_string()))
        }
    }

    #[tokio::test]
    async fn handle_track_creates_new_when_not_exists() {
        let tracking_service = setup_tracking_service().await;
        let res = tracking_service
            .handle_track(100, "repo-one", "https://github.com/owner/repo-one")
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
        let tracking_service = setup_tracking_service().await;

        // First, create
        let _ = tracking_service
            .handle_track(42, "repo-two", "https://github.com/owner/repo-two")
            .await
            .expect("create should succeed");

        // Second, same chat and same url -> already tracking
        let res = tracking_service
            .handle_track(42, "repo-two", "https://github.com/owner/repo-two")
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
        let tracking_service = setup_tracking_service().await;

        // Create tracked in chat 1
        let _ = tracking_service
            .handle_track(1, "repo-three", "https://github.com/owner/repo-three")
            .await
            .expect("create should succeed");

        // Track same url in different chat -> should Update (then outer flow can move chat)
        let res = tracking_service
            .handle_track(2, "repo-three", "https://github.com/owner/repo-three")
            .await
            .expect("should succeed");

        match res {
            HandleTrackResult::Updated { id: _, message } => {
                assert!(message.contains("Updated tracking"));
            }
            _ => panic!("expected Updated"),
        }
    }

    #[tokio::test]
    async fn handle_untrack_removes_tracked_repository() {
        let tracking_service = setup_tracking_service().await;

        // First, track a repository
        let track_res = tracking_service
            .handle_track(100, "repo-untrack", "https://github.com/owner/repo-untrack")
            .await
            .expect("track should succeed");
        match track_res {
            HandleTrackResult::Created { .. } => (),
            _ => panic!("expected Created"),
        };

        // Now untrack it
        let untrack_msg = tracking_service
            .handle_untrack(100, "https://github.com/owner/repo-untrack")
            .await
            .expect("untrack should succeed");

        assert!(untrack_msg.contains("Stopped tracking"));
        assert!(untrack_msg.contains("repo-untrack"));
    }

    #[tokio::test]
    async fn handle_untrack_fails_if_not_tracked() {
        let tracking_service = setup_tracking_service().await;

        let err = tracking_service
            .handle_untrack(100, "https://github.com/owner/not-tracked")
            .await
            .expect_err("should fail");

        assert!(err.contains("not being tracked"));
    }

    #[tokio::test]
    async fn handle_untrack_fails_if_tracked_in_different_chat() {
        let tracking_service = setup_tracking_service().await;

        // Track in chat 1
        let _ = tracking_service
            .handle_track(1, "repo-chat", "https://github.com/owner/repo-chat")
            .await
            .expect("track should succeed");

        // Try to untrack from chat 2
        let err = tracking_service
            .handle_untrack(2, "https://github.com/owner/repo-chat")
            .await
            .expect_err("should fail");

        assert!(err.contains("not tracking"));
    }
}
