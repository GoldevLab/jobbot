//! Parallel profile coach — GitHub / LinkedIn / general.
//!
//! Isolated from the apply worker: own run flag, own events table, no Chrome,
//! never mutates `jobs`.

use crate::agent::LlmAgent;
use crate::db::{self, Settings};
use crate::style;
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const PLATFORMS: [&str; 3] = ["github", "linkedin", "general"];

pub struct ProfileWorkerHandle {
    #[allow(dead_code)]
    pub stop: Arc<AtomicBool>,
}

pub fn spawn() -> ProfileWorkerHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    tokio::spawn(async move {
        if let Err(e) = run_loop(stop2).await {
            db::log_profile_event("error", format!("profile worker crashed: {e:#}")).await;
        }
    });
    ProfileWorkerHandle { stop }
}

async fn run_loop(stop: Arc<AtomicBool>) -> Result<()> {
    db::log_profile_event("info", "profile worker started (paused until Run)").await;
    let mut idx: usize = 0;
    while !stop.load(Ordering::Relaxed) {
        let settings = match db::get_settings().await {
            Ok(s) => s,
            Err(e) => {
                db::log_profile_event("error", format!("settings: {e}")).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        if settings.profile_worker_running == 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        let platform = PLATFORMS[idx % PLATFORMS.len()];
        idx = idx.wrapping_add(1);

        match coach_platform(platform, &settings).await {
            Ok(n) => {
                db::log_profile_event(
                    "info",
                    format!("coached {platform}: {n} suggestion(s)"),
                )
                .await;
            }
            Err(e) => {
                db::log_profile_event("warn", format!("coach {platform}: {e:#}")).await;
            }
        }

        // Slow loop so OpenRouter + apply worker are not starved.
        let pause = settings.rate_limit_secs.max(60) as u64;
        tokio::time::sleep(Duration::from_secs(pause)).await;
    }
    Ok(())
}

/// One-shot: coach all platforms (UI "Analyze now"). Ignores run flag.
pub async fn run_analyze_now() -> Result<()> {
    let settings = db::get_settings().await?;
    db::log_profile_event("info", "manual profile analyze started").await;
    for platform in PLATFORMS {
        match coach_platform(platform, &settings).await {
            Ok(n) => {
                db::log_profile_event(
                    "info",
                    format!("coached {platform}: {n} suggestion(s)"),
                )
                .await;
            }
            Err(e) => {
                db::log_profile_event("warn", format!("coach {platform}: {e:#}")).await;
            }
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    db::log_profile_event("info", "manual profile analyze finished").await;
    Ok(())
}

async fn coach_platform(platform: &str, settings: &Settings) -> Result<usize> {
    let snapshot = match platform {
        "github" => fetch_github_snapshot(&settings.github).await.unwrap_or_else(|e| {
            format!("(github fetch failed: {e:#}; coach from URL + notes only)\nURL: {}", settings.github)
        }),
        "linkedin" => format!(
            "LinkedIn URL: {}\nPaste notes used as source of truth (no scrape):\n{}",
            settings.linkedin,
            if settings.profile_notes.trim().is_empty() {
                "(empty — suggest what to write + what to paste into Settings → Profile notes)"
            } else {
                settings.profile_notes.as_str()
            }
        ),
        _ => format!(
            "Cross-profile consistency check.\nGitHub: {}\nLinkedIn: {}\nCV path: {}\nKeywords: {}\nLocations: {}",
            settings.github,
            settings.linkedin,
            settings.cv_path,
            settings.keywords,
            settings.locations
        ),
    };

    let learning = db::profile_learning_context(24).await;
    let agent = LlmAgent::from_env()?;
    let prompt = style::profile_coach_prompt(platform, settings, &snapshot, &learning);
    let json = agent.complete_json(&prompt).await?;
    let n = persist_suggestions(platform, &json, Some(&snapshot)).await?;

    // Auto-apply GitHub (bio + topics) with no confirmation when token is set.
    if platform == "github" {
        auto_apply_github(settings, &json).await;
    }

    Ok(n)
}

async fn auto_apply_github(settings: &Settings, json: &Value) {
    if crate::github::token_from_env().is_none() {
        db::log_profile_event(
            "warn",
            "GITHUB_TOKEN not set — coach will not push bio/topics to GitHub",
        )
        .await;
        return;
    }

    let owner = github_login(&settings.github).unwrap_or_default();
    let mut applied_any = false;

    let actions = json
        .get("actions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if !actions.is_empty() {
        let logs = crate::github::apply_actions(&owner, &actions).await;
        for line in logs {
            let ok = line.starts_with("applied");
            if ok {
                applied_any = true;
            }
            db::log_profile_event(if ok { "info" } else { "warn" }, &line).await;
        }
    } else if let Some(suggestions) = json.get("suggestions").and_then(|v| v.as_array()) {
        // Fallback only when LLM omitted structured actions (avoids double PATCHes).
        for item in suggestions.iter().take(6) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let body = item
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if title.is_empty() || body.is_empty() {
                continue;
            }
            if let Some(msg) = crate::github::apply_from_suggestion(&owner, title, body).await {
                let ok = msg.contains("auto-applied");
                if ok {
                    applied_any = true;
                }
                db::log_profile_event(if ok { "info" } else { "warn" }, &msg).await;
            }
        }
    }

    if applied_any {
        let _ = db::insert_profile_lesson(
            "applied",
            "github",
            "auto-apply",
            "GitHub bio/topics were pushed via API this cycle",
            0.5,
        )
        .await;
        // Mark newest github suggestions as applied so UI shows they landed.
        if let Ok(rows) = db::list_profile_suggestions(12).await {
            for sug in rows
                .into_iter()
                .filter(|s| s.platform == "github" && s.status == "new")
                .take(4)
            {
                let t = sug.title.to_ascii_lowercase();
                if t.contains("bio") || t.contains("topic") || t.contains("overview") {
                    let _ = db::set_profile_suggestion_status(sug.id, "applied").await;
                }
            }
        }
    }
}

fn normalize_platform(requested: &str, title: &str) -> String {
    let t = title.to_ascii_lowercase();
    if t.contains("linkedin") {
        return "linkedin".into();
    }
    if t.contains("github") || t.contains("readme") || t.contains("pinned") || t.contains("topic")
    {
        return "github".into();
    }
    if t.contains("bio") && requested == "github" {
        return "github".into();
    }
    if (t.contains("headline") || t.contains("about")) && requested != "github" {
        return "linkedin".into();
    }
    requested.to_string()
}

async fn persist_suggestions(platform: &str, json: &Value, snapshot: Option<&str>) -> Result<usize> {
    let suggestions = json
        .get("suggestions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("LLM JSON missing suggestions[]"))?;

    let summary = json
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if !summary.is_empty() {
        let _ = db::insert_profile_suggestion(
            platform,
            &format!("{platform}: overview"),
            summary,
            1,
            snapshot.map(|s| style::truncate(s, 800)).as_deref(),
        )
        .await;
    }

    let mut n = if summary.is_empty() { 0 } else { 1 };
    for item in suggestions.iter().take(6) {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("suggestion")
            .trim();
        let body = item
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if body.is_empty() {
            continue;
        }
        let priority = item
            .get("priority")
            .and_then(|v| v.as_i64())
            .unwrap_or(2)
            .clamp(1, 3);
        let plat = normalize_platform(platform, title);
        db::insert_profile_suggestion(&plat, title, body, priority, None).await?;
        n += 1;
    }

    // LinkedIn: auto-save About/headline into profile_notes so next cycles + drafts use them.
    if platform == "linkedin" {
        sync_linkedin_notes_from_json(json).await;
    }

    Ok(n)
}

async fn sync_linkedin_notes_from_json(json: &Value) {
    let Some(arr) = json.get("suggestions").and_then(|v| v.as_array()) else {
        return;
    };
    let mut headline = None;
    let mut about = None;
    for item in arr {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let body = item
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if body.is_empty() {
            continue;
        }
        if title.contains("headline") {
            headline = Some(body.to_string());
        } else if title.contains("about") {
            about = Some(body.to_string());
        }
    }
    if headline.is_none() && about.is_none() {
        return;
    }
    let Ok(settings) = db::get_settings().await else {
        return;
    };
    let before = settings.profile_notes.clone();
    let mut notes = settings.profile_notes;
    if let Some(h) = headline {
        if !notes.to_ascii_lowercase().contains("headline:") {
            if !notes.is_empty() {
                notes.push_str("\n\n");
            }
            notes.push_str("Headline:\n");
            notes.push_str(&h);
        }
    }
    if let Some(a) = about {
        if !notes.to_ascii_lowercase().contains("about:") {
            if !notes.is_empty() {
                notes.push_str("\n\n");
            }
            notes.push_str("About:\n");
            notes.push_str(&a);
        }
    }
    if notes != before {
        let _ = sqlx::query(
            "UPDATE settings SET profile_notes = ?, updated_at = datetime('now') WHERE id = 1",
        )
        .bind(&notes)
        .execute(db::pool())
        .await;
        db::log_profile_event(
            "info",
            "auto-saved LinkedIn headline/About into Settings → Profile notes (paste on LinkedIn)",
        )
        .await;
    }
}

fn github_login(url_or_login: &str) -> Option<String> {
    let s = url_or_login.trim().trim_end_matches('/');
    if s.is_empty() {
        return None;
    }
    if !s.contains('/') {
        return Some(s.trim_start_matches('@').to_string());
    }
    let path = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let path = path.strip_prefix("www.").unwrap_or(path);
    let mut parts = path.split('/');
    let host = parts.next()?;
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    let login = parts.next()?.trim();
    if login.is_empty() || login.eq_ignore_ascii_case("orgs") {
        return None;
    }
    Some(login.to_string())
}

async fn fetch_github_snapshot(github_url: &str) -> Result<String> {
    let login =
        github_login(github_url).ok_or_else(|| anyhow!("could not parse GitHub login from URL"))?;
    let client = reqwest::Client::builder()
        .user_agent("jobbot-profile-coach/0.1 (+https://github.com/GoldevLab)")
        .timeout(Duration::from_secs(20))
        .build()?;

    let user: Value = client
        .get(format!("https://api.github.com/users/{login}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("github user")?
        .error_for_status()
        .context("github user status")?
        .json()
        .await?;

    let repos: Value = client
        .get(format!(
            "https://api.github.com/users/{login}/repos?sort=updated&per_page=8&type=owner"
        ))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("github repos")?
        .error_for_status()
        .context("github repos status")?
        .json()
        .await?;

    let bio = user.get("bio").and_then(|v| v.as_str()).unwrap_or("");
    let company = user.get("company").and_then(|v| v.as_str()).unwrap_or("");
    let blog = user.get("blog").and_then(|v| v.as_str()).unwrap_or("");
    let location = user.get("location").and_then(|v| v.as_str()).unwrap_or("");
    let followers = user.get("followers").and_then(|v| v.as_i64()).unwrap_or(0);
    let public_repos = user
        .get("public_repos")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let mut repo_lines = String::new();
    if let Some(arr) = repos.as_array() {
        for r in arr.iter().take(8) {
            let name = r.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let desc = r.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let lang = r
                .get("language")
                .and_then(|v| v.as_str())
                .unwrap_or("n/a");
            let stars = r
                .get("stargazers_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let fork = r.get("fork").and_then(|v| v.as_bool()).unwrap_or(false);
            if fork {
                continue;
            }
            repo_lines.push_str(&format!(
                "- {name} ({lang}, ★{stars}): {}\n",
                style::truncate(desc, 120)
            ));
        }
    }

    Ok(format!(
        "login: {login}\nbio: {bio}\ncompany: {company}\nblog: {blog}\nlocation: {location}\nfollowers: {followers}\npublic_repos: {public_repos}\n\nRecent non-fork repos:\n{repo_lines}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_urls() {
        assert_eq!(
            github_login("https://github.com/GoldevLab").as_deref(),
            Some("GoldevLab")
        );
        assert_eq!(github_login("GoldevLab").as_deref(), Some("GoldevLab"));
        assert_eq!(github_login("@GoldevLab").as_deref(), Some("GoldevLab"));
    }
}
