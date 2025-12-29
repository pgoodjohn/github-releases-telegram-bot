use std::sync::Arc;
use teloxide::prelude::*;

mod adapters;
mod bot;
mod configuration;
mod db;
mod github;
mod logger;
mod poller;
mod ports;
mod services;
mod tracked_repositories;
mod utils;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting github release bot");

    if std::env::var("ENVIRONMENT_FILE")
        .unwrap_or("true".to_string())
        .parse::<bool>()
        .unwrap()
    {
        println!("Debug mode - loading .env file.");
        dotenvy::dotenv().expect("Failed to load .env file.");
    }
    logger::init_from_environment();

    log::info!("Starting github release bot...");

    log::debug!("Loading configuration");
    let config = configuration::Configuration::from_env();

    log::debug!("Initializing database");
    let pool = db::initialize_db(config.clone()).await?;

    let bot = Bot::new(config.teloxide_token.clone());

    // Create repositories
    let tracked_repo = Arc::new(
        crate::tracked_repositories::repository::SqliteTrackedRepositoriesRepository::new(
            pool.clone(),
        ),
    );
    let cached_repo = Arc::new(crate::tracked_repositories::tracked_repositories_releases::repository::SqliteCachedRepositoryReleasesRepository::new(pool.clone()));

    // Create adapters
    let github_client = reqwest::Client::new();
    let github_service = Arc::new(adapters::github::ReqwestGitHubService::new(
        github_client,
        config.github_token.clone(),
    ));
    let messenger = Arc::new(adapters::messenger::TeloxideMessenger::new(bot.clone()));

    // Create services
    let tracking_service = Arc::new(services::tracking::TrackingService::new(
        tracked_repo.clone(),
        cached_repo.clone(),
        github_service.clone(),
    ));

    let polling_service = Arc::new(services::polling::PollingService::new(
        tracked_repo,
        cached_repo,
        github_service,
        messenger,
    ));

    let bot_state = Arc::new(bot::BotState { tracking_service });

    let polling_state = Arc::new(poller::AppState { polling_service });
    let polling_bot = bot.clone();
    poller::spawn(polling_state, polling_bot, config.clone()).await;

    bot::run(bot, bot_state).await;

    Ok(())
}
