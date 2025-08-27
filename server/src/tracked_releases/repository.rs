use std::error::Error;
use async_trait::async_trait;
use sqlx::{self, sqlite::SqlitePool};
use crate::tracked_releases::{TrackedRelease};

#[async_trait]
pub trait TrackedReleasesRepository: Send + Sync {
    async fn save(&self, tracked_release: &mut TrackedRelease) -> Result<(), Box<dyn Error + Send + Sync>>;
    async fn find_all(&self) -> Result<Vec<TrackedRelease>, Box<dyn Error + Send + Sync>>;
    async fn find_by_id(&self, id: &str) -> Result<Option<TrackedRelease>, Box<dyn Error + Send + Sync>>;
    async fn find_by_repository_url(&self, repository_url: &str) -> Result<Option<TrackedRelease>, Box<dyn Error + Send + Sync>>;
    async fn delete(&self, id: &str) -> Result<(), Box<dyn Error + Send + Sync>>;
}

pub struct SqliteTrackedReleasesRepository {
    pool: SqlitePool,
}

impl SqliteTrackedReleasesRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TrackedReleasesRepository for SqliteTrackedReleasesRepository {
    async fn save(&self, tracked_release: &mut TrackedRelease) -> Result<(), Box<dyn Error + Send + Sync>> {
        sqlx::query(
            r#"
            INSERT INTO tracked_releases (id, repository_name, repository_url, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                repository_name = excluded.repository_name,
                repository_url = excluded.repository_url,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(tracked_release.id.to_string())
        .bind(&tracked_release.repository_name)
        .bind(tracked_release.repository_url.url())
        .bind(tracked_release.created_at)
        .bind(tracked_release.updated_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn find_all(&self) -> Result<Vec<TrackedRelease>, Box<dyn Error + Send + Sync>> {
        let releases = sqlx::query_as::<_, TrackedRelease>(
            r#"
            SELECT id, repository_name, repository_url, created_at, updated_at
            FROM tracked_releases
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(releases)
    }

    async fn find_by_id(&self, id: &str) -> Result<Option<TrackedRelease>, Box<dyn Error + Send + Sync>> {
        let rec = sqlx::query_as::<_, TrackedRelease>(
            r#"
            SELECT id, repository_name, repository_url, created_at, updated_at
            FROM tracked_releases WHERE id = ?1
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
            SELECT id, repository_name, repository_url, created_at, updated_at
            FROM tracked_releases WHERE repository_url = ?1
            "#,
        )
        .bind(repository_url)
        .fetch_optional(&self.pool)
        .await?;

        Ok(rec)
    }

    async fn delete(&self, id: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        sqlx::query("DELETE FROM tracked_releases WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}


