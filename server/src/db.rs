use crate::configuration;
use sqlx::sqlite::SqlitePool;

pub async fn initialize_db(
    config: configuration::Configuration,
) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    log::debug!("Initializing database with path {}", config.database_path);
    let db_url = format!("sqlite://{}", config.database_path);
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
