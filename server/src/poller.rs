use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{ParseMode, ChatId};
use tokio::time::{sleep, Duration};

use crate::tracked_repositories::repository::SqliteTrackedRepositoriesRepository;
use crate::tracked_repositories::repository::TrackedRepositoriesRepository;
use crate::tracked_repositories::tracked_repositories_releases::repository::SqliteCachedRepositoryReleasesRepository;
use crate::tracked_repositories::tracked_repositories_releases::repository::CachedRepositoryReleasesRepository;
use crate::tracked_repositories::tracked_repositories_releases::CachedRepositoryRelease;
use crate::utils::html_escape;
use crate::github::fetch_latest_release_tag;
use crate::configuration::Configuration;

pub struct AppState {
    pub db: sqlx::sqlite::SqlitePool,
}

pub async fn spawn(state: Arc<AppState>, bot: Bot, config: Configuration) {
    tokio::spawn(async move {
        run(state, bot, config).await;
    });
}

async fn run(state: Arc<AppState>, bot: Bot, config: Configuration) {
    log::info!("Starting release poller");

    let client = reqwest::Client::new();
    let token_opt = config.github_token.as_deref();

    loop {
        log::info!("Polling for new releases");
        let repos_repo = SqliteTrackedRepositoriesRepository::new(state.db.clone());
        let cache_repo = SqliteCachedRepositoryReleasesRepository::new(state.db.clone());

        match repos_repo.find_all().await {
            Ok(repos) => {
                for r in repos {
                    if let Some((owner, repo)) = r.repository_url.owner_and_repo() {
                        match fetch_latest_release_tag(&client, &owner, &repo, token_opt).await {
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
                                        should_notify = false;
                                        None
                                    }
                                    Err(_) => None,
                                };

                                if previous_tag.as_deref() != Some(latest_tag.as_str()) {
                                    let cached = CachedRepositoryRelease {
                                        tracked_repository_id: r.id,
                                        tag_name: latest_tag.clone(),
                                        first_seen_at: chrono::Utc::now(),
                                    };
                                    let _ = cache_repo.save(&cached).await;
                                }

                                if should_notify {
                                    log::debug!("Sending notification for {}/{} to {}", owner, repo, r.chat_id);

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
                                    let _ = bot
                                        .send_message(ChatId(r.chat_id), text)
                                        .parse_mode(ParseMode::Html)
                                        .await;
                                }
                            }
                            Ok(None) => {
                                log::info!("No new release for {}/{}", owner, repo);
                            }
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

        sleep(Duration::from_secs(config.interval_secs)).await;
    }
}


