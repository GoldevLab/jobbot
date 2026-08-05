-- Lessons from keep/dismiss + apply-agent drafts; track GitHub auto-applies.

CREATE TABLE IF NOT EXISTS profile_lessons (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    platform TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 1.0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_profile_lessons_created
    ON profile_lessons(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_profile_lessons_source
    ON profile_lessons(source, created_at DESC);
