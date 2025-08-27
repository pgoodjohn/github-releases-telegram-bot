use crate::configuration;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use crate::tracked_repositories::repository::{TrackedRepositoriesRepository, SqliteTrackedRepositoriesRepository};

pub struct RepositoryProvider {
    tracked_repositories: Arc<dyn TrackedRepositoriesRepository>,
}

impl RepositoryProvider {
    pub async fn new(pool: SqlitePool) -> Self {
        let tracked_releases = Arc::new(SqliteTrackedRepositoriesRepository::new(pool.clone())) as Arc<dyn TrackedRepositoriesRepository>;
        Self { tracked_repositories: tracked_releases }
    }

    pub fn tracked_repositories(&self) -> Arc<dyn TrackedRepositoriesRepository> {
        self.tracked_repositories.clone()
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