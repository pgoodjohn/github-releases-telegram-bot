use std::sync::Arc;
use teloxide::prelude::*;

mod bot;
mod configuration;
mod db;
mod github;
mod logger;
mod poller;
mod tracked_repositories;
mod utils;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting github release bot");

    if std::env::var("ENVIRONMENT_FILE").unwrap_or("true".to_string()).parse::<bool>().unwrap() {
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

    let bot_state = Arc::new(bot::BotState {
        db: pool.clone(),
        config: config.clone(),
    });

    let polling_state = Arc::new(poller::AppState { db: pool.clone() });
    let polling_bot = bot.clone();
    poller::spawn(polling_state, polling_bot, config.clone()).await;

    bot::run(bot, bot_state).await;

    Ok(())
}
