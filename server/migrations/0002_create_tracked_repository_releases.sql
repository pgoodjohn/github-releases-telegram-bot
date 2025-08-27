-- Cache table for the latest release per tracked repository
-- Stores the tag and when we first saw it
CREATE TABLE IF NOT EXISTS tracked_repository_releases (
    tracked_repository_id TEXT PRIMARY KEY NOT NULL,
    tag_name TEXT NOT NULL,
    first_seen_at TEXT NOT NULL,
    FOREIGN KEY (tracked_repository_id) REFERENCES tracked_repositories(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tracked_repository_releases_tag_name ON tracked_repository_releases(tag_name);


