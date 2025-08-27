use crate::configuration;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use crate::tracked_releases::repository::{TrackedReleasesRepository, SqliteTrackedReleasesRepository};

pub struct RepositoryProvider {
    tracked_releases: Arc<dyn TrackedReleasesRepository>,
}

impl RepositoryProvider {
    pub async fn new(pool: SqlitePool) -> Self {
        let tracked_releases = Arc::new(SqliteTrackedReleasesRepository::new(pool.clone())) as Arc<dyn TrackedReleasesRepository>;
        Self { tracked_releases }
    }

    pub fn tracked_releases(&self) -> Arc<dyn TrackedReleasesRepository> {
        self.tracked_releases.clone()
    }
}

pub async fn initialize_db(config: configuration::Configuration) -> Result<SqlitePool, Box<dyn std::error::Error>> {

    log::debug!("Initializing database with path {}", config.database_path);
    let db_url = format!("sqlite://{}", config.database_path);
    let pool = SqlitePool::connect(&db_url).await?;

    log::debug!("Running migrations");
    sqlx::migrate!("./migrations").run(&pool).await.expect("Failed to run migrations");
    log::debug!("Migrations run successfully");

    log::debug!("Database initialized");

    Ok(pool)
}