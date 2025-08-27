use teloxide::{prelude::*, utils::command::BotCommands};
use teloxide::dispatching::{Dispatcher, UpdateFilterExt};
use teloxide::dptree;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use crate::tracked_repositories::repository::{
    TrackedRepositoriesRepository,
    SqliteTrackedRepositoriesRepository,
};
use crate::tracked_repositories::tracked_repositories_releases::repository::{
    CachedRepositoryReleasesRepository,
    SqliteCachedRepositoryReleasesRepository,
};
use crate::tracked_repositories::subscriptions::repository::{
    SubscriptionsRepository,
    SqliteSubscriptionsRepository,
};
use crate::tracked_repositories::tracked_repositories_releases::CachedRepositoryRelease;
use serde::Deserialize;
use teloxide::types::{ParseMode, ChatId};
use std::borrow::Cow;
use tokio::time::{sleep, Duration};

mod db;
mod configuration;
mod logger;
mod tracked_repositories;

struct State {
    db: SqlitePool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting github release bot");

    if cfg!(debug_assertions) {
        println!("Debug mode - loading .env file.");
        dotenvy::dotenv().expect("Failed to load .env file.");
    }
    logger::init_from_environment();

    log::info!("Starting github release bot...");

    log::debug!("Loading configuration");
    let config = configuration::Configuration::from_env();

    log::debug!("Initializing database");
    let pool = db::initialize_db(config.clone()).await?;

    let bot = Bot::new(config.teloxide_token);

    let state = Arc::new(State { db: pool });

    let handler = Update::filter_message()
        .branch(
            dptree::entry()
                .filter_command::<Command>()
                .endpoint(answer)
        )
        .branch(dptree::endpoint(fallback));

    let mut dispatcher = Dispatcher::builder(bot.clone(), handler)
        .dependencies(dptree::deps![state.clone()])
        .build();

    let polling_state = state.clone();
    let polling_bot = bot.clone();
    tokio::spawn(async move {
        run_release_poller(polling_state, polling_bot).await;
    });

    dispatcher.dispatch().await;

    Ok(())
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "snake_case", description = "These commands are supported:")]
enum Command {
    #[command(description = "track a repository: <name> <url>", parse_with = "split")]
    TrackRepo { name: String, url: String },
    #[command(description = "list all tracked repositories")]
    ListRepos,
    #[command(description = "display this help message")]
    Help,
}

async fn answer(bot: Bot, msg: Message, cmd: Command, state: Arc<State>) -> ResponseResult<()> {
    match cmd {
        Command::TrackRepo { name, url } => {
            log::info!("Tracking repository: {name} ({url})");

            // validate the name
            if name.is_empty() {
                bot.send_message(msg.chat.id, "Please provide a name for the repository.").await?;
                return Ok(());
            }

            // Validate URL format first
            let repo_url = match crate::tracked_repositories::RepositoryUrl::new(url.clone()) {
                Ok(u) => u,
                Err(err_msg) => {
                    bot.send_message(msg.chat.id, err_msg).await?;
                    return Ok(());
                }
            };

            let repository = SqliteTrackedRepositoriesRepository::new(state.db.clone());

            // Try to find existing by URL to avoid UNIQUE constraint violations
            match repository.find_by_repository_url(&repo_url.url()).await {
                Ok(Some(mut existing)) => {
                    existing.repository_name = name.clone();
                    existing.updated_at = chrono::Utc::now();
                    if let Err(e) = repository.save(&mut existing).await {
                        bot.send_message(msg.chat.id, format!("Failed to update tracked repository: {e}")).await?;
                    } else {
                        bot.send_message(msg.chat.id, format!("Updated tracking for {name} ({url}).")).await?;
                        // Optionally refresh cache for existing repos as well
                        if let Some((owner, repo)) = existing.repository_url.owner_and_repo() {
                            let client = reqwest::Client::new();
                            let token_opt = std::env::var("GITHUB_TOKEN").ok();
                            if let Ok(Some(tag)) = fetch_latest_release_tag(&client, &owner, &repo, token_opt.as_deref()).await {
                                let cache_repo = SqliteCachedRepositoryReleasesRepository::new(state.db.clone());
                                let cached = CachedRepositoryRelease {
                                    tracked_repository_id: existing.id,
                                    tag_name: tag,
                                    first_seen_at: chrono::Utc::now(),
                                };
                                let _ = cache_repo.save(&cached).await;
                            }
                        }
                        // Ensure subscription for this chat
                        let subs = SqliteSubscriptionsRepository::new(state.db.clone());
                        let _ = subs.subscribe(&existing.id, msg.chat.id.0).await;
                    }
                }
                Ok(None) => {
                    let now = chrono::Utc::now();
                    let mut tracked = crate::tracked_repositories::TrackedRelease {
                        id: uuid::Uuid::now_v7(),
                        repository_name: name.clone(),
                        repository_url: repo_url,
                        created_at: now,
                        updated_at: now,
                    };
                    if let Err(e) = repository.save(&mut tracked).await {
                        bot.send_message(msg.chat.id, format!("Failed to track repository: {e}")).await?;
                    } else {
                        bot.send_message(msg.chat.id, format!("Now tracking {name} ({url}).")).await?;
                        // Fetch and cache latest release for the new tracked repository
                        if let Some((owner, repo)) = tracked.repository_url.owner_and_repo() {
                            let client = reqwest::Client::new();
                            let token_opt = std::env::var("GITHUB_TOKEN").ok();
                            if let Ok(Some(tag)) = fetch_latest_release_tag(&client, &owner, &repo, token_opt.as_deref()).await {
                                let cache_repo = SqliteCachedRepositoryReleasesRepository::new(state.db.clone());
                                let cached = CachedRepositoryRelease {
                                    tracked_repository_id: tracked.id,
                                    tag_name: tag,
                                    first_seen_at: chrono::Utc::now(),
                                };
                                let _ = cache_repo.save(&cached).await;
                            }
                        }
                        // Subscribe the chat to updates for this repository
                        let subs = SqliteSubscriptionsRepository::new(state.db.clone());
                        let _ = subs.subscribe(&tracked.id, msg.chat.id.0).await;
                    }
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Failed to query repository: {e}")).await?;
                }
            }
        }
        Command::ListRepos => {
            let repository = SqliteTrackedRepositoriesRepository::new(state.db.clone());
            match repository.find_all().await {
                Ok(repos) => {
                    if repos.is_empty() {
                        bot.send_message(msg.chat.id, "No repositories tracked yet.").await?;
                    } else {
                        let mut lines: Vec<String> = Vec::with_capacity(repos.len());
                        let cache_repo = SqliteCachedRepositoryReleasesRepository::new(state.db.clone());

                        for r in repos {
                            let latest_str = match cache_repo.find_by_tracked_release_id(&r.id).await {
                                Ok(Some(cached)) => format!("latest: {}", cached.tag_name),
                                _ => "latest: unknown".to_string(),
                            };
                            let url_string = r.repository_url.to_string();
                            let url_escaped = html_escape(&url_string);
                            let name_escaped = html_escape(&r.repository_name);
                            lines.push(format!("- <a href=\"{}\">{}</a> - {}", url_escaped, name_escaped, latest_str));
                        }
                        let text = format!("Tracked repositories:\n{}", lines.join("\n"));
                        bot.send_message(msg.chat.id, text).parse_mode(ParseMode::Html).await?;
                    }
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("Failed to list repositories: {e}")).await?;
                }
            }
        }
        Command::Help => {
            bot.send_message(msg.chat.id, Command::descriptions().to_string()).await?;
        }
    };

    Ok(())
}

async fn fallback(bot: Bot, msg: Message) -> ResponseResult<()> {
    if let Some(text) = msg.text() {
        if text.starts_with('/') {
            bot.send_message(msg.chat.id, Command::descriptions().to_string()).await?;
        } else {
            bot.send_message(msg.chat.id, format!("Sorry, I only work with commands. \n\n{}", Command::descriptions().to_string())).await?;
        }
    }
    Ok(())
}

#[derive(Deserialize, Debug)]
struct ReleaseResponse {
    tag_name: String,
}

#[derive(Deserialize)]
struct TagResponse {
    name: String,
}

async fn fetch_latest_release_tag(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let base = "https://api.github.com";
    let release_url = format!("{}/repos/{}/{}/releases/latest", base, owner, repo);

    let mut req = client
        .get(release_url)
        .header("User-Agent", "github-release-bot/0.1")
        .header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await?;

    if resp.status().is_success() {
        let release: ReleaseResponse = resp.json().await?;
        log::debug!("Latest release for {owner}/{repo} is {release:?}");

        if release.tag_name.is_empty() {
            log::debug!("Latest release for {owner}/{repo} is empty");
            return Ok(None);
        }

        return Ok(Some(release.tag_name));
    } else if resp.status().as_u16() == 404 {
        // Fallback: try tags
        let tags_url = format!("{}/repos/{}/{}/tags?per_page=1", base, owner, repo);
        let mut req = client
            .get(tags_url)
            .header("User-Agent", "github-release-bot/0.1")
            .header("Accept", "application/vnd.github+json");
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await?;
        if resp.status().is_success() {
            let tags: Vec<TagResponse> = resp.json().await?;
            if let Some(first) = tags.into_iter().next() {
                return Ok(Some(first.name));
            }
        }
        return Ok(None);
    }

    Ok(None)
}

fn html_escape(input: &str) -> Cow<'_, str> {
    // Minimal escaping for Telegram HTML: &, <, >, and quotes inside attributes
    let mut needs_escaping = false;
    for ch in input.chars() {
        match ch {
            '&' | '<' | '>' | '"' | '\'' => { needs_escaping = true; break; }
            _ => {}
        }
    }
    if !needs_escaping {
        return Cow::Borrowed(input);
    }
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    Cow::Owned(escaped)
}

async fn run_release_poller(state: Arc<State>, bot: Bot) {

    log::info!("Starting release poller");

    let client = reqwest::Client::new();
    let token_opt = std::env::var("GITHUB_TOKEN").ok();
    let interval_secs: u64 = std::env::var("POLL_INTERVAL_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(60);

    loop {
        log::info!("Polling for new releases");
        let repos_repo = SqliteTrackedRepositoriesRepository::new(state.db.clone());
        let cache_repo = SqliteCachedRepositoryReleasesRepository::new(state.db.clone());
        let subs_repo = SqliteSubscriptionsRepository::new(state.db.clone());

        match repos_repo.find_all().await {
            Ok(repos) => {
                for r in repos {
                    if let Some((owner, repo)) = r.repository_url.owner_and_repo() {
                        match fetch_latest_release_tag(&client, &owner, &repo, token_opt.as_deref()).await {
                            Ok(Some(latest_tag)) => {
                                let mut should_notify = false;
                                let previous_tag = match cache_repo.find_by_tracked_release_id(&r.id).await {
                                    Ok(Some(cached)) => {
                                        if cached.tag_name != latest_tag {
                                            should_notify = true;
                                        }
                                        Some(cached.tag_name)
                                    }
                                    Ok(None) => {
                                        // First time we see this, don't notify immediately; just cache it
                                        should_notify = false;
                                        None
                                    }
                                    Err(_) => None,
                                };

                                // Update cache if new or changed
                                if previous_tag.as_deref() != Some(latest_tag.as_str()) {
                                    let cached = CachedRepositoryRelease {
                                        tracked_repository_id: r.id,
                                        tag_name: latest_tag.clone(),
                                        first_seen_at: chrono::Utc::now(),
                                    };
                                    let _ = cache_repo.save(&cached).await;
                                }

                                if should_notify {
                                    if let Ok(chat_ids) = subs_repo.list_chat_ids_for_repo(&r.id).await {
                                        if !chat_ids.is_empty() {
                                            let url_string = r.repository_url.to_string();
                                            let url_escaped = html_escape(&url_string);
                                            let name_escaped = html_escape(&r.repository_name);
                                            let tag_escaped = html_escape(&latest_tag);
                                            let text = format!(
                                                "New release for <a href=\"{}\">{}</a>: <b>{}</b>",
                                                url_escaped,
                                                name_escaped,
                                                tag_escaped,
                                            );
                                            for chat_id in chat_ids {
                                                let _ = bot.send_message(ChatId(chat_id), text.clone()).parse_mode(ParseMode::Html).await;
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                log::warn!("Poller failed to fetch latest release for {}: {}", r.repository_url, e);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("Poller failed to list repositories: {}", e);
            }
        }

        sleep(Duration::from_secs(interval_secs)).await;
    }
}