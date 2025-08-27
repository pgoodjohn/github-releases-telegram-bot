pub mod repository;

use chrono::{DateTime, Utc};
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use sqlx::{FromRow, Row};
use sqlx::sqlite::SqliteRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedRepositoryRelease {
    pub tracked_repository_id: Uuid,
    pub tag_name: String,
    pub first_seen_at: DateTime<Utc>,
}

impl<'r> FromRow<'r, SqliteRow> for CachedRepositoryRelease {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let tracked_repository_id_str: String = row.try_get("tracked_repository_id")?;
        let tracked_repository_id = Uuid::parse_str(&tracked_repository_id_str)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;

        let tag_name: String = row.try_get("tag_name")?;
        let first_seen_at: DateTime<Utc> = row.try_get("first_seen_at")?;

        Ok(Self { tracked_repository_id, tag_name, first_seen_at })
    }
}


