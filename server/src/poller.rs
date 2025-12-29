use std::sync::Arc;
use teloxide::prelude::*;
use tokio::time::{Duration, sleep};

use crate::configuration::Configuration;

use crate::services::polling::PollingService;
use crate::tracked_repositories::repository::SqliteTrackedRepositoriesRepository;
use crate::tracked_repositories::tracked_repositories_releases::repository::SqliteCachedRepositoryReleasesRepository;

pub struct AppState {
    pub polling_service: Arc<
        PollingService<
            SqliteTrackedRepositoriesRepository,
            SqliteCachedRepositoryReleasesRepository,
        >,
    >,
}

pub async fn spawn(state: Arc<AppState>, bot: Bot, config: Configuration) {
    tokio::spawn(async move {
        run(state, bot, config).await;
    });
}

async fn run(state: Arc<AppState>, _bot: Bot, config: Configuration) {
    log::info!("Starting release poller");

    loop {
        let _ = state.polling_service.poll_once().await;

        sleep(Duration::from_secs(config.interval_secs)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracked_repositories::repository::TrackedRepositoriesRepository;
    use crate::tracked_repositories::tracked_repositories_releases::repository::CachedRepositoryReleasesRepository;
    use crate::tracked_repositories::{RepositoryUrl, TrackedRelease};
    use chrono::Utc;
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    // Mock implementations for testing
    #[derive(Clone)]
    struct MockGitHubService {
        responses:
            std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Option<String>>>>,
    }

    impl MockGitHubService {
        fn new() -> Self {
            Self {
                responses: std::sync::Arc::new(std::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
            }
        }

        fn add_response(&self, owner_repo: &str, tag: Option<String>) {
            self.responses
                .lock()
                .unwrap()
                .insert(owner_repo.to_string(), tag);
        }
    }

    #[async_trait::async_trait]
    impl crate::ports::GitHubService for MockGitHubService {
        async fn fetch_latest_release_tag(
            &self,
            owner: &str,
            repo: &str,
        ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
            let key = format!("{}/{}", owner, repo);
            Ok(self.responses.lock().unwrap().get(&key).cloned().flatten())
        }
    }

    struct MockMessenger {
        messages: std::sync::Mutex<Vec<(i64, String)>>,
    }

    impl MockMessenger {
        fn new() -> Self {
            Self {
                messages: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn get_messages(&self) -> Vec<(i64, String)> {
            self.messages.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl crate::ports::Messenger for MockMessenger {
        async fn send_message(
            &self,
            chat_id: i64,
            text: &str,
            _parse_mode: Option<teloxide::types::ParseMode>,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.messages
                .lock()
                .unwrap()
                .push((chat_id, text.to_string()));
            Ok(())
        }
    }

    async fn insert_tracked(
        pool: &sqlx::SqlitePool,
        name: &str,
        url: &str,
        chat_id: i64,
    ) -> TrackedRelease {
        let repo = SqliteTrackedRepositoriesRepository::new(pool.clone());
        let mut tr = TrackedRelease {
            id: Uuid::new_v4(),
            repository_name: name.to_string(),
            repository_url: RepositoryUrl::new(url.to_string()).unwrap(),
            chat_id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        repo.save(&mut tr).await.unwrap();
        tr
    }

    #[tokio::test]
    async fn poller_behaviour_caches_and_notifies_as_expected() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("failed to create in-memory sqlite pool");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("failed to run migrations");

        // Create repositories
        let tracked_repo = Arc::new(SqliteTrackedRepositoriesRepository::new(pool.clone()));
        let cached_repo = Arc::new(SqliteCachedRepositoryReleasesRepository::new(pool.clone()));

        // Create mock adapters
        let github_service = Arc::new(MockGitHubService::new());
        let messenger = Arc::new(MockMessenger::new());

        // Create service
        let polling_service = Arc::new(PollingService::new(
            tracked_repo.clone(),
            cached_repo.clone(),
            github_service.clone(),
            messenger.clone(),
        ));

        // Track repository
        let tracked =
            insert_tracked(&pool, "owner/repo", "https://github.com/owner/repo", 123).await;

        // 1) First time seeing tag -> cache saved, no notify
        github_service.add_response("owner/repo", Some("v1.0.0".to_string()));

        polling_service.poll_once().await.unwrap();

        let cached = cached_repo
            .find_by_tracked_release_id(&tracked.id)
            .await
            .unwrap()
            .expect("cached row");
        assert_eq!(cached.tag_name, "v1.0.0");
        assert_eq!(messenger.get_messages().len(), 0);

        // 2) Same tag again -> no notify, cache unchanged
        let first_seen_at_before = cached.first_seen_at;
        polling_service.poll_once().await.unwrap();
        let cached_again = cached_repo
            .find_by_tracked_release_id(&tracked.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cached_again.tag_name, "v1.0.0");
        assert_eq!(cached_again.first_seen_at, first_seen_at_before);
        assert_eq!(messenger.get_messages().len(), 0);

        // 3) New tag -> notify once and cache updates
        github_service.add_response("owner/repo", Some("v1.1.0".to_string()));
        polling_service.poll_once().await.unwrap();

        let cached_new = cached_repo
            .find_by_tracked_release_id(&tracked.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cached_new.tag_name, "v1.1.0");
        assert!(cached_new.first_seen_at > first_seen_at_before);

        // Check that notification was sent
        let messages = messenger.get_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, 123); // chat_id
        assert!(messages[0].1.contains("New release"));
        assert!(messages[0].1.contains("v1.1.0"));
    }
}
