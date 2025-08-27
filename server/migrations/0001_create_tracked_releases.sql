CREATE TABLE IF NOT EXISTS tracked_releases (
    id TEXT PRIMARY KEY NOT NULL,
    repository_name TEXT NOT NULL,
    repository_url TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tracked_releases_repository_name ON tracked_releases(repository_name);

