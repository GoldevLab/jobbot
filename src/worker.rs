//! Background loop: discover → score → draft → apply.

use crate::agent::LlmAgent;
use crate::browser::{self, SharedChrome};
use crate::db::{self, Settings};
use crate::sources::{self, web3_career};
use crate::style;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct WorkerHandle {
    #[allow(dead_code)]
    pub stop: Arc<AtomicBool>,
}

pub fn spawn(chrome: SharedChrome) -> WorkerHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    tokio::spawn(async move {
        if let Err(e) = run_loop(stop2, chrome).await {
            db::log_event(None, "error", format!("worker crashed: {e:#}")).await;
        }
    });
    WorkerHandle { stop }
}

async fn run_loop(stop: Arc<AtomicBool>, chrome: SharedChrome) -> Result<()> {
    db::log_event(None, "info", "worker started").await;
    let mut tick: u64 = 0;
    let mut last_discover = std::time::Instant::now()
        .checked_sub(Duration::from_secs(3600))
        .unwrap_or_else(std::time::Instant::now);
    while !stop.load(Ordering::Relaxed) {
        let settings = match db::get_settings().await {
            Ok(s) => s,
            Err(e) => {
                db::log_event(None, "error", format!("settings: {e}")).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        if settings.worker_running == 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        // Unstick jobs left mid-flight after crashes / restarts.
        let _ = sqlx::query(
            r#"
            UPDATE jobs SET status = 'discovered', updated_at = datetime('now')
            WHERE status = 'scoring'
              AND updated_at < datetime('now', '-3 minutes')
            "#,
        )
        .execute(db::pool())
        .await;
        let _ = sqlx::query(
            r#"
            UPDATE jobs SET status = 'ready_draft', updated_at = datetime('now')
            WHERE status = 'drafting'
              AND updated_at < datetime('now', '-5 minutes')
            "#,
        )
        .execute(db::pool())
        .await;

        tick += 1;

        let backlog = db::jobs_by_status("discovered", 1)
            .await
            .map(|v| !v.is_empty())
            .unwrap_or(false)
            || db::jobs_by_status("ready_draft", 1)
                .await
                .map(|v| !v.is_empty())
                .unwrap_or(false);

        // Idle queue: scrape at most every 30 min. With backlog: every ~20 ticks.
        let due_idle = last_discover.elapsed() >= Duration::from_secs(30 * 60);
        let due_busy = backlog && (tick == 1 || tick % 20 == 0);
        if tick == 1 || due_busy || (!backlog && due_idle) {
            if let Err(e) = discover_once(&settings).await {
                db::log_event(None, "warn", format!("discover: {e:#}")).await;
            }
            last_discover = std::time::Instant::now();
        }

        let agent = match LlmAgent::from_env() {
            Ok(a) => a,
            Err(e) => {
                db::log_event(None, "error", format!("llm: {e:#}")).await;
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };

        if let Err(e) = score_batch(&agent, 8).await {
            db::log_event(None, "warn", format!("score: {e:#}")).await;
        }
        if let Err(e) = draft_batch(&agent, &settings, 4).await {
            db::log_event(None, "warn", format!("draft: {e:#}")).await;
        }

        if settings.auto_apply != 0 {
            if let Err(e) = apply_batch(&chrome, &settings, 1).await {
                db::log_event(None, "warn", format!("apply: {e:#}")).await;
            }
        }

        // Faster with backlog; idle → longer sleep so logs stay quiet.
        let wait = if backlog {
            3
        } else {
            settings.rate_limit_secs.max(60) as u64
        };
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
    db::log_event(None, "info", "worker stopped").await;
    Ok(())
}

async fn discover_once(settings: &Settings) -> Result<()> {
    db::log_event(None, "info", "discovering web3.career…").await;
    let mut jobs = web3_career::discover(&settings.keywords, &settings.locations).await?;
    jobs.insert(0, web3_career::seed_tether_norway());
    let rows: Vec<_> = jobs
        .into_iter()
        .map(|j| {
            (
                j.source,
                j.external_id,
                j.title,
                j.company,
                j.location,
                j.url,
                j.apply_url,
                j.description,
            )
        })
        .collect();
    let (touched, newly) = db::upsert_jobs_batch(&rows).await?;
    if newly > 0 {
        db::log_event(
            None,
            "info",
            format!("discovered {newly} new jobs ({touched} seen)"),
        )
        .await;
    } else {
        db::log_event(
            None,
            "info",
            format!("discover refresh: 0 new ({touched} already in queue)"),
        )
        .await;
    }
    if touched <= 1 {
        db::log_event(
            None,
            "warn",
            "only seed job found — web3.career scrape may have returned nothing",
        )
        .await;
    }
    Ok(())
}

fn quick_skip(title: &str, company: &str) -> Option<&'static str> {
    let hay = format!("{title} {company}").to_lowercase();
    let junk = [
        "product designer",
        "designer",
        "financial analyst",
        "fp a manager",
        "fp&a",
        "compliance executive",
        "financial crime",
        "supply chain",
        "network engineer",
        "product support",
        "talent program",
        "graduate junior",
        "site reliability",
        " sre ",
        "production engineer",
    ];
    for j in junk {
        if hay.contains(j) {
            return Some("quick-skip: not a backend/Node fit");
        }
    }
    None
}

async fn score_batch(agent: &LlmAgent, limit: i64) -> Result<()> {
    let jobs = db::jobs_by_status("discovered", limit).await?;
    if !jobs.is_empty() {
        db::log_event(
            None,
            "info",
            format!("scoring {} jobs…", jobs.len()),
        )
        .await;
    }
    for job in jobs {
        if let Some(reason) = quick_skip(&job.title, &job.company) {
            db::update_job_status(job.id, "skipped", Some(5.0), None, Some(reason))
                .await?;
            db::log_event(Some(job.id), "info", format!("{} — {}", job.title, reason))
                .await;
            continue;
        }

        let _ = db::update_job_status(job.id, "scoring", None, None, None).await;
        db::log_event(
            Some(job.id),
            "info",
            format!("LLM score: {} @ {}", job.title, job.company),
        )
        .await;
        let prompt = style::score_prompt(&job.title, &job.company, &job.location, &job.description);
        match agent.complete_json(&prompt).await {
            Ok(v) => {
                let score = v.get("score").and_then(|x| x.as_f64()).unwrap_or(50.0);
                let skip = v.get("skip").and_then(|x| x.as_bool()).unwrap_or(false);
                let reason = v
                    .get("reason")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if skip || score < 55.0 {
                    db::update_job_status(job.id, "skipped", Some(score), None, Some(&reason))
                        .await?;
                    db::log_event(Some(job.id), "info", format!("skipped ({score}): {reason}"))
                        .await;
                } else {
                    db::update_job_status(job.id, "ready_draft", Some(score), None, None)
                        .await?;
                    db::log_event(Some(job.id), "info", format!("score {score}: {reason}"))
                        .await;
                }
            }
            Err(e) => {
                let hay = format!("{} {}", job.title, job.description).to_lowercase();
                let score = if hay.contains("backend") || hay.contains("node") {
                    70.0
                } else if hay.contains("rust") || hay.contains("web3") {
                    65.0
                } else {
                    40.0
                };
                let status = if score >= 55.0 {
                    "ready_draft"
                } else {
                    "skipped"
                };
                db::update_job_status(
                    job.id,
                    status,
                    Some(score),
                    None,
                    Some(&format!("llm fail, heuristic: {e}")),
                )
                .await?;
                db::log_event(
                    Some(job.id),
                    "warn",
                    format!("score heuristic {score} ({e})"),
                )
                .await;
            }
        }
    }
    Ok(())
}

async fn draft_batch(agent: &LlmAgent, settings: &Settings, limit: i64) -> Result<()> {
    let jobs = db::jobs_by_status("ready_draft", limit).await?;
    if !jobs.is_empty() {
        db::log_event(None, "info", format!("drafting {} tailored apps…", jobs.len())).await;
    }
    for job in jobs {
        let _ = db::update_job_status(job.id, "drafting", None, None, None).await;
        db::log_event(
            Some(job.id),
            "info",
            format!("tailoring CV/answers for {} @ {}", job.title, job.company),
        )
        .await;
        let memory = db::draft_learning_context(10).await;
        let prompt = style::draft_prompt(
            &job.title,
            &job.company,
            &job.location,
            &job.description,
            settings,
            &memory,
        );
        match agent.complete_json(&prompt).await {
            Ok(v) => {
                let s = serde_json::to_string_pretty(&v)?;
                // Persist a human-readable snippet next to DB draft
                let _ = save_draft_file(job.id, &job.title, &job.company, &v);
                db::update_job_status(job.id, "ready", None, Some(&s), None).await?;
                let pitch = v
                    .get("pitch")
                    .and_then(|x| x.as_str())
                    .unwrap_or("draft ready");
                db::log_event(Some(job.id), "info", format!("draft ready — {pitch}")).await;
                // Feed apply-agent wins into the profile coach memory.
                let emphasize = v
                    .get("emphasize")
                    .and_then(|a| a.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                let lesson = if emphasize.is_empty() {
                    pitch.to_string()
                } else {
                    format!("{pitch} | emphasize: {emphasize}")
                };
                let _ = db::insert_profile_lesson(
                    "apply_agent",
                    "jobs",
                    &format!("{} @ {}", job.title, job.company),
                    &lesson,
                    job.score.unwrap_or(70.0) / 100.0,
                )
                .await;
            }
            Err(e) => {
                let fallback = serde_json::json!({
                    "pitch": format!(
                        "I've been shipping Node/TypeScript backends and Web3 services for years. {} looks like a place where that stack matters.",
                        job.company
                    ),
                    "cv_bullets": [
                        "Node/TypeScript backends and APIs in production since 2017 (Gravitad/Koolinart since 2022)",
                        "Postgres indexer over Geth JSON-RPC; marketplace/DEX/bots on Base + Orionchain",
                        "Docker/Fly.io deploys, SQL-heavy features, remote delivery"
                    ],
                    "emphasize": ["Node.js", "TypeScript", "PostgreSQL", "RPC", "Web3"],
                    "why_company": format!(
                        "I've been building Node/TypeScript backends and Web3 services for years — marketplaces, indexers talking to chain RPC, and stuff that has to stay up on Fly. {} looks like a place where that experience actually matters. I work remote from Venezuela, open to worldwide remote and flexible timezone overlap.",
                        job.company
                    ),
                    "node_experience": "5_plus",
                    "databases": "Most production work is relational — Postgres for an on-chain indexer (sync, pagination, lag checks) and Turso/libSQL with Drizzle on product apps.",
                    "compliance_finance": "Not classic bank KYC/AML. Adjacent: DeFi/payment-ish flows where records need to stay consistent.",
                    "p2p": "Some messaging/bot work; not a deep libp2p specialist.",
                    "country": settings.country,
                    "salary_usd": settings.expected_salary_usd,
                    "cover_note": ""
                });
                let s = serde_json::to_string_pretty(&fallback)?;
                let _ = save_draft_file(job.id, &job.title, &job.company, &fallback);
                db::update_job_status(
                    job.id,
                    "ready",
                    None,
                    Some(&s),
                    Some(&format!("llm draft fail, used template: {e}")),
                )
                .await?;
                db::log_event(Some(job.id), "warn", format!("draft fallback: {e}")).await;
            }
        }
    }
    Ok(())
}

fn save_draft_file(
    id: i64,
    title: &str,
    company: &str,
    draft: &serde_json::Value,
) -> anyhow::Result<()> {
    let dir = std::path::Path::new("data/drafts");
    std::fs::create_dir_all(dir)?;
    let pitch = draft.get("pitch").and_then(|v| v.as_str()).unwrap_or("");
    let why = draft
        .get("why_company")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let bullets = draft
        .get("cv_bullets")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|b| format!("- {b}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let emphasize = draft
        .get("emphasize")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let body = format!(
        "# {title} @ {company}\n\n## Pitch\n{pitch}\n\n## Emphasize\n{emphasize}\n\n## CV bullets (tailored)\n{bullets}\n\n## Why them\n{why}\n\n## Raw JSON\n{}\n",
        serde_json::to_string_pretty(draft)?
    );
    std::fs::write(dir.join(format!("{id}.md")), body)?;
    Ok(())
}

async fn apply_batch(chrome: &SharedChrome, settings: &Settings, limit: i64) -> Result<()> {
    // Rescue drafts that were wrongly marked skipped (no ATS URL).
    let _ = sqlx::query(
        r#"
        UPDATE jobs
        SET status = 'manual',
            last_error = 'draft ready — ATS not auto-applyable yet; use /jobs/:id',
            updated_at = datetime('now')
        WHERE status = 'skipped'
          AND draft_json IS NOT NULL
          AND score IS NOT NULL
          AND score >= 55
        "#,
    )
    .execute(db::pool())
    .await;

    let jobs = jobs_ready_for_apply(limit).await?;
    if jobs.is_empty() {
        // Keep tailored drafts visible — do NOT skip them.
        let stranded = db::jobs_by_status("ready", 20).await?;
        for job in stranded {
            let apply = job.apply_url.clone().unwrap_or_default();
            if apply.is_empty() || apply.contains("web3.career") {
                db::update_job_status(
                    job.id,
                    "manual",
                    None,
                    None,
                    Some("draft ready — open /jobs/:id; auto-apply needs external ATS URL"),
                )
                .await?;
                db::log_event(
                    Some(job.id),
                    "info",
                    format!(
                        "draft kept as manual (no ATS apply URL): {} @ {}",
                        job.title, job.company
                    ),
                )
                .await;
            }
        }
        return Ok(());
    }

    if let Err(e) = browser::ensure_chrome(chrome).await {
        db::log_event(
            None,
            "error",
            format!("Chrome CDP unavailable ({e:#}). Start scripts/chrome-cdp.sh"),
        )
        .await;
        return Ok(());
    }

    let guard = chrome.lock().await;
    let session = guard.as_ref().unwrap();

    for job in jobs {
        let apply_url = job
            .apply_url
            .clone()
            .unwrap_or_else(|| job.url.clone());

        let draft: serde_json::Value = job
            .draft_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::json!({}));

        let ats = sources::apply_common::classify_ats(&apply_url);
        if !ats.auto_apply_supported() {
            let note = format!(
                "draft ready — {} auto-apply not supported yet; use /jobs/:id",
                ats.as_str()
            );
            db::update_job_status(job.id, "manual", None, None, Some(&note)).await?;
            db::log_event(Some(job.id), "info", note).await;
            continue;
        }

        let _ = db::update_job_status(job.id, "applying", None, None, None).await;
        db::log_event(
            Some(job.id),
            "info",
            format!("applying ({}) → {apply_url}", ats.as_str()),
        )
        .await;

        match sources::apply_with_draft(
            session,
            &apply_url,
            settings,
            &draft,
            &settings.cv_path,
        )
        .await
        {
            Ok(res) => {
                if res.submitted {
                    db::update_job_status(job.id, "applied", None, None, Some(&res.note))
                        .await?;
                    db::log_event(Some(job.id), "info", format!("applied: {}", res.note)).await;
                } else {
                    // Unclear confirmation must not look like a successful apply.
                    db::update_job_status(job.id, "failed", None, None, Some(&res.note))
                        .await?;
                    db::log_event(Some(job.id), "warn", format!("apply incomplete: {}", res.note))
                        .await;
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                db::update_job_status(job.id, "failed", None, None, Some(&msg)).await?;
                db::log_event(Some(job.id), "error", format!("apply error: {msg}")).await;
            }
        }
    }
    Ok(())
}

async fn jobs_ready_for_apply(limit: i64) -> anyhow::Result<Vec<db::Job>> {
    let rows = sqlx::query_as::<_, db::Job>(
        r#"
        SELECT * FROM jobs
        WHERE status = 'ready'
          AND apply_url IS NOT NULL
          AND (
            lower(apply_url) LIKE '%recruitee%'
            OR lower(apply_url) LIKE '%careers.tether%'
            OR lower(apply_url) LIKE '%greenhouse%'
            OR lower(apply_url) LIKE '%ashbyhq%'
            OR lower(apply_url) LIKE '%lever.co%'
          )
        ORDER BY
          CASE WHEN lower(apply_url) LIKE '%tether%' THEN 0 ELSE 1 END,
          id ASC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(db::pool())
    .await?;
    Ok(rows)
}

/// One-shot discover from UI without waiting for loop.
pub async fn run_discover_now() -> Result<()> {
    let settings = db::get_settings().await?;
    discover_once(&settings).await
}
