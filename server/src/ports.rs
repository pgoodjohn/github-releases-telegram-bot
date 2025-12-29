use async_trait::async_trait;
use std::error::Error;

/// Port for interacting with GitHub API
#[async_trait]
pub trait GitHubService: Send + Sync {
    async fn fetch_latest_release_tag(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Option<String>, Box<dyn Error + Send + Sync>>;
}

/// Port for sending messages to chats
#[async_trait]
pub trait Messenger: Send + Sync {
    async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<teloxide::types::ParseMode>,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
}
