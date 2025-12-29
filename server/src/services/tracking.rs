use std::sync::Arc;
use uuid::Uuid;

use crate::ports::GitHubService;
use crate::tracked_repositories::repository::TrackedRepositoriesRepository;
use crate::tracked_repositories::tracked_repositories_releases::CachedRepositoryRelease;
use crate::tracked_repositories::tracked_repositories_releases::repository::CachedRepositoryReleasesRepository;
use crate::tracked_repositories::{RepositoryUrl, TrackedRelease};
use crate::utils::html_escape;
use urlencoding::encode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleTrackResult {
    AlreadyTracking { message: String },
    Updated { id: Uuid, message: String },
    Created { id: Uuid, message: String },
}

pub struct TrackingService<R, C> {
    tracked_repo: Arc<R>,
    cached_repo: Arc<C>,
    github_service: Arc<dyn GitHubService>,
}

impl<R, C> TrackingService<R, C>
where
    R: TrackedRepositoriesRepository,
    C: CachedRepositoryReleasesRepository,
{
    pub fn new(
        tracked_repo: Arc<R>,
        cached_repo: Arc<C>,
        github_service: Arc<dyn GitHubService>,
    ) -> Self {
        Self {
            tracked_repo,
            cached_repo,
            github_service,
        }
    }

    pub async fn handle_track(
        &self,
        chat_id: i64,
        name: &str,
        url: &str,
    ) -> Result<HandleTrackResult, String> {
        if name.is_empty() {
            return Err("Please provide a name for the repository.".to_string());
        }

        let repo_url = match RepositoryUrl::new(url.to_string()) {
            Ok(u) => u,
            Err(err_msg) => return Err(err_msg),
        };

        match self
            .tracked_repo
            .find_by_repository_url(&repo_url.url())
            .await
            .map_err(|e| format!("Failed to query repository: {e}"))?
        {
            Some(mut existing) => {
                if existing.chat_id == chat_id {
                    return Ok(HandleTrackResult::AlreadyTracking {
                        message: format!("This chat is already tracking {name} ({url})."),
                    });
                }

                existing.repository_name = name.to_string();
                existing.updated_at = chrono::Utc::now();
                TrackedRepositoriesRepository::save(&*self.tracked_repo, &mut existing)
                    .await
                    .map_err(|e| format!("Failed to update tracked repository: {e}"))?;

                Ok(HandleTrackResult::Updated {
                    id: existing.id,
                    message: format!("Updated tracking for {name} ({url})."),
                })
            }
            None => {
                let now = chrono::Utc::now();
                let mut tracked = TrackedRelease {
                    id: Uuid::now_v7(),
                    repository_name: name.to_string(),
                    repository_url: repo_url,
                    chat_id,
                    created_at: now,
                    updated_at: now,
                };

                TrackedRepositoriesRepository::save(&*self.tracked_repo, &mut tracked)
                    .await
                    .map_err(|e| format!("Failed to track repository: {e}"))?;

                Ok(HandleTrackResult::Created {
                    id: tracked.id,
                    message: format!("Now tracking {name} ({url})."),
                })
            }
        }
    }

    pub async fn handle_untrack(&self, chat_id: i64, url: &str) -> Result<String, String> {
        let repo_url = match RepositoryUrl::new(url.to_string()) {
            Ok(u) => u,
            Err(err_msg) => return Err(err_msg),
        };

        match self
            .tracked_repo
            .find_by_repository_url(&repo_url.url())
            .await
            .map_err(|e| format!("Failed to query repository: {e}"))?
        {
            Some(existing) => {
                if existing.chat_id != chat_id {
                    return Err(format!("This chat is not tracking {url}."));
                }

                // Delete cached releases first
                self.cached_repo
                    .delete_by_tracked_release_id(&existing.id)
                    .await
                    .map_err(|e| format!("Failed to delete cached releases: {e}"))?;

                // Delete the tracked repository
                TrackedRepositoriesRepository::delete(
                    &*self.tracked_repo,
                    &existing.id.to_string(),
                )
                .await
                .map_err(|e| format!("Failed to untrack repository: {e}"))?;

                Ok(format!(
                    "Stopped tracking {} ({url}).",
                    existing.repository_name
                ))
            }
            None => Err(format!("This repository ({url}) is not being tracked.")),
        }
    }

    pub async fn handle_list(&self, chat_id: i64) -> Result<String, String> {
        match self.tracked_repo.find_all_by_chat_id(chat_id).await {
            Ok(repos) => {
                if repos.is_empty() {
                    return Ok("No repositories tracked yet.".to_string());
                }

                let mut lines: Vec<String> = Vec::with_capacity(repos.len());

                for r in repos {
                    let url_string = r.repository_url.to_string();
                    let url_escaped = html_escape(&url_string);
                    let name_escaped = html_escape(&r.repository_name);
                    let latest_str = match self.cached_repo.find_by_tracked_release_id(&r.id).await
                    {
                        Ok(Some(cached)) => {
                            if let Some((owner, repo)) = r.repository_url.owner_and_repo() {
                                let release_url = format!(
                                    "https://github.com/{}/{}/releases/tag/{}",
                                    owner,
                                    repo,
                                    encode(&cached.tag_name)
                                );
                                let release_url_escaped = html_escape(&release_url);
                                let tag_escaped = html_escape(&cached.tag_name);
                                format!(
                                    "latest: <a href=\"{}\">{}</a>",
                                    release_url_escaped, tag_escaped
                                )
                            } else {
                                let tag_escaped = html_escape(&cached.tag_name);
                                format!("latest: {}", tag_escaped)
                            }
                        }
                        _ => "latest: unknown".to_string(),
                    };
                    lines.push(format!(
                        "- <a href=\"{}\">{}</a> - {}",
                        url_escaped, name_escaped, latest_str
                    ));
                }
                let text = format!("Tracked repositories:\n{}", lines.join("\n"));
                Ok(text)
            }
            Err(e) => Err(format!("Failed to list repositories: {e}")),
        }
    }

    pub async fn cache_latest_release_for_new_track(
        &self,
        track_result: &HandleTrackResult,
        url: &str,
    ) -> Result<(), String> {
        let id = match track_result {
            HandleTrackResult::Created { id, .. } => id,
            HandleTrackResult::Updated { id, .. } => id,
            _ => return Ok(()),
        };

        if let Some((owner, repo)) = RepositoryUrl::new(url.to_string())
            .ok()
            .and_then(|u| u.owner_and_repo())
        {
            if let Ok(Some(tag)) = self
                .github_service
                .fetch_latest_release_tag(&owner, &repo)
                .await
            {
                let cached = CachedRepositoryRelease {
                    tracked_repository_id: *id,
                    tag_name: tag,
                    first_seen_at: chrono::Utc::now(),
                };
                let _ = self.cached_repo.save(&cached).await;
            }
        }
        Ok(())
    }

    pub async fn update_chat_for_track(
        &self,
        track_result: &HandleTrackResult,
        chat_id: i64,
        url: &str,
    ) -> Result<(), String> {
        if let HandleTrackResult::Updated { .. } = track_result {
            if let Ok(Some(mut existing)) = self.tracked_repo.find_by_repository_url(url).await {
                existing.chat_id = chat_id;
                let _ = self.tracked_repo.save(&mut existing).await;
            }
        }
        Ok(())
    }
}
