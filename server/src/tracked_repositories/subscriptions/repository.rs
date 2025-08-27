use std::error::Error;
use async_trait::async_trait;
use sqlx::{self, sqlite::SqlitePool};
use uuid::Uuid;
use sqlx::Row;

#[async_trait]
pub trait SubscriptionsRepository: Send + Sync {
    async fn subscribe(&self, tracked_repository_id: &Uuid, chat_id: i64) -> Result<(), Box<dyn Error + Send + Sync>>;
    async fn unsubscribe(&self, tracked_repository_id: &Uuid, chat_id: i64) -> Result<(), Box<dyn Error + Send + Sync>>;
    async fn list_chat_ids_for_repo(&self, tracked_repository_id: &Uuid) -> Result<Vec<i64>, Box<dyn Error + Send + Sync>>;
}

pub struct SqliteSubscriptionsRepository {
    pool: SqlitePool,
}

impl SqliteSubscriptionsRepository {
    pub fn new(pool: SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl SubscriptionsRepository for SqliteSubscriptionsRepository {
    async fn subscribe(&self, tracked_repository_id: &Uuid, chat_id: i64) -> Result<(), Box<dyn Error + Send + Sync>> {
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            INSERT INTO subscriptions (tracked_repository_id, chat_id, created_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(tracked_repository_id, chat_id) DO NOTHING
            "#,
        )
        .bind(tracked_repository_id.to_string())
        .bind(chat_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn unsubscribe(&self, tracked_repository_id: &Uuid, chat_id: i64) -> Result<(), Box<dyn Error + Send + Sync>> {
        sqlx::query(
            r#"
            DELETE FROM subscriptions WHERE tracked_repository_id = ?1 AND chat_id = ?2
            "#,
        )
        .bind(tracked_repository_id.to_string())
        .bind(chat_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_chat_ids_for_repo(&self, tracked_repository_id: &Uuid) -> Result<Vec<i64>, Box<dyn Error + Send + Sync>> {
        let rows = sqlx::query(
            r#"
            SELECT chat_id FROM subscriptions WHERE tracked_repository_id = ?1
            "#,
        )
        .bind(tracked_repository_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let chats = rows.into_iter().filter_map(|r| r.try_get::<i64, _>("chat_id").ok()).collect();
        Ok(chats)
    }
}


