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

    let agent = LlmAgent::from_env()?;
    let prompt = style::profile_coach_prompt(platform, settings, &snapshot);
    let json = agent.complete_json(&prompt).await?;
    persist_suggestions(platform, &json, Some(&snapshot)).await
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
        db::insert_profile_suggestion(platform, title, body, priority, None).await?;
        n += 1;
    }
    Ok(n)
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
