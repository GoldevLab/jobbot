//! GitHub REST helpers for profile coach auto-apply (bio + repo topics).

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const UA: &str = "jobbot-profile-coach/0.1 (+https://github.com/GoldevLab/jobbot)";

/// Once a PATCH /user returns 404, skip further bio attempts (token lacks profile write).
static BIO_WRITE_BLOCKED: AtomicBool = AtomicBool::new(false);

pub fn token_from_env() -> Option<String> {
    for key in ["GITHUB_TOKEN", "GH_TOKEN", "JOBBOT_GITHUB_TOKEN"] {
        if let Ok(t) = std::env::var(key) {
            let t = t.trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

pub fn bio_write_blocked() -> bool {
    BIO_WRITE_BLOCKED.load(Ordering::Relaxed)
}

fn client(token: &str) -> Result<reqwest::Client> {
    // Classic PATs accept `token`; fine-grained/OAuth accept `Bearer`. Send Bearer (GitHub accepts both for classic).
    Ok(reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(25))
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {token}")
                    .parse()
                    .context("Authorization header")?,
            );
            h.insert(
                reqwest::header::ACCEPT,
                "application/vnd.github+json"
                    .parse()
                    .context("Accept header")?,
            );
            h.insert(
                "X-GitHub-Api-Version",
                "2022-11-28".parse().context("API version header")?,
            );
            h
        })
        .build()?)
}

/// Strip emails / long noise from bios before PATCH.
pub fn sanitize_bio(raw: &str) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.contains('@') && (t.contains(".com") || t.contains(".net") || t.contains("gmail")) {
            continue;
        }
        if out.chars().count() + t.chars().count() + 1 > 160 {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(t);
    }
    if out.is_empty() {
        raw.chars().take(160).collect()
    } else {
        out.chars().take(160).collect()
    }
}

/// Who the token authenticates as (login).
pub async fn authenticated_login() -> Result<String> {
    let token = token_from_env().ok_or_else(|| anyhow!("GITHUB_TOKEN not set"))?;
    let client = client(&token)?;
    let resp = client
        .get("https://api.github.com/user")
        .send()
        .await
        .context("GET /user")?;
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        return Err(anyhow!("GET /user failed ({status}): {v}"));
    }
    v.get("login")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("GET /user missing login"))
}

pub async fn set_authenticated_bio(bio: &str) -> Result<String> {
    if BIO_WRITE_BLOCKED.load(Ordering::Relaxed) {
        return Err(anyhow!(
            "bio write blocked: GITHUB_TOKEN lacks profile permission (need classic PAT with `user` scope, or fine-grained Account→Profile: Read and write)"
        ));
    }
    let token = token_from_env().ok_or_else(|| anyhow!("GITHUB_TOKEN not set"))?;
    let bio = sanitize_bio(bio);
    let client = client(&token)?;
    let resp = client
        .patch("https://api.github.com/user")
        .json(&json!({ "bio": bio }))
        .send()
        .await
        .context("PATCH /user")?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if status.as_u16() == 404 {
        BIO_WRITE_BLOCKED.store(true, Ordering::Relaxed);
        return Err(anyhow!(
            "GitHub bio 404 — token can write repos but NOT profile. Create a classic PAT with scope `user` (https://github.com/settings/tokens) and set fly secrets set -a golfredo-jobbot GITHUB_TOKEN=ghp_…"
        ));
    }
    if !status.is_success() {
        return Err(anyhow!("GitHub bio update failed ({status}): {body}"));
    }
    Ok(bio)
}

pub async fn set_repo_topics(owner: &str, repo: &str, topics: &[String]) -> Result<()> {
    let token = token_from_env().ok_or_else(|| anyhow!("GITHUB_TOKEN not set"))?;
    let client = client(&token)?;
    let names: Vec<String> = topics
        .iter()
        .map(|t| {
            t.trim()
                .to_lowercase()
                .replace(' ', "-")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect::<String>()
        })
        .filter(|t| !t.is_empty() && t.len() <= 50)
        .take(20)
        .collect();
    if names.is_empty() {
        return Err(anyhow!("no valid topics"));
    }
    let resp = client
        .put(format!(
            "https://api.github.com/repos/{owner}/{repo}/topics"
        ))
        .header(
            reqwest::header::ACCEPT,
            "application/vnd.github.mercy-preview+json",
        )
        .json(&json!({ "names": names }))
        .send()
        .await
        .context("PUT topics")?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "GitHub topics update failed for {owner}/{repo} ({status}): {body}"
        ));
    }
    Ok(())
}

/// Repo names from a github snapshot (`- name (` lines).
pub fn repos_from_snapshot(snapshot: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in snapshot.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("- ") {
            let name = rest.split_whitespace().next().unwrap_or("");
            let name = name.trim_end_matches(|c: char| c == '(' || c == ',');
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn repo_allowed(repo: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return true;
    }
    let r = repo.to_ascii_lowercase();
    allowed.iter().any(|a| a.eq_ignore_ascii_case(&r))
}

/// Apply structured actions from the coach LLM JSON.
pub async fn apply_actions(
    owner_hint: &str,
    actions: &[Value],
    allowed_repos: &[String],
) -> Vec<String> {
    let mut log = Vec::new();
    let owner = owner_hint.trim().trim_start_matches('@');
    // Prefer the login the token actually belongs to for /user PATCH.
    let auth_login = authenticated_login().await.ok();
    if let Some(ref login) = auth_login {
        if !owner.is_empty() && !login.eq_ignore_ascii_case(owner) {
            log.push(format!(
                "token user is {login} but settings github is {owner} — bio updates still go to the authenticated user"
            ));
        }
    }

    for action in actions {
        let kind = action
            .get("type")
            .or_else(|| action.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match kind.as_str() {
            "set_bio" | "bio" | "github_bio" => {
                let value = action
                    .get("value")
                    .or_else(|| action.get("body"))
                    .or_else(|| action.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if value.is_empty() {
                    continue;
                }
                match set_authenticated_bio(value).await {
                    Ok(applied) => log.push(format!("applied GitHub bio ({})", applied.len())),
                    Err(e) => log.push(format!("bio apply failed: {e:#}")),
                }
            }
            "set_topics" | "topics" | "github_topics" => {
                let repo = action
                    .get("repo")
                    .or_else(|| action.get("repository"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                let repo = repo.rsplit('/').next().unwrap_or(repo).trim();
                if repo.is_empty() || owner.is_empty() {
                    log.push("topics apply skipped: missing owner/repo".into());
                    continue;
                }
                if !repo_allowed(repo, allowed_repos) {
                    log.push(format!(
                        "topics skipped for {owner}/{repo} — not in live snapshot (LLM invented?)"
                    ));
                    continue;
                }
                let topics: Vec<String> = action
                    .get("topics")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .or_else(|| {
                        action.get("value").and_then(|v| v.as_str()).map(|s| {
                            s.split(|c: char| c == ',' || c == ' ' || c == '\n')
                                .map(|t| t.trim().to_string())
                                .filter(|t| !t.is_empty())
                                .collect()
                        })
                    })
                    .unwrap_or_default();
                match set_repo_topics(owner, repo, &topics).await {
                    Ok(()) => log.push(format!("applied topics on {owner}/{repo}")),
                    Err(e) => log.push(format!("topics apply failed: {e:#}")),
                }
            }
            "" => {}
            other => log.push(format!("skipped unknown action: {other}")),
        }
    }
    log
}

/// Heuristic: apply bio/topics from suggestion titles when LLM omitted `actions`.
pub async fn apply_from_suggestion(
    owner: &str,
    title: &str,
    body: &str,
    allowed_repos: &[String],
) -> Option<String> {
    let t = title.to_ascii_lowercase();
    if t.contains("bio") && !t.contains("linkedin") {
        return match set_authenticated_bio(body).await {
            Ok(b) => Some(format!("auto-applied GitHub bio ({})", b.len())),
            Err(e) => Some(format!("bio auto-apply failed: {e:#}")),
        };
    }
    if t.contains("topic") {
        let topics: Vec<String> = body
            .split(|c: char| c == ',' || c == '\n' || c == ' ')
            .map(|s| {
                s.trim()
                    .trim_matches(|c: char| c == ':' || c == '-' || c == '*')
                    .to_string()
            })
            .filter(|s| {
                !s.is_empty()
                    && s.len() < 40
                    && !s.contains(' ')
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
            .collect();
        let repo = body
            .split_whitespace()
            .find(|w| w.contains('/') && !w.contains("http"))
            .and_then(|w| w.split('/').nth(1))
            .map(|s| s.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_'))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| {
                allowed_repos
                    .iter()
                    .find(|r| r.eq_ignore_ascii_case("resuma"))
                    .cloned()
                    .or_else(|| allowed_repos.first().cloned())
            });
        if let Some(repo) = repo {
            if !repo_allowed(&repo, allowed_repos) {
                return Some(format!(
                    "topics skipped for {owner}/{repo} — not in live snapshot"
                ));
            }
            return match set_repo_topics(owner, &repo, &topics).await {
                Ok(()) => Some(format!("auto-applied topics on {owner}/{repo}")),
                Err(e) => Some(format!("topics auto-apply failed: {e:#}")),
            };
        }
        if !topics.is_empty() {
            return Some(format!(
                "topics listed ({}) but no repo target — set actions[].repo next cycle",
                topics.len()
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_email_from_bio() {
        let b = sanitize_bio("Backend engineer\nEmail: golfredo.pf@gmail.com\nRemote EU");
        assert!(!b.contains('@'));
        assert!(b.contains("Backend"));
    }

    #[test]
    fn parses_repos_from_snapshot() {
        let snap = "login: x\n\nRecent non-fork repos:\n- resuma (Rust, ★1): SSR\n- jobbot (Rust, ★0): bot\n";
        let repos = repos_from_snapshot(snap);
        assert!(repos.iter().any(|r| r == "resuma"));
        assert!(repos.iter().any(|r| r == "jobbot"));
    }
}
