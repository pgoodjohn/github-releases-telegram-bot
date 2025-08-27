use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{ChatId, ParseMode};
use tokio::time::{Duration, sleep};

use crate::configuration::Configuration;
use crate::github::{fetch_latest_release_tag, fetch_latest_release_tag_with_base};
use crate::tracked_repositories::repository::SqliteTrackedRepositoriesRepository;
use crate::tracked_repositories::repository::TrackedRepositoriesRepository;
use crate::tracked_repositories::tracked_repositories_releases::CachedRepositoryRelease;
use crate::tracked_repositories::tracked_repositories_releases::repository::CachedRepositoryReleasesRepository;
use crate::tracked_repositories::tracked_repositories_releases::repository::SqliteCachedRepositoryReleasesRepository;
use crate::utils::html_escape;
use urlencoding::encode;

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
        poll_once(state.clone(), &bot, &client, token_opt, None).await;

        sleep(Duration::from_secs(config.interval_secs)).await;
    }
}

pub(crate) async fn poll_once(
    state: Arc<AppState>,
    bot: &Bot,
    client: &reqwest::Client,
    token_opt: Option<&str>,
    github_base_override: Option<&str>,
) {
    log::info!("Polling for new releases");
    let repos_repo = SqliteTrackedRepositoriesRepository::new(state.db.clone());
    let cache_repo = SqliteCachedRepositoryReleasesRepository::new(state.db.clone());

    match repos_repo.find_all().await {
        Ok(repos) => {
            for r in repos {
                if let Some((owner, repo)) = r.repository_url.owner_and_repo() {
                    let latest = if let Some(base) = github_base_override {
                        fetch_latest_release_tag_with_base(client, &owner, &repo, token_opt, base)
                            .await
                    } else {
                        fetch_latest_release_tag(client, &owner, &repo, token_opt).await
                    };
                    match latest {
                        Ok(Some(latest_tag)) => {
                            let mut should_notify = false;
                            let previous_tag =
                                match cache_repo.find_by_tracked_release_id(&r.id).await {
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
                                log::debug!(
                                    "Sending notification for {}/{} to {}",
                                    owner,
                                    repo,
                                    r.chat_id
                                );

                                let url_string = r.repository_url.to_string();
                                let url_escaped = html_escape(&url_string);
                                let name_escaped = html_escape(&r.repository_name);
                                let tag_escaped = html_escape(&latest_tag);
                                let release_url = format!(
                                    "https://github.com/{}/{}/releases/tag/{}",
                                    owner,
                                    repo,
                                    encode(&latest_tag)
                                );
                                let release_url_escaped = html_escape(&release_url);
                                let text = format!(
                                    "New release for <a href=\"{}\">{}</a>: <a href=\"{}\"><b>{}</b></a>",
                                    url_escaped, name_escaped, release_url_escaped, tag_escaped,
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
                            log::warn!(
                                "Poller failed to fetch latest release for {}: {}",
                                r.repository_url,
                                e
                            );
                        }
                    }
                }
            }
        }
        Err(e) => {
            log::warn!("Poller failed to list repositories: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracked_repositories::{RepositoryUrl, TrackedRelease};
    use chrono::Utc;
    use mockito::Server;
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    async fn setup_state() -> Arc<AppState> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("failed to create in-memory sqlite pool");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("failed to run migrations");

        Arc::new(AppState { db: pool })
    }

    async fn insert_tracked(
        state: &Arc<AppState>,
        name: &str,
        url: &str,
        chat_id: i64,
    ) -> TrackedRelease {
        let repo = SqliteTrackedRepositoriesRepository::new(state.db.clone());
        let mut tr = TrackedRelease {
            id: Uuid::new_v4(),
            repository_name: name.to_string(),
            repository_url: RepositoryUrl::new(url.to_string()).unwrap(),
            chat_id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        repo.save(&mut tr).await.unwrap();
        tr
    }

    #[tokio::test]
    async fn poller_behaviour_caches_and_notifies_as_expected() {
        let state = setup_state().await;
        let client = reqwest::Client::new();

        // Dedicated mock servers
        let mut gh = Server::new_async().await;
        let mut tg = Server::new_async().await;

        // Configure bot to hit mock Telegram
        let token = "TESTTOKEN";
        let bot = Bot::new(token).set_api_url(reqwest::Url::parse(&tg.url()).unwrap());

        // Track repository
        let tracked =
            insert_tracked(&state, "owner/repo", "https://github.com/owner/repo", 123).await;

        // 1) First time seeing tag -> cache saved, no notify
        let _m_gh1 = gh
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"tag_name": "v1.0.0"}).to_string())
            .expect(1)
            .create_async()
            .await;

        let _m_tg0 = tg
            .mock(
                "POST",
                mockito::Matcher::Exact(format!("/bot{token}/SendMessage")),
            )
            .with_status(200)
            .with_body("invalid-json")
            .expect(0)
            .create_async()
            .await;

        poll_once(state.clone(), &bot, &client, None, Some(&gh.url())).await;

        let cache_repo = SqliteCachedRepositoryReleasesRepository::new(state.db.clone());
        let cached = cache_repo
            .find_by_tracked_release_id(&tracked.id)
            .await
            .unwrap()
            .expect("cached row");
        assert_eq!(cached.tag_name, "v1.0.0");

        // 2) Same tag again -> no notify, cache unchanged
        let _m_gh2 = gh
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"tag_name": "v1.0.0"}).to_string())
            .expect(1)
            .create_async()
            .await;

        let _m_tg1 = tg
            .mock(
                "POST",
                mockito::Matcher::Exact(format!("/bot{token}/SendMessage")),
            )
            .with_status(200)
            .with_body("invalid-json")
            .expect(0)
            .create_async()
            .await;

        let first_seen_at_before = cached.first_seen_at;
        poll_once(state.clone(), &bot, &client, None, Some(&gh.url())).await;
        let cached_again = cache_repo
            .find_by_tracked_release_id(&tracked.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cached_again.tag_name, "v1.0.0");
        assert_eq!(cached_again.first_seen_at, first_seen_at_before);

        // 3) New tag -> notify once and cache updates
        let _m_gh3 = gh
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"tag_name": "v1.1.0"}).to_string())
            .expect(1)
            .create_async()
            .await;

        let m_tg2 = tg
            .mock(
                "POST",
                mockito::Matcher::Exact(format!("/bot{token}/SendMessage")),
            )
            .with_status(200)
            // We can return invalid JSON; the poller ignores send errors
            .with_body("invalid-json")
            .expect(1)
            .create_async()
            .await;

        poll_once(state.clone(), &bot, &client, None, Some(&gh.url())).await;
        m_tg2.assert();

        let cached_new = cache_repo
            .find_by_tracked_release_id(&tracked.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cached_new.tag_name, "v1.1.0");
        assert!(cached_new.first_seen_at > first_seen_at_before);
    }
}
