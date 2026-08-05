-- JobBot schema
CREATE TABLE IF NOT EXISTS settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    full_name TEXT NOT NULL DEFAULT 'Golfredo Pérez Fernández',
    email TEXT NOT NULL DEFAULT 'golfredo.pf@gmail.com',
    phone TEXT NOT NULL DEFAULT '+58 416 210 9369',
    linkedin TEXT NOT NULL DEFAULT 'https://linkedin.com/in/golfredo-perez-fernandez',
    github TEXT NOT NULL DEFAULT 'https://github.com/GoldevLab',
    country TEXT NOT NULL DEFAULT 'Venezuela',
    expected_salary_usd TEXT NOT NULL DEFAULT '70000',
    cv_path TEXT NOT NULL DEFAULT '',
    keywords TEXT NOT NULL DEFAULT 'backend,nodejs,typescript,web3,blockchain',
    locations TEXT NOT NULL DEFAULT 'norway,oslo,remote,europe',
    auto_apply INTEGER NOT NULL DEFAULT 1,
    worker_running INTEGER NOT NULL DEFAULT 0,
    rate_limit_secs INTEGER NOT NULL DEFAULT 45,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO settings (id) VALUES (1);

CREATE TABLE IF NOT EXISTS jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    external_id TEXT NOT NULL,
    title TEXT NOT NULL,
    company TEXT NOT NULL DEFAULT '',
    location TEXT NOT NULL DEFAULT '',
    url TEXT NOT NULL,
    apply_url TEXT,
    description TEXT NOT NULL DEFAULT '',
    score REAL,
    status TEXT NOT NULL DEFAULT 'discovered',
    draft_json TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(source, external_id)
);

CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);

CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id INTEGER,
    level TEXT NOT NULL DEFAULT 'info',
    message TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY(job_id) REFERENCES jobs(id)
);

CREATE INDEX IF NOT EXISTS idx_events_created ON events(created_at DESC);
