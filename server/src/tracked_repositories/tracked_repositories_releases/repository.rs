use std::error::Error;
use async_trait::async_trait;
use sqlx::{self, sqlite::SqlitePool};
use crate::tracked_repositories::tracked_repositories_releases::CachedRepositoryRelease;

#[async_trait]
pub trait CachedRepositoryReleasesRepository: Send + Sync {
    async fn save(&self, cached: &CachedRepositoryRelease) -> Result<(), Box<dyn Error + Send + Sync>>;
    async fn find_by_tracked_release_id(&self, id: &uuid::Uuid) -> Result<Option<CachedRepositoryRelease>, Box<dyn Error + Send + Sync>>;
}

pub struct SqliteCachedRepositoryReleasesRepository {
    pool: SqlitePool,
}

impl SqliteCachedRepositoryReleasesRepository {
    pub fn new(pool: SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl CachedRepositoryReleasesRepository for SqliteCachedRepositoryReleasesRepository {
    async fn save(&self, cached: &CachedRepositoryRelease) -> Result<(), Box<dyn Error + Send + Sync>> {
        sqlx::query(
            r#"
            INSERT INTO tracked_repository_releases (tracked_repository_id, tag_name, first_seen_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(tracked_repository_id) DO UPDATE SET
                tag_name = excluded.tag_name,
                first_seen_at = CASE
                    WHEN excluded.tag_name != tag_name THEN excluded.first_seen_at
                    ELSE first_seen_at
                END
            "#,
        )
        .bind(cached.tracked_repository_id.to_string())
        .bind(&cached.tag_name)
        .bind(cached.first_seen_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn find_by_tracked_release_id(&self, id: &uuid::Uuid) -> Result<Option<CachedRepositoryRelease>, Box<dyn Error + Send + Sync>> {
        let rec = sqlx::query_as::<_, CachedRepositoryRelease>(
            r#"
            SELECT tracked_repository_id, tag_name, first_seen_at
            FROM tracked_repository_releases
            WHERE tracked_repository_id = ?1
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        Ok(rec)
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracked_repositories::{TrackedRelease, RepositoryUrl};
    use crate::tracked_repositories::repository::{SqliteTrackedRepositoriesRepository, TrackedRepositoriesRepository};
    use chrono::{Utc, Duration};
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("failed to connect to sqlite in-memory");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations should run");

        pool
    }

    async fn insert_tracked_repository(pool: &SqlitePool) -> TrackedRelease {
        let repo_repo = SqliteTrackedRepositoriesRepository::new(pool.clone());
        let now = Utc::now();
        let mut tracked = TrackedRelease {
            id: Uuid::now_v7(),
            repository_name: "owner/repo".to_string(),
            repository_url: RepositoryUrl::new("https://github.com/owner/repo".to_string()).unwrap(),
            chat_id: 1,
            created_at: now,
            updated_at: now,
        };
        repo_repo.save(&mut tracked).await.unwrap();
        tracked
    }

    #[tokio::test]
    async fn save_and_find_roundtrip() {
        let pool = setup_pool().await;
        let tracked = insert_tracked_repository(&pool).await;
        let repo = SqliteCachedRepositoryReleasesRepository::new(pool.clone());

        let first_seen = Utc::now();
        let cached = CachedRepositoryRelease {
            tracked_repository_id: tracked.id,
            tag_name: "v1.0.0".to_string(),
            first_seen_at: first_seen,
        };

        repo.save(&cached).await.expect("save should succeed");

        let fetched = repo
            .find_by_tracked_release_id(&tracked.id)
            .await
            .expect("query should succeed")
            .expect("row should exist");

        assert_eq!(fetched.tracked_repository_id, tracked.id);
        assert_eq!(fetched.tag_name, "v1.0.0");
        assert_eq!(fetched.first_seen_at, first_seen);
    }

    #[tokio::test]
    async fn upsert_same_tag_keeps_first_seen_at() {
        let pool = setup_pool().await;
        let tracked = insert_tracked_repository(&pool).await;
        let repo = SqliteCachedRepositoryReleasesRepository::new(pool.clone());

        let t1 = Utc::now();
        let initial = CachedRepositoryRelease {
            tracked_repository_id: tracked.id,
            tag_name: "v1.0.0".to_string(),
            first_seen_at: t1,
        };
        repo.save(&initial).await.unwrap();

        // same tag, later timestamp; first_seen_at should NOT change
        let t2 = t1 + Duration::minutes(10);
        let same_tag = CachedRepositoryRelease {
            tracked_repository_id: tracked.id,
            tag_name: "v1.0.0".to_string(),
            first_seen_at: t2,
        };
        repo.save(&same_tag).await.unwrap();

        let fetched = repo
            .find_by_tracked_release_id(&tracked.id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fetched.tag_name, "v1.0.0");
        assert_eq!(fetched.first_seen_at, t1);
    }

    #[tokio::test]
    async fn upsert_new_tag_updates_first_seen_at() {
        let pool = setup_pool().await;
        let tracked = insert_tracked_repository(&pool).await;
        let repo = SqliteCachedRepositoryReleasesRepository::new(pool.clone());

        let t1 = Utc::now();
        let initial = CachedRepositoryRelease {
            tracked_repository_id: tracked.id,
            tag_name: "v1.0.0".to_string(),
            first_seen_at: t1,
        };
        repo.save(&initial).await.unwrap();

        // different tag, later timestamp; first_seen_at SHOULD change
        let t2 = t1 + Duration::minutes(5);
        let new_tag = CachedRepositoryRelease {
            tracked_repository_id: tracked.id,
            tag_name: "v1.1.0".to_string(),
            first_seen_at: t2,
        };
        repo.save(&new_tag).await.unwrap();

        let fetched = repo
            .find_by_tracked_release_id(&tracked.id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fetched.tag_name, "v1.1.0");
        assert_eq!(fetched.first_seen_at, t2);
    }
}

