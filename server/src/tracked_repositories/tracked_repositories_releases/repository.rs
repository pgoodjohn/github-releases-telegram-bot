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


