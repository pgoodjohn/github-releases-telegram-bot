CREATE TABLE IF NOT EXISTS tracked_repositories (
    id TEXT PRIMARY KEY NOT NULL,
    repository_name TEXT NOT NULL,
    repository_url TEXT NOT NULL UNIQUE,
    chat_id INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tracked_repositories_repository_name ON tracked_repositories(repository_name);

