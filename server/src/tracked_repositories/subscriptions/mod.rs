pub mod repository;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row};
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub tracked_repository_id: Uuid,
    pub chat_id: i64,
    pub created_at: DateTime<Utc>,
}

impl<'r> FromRow<'r, SqliteRow> for Subscription {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let tracked_repository_id_str: String = row.try_get("tracked_repository_id")?;
        let tracked_repository_id = Uuid::parse_str(&tracked_repository_id_str)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let chat_id: i64 = row.try_get("chat_id")?;
        let created_at: DateTime<Utc> = row.try_get("created_at")?;
        Ok(Self { tracked_repository_id, chat_id, created_at })
    }
}


