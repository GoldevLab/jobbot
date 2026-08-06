//! Recruitee Careers Site API apply — works on Fly without Chrome.
//!
//! Docs: POST /api/offers/{slug}/candidates (public, no auth).
//! Skips offers that require video/file answers we cannot produce.

use super::apply_common::{split_name, ApplyResult};
use crate::db::Settings;
use anyhow::{anyhow, Context, Result};
use reqwest::multipart::{Form, Part};
use serde_json::{json, Value};
use std::path::Path;

const UA: &str = "jobbot/0.1 (+https://github.com/GoldevLab/jobbot)";

#[derive(Debug)]
struct CareersTarget {
    api_base: String,
    slug: String,
}

fn parse_target(apply_url: &str) -> Result<CareersTarget> {
    let u = url::Url::parse(apply_url).context("parse apply url")?;
    let host = u.host_str().unwrap_or("").to_string();
    if host.is_empty() {
        return Err(anyhow!("apply url missing host"));
    }
    let mut slug = u
        .path_segments()
        .into_iter()
        .flatten()
        .skip_while(|s| *s != "o")
        .nth(1)
        .unwrap_or("")
        .to_string();
    if slug.is_empty() {
        slug = u
            .path_segments()
            .into_iter()
            .flatten()
            .next_back()
            .unwrap_or("")
            .to_string();
    }
    if slug.is_empty() || slug == "new" || slug == "c" {
        return Err(anyhow!("could not parse offer slug from {apply_url}"));
    }
    Ok(CareersTarget {
        api_base: format!("https://{host}/api"),
        slug,
    })
}

fn draft_str(draft: &Value, key: &str) -> String {
    draft
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn pick_node_option(options: &[Value], draft: &Value) -> Option<String> {
    let hint = draft_str(draft, "node_experience").to_ascii_lowercase();
    let prefer = if hint.contains("5") || hint.contains("plus") || hint.contains("6") {
        "more than 5"
    } else if hint.contains("3_5") || hint.contains("3-5") {
        "3-5"
    } else if hint.contains("1_3") || hint.contains("1-3") {
        "1-3"
    } else {
        "more than 5"
    };
    for o in options {
        let body = o.get("body").and_then(|v| v.as_str()).unwrap_or("");
        if body.to_ascii_lowercase().contains(prefer) {
            return Some(body.to_string());
        }
    }
    options
        .last()
        .and_then(|o| o.get("body").and_then(|v| v.as_str()).map(|s| s.to_string()))
}

fn answer_for_question(q: &Value, settings: &Settings, draft: &Value) -> Option<(String, Value)> {
    let id = q.get("id")?.as_i64()?;
    let kind = q.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let body = q
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let required = q.get("required").and_then(|v| v.as_bool()).unwrap_or(false);

    match kind {
        "infobox" => None,
        "video" | "file" | "multi_file" => {
            if required {
                Some((
                    "BLOCK".into(),
                    json!({ "reason": format!("required {kind} question") }),
                ))
            } else {
                None
            }
        }
        "boolean" | "legal" => {
            let flag = if body.contains("p2p") || body.contains("peer-to-peer") {
                let p2p = draft_str(draft, "p2p").to_ascii_lowercase();
                p2p.contains("yes") || p2p.contains("some") || p2p.contains("messaging")
            } else {
                true
            };
            Some((
                id.to_string(),
                json!({ "open_question_id": id, "flag": flag }),
            ))
        }
        "multi_choice" => {
            let opts = q
                .get("open_question_options")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let choice = if body.contains("nodejs") || body.contains("node") {
                pick_node_option(&opts, draft)
            } else {
                opts.first()
                    .and_then(|o| o.get("body").and_then(|v| v.as_str()).map(|s| s.to_string()))
            }?;
            Some((
                id.to_string(),
                json!({ "open_question_id": id, "multi_content": [choice] }),
            ))
        }
        "single_choice" => {
            let opts = q
                .get("open_question_options")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let choice = opts
                .first()
                .and_then(|o| o.get("body").and_then(|v| v.as_str()))?
                .to_string();
            Some((
                id.to_string(),
                json!({ "open_question_id": id, "content": choice }),
            ))
        }
        "salary" | "number" => {
            let sal = draft_str(draft, "salary_usd");
            let sal = if sal.is_empty() {
                settings.expected_salary_usd.clone()
            } else {
                sal
            };
            let digits: String = sal.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                return None;
            }
            Some((
                id.to_string(),
                json!({ "open_question_id": id, "content": digits }),
            ))
        }
        "string" | "text" | "date" => {
            let content = if body.contains("linkedin") {
                settings.linkedin.clone()
            } else if body.contains("github") || body.contains("useful link") {
                settings.github.clone()
            } else if body.contains("country") || body.contains("from which country") {
                let c = draft_str(draft, "country");
                if c.is_empty() {
                    settings.country.clone()
                } else {
                    c
                }
            } else if body.contains("database") {
                let d = draft_str(draft, "databases");
                if d.is_empty() {
                    "Most production work is relational — Postgres for an on-chain indexer and product apps with SQL-heavy features.".into()
                } else {
                    d
                }
            } else if body.contains("compliance") || body.contains("financial") {
                let d = draft_str(draft, "compliance_finance");
                if d.is_empty() {
                    "Not classic bank KYC/AML. Adjacent DeFi/payment-ish flows where records need to stay consistent.".into()
                } else {
                    d
                }
            } else if body.contains("why") || body.contains("interested") {
                let d = draft_str(draft, "why_company");
                if d.is_empty() {
                    draft_str(draft, "pitch")
                } else {
                    d
                }
            } else {
                let pitch = draft_str(draft, "pitch");
                if pitch.is_empty() {
                    format!(
                        "Backend engineer (Node/TS, Postgres, Web3). Remote from {}.",
                        settings.country
                    )
                } else {
                    pitch
                }
            };
            if content.is_empty() {
                return None;
            }
            Some((
                id.to_string(),
                json!({ "open_question_id": id, "content": content }),
            ))
        }
        _ => {
            if required {
                Some((
                    "BLOCK".into(),
                    json!({ "reason": format!("unsupported required question kind: {kind}") }),
                ))
            } else {
                None
            }
        }
    }
}

/// Apply via Recruitee Careers Site API (no Chrome).
pub async fn apply_http(
    apply_url: &str,
    settings: &Settings,
    draft: &Value,
    cv_path: &str,
) -> Result<ApplyResult> {
    let target = parse_target(apply_url)?;
    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;

    let offer_url = format!("{}/offers/{}", target.api_base, target.slug);
    let offer_resp = client
        .get(&offer_url)
        .send()
        .await
        .context("GET offer")?
        .error_for_status()
        .context("GET offer status")?;
    let offer_json: Value = offer_resp.json().await.context("offer json")?;
    let offer = offer_json
        .get("offer")
        .cloned()
        .ok_or_else(|| anyhow!("offer payload missing"))?;

    let questions = offer
        .get("open_questions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut answers = Vec::new();
    for q in &questions {
        if let Some((key, ans)) = answer_for_question(q, settings, draft) {
            if key == "BLOCK" {
                let reason = ans
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("blocked question");
                return Ok(ApplyResult {
                    submitted: false,
                    note: format!("manual: {reason} — open /jobs draft and apply by hand"),
                });
            }
            answers.push(ans);
        }
    }

    let (_first, _last) = split_name(&settings.full_name);
    let phone = settings.phone.trim();
    let email = settings.email.trim();
    if settings.full_name.trim().is_empty() || email.is_empty() {
        return Err(anyhow!("settings missing name/email"));
    }

    let cv = Path::new(cv_path);
    if !cv.is_file() {
        return Ok(ApplyResult {
            submitted: false,
            note: format!("manual: CV not found at {cv_path}"),
        });
    }
    let cv_bytes = tokio::fs::read(cv).await.context("read cv")?;
    let filename = cv
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("cv.pdf")
        .to_string();

    let mut form = Form::new()
        .text("candidate[name]", settings.full_name.clone())
        .text("candidate[email]", email.to_string())
        .text("referrer", "jobbot");

    if !phone.is_empty() {
        form = form.text("candidate[phone]", phone.to_string());
    }

    // Preferred work location (when the offer asks).
    if let Some(locs) = offer.get("locations").and_then(|v| v.as_array()) {
        if let Some(id) = locs
            .iter()
            .find(|l| {
                l.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains("remote")
            })
            .or_else(|| locs.first())
            .and_then(|l| l.get("id"))
            .and_then(|v| v.as_i64())
        {
            form = form.text("candidate[location_ids][]", id.to_string());
        }
    }

    for (i, ans) in answers.iter().enumerate() {
        let qid = ans
            .get("open_question_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        form = form.text(
            format!("candidate[open_question_answers_attributes][{i}][open_question_id]"),
            qid.to_string(),
        );
        if let Some(flag) = ans.get("flag") {
            form = form.text(
                format!("candidate[open_question_answers_attributes][{i}][flag]"),
                flag.to_string(),
            );
        }
        if let Some(content) = ans.get("content").and_then(|v| v.as_str()) {
            form = form.text(
                format!("candidate[open_question_answers_attributes][{i}][content]"),
                content.to_string(),
            );
        }
        if let Some(arr) = ans.get("multi_content").and_then(|v| v.as_array()) {
            for (j, c) in arr.iter().enumerate() {
                if let Some(s) = c.as_str() {
                    form = form.text(
                        format!(
                            "candidate[open_question_answers_attributes][{i}][multi_content][{j}]"
                        ),
                        s.to_string(),
                    );
                }
            }
        }
    }

    let cv_part = Part::bytes(cv_bytes)
        .file_name(filename)
        .mime_str("application/pdf")
        .context("cv mime")?;
    form = form.part("candidate[cv]", cv_part);

    let post_url = format!(
        "{}/offers/{}/candidates?async=true",
        target.api_base, target.slug
    );
    let resp = client
        .post(&post_url)
        .multipart(form)
        .send()
        .await
        .context("POST candidate")?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if status.is_success() {
        return Ok(ApplyResult {
            submitted: true,
            note: format!("recruitee-http OK ({status}) slug={}", target.slug),
        });
    }

    let lower = body.to_ascii_lowercase();
    if lower.contains("already applied") || lower.contains("already submitted") {
        return Ok(ApplyResult {
            submitted: true,
            note: format!("recruitee-http already applied ({status})"),
        });
    }
    if lower.contains("captcha") {
        return Ok(ApplyResult {
            submitted: false,
            note: "manual: recruitee captcha required — apply with Chrome local or by hand"
                .into(),
        });
    }

    Err(anyhow!(
        "recruitee-http failed ({status}): {}",
        crate::style::truncate(&body, 400)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tether_and_recruitee_hosts() {
        let t = parse_target("https://careers.tether.io/o/senior-backend-developer-norway")
            .unwrap();
        assert_eq!(t.slug, "senior-backend-developer-norway");
        assert!(t.api_base.contains("careers.tether.io"));

        let t = parse_target("https://acme.recruitee.com/o/backend-engineer/c/new").unwrap();
        assert_eq!(t.slug, "backend-engineer");
    }
}
