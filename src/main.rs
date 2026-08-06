//! Job Apply Bot — Resuma UI + ADK OpenRouter + Chrome CDP worker.

mod agent;
mod browser;
mod db;
mod github;
mod packet;
mod pages;
mod profile_worker;
mod sources;
mod style;
mod worker;

use axum::extract::Path;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use pages::PagesRegistry;
use resuma::prelude::*;
use serde::Deserialize;
use std::sync::Arc;

const CSS: &str = concat!(
    r#"<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@400;500;600;700&family=Syne:wght@700;800&display=swap" rel="stylesheet">
<style>"#,
    include_str!("styles.css"),
    "</style>"
);

#[derive(Debug, Deserialize)]
struct SettingsForm {
    full_name: String,
    email: String,
    phone: String,
    linkedin: String,
    github: String,
    country: String,
    expected_salary_usd: String,
    cv_path: String,
    keywords: String,
    locations: String,
    rate_limit_secs: String,
    #[serde(default)]
    profile_notes: String,
    #[serde(default)]
    auto_apply: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdForm {
    id: String,
}

#[submit]
async fn save_settings(
    form: SettingsForm,
    _req: &FlowRequest,
) -> std::result::Result<Redirect, SubmitError> {
    let auto = form.auto_apply.as_deref() == Some("on") || form.auto_apply.as_deref() == Some("1");
    let rate: i64 = form.rate_limit_secs.trim().parse().unwrap_or(45);
    sqlx::query(
        r#"
        UPDATE settings SET
            full_name = ?, email = ?, phone = ?, linkedin = ?, github = ?,
            country = ?, expected_salary_usd = ?, cv_path = ?, keywords = ?,
            locations = ?, auto_apply = ?, rate_limit_secs = ?, profile_notes = ?,
            updated_at = datetime('now')
        WHERE id = 1
        "#,
    )
    .bind(form.full_name.trim())
    .bind(form.email.trim())
    .bind(form.phone.trim())
    .bind(form.linkedin.trim())
    .bind(form.github.trim())
    .bind(form.country.trim())
    .bind(form.expected_salary_usd.trim())
    .bind(form.cv_path.trim())
    .bind(form.keywords.trim())
    .bind(form.locations.trim())
    .bind(if auto { 1 } else { 0 })
    .bind(rate)
    .bind(form.profile_notes.trim())
    .execute(db::pool())
    .await
    .map_err(|_| SubmitError::new("Could not save settings"))?;
    Ok(Redirect::to("/settings"))
}

#[submit]
async fn start_worker(_form: serde_json::Value, _req: &FlowRequest) -> std::result::Result<Redirect, SubmitError> {
    db::set_worker_running(true)
        .await
        .map_err(|_| SubmitError::new("Could not start"))?;
    db::log_event(None, "info", "worker enabled from UI").await;
    Ok(Redirect::to("/"))
}

#[submit]
async fn stop_worker(_form: serde_json::Value, _req: &FlowRequest) -> std::result::Result<Redirect, SubmitError> {
    db::set_worker_running(false)
        .await
        .map_err(|_| SubmitError::new("Could not stop"))?;
    db::log_event(None, "info", "worker paused from UI").await;
    Ok(Redirect::to("/"))
}

#[submit]
async fn discover_now(_form: serde_json::Value, _req: &FlowRequest) -> std::result::Result<Redirect, SubmitError> {
    // Scrape can exceed submit timeout — run in background.
    tokio::spawn(async {
        if let Err(e) = worker::run_discover_now().await {
            db::log_event(None, "error", format!("manual discover: {e:#}")).await;
        }
    });
    db::log_event(None, "info", "manual discover queued").await;
    Ok(Redirect::to("/"))
}

#[submit]
async fn start_profile_worker(
    _form: serde_json::Value,
    _req: &FlowRequest,
) -> std::result::Result<Redirect, SubmitError> {
    db::set_profile_worker_running(true)
        .await
        .map_err(|_| SubmitError::new("Could not start profile coach"))?;
    db::log_profile_event("info", "profile coach enabled from UI").await;
    Ok(Redirect::to("/profile"))
}

#[submit]
async fn stop_profile_worker(
    _form: serde_json::Value,
    _req: &FlowRequest,
) -> std::result::Result<Redirect, SubmitError> {
    db::set_profile_worker_running(false)
        .await
        .map_err(|_| SubmitError::new("Could not stop profile coach"))?;
    db::log_profile_event("info", "profile coach paused from UI").await;
    Ok(Redirect::to("/profile"))
}

#[submit]
async fn analyze_profiles_now(
    _form: serde_json::Value,
    _req: &FlowRequest,
) -> std::result::Result<Redirect, SubmitError> {
    // Don't block the HTTP submit on 3 LLM round-trips.
    tokio::spawn(async {
        if let Err(e) = profile_worker::run_analyze_now().await {
            db::log_profile_event("error", format!("manual analyze: {e:#}")).await;
        }
    });
    db::log_profile_event("info", "manual profile analyze queued").await;
    Ok(Redirect::to("/profile"))
}

#[submit]
async fn keep_profile_suggestion(
    form: IdForm,
    _req: &FlowRequest,
) -> std::result::Result<Redirect, SubmitError> {
    let id: i64 = form.id.trim().parse().unwrap_or(0);
    if id > 0 {
        if let Ok(Some(sug)) = db::get_profile_suggestion(id).await {
            let _ = db::insert_profile_lesson(
                "keep",
                &sug.platform,
                &sug.title,
                &sug.body,
                1.5,
            )
            .await;
            // Prefer auto-push when possible.
            let title_l = sug.title.to_ascii_lowercase();
            if sug.platform == "github"
                && (title_l.contains("bio") || title_l.contains("topic"))
                && github::token_from_env().is_some()
            {
                let mut owner = sug
                    .source_json
                    .as_deref()
                    .and_then(|s| {
                        s.lines()
                            .find(|l| l.starts_with("login:"))
                            .map(|l| l.trim_start_matches("login:").trim().to_string())
                    })
                    .unwrap_or_default();
                if owner.is_empty() {
                    if let Ok(s) = db::get_settings().await {
                        owner = s
                            .github
                            .trim()
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                            .unwrap_or("GoldevLab")
                            .to_string();
                    } else {
                        owner = "GoldevLab".into();
                    }
                }
                if let Some(msg) = github::apply_from_suggestion(
                    &owner,
                    &sug.title,
                    &sug.body,
                    &[],
                )
                .await
                {
                    db::log_profile_event("info", format!("keep→auto: {msg}")).await;
                    let _ = db::set_profile_suggestion_status(id, "applied").await;
                    return Ok(Redirect::to("/profile"));
                }
            }
            if sug.platform == "linkedin"
                && (title_l.contains("about") || title_l.contains("headline"))
            {
                if let Ok(settings) = db::get_settings().await {
                    let mut notes = settings.profile_notes;
                    let label = if title_l.contains("headline") {
                        "Headline"
                    } else {
                        "About"
                    };
                    let block = format!("{label}:\n{}", sug.body.trim());
                    if !notes.contains(sug.body.trim()) {
                        if !notes.is_empty() {
                            notes.push_str("\n\n");
                        }
                        notes.push_str(&block);
                        let _ = sqlx::query(
                            "UPDATE settings SET profile_notes = ?, updated_at = datetime('now') WHERE id = 1",
                        )
                        .bind(&notes)
                        .execute(db::pool())
                        .await;
                        db::log_profile_event(
                            "info",
                            format!("keep→notes: saved LinkedIn {label} into Profile notes"),
                        )
                        .await;
                    }
                }
            }
            db::log_profile_event(
                "info",
                format!("learned keep: {} · {}", sug.platform, sug.title),
            )
            .await;
        }
        let _ = db::set_profile_suggestion_status(id, "kept").await;
    }
    Ok(Redirect::to("/profile"))
}

#[submit]
async fn dismiss_profile_suggestion(
    form: IdForm,
    _req: &FlowRequest,
) -> std::result::Result<Redirect, SubmitError> {
    let id: i64 = form.id.trim().parse().unwrap_or(0);
    if id > 0 {
        if let Ok(Some(sug)) = db::get_profile_suggestion(id).await {
            let _ = db::insert_profile_lesson(
                "dismiss",
                &sug.platform,
                &sug.title,
                &sug.body,
                -1.0,
            )
            .await;
            db::log_profile_event(
                "info",
                format!("learned dismiss: {} · {}", sug.platform, sug.title),
            )
            .await;
        }
        let _ = db::set_profile_suggestion_status(id, "dismissed").await;
    }
    Ok(Redirect::to("/profile"))
}

#[layout("/")]
fn RootLayout() -> View {
    view! {
        <div class="shell">
            <header class="top">
                <a class="brand" href="/">"Job"<span>"Bot"</span></a>
                <nav>
                    <NavLink href="/" activeClass="active">"Queue"</NavLink>
                    <NavLink href="/profile" activeClass="active">"Profile"</NavLink>
                    <NavLink href="/settings" activeClass="active">"Settings"</NavLink>
                    <NavLink href="/logs" activeClass="active">"Logs"</NavLink>
                </nav>
            </header>
            <main class="main">
                <Slot />
            </main>
        </div>
    }
}

fn not_found_page() -> View {
    view! {
        <div class="card">
            <h1>"404"</h1>
            <p><a href="/">"Back"</a></p>
        </div>
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let _ = dotenvy::dotenv();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    db::init_db()
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--smoke") {
        return run_smoke().await.map_err(|e| std::io::Error::other(e.to_string()));
    }

    let chrome = browser::shared_chrome();
    let _worker = worker::spawn(chrome);
    let _profile = profile_worker::spawn();

    // Start paused; user hits Run in UI (or set worker_running via env bootstrap)
    if std::env::var("JOBBOT_AUTO_START").ok().as_deref() == Some("1") {
        let _ = db::set_worker_running(true).await;
    }
    if std::env::var("JOBBOT_PROFILE_AUTO_START").ok().as_deref() == Some("1") {
        let _ = db::set_profile_worker_running(true).await;
    }

    let _ = Arc::new(());

    // Local: CARGO_MANIFEST_DIR. Fly/Docker: RESUMA_PUBLIC_DIR / RESUMA_PAGES_ROOT.
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let public_dir = std::env::var_os("RESUMA_PUBLIC_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| manifest.join("public"));
    let pages_dir = std::env::var_os("RESUMA_PAGES_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| manifest.join("src/pages"));

    FlowApp::new()
        .with_title("JobBot")
        .with_head(CSS)
        .not_found(|| not_found_page())
        .with_public_dir(public_dir)
        .auto_pages(pages_dir, PagesRegistry)
        .route("/jobs/{id}/packet.txt", get(download_packet_txt))
        .route("/jobs/{id}/cv.pdf", get(download_cv_pdf))
        .serve(FlowServeOptions::from_env())
        .await
}

async fn download_packet_txt(Path(id): Path<i64>) -> Response {
    let Ok(Some(job)) = db::get_job(id).await else {
        return (StatusCode::NOT_FOUND, "job not found").into_response();
    };
    let Ok(settings) = db::get_settings().await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "settings error").into_response();
    };
    let draft: serde_json::Value = job
        .draft_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::json!({}));
    let text = packet::build_packet_text(&job, &settings, &draft);
    let _ = packet::save_packet_file(job.id, &text);
    let filename = packet::safe_filename(&job.title, &job.company, "txt");
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    (headers, text).into_response()
}

async fn download_cv_pdf(Path(id): Path<i64>) -> Response {
    let Ok(Some(job)) = db::get_job(id).await else {
        return (StatusCode::NOT_FOUND, "job not found").into_response();
    };
    let Ok(settings) = db::get_settings().await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "settings error").into_response();
    };
    let path = std::path::Path::new(&settings.cv_path);
    if !path.is_file() {
        return (
            StatusCode::NOT_FOUND,
            format!(
                "CV not found at {}. Place the PDF on the server (JOBBOT_CV_PATH) or Settings → CV path.",
                settings.cv_path
            ),
        )
            .into_response();
    }
    let Ok(bytes) = tokio::fs::read(path).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "could not read CV").into_response();
    };
    let filename = packet::safe_filename(&job.title, &job.company, "pdf");
    // Prefer original CV basename when it looks like a pdf.
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| s.to_ascii_lowercase().ends_with(".pdf"))
        .map(|s| s.to_string())
        .unwrap_or(filename);
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    (headers, bytes).into_response()
}

async fn run_smoke() -> anyhow::Result<()> {
    println!("== JobBot smoke ==");

    // 1) OpenRouter
    let agent = agent::LlmAgent::from_env()?;
    let reply = agent.complete("Reply with exactly: OK").await?;
    println!("llm: {}", reply.trim().chars().take(80).collect::<String>());
    anyhow::ensure!(
        reply.to_uppercase().contains("OK"),
        "LLM did not return OK (got: {})",
        reply.chars().take(120).collect::<String>()
    );

    // 2) Discover
    let settings = db::get_settings().await?;
    worker::run_discover_now().await?;
    let jobs = db::list_jobs(20).await?;
    println!("jobs_in_db: {}", jobs.len());
    for j in jobs.iter().take(5) {
        println!(
            "  - [{}] {} @ {} | apply={}",
            j.status,
            j.title,
            j.company,
            j.apply_url.as_deref().unwrap_or("-")
        );
    }
    anyhow::ensure!(!jobs.is_empty(), "no jobs after discover");
    let tether = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM jobs WHERE external_id LIKE '%tether%' OR apply_url LIKE '%tether%'",
    )
    .fetch_one(db::pool())
    .await?;
    anyhow::ensure!(tether > 0, "tether seed missing");
    println!("tether_seed: ok");

    // 3) Score one
    let discovered = jobs
        .iter()
        .find(|j| j.status == "discovered")
        .cloned();
    if let Some(job) = discovered {
        let prompt =
            style::score_prompt(&job.title, &job.company, &job.location, &job.description);
        match agent.complete_json(&prompt).await {
            Ok(v) => println!("score_json: {v}"),
            Err(e) => println!("score_json_fallback_ok: {e}"),
        }
    }

    // 4) Chrome CDP
    let cdp = std::env::var("JOBBOT_CHROME_CDP")
        .unwrap_or_else(|_| "http://127.0.0.1:9222".into());
    match reqwest::get(format!("{}/json/version", cdp.trim_end_matches('/'))).await {
        Ok(r) if r.status().is_success() => {
            println!("chrome_cdp: ok ({cdp})");
            let chrome = browser::shared_chrome();
            browser::ensure_chrome(&chrome).await?;
            let g = chrome.lock().await;
            let session = g.as_ref().unwrap();
            let page = session
                .new_page("https://careers.tether.io/o/senior-backend-developer-norway")
                .await?;
            let text: String = page
                .evaluate(
                    r#"(() => (document.body && document.body.innerText) ? document.body.innerText.slice(0, 200) : "")()"#,
                )
                .await?
                .into_value()
                .unwrap_or_default();
            println!("tether_page: {}", text.chars().take(120).collect::<String>().replace('\n', " "));
            anyhow::ensure!(
                text.to_lowercase().contains("backend") || text.to_lowercase().contains("tether"),
                "tether page text unexpected"
            );
        }
        _ => println!("chrome_cdp: SKIP (start ./scripts/chrome-cdp.sh)"),
    }

    let _ = settings;
    println!("== smoke PASSED ==");
    Ok(())
}
