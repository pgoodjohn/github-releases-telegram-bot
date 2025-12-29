use std::sync::Arc;

use crate::ports::{GitHubService, Messenger};
use crate::tracked_repositories::repository::TrackedRepositoriesRepository;
use crate::tracked_repositories::tracked_repositories_releases::CachedRepositoryRelease;
use crate::tracked_repositories::tracked_repositories_releases::repository::CachedRepositoryReleasesRepository;
use crate::utils::html_escape;
use urlencoding::encode;

pub struct PollingService<R, C> {
    tracked_repo: Arc<R>,
    cached_repo: Arc<C>,
    github_service: Arc<dyn GitHubService>,
    messenger: Arc<dyn Messenger>,
}

impl<R, C> PollingService<R, C>
where
    R: TrackedRepositoriesRepository,
    C: CachedRepositoryReleasesRepository,
{
    pub fn new(
        tracked_repo: Arc<R>,
        cached_repo: Arc<C>,
        github_service: Arc<dyn GitHubService>,
        messenger: Arc<dyn Messenger>,
    ) -> Self {
        Self {
            tracked_repo,
            cached_repo,
            github_service,
            messenger,
        }
    }

    pub async fn poll_once(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        log::info!("Polling for new releases");
        let repos = self.tracked_repo.find_all().await?;

        for r in repos {
            if let Some((owner, repo)) = r.repository_url.owner_and_repo() {
                match self
                    .github_service
                    .fetch_latest_release_tag(&owner, &repo)
                    .await
                {
                    Ok(Some(latest_tag)) => {
                        let mut should_notify = false;
                        let previous_tag =
                            match self.cached_repo.find_by_tracked_release_id(&r.id).await {
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
                            let _ = self.cached_repo.save(&cached).await;
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
                            let _ = self
                                .messenger
                                .send_message(
                                    r.chat_id,
                                    &text,
                                    Some(teloxide::types::ParseMode::Html),
                                )
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
        Ok(())
    }
}
