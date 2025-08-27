-- Subscriptions link tracked repositories to Telegram chat IDs to notify
CREATE TABLE IF NOT EXISTS subscriptions (
    tracked_repository_id TEXT NOT NULL,
    chat_id INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (tracked_repository_id, chat_id),
    FOREIGN KEY (tracked_repository_id) REFERENCES tracked_repositories(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_chat_id ON subscriptions(chat_id);


