//! SQLite pool — init before `FlowApp::serve()`.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;

static POOL: OnceLock<sqlx::SqlitePool> = OnceLock::new();

pub async fn init_db() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:data/jobbot.db".into());
    if let Some(path) = url.strip_prefix("sqlite:") {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let options = SqliteConnectOptions::from_str(&url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(15));

    // SQLite: keep pool small; WAL allows readers during writes.
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&pool)
        .await?;
    sqlx::query("PRAGMA wal_autocheckpoint = 1000;")
        .execute(&pool)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    // Seed CV path from env if settings still empty
    if let Ok(cv) = std::env::var("JOBBOT_CV_PATH") {
        if !cv.trim().is_empty() {
            let _ = sqlx::query(
                "UPDATE settings SET cv_path = CASE WHEN cv_path = '' OR cv_path IS NULL THEN ? ELSE cv_path END WHERE id = 1",
            )
            .bind(&cv)
            .execute(&pool)
            .await;
        }
    }
    if let Ok(kw) = std::env::var("JOBBOT_KEYWORDS") {
        if !kw.trim().is_empty() {
            let _ = sqlx::query("UPDATE settings SET keywords = ? WHERE id = 1")
                .bind(kw.trim())
                .execute(&pool)
                .await;
        }
    }
    if let Ok(loc) = std::env::var("JOBBOT_LOCATIONS") {
        let loc = loc.trim();
        // Empty / * / all → worldwide (no location filter).
        let value = if loc.is_empty()
            || loc == "*"
            || loc.eq_ignore_ascii_case("all")
            || loc.eq_ignore_ascii_case("worldwide")
        {
            ""
        } else {
            loc
        };
        let _ = sqlx::query("UPDATE settings SET locations = ? WHERE id = 1")
            .bind(value)
            .execute(&pool)
            .await;
    }
    if let Ok(rate) = std::env::var("JOBBOT_RATE_LIMIT_SECS") {
        if let Ok(n) = rate.trim().parse::<i64>() {
            let _ = sqlx::query("UPDATE settings SET rate_limit_secs = ? WHERE id = 1")
                .bind(n)
                .execute(&pool)
                .await;
        }
    }
    if let Ok(auto) = std::env::var("JOBBOT_AUTO_APPLY") {
        let v = matches!(auto.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on");
        let _ = sqlx::query("UPDATE settings SET auto_apply = ? WHERE id = 1")
            .bind(if v { 1 } else { 0 })
            .execute(&pool)
            .await;
    }

    POOL.set(pool)
        .map_err(|_| anyhow::anyhow!("database pool already initialized"))?;
    Ok(())
}

pub fn pool() -> &'static sqlx::SqlitePool {
    POOL.get()
        .expect("call db::init_db() before FlowApp::serve()")
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub id: i64,
    pub full_name: String,
    pub email: String,
    pub phone: String,
    pub linkedin: String,
    pub github: String,
    pub country: String,
    pub expected_salary_usd: String,
    pub cv_path: String,
    pub keywords: String,
    pub locations: String,
    pub auto_apply: i64,
    pub worker_running: i64,
    pub rate_limit_secs: i64,
    pub profile_worker_running: i64,
    pub profile_notes: String,
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct ProfileSuggestion {
    pub id: i64,
    pub platform: String,
    pub title: String,
    pub body: String,
    pub priority: i64,
    pub status: String,
    pub source_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct ProfileEventRow {
    pub id: i64,
    pub level: String,
    pub message: String,
    pub created_at: String,
}

impl Settings {
    pub fn fallback() -> Self {
        Self {
            id: 1,
            full_name: "Golfredo Pérez Fernández".into(),
            email: "golfredo.pf@gmail.com".into(),
            phone: "+58 416 210 9369".into(),
            linkedin: "https://linkedin.com/in/golfredo-perez-fernandez".into(),
            github: "https://github.com/GoldevLab".into(),
            country: "Venezuela".into(),
            expected_salary_usd: "70000".into(),
            cv_path: String::new(),
            keywords: "backend,nodejs,typescript,web3".into(),
            locations: String::new(),
            auto_apply: 1,
            worker_running: 0,
            rate_limit_secs: 45,
            profile_worker_running: 0,
            profile_notes: String::new(),
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct Job {
    pub id: i64,
    pub source: String,
    pub external_id: String,
    pub title: String,
    pub company: String,
    pub location: String,
    pub url: String,
    pub apply_url: Option<String>,
    pub description: String,
    pub score: Option<f64>,
    pub status: String,
    pub draft_json: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct EventRow {
    pub id: i64,
    pub job_id: Option<i64>,
    pub level: String,
    pub message: String,
    pub created_at: String,
}

pub async fn get_settings() -> anyhow::Result<Settings> {
    let s = sqlx::query_as::<_, Settings>("SELECT * FROM settings WHERE id = 1")
        .fetch_one(pool())
        .await?;
    Ok(s)
}

pub async fn set_worker_running(running: bool) -> anyhow::Result<()> {
    sqlx::query("UPDATE settings SET worker_running = ?, updated_at = datetime('now') WHERE id = 1")
        .bind(if running { 1 } else { 0 })
        .execute(pool())
        .await?;
    Ok(())
}

pub async fn set_profile_worker_running(running: bool) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE settings SET profile_worker_running = ?, updated_at = datetime('now') WHERE id = 1",
    )
    .bind(if running { 1 } else { 0 })
    .execute(pool())
    .await?;
    Ok(())
}

pub async fn log_profile_event(level: &str, message: impl AsRef<str>) {
    let msg = message.as_ref().to_string();
    log::info!(target: "jobbot.profile", "[{}] {}", level, msg);
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(800),
        sqlx::query("INSERT INTO profile_events (level, message) VALUES (?, ?)")
            .bind(level)
            .bind(&msg)
            .execute(pool()),
    )
    .await;
}

pub async fn insert_profile_suggestion(
    platform: &str,
    title: &str,
    body: &str,
    priority: i64,
    source_json: Option<&str>,
) -> anyhow::Result<i64> {
    let res = sqlx::query(
        r#"
        INSERT INTO profile_suggestions (platform, title, body, priority, source_json)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(platform)
    .bind(title)
    .bind(body)
    .bind(priority)
    .bind(source_json)
    .execute(pool())
    .await?;
    Ok(res.last_insert_rowid())
}

/// True if same platform+title+body already exists as new/kept/applied (avoid coach spam).
pub async fn profile_suggestion_duplicate(platform: &str, title: &str, body: &str) -> bool {
    let body_norm = body.trim();
    let row: Option<(i64,)> = sqlx::query_as(
        r#"
        SELECT id FROM profile_suggestions
        WHERE platform = ? AND title = ? AND body = ?
          AND status IN ('new', 'kept', 'applied')
        LIMIT 1
        "#,
    )
    .bind(platform)
    .bind(title)
    .bind(body_norm)
    .fetch_optional(pool())
    .await
    .ok()
    .flatten();
    row.is_some()
}

/// Count open (`new`) suggestions whose title matches a kind (bio/headline/about/overview).
pub async fn profile_open_kind_count(platform: &str, kind: &str) -> i64 {
    let like = format!("%{kind}%").to_ascii_lowercase();
    let row: Option<(i64,)> = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM profile_suggestions
        WHERE platform = ? AND status = 'new' AND lower(title) LIKE ?
        "#,
    )
    .bind(platform)
    .bind(like)
    .fetch_optional(pool())
    .await
    .ok()
    .flatten();
    row.map(|r| r.0).unwrap_or(0)
}

/// Keep at most one open (`new`) suggestion per kind (bio/headline/about/overview) per platform.
/// Older duplicates are dismissed so the queue stays usable.
pub async fn prune_open_profile_suggestions(platform: &str) -> anyhow::Result<u64> {
    let kinds = ["bio", "headline", "about", "overview"];
    let mut n = 0u64;
    for kind in kinds {
        let like = format!("%{kind}%");
        let ids: Vec<(i64,)> = sqlx::query_as(
            r#"
            SELECT id FROM profile_suggestions
            WHERE platform = ? AND status = 'new' AND lower(title) LIKE ?
            ORDER BY id DESC
            "#,
        )
        .bind(platform)
        .bind(like.to_ascii_lowercase())
        .fetch_all(pool())
        .await?;
        // Keep newest; dismiss the rest.
        for (id,) in ids.into_iter().skip(1) {
            let res = sqlx::query(
                "UPDATE profile_suggestions SET status = 'dismissed', updated_at = datetime('now') WHERE id = ?",
            )
            .bind(id)
            .execute(pool())
            .await?;
            n += res.rows_affected();
        }
    }
    Ok(n)
}

/// Nuclear cleanup: dismiss every open overview across platforms, and cap open copy slots.
pub async fn prune_all_profile_suggestion_spam() -> anyhow::Result<u64> {
    let mut n = 0u64;
    // 1) Dismiss ALL open overviews (they are noise — actionable copy is bio/headline/About).
    let res = sqlx::query(
        r#"
        UPDATE profile_suggestions
        SET status = 'dismissed', updated_at = datetime('now')
        WHERE status = 'new' AND lower(title) LIKE '%overview%'
        "#,
    )
    .execute(pool())
    .await?;
    n += res.rows_affected();

    for platform in ["github", "linkedin", "general"] {
        n += prune_open_profile_suggestions(platform).await?;
    }

    // 2) Hard cap: keep at most 6 newest `new` suggestions total.
    let ids: Vec<(i64,)> = sqlx::query_as(
        r#"
        SELECT id FROM profile_suggestions
        WHERE status = 'new'
        ORDER BY id DESC
        "#,
    )
    .fetch_all(pool())
    .await?;
    for (id,) in ids.into_iter().skip(6) {
        let res = sqlx::query(
            "UPDATE profile_suggestions SET status = 'dismissed', updated_at = datetime('now') WHERE id = ?",
        )
        .bind(id)
        .execute(pool())
        .await?;
        n += res.rows_affected();
    }
    Ok(n)
}

pub async fn count_open_profile_suggestions() -> i64 {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM profile_suggestions WHERE status = 'new'",
    )
    .fetch_optional(pool())
    .await
    .ok()
    .flatten();
    row.map(|r| r.0).unwrap_or(0)
}

pub async fn list_profile_suggestions(limit: i64) -> anyhow::Result<Vec<ProfileSuggestion>> {
    // Only open (`new`) cards — Keep/Dismiss/Apply must clear them from the UI.
    let rows = sqlx::query_as::<_, ProfileSuggestion>(
        r#"
        SELECT * FROM profile_suggestions
        WHERE status = 'new'
          AND lower(title) NOT LIKE '%overview%'
        ORDER BY priority ASC, id DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool())
    .await?;
    Ok(rows)
}

/// Dismiss every open suggestion (clears the coach idle gate).
pub async fn dismiss_all_open_profile_suggestions() -> anyhow::Result<u64> {
    let res = sqlx::query(
        r#"
        UPDATE profile_suggestions
        SET status = 'dismissed', updated_at = datetime('now')
        WHERE status = 'new'
        "#,
    )
    .execute(pool())
    .await?;
    Ok(res.rows_affected())
}

pub async fn set_profile_suggestion_status(id: i64, status: &str) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE profile_suggestions SET status = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(status)
    .bind(id)
    .execute(pool())
    .await?;
    Ok(())
}

pub async fn get_profile_suggestion(id: i64) -> anyhow::Result<Option<ProfileSuggestion>> {
    let row = sqlx::query_as::<_, ProfileSuggestion>(
        "SELECT * FROM profile_suggestions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool())
    .await?;
    Ok(row)
}

pub async fn insert_profile_lesson(
    source: &str,
    platform: &str,
    title: &str,
    body: &str,
    weight: f64,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO profile_lessons (source, platform, title, body, weight)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(source)
    .bind(platform)
    .bind(title)
    .bind(body)
    .bind(weight)
    .execute(pool())
    .await?;
    Ok(())
}

/// Kept profile copy for the apply-agent drafts (coach → jobs).
pub async fn draft_learning_context(limit: i64) -> String {
    let Ok(rows) = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT platform, title, body FROM profile_lessons
        WHERE source IN ('keep', 'applied')
        ORDER BY id DESC LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool())
    .await
    else {
        return String::new();
    };
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::from("Kept profile lines:\n");
    for (platform, title, body) in rows {
        out.push_str(&format!(
            "- [{platform}] {title}: {}\n",
            crate::style::truncate(&body, 200)
        ));
    }
    out
}

/// Compact memory for the coach prompt: keeps, dismissals, apply-agent pitches.
pub async fn profile_learning_context(limit: i64) -> String {
    let mut out = String::new();

    if let Ok(rows) = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT source, title, body FROM profile_lessons
        ORDER BY id DESC LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool())
    .await
    {
        if !rows.is_empty() {
            out.push_str("Lessons (keep = prefer this style; dismiss = avoid; apply_agent = what sells in drafts):\n");
            for (source, title, body) in rows {
                let body = crate::style::truncate(&body, 220);
                out.push_str(&format!("- [{source}] {title}: {body}\n"));
            }
            out.push('\n');
        }
    }

    if let Ok(jobs) = sqlx::query_as::<_, (String, String, Option<f64>, Option<String>)>(
        r#"
        SELECT title, company, score, draft_json FROM jobs
        WHERE status IN ('ready', 'manual', 'applied')
          AND draft_json IS NOT NULL
        ORDER BY COALESCE(score, 0) DESC, id DESC
        LIMIT 8
        "#,
    )
    .fetch_all(pool())
    .await
    {
        if !jobs.is_empty() {
            out.push_str("Apply-agent winning pitches (mirror this voice on GitHub/LinkedIn):\n");
            for (title, company, score, draft) in jobs {
                let pitch = draft
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .and_then(|v| {
                        v.get("pitch")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_default();
                if pitch.is_empty() {
                    continue;
                }
                let sc = score.map(|s| format!("{s:.0}")).unwrap_or_else(|| "?".into());
                out.push_str(&format!(
                    "- [{sc}] {title} @ {company}: {}\n",
                    crate::style::truncate(&pitch, 180)
                ));
            }
        }
    }

    out
}

pub async fn list_profile_events(limit: i64) -> anyhow::Result<Vec<ProfileEventRow>> {
    let rows = sqlx::query_as::<_, ProfileEventRow>(
        "SELECT * FROM profile_events ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool())
    .await?;
    Ok(rows)
}

pub async fn log_event(job_id: Option<i64>, level: &str, message: impl AsRef<str>) {
    let msg = message.as_ref().to_string();
    log::info!(target: "jobbot", "[{}] {}", level, msg);
    // Best-effort; never block the worker loop on log insert contention.
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(800),
        sqlx::query("INSERT INTO events (job_id, level, message) VALUES (?, ?, ?)")
            .bind(job_id)
            .bind(level)
            .bind(&msg)
            .execute(pool()),
    )
    .await;
}

/// Returns `(touched, newly_inserted)`.
pub async fn upsert_jobs_batch(
    jobs: &[(
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
    )],
) -> anyhow::Result<(usize, usize)> {
    let mut tx = pool().begin().await?;
    let mut newly = 0usize;
    for (source, external_id, title, company, location, url, apply_url, description) in jobs {
        let existed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs WHERE source = ? AND external_id = ?",
        )
        .bind(source)
        .bind(external_id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO jobs (source, external_id, title, company, location, url, apply_url, description)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(source, external_id) DO UPDATE SET
                title = excluded.title,
                company = excluded.company,
                location = excluded.location,
                url = excluded.url,
                apply_url = COALESCE(excluded.apply_url, jobs.apply_url),
                description = CASE WHEN length(excluded.description) > length(jobs.description)
                    THEN excluded.description ELSE jobs.description END,
                updated_at = datetime('now')
            "#,
        )
        .bind(source)
        .bind(external_id)
        .bind(title)
        .bind(company)
        .bind(location)
        .bind(url)
        .bind(apply_url)
        .bind(description)
        .execute(&mut *tx)
        .await?;
        if existed == 0 {
            newly += 1;
        }
    }
    tx.commit().await?;
    Ok((jobs.len(), newly))
}

#[allow(dead_code)]
pub async fn upsert_job(
    source: &str,
    external_id: &str,
    title: &str,
    company: &str,
    location: &str,
    url: &str,
    apply_url: Option<&str>,
    description: &str,
) -> anyhow::Result<i64> {
    sqlx::query(
        r#"
        INSERT INTO jobs (source, external_id, title, company, location, url, apply_url, description)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(source, external_id) DO UPDATE SET
            title = excluded.title,
            company = excluded.company,
            location = excluded.location,
            url = excluded.url,
            apply_url = COALESCE(excluded.apply_url, jobs.apply_url),
            description = CASE WHEN length(excluded.description) > length(jobs.description)
                THEN excluded.description ELSE jobs.description END,
            updated_at = datetime('now')
        "#,
    )
    .bind(source)
    .bind(external_id)
    .bind(title)
    .bind(company)
    .bind(location)
    .bind(url)
    .bind(apply_url)
    .bind(description)
    .execute(pool())
    .await?;

    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM jobs WHERE source = ? AND external_id = ?",
    )
    .bind(source)
    .bind(external_id)
    .fetch_one(pool())
    .await?;
    Ok(id)
}

pub async fn list_jobs(limit: i64) -> anyhow::Result<Vec<Job>> {
    let rows = sqlx::query_as::<_, Job>(
        "SELECT * FROM jobs ORDER BY CASE status
            WHEN 'discovered' THEN 0
            WHEN 'scoring' THEN 1
            WHEN 'drafting' THEN 2
            WHEN 'ready' THEN 3
            WHEN 'applying' THEN 4
            WHEN 'applied' THEN 5
            WHEN 'skipped' THEN 6
            WHEN 'failed' THEN 7
            ELSE 8 END, updated_at DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool())
    .await?;
    Ok(rows)
}

pub async fn list_events(limit: i64) -> anyhow::Result<Vec<EventRow>> {
    let rows = sqlx::query_as::<_, EventRow>(
        "SELECT * FROM events ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool())
    .await?;
    Ok(rows)
}

pub async fn jobs_by_status(status: &str, limit: i64) -> anyhow::Result<Vec<Job>> {
    let rows = sqlx::query_as::<_, Job>(
        "SELECT * FROM jobs WHERE status = ? ORDER BY id ASC LIMIT ?",
    )
    .bind(status)
    .bind(limit)
    .fetch_all(pool())
    .await?;
    Ok(rows)
}

pub async fn update_job_status(
    id: i64,
    status: &str,
    score: Option<f64>,
    draft_json: Option<&str>,
    last_error: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE jobs SET
            status = ?,
            score = COALESCE(?, score),
            draft_json = COALESCE(?, draft_json),
            last_error = ?,
            updated_at = datetime('now')
        WHERE id = ?
        "#,
    )
    .bind(status)
    .bind(score)
    .bind(draft_json)
    .bind(last_error)
    .bind(id)
    .execute(pool())
    .await?;
    Ok(())
}

pub async fn update_job_apply_url(id: i64, apply_url: &str) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE jobs SET apply_url = ?, updated_at = datetime('now')
        WHERE id = ?
        "#,
    )
    .bind(apply_url)
    .bind(id)
    .execute(pool())
    .await?;
    Ok(())
}

/// Ready drafts whose apply_url is missing or still on web3.career (not an ATS).
/// Skips recently failed enrich attempts so we do not hammer web3.career (429).
pub async fn jobs_needing_apply_enrich(limit: i64) -> anyhow::Result<Vec<Job>> {
    let rows = sqlx::query_as::<_, Job>(
        r#"
        SELECT * FROM jobs
        WHERE status IN ('ready', 'manual')
          AND score IS NOT NULL AND score >= 55
          AND (
            apply_url IS NULL
            OR trim(apply_url) = ''
            OR lower(apply_url) LIKE '%web3.career%'
          )
          AND url LIKE '%web3.career%'
          AND (
            last_error IS NULL
            OR last_error NOT LIKE 'enrich:%'
            OR updated_at < datetime('now', '-6 hours')
          )
        ORDER BY
          score DESC,
          id ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool())
    .await?;
    Ok(rows)
}

pub async fn touch_job_enrich_note(id: i64, note: &str) -> anyhow::Result<()> {
    if note.is_empty() {
        sqlx::query(
            "UPDATE jobs SET last_error = NULL, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(id)
        .execute(pool())
        .await?;
    } else {
        sqlx::query(
            "UPDATE jobs SET last_error = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(note)
        .bind(id)
        .execute(pool())
        .await?;
    }
    Ok(())
}

pub async fn get_job(id: i64) -> anyhow::Result<Option<Job>> {
    let row = sqlx::query_as::<_, Job>("SELECT * FROM jobs WHERE id = ?")
        .bind(id)
        .fetch_optional(pool())
        .await?;
    Ok(row)
}
