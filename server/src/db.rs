use crate::configuration;
use sqlx::sqlite::SqlitePool;
use std::fs::File;
use std::path::Path;

pub async fn initialize_db(
    config: configuration::Configuration,
) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    log::debug!("Initializing database with path {}", config.database_path);
    let db_url = format!("sqlite://{}", config.database_path);

    // Check db file exists, create it if it doesn't
    if !Path::new(&config.database_path).exists() {
        log::debug!("Database file does not exist, creating it");
        File::create(&config.database_path)?;
    }

    let pool = SqlitePool::connect(&db_url).await?;

    log::debug!("Running migrations");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");
    log::debug!("Migrations run successfully");

    log::debug!("Database initialized");

    Ok(pool)
}
