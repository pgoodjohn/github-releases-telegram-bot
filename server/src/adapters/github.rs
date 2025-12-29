use async_trait::async_trait;
use reqwest::Client;
use std::error::Error;

use crate::github;
use crate::ports::GitHubService;

pub struct ReqwestGitHubService {
    client: Client,
    token: Option<String>,
}

impl ReqwestGitHubService {
    pub fn new(client: Client, token: Option<String>) -> Self {
        Self { client, token }
    }
}

#[async_trait]
impl GitHubService for ReqwestGitHubService {
    async fn fetch_latest_release_tag(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Option<String>, Box<dyn Error + Send + Sync>> {
        github::fetch_latest_release_tag(&self.client, owner, repo, self.token.as_deref()).await
    }
}
