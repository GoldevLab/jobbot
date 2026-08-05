-- Parallel profile coach (GitHub / LinkedIn / general) — isolated from apply worker.

ALTER TABLE settings ADD COLUMN profile_worker_running INTEGER NOT NULL DEFAULT 0;
ALTER TABLE settings ADD COLUMN profile_notes TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS profile_suggestions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    platform TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 2,
    status TEXT NOT NULL DEFAULT 'new',
    source_json TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_profile_suggestions_status
    ON profile_suggestions(status, created_at DESC);

CREATE TABLE IF NOT EXISTS profile_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    level TEXT NOT NULL DEFAULT 'info',
    message TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_profile_events_created
    ON profile_events(created_at DESC);
