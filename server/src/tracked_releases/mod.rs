pub mod repository;

use chrono::{DateTime, Utc};
use uuid::{Uuid};
use serde::{Serialize, Deserialize};
use sqlx::{FromRow, Row};
use sqlx::sqlite::SqliteRow;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedRelease {
    pub id: Uuid,
    pub repository_name: String,
    pub repository_url: RepositoryUrl,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryUrl {
   url: String,
}

impl RepositoryUrl {
    pub fn new(url: String) -> Result<Self, String> {
        if !url.starts_with("https://github.com/") {
            log::warn!("Invalid GitHub repository URL: {url}");
            return Err(format!("Invalid GitHub repository URL: {url}"));
        }

        Ok(Self { url })
    }

    pub fn url(&self) -> String {
        self.url.clone()
    }

    pub fn owner_and_repo(&self) -> Option<(String, String)> {
        let trimmed = self.url.strip_prefix("https://github.com/")?;
        let mut parts = trimmed.split('/');
        let owner = parts.next()?.trim();
        let repo_raw = parts.next()?.trim();
        if owner.is_empty() || repo_raw.is_empty() {
            return None;
        }
        let repo = repo_raw.trim_end_matches(".git");
        Some((owner.to_string(), repo.to_string()))
    }
}

impl<'r> FromRow<'r, SqliteRow> for TrackedRelease {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let id_str: String = row.try_get("id")?;
        let id = Uuid::parse_str(&id_str)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;

        let repository_name: String = row.try_get("repository_name")?;
        let repository_url_str: String = row.try_get("repository_url")?;

        // Construct directly to avoid validating DB contents at read time
        let repository_url = RepositoryUrl { url: repository_url_str };

        let created_at: DateTime<Utc> = row.try_get("created_at")?;
        let updated_at: DateTime<Utc> = row.try_get("updated_at")?;

        Ok(Self {
            id,
            repository_name,
            repository_url,
            created_at,
            updated_at,
        })
    }
}

impl fmt::Display for RepositoryUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.url)
    }
}
