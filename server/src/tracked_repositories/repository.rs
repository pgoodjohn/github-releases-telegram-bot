use std::error::Error;
use async_trait::async_trait;
use sqlx::{self, sqlite::SqlitePool};
use crate::tracked_repositories::TrackedRelease;

#[async_trait]
pub trait TrackedRepositoriesRepository: Send + Sync {
    async fn save(&self, tracked_release: &mut TrackedRelease) -> Result<(), Box<dyn Error + Send + Sync>>;
    async fn find_all(&self) -> Result<Vec<TrackedRelease>, Box<dyn Error + Send + Sync>>;
    async fn find_all_by_chat_id(&self, chat_id: i64) -> Result<Vec<TrackedRelease>, Box<dyn Error + Send + Sync>>;
    async fn find_by_id(&self, id: &str) -> Result<Option<TrackedRelease>, Box<dyn Error + Send + Sync>>;
    async fn find_by_repository_url(&self, repository_url: &str) -> Result<Option<TrackedRelease>, Box<dyn Error + Send + Sync>>;
    async fn delete(&self, id: &str) -> Result<(), Box<dyn Error + Send + Sync>>;
}

pub struct SqliteTrackedRepositoriesRepository {
    pool: SqlitePool,
}

impl SqliteTrackedRepositoriesRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TrackedRepositoriesRepository for SqliteTrackedRepositoriesRepository {
    async fn save(&self, tracked_release: &mut TrackedRelease) -> Result<(), Box<dyn Error + Send + Sync>> {
        sqlx::query(
            r#"
            INSERT INTO tracked_repositories (id, repository_name, repository_url, chat_id, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                repository_name = excluded.repository_name,
                repository_url = excluded.repository_url,
                chat_id = excluded.chat_id,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(tracked_release.id.to_string())
        .bind(&tracked_release.repository_name)
        .bind(tracked_release.repository_url.url())
        .bind(tracked_release.chat_id)
        .bind(tracked_release.created_at)
        .bind(tracked_release.updated_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn find_all(&self) -> Result<Vec<TrackedRelease>, Box<dyn Error + Send + Sync>> {
        let releases = sqlx::query_as::<_, TrackedRelease>(
            r#"
            SELECT id, repository_name, repository_url, chat_id, created_at, updated_at
            FROM tracked_repositories
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(releases)
    }

    async fn find_all_by_chat_id(&self, chat_id: i64) -> Result<Vec<TrackedRelease>, Box<dyn Error + Send + Sync>> {
        let releases = sqlx::query_as::<_, TrackedRelease>(
            r#"
            SELECT id, repository_name, repository_url, chat_id, created_at, updated_at
            FROM tracked_repositories
            WHERE chat_id = ?1
            ORDER BY created_at DESC
            "#,
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(releases)
    }

    async fn find_by_id(&self, id: &str) -> Result<Option<TrackedRelease>, Box<dyn Error + Send + Sync>> {
        let rec = sqlx::query_as::<_, TrackedRelease>(
            r#"
            SELECT id, repository_name, repository_url, chat_id, created_at, updated_at
            FROM tracked_repositories WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(rec)
    }

    async fn find_by_repository_url(&self, repository_url: &str) -> Result<Option<TrackedRelease>, Box<dyn Error + Send + Sync>> {
        let rec = sqlx::query_as::<_, TrackedRelease>(
            r#"
            SELECT id, repository_name, repository_url, chat_id, created_at, updated_at
            FROM tracked_repositories WHERE repository_url = ?1
            "#,
        )
        .bind(repository_url)
        .fetch_optional(&self.pool)
        .await?;

        Ok(rec)
    }

    async fn delete(&self, id: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        sqlx::query("DELETE FROM tracked_repositories WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

// Cached releases repository moved under tracked_repositories_releases



#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracked_repositories::{TrackedRelease, RepositoryUrl};
    use chrono::{Duration, Utc};
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    async fn setup_repo() -> SqliteTrackedRepositoriesRepository {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("failed to create in-memory sqlite pool");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("failed to run migrations");

        SqliteTrackedRepositoriesRepository::new(pool)
    }

    fn make_release(
        repository_name: &str,
        repository_url: &str,
        chat_id: i64,
        created_at: chrono::DateTime<Utc>,
        updated_at: chrono::DateTime<Utc>,
    ) -> TrackedRelease {
        TrackedRelease {
            id: Uuid::now_v7(),
            repository_name: repository_name.to_string(),
            repository_url: RepositoryUrl::new(repository_url.to_string()).expect("valid github url"),
            chat_id,
            created_at,
            updated_at,
        }
    }

    #[tokio::test]
    async fn save_and_find_by_id_roundtrip() {
        let repo = setup_repo().await;
        let now = Utc::now();
        let mut rel = make_release(
            "repo-one",
            "https://github.com/owner/repo-one",
            42,
            now,
            now,
        );

        TrackedRepositoriesRepository::save(&repo, &mut rel)
            .await
            .expect("save should succeed");

        let fetched = TrackedRepositoriesRepository::find_by_id(&repo, &rel.id.to_string())
            .await
            .expect("find_by_id should succeed")
            .expect("record should exist");

        assert_eq!(fetched.id, rel.id);
        assert_eq!(fetched.repository_name, "repo-one");
        assert_eq!(fetched.repository_url.url(), "https://github.com/owner/repo-one");
        assert_eq!(fetched.chat_id, 42);
    }

    #[tokio::test]
    async fn find_by_repository_url() {
        let repo = setup_repo().await;
        let now = Utc::now();
        let url = "https://github.com/owner/repo-two";
        let mut rel = make_release("repo-two", url, 7, now, now);

        TrackedRepositoriesRepository::save(&repo, &mut rel)
            .await
            .expect("save should succeed");

        let fetched = TrackedRepositoriesRepository::find_by_repository_url(&repo, url)
            .await
            .expect("find_by_repository_url should succeed")
            .expect("record should exist");

        assert_eq!(fetched.id, rel.id);
        assert_eq!(fetched.repository_name, "repo-two");
        assert_eq!(fetched.repository_url.url(), url);
    }

    #[tokio::test]
    async fn find_all_and_by_chat_id() {
        let repo = setup_repo().await;
        let now = Utc::now();
        let earlier = now - Duration::minutes(5);

        let mut a = make_release(
            "alpha",
            "https://github.com/owner/alpha",
            100,
            earlier,
            earlier,
        );
        let mut b = make_release(
            "beta",
            "https://github.com/owner/beta",
            200,
            now,
            now,
        );

        TrackedRepositoriesRepository::save(&repo, &mut a).await.unwrap();
        TrackedRepositoriesRepository::save(&repo, &mut b).await.unwrap();

        // find_all ordered by created_at DESC -> b then a
        let all = TrackedRepositoriesRepository::find_all(&repo).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, b.id);
        assert_eq!(all[1].id, a.id);

        // by chat id
        let only_100 = TrackedRepositoriesRepository::find_all_by_chat_id(&repo, 100)
            .await
            .unwrap();
        assert_eq!(only_100.len(), 1);
        assert_eq!(only_100[0].id, a.id);
    }

    #[tokio::test]
    async fn save_updates_on_conflict_by_id() {
        let repo = setup_repo().await;
        let now = Utc::now();
        let later = now + Duration::minutes(1);
        let mut rel = make_release(
            "gamma",
            "https://github.com/owner/gamma",
            1,
            now,
            now,
        );

        TrackedRepositoriesRepository::save(&repo, &mut rel).await.unwrap();

        // modify fields and save again
        rel.repository_name = "gamma-renamed".to_string();
        rel.chat_id = 2;
        rel.repository_url = RepositoryUrl::new("https://github.com/owner/gamma-renamed".to_string()).unwrap();
        rel.updated_at = later;

        TrackedRepositoriesRepository::save(&repo, &mut rel).await.unwrap();

        let fetched = TrackedRepositoriesRepository::find_by_id(&repo, &rel.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fetched.repository_name, "gamma-renamed");
        assert_eq!(fetched.chat_id, 2);
        assert_eq!(fetched.repository_url.url(), "https://github.com/owner/gamma-renamed");
        assert_eq!(fetched.updated_at, later);
    }

    #[tokio::test]
    async fn delete_removes_record() {
        let repo = setup_repo().await;
        let now = Utc::now();
        let mut rel = make_release(
            "delta",
            "https://github.com/owner/delta",
            99,
            now,
            now,
        );

        TrackedRepositoriesRepository::save(&repo, &mut rel).await.unwrap();

        TrackedRepositoriesRepository::delete(&repo, &rel.id.to_string())
            .await
            .expect("delete should succeed");

        let fetched = TrackedRepositoriesRepository::find_by_id(&repo, &rel.id.to_string())
            .await
            .unwrap();
        assert!(fetched.is_none());
    }
}
