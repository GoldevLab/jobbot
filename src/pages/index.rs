use crate::db::{EventRow, Funnel, Job, Settings};
use crate::fit;
use resuma::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct QueueSnapshot {
    settings: Settings,
    today: Vec<Job>,
    followups: Vec<Job>,
    rest: Vec<Job>,
    funnel: Funnel,
    events: Vec<EventRow>,
}

#[load]
async fn queue_page(_req: &FlowRequest) -> QueueSnapshot {
    let settings = crate::db::get_settings()
        .await
        .unwrap_or_else(|_| Settings::fallback());
    let today = crate::db::list_today_queue(8).await.unwrap_or_default();
    let followups = crate::db::list_followup_jobs(12).await.unwrap_or_default();
    let skip: std::collections::HashSet<i64> = today
        .iter()
        .chain(followups.iter())
        .map(|j| j.id)
        .collect();
    let rest = crate::db::list_jobs(80)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|j| !skip.contains(&j.id))
        .take(40)
        .collect();
    let funnel = crate::db::funnel_counts().await.unwrap_or_default();
    let events = crate::db::list_events(25).await.unwrap_or_default();
    QueueSnapshot {
        settings,
        today,
        followups,
        rest,
        funnel,
        events,
    }
}

fn status_badge(status: &str, outcome: Option<&str>) -> View {
    if let Some(o) = outcome.filter(|s| !s.is_empty()) {
        let (cls, label) = match o {
            "replied" => ("badge badge-info", "replied"),
            "interview" => ("badge badge-ok", "interview"),
            "rejected" => ("badge badge-danger", "rejected"),
            "ghost" => ("badge", "ghost"),
            _ => ("badge", o),
        };
        return view! { <span class={cls.to_string()}>{label}</span> };
    }
    let (cls, label) = match status {
        "discovered" => ("badge badge-info", "discovered"),
        "scoring" | "drafting" | "applying" => ("badge badge-warn", status),
        "ready_draft" | "ready" => ("badge badge-info", status),
        "manual" => ("badge badge-warn", "manual"),
        "applied" => ("badge badge-ok", "applied"),
        "skipped" => ("badge", "skipped"),
        "failed" => ("badge badge-danger", "failed"),
        _ => ("badge", status),
    };
    view! { <span class={cls.to_string()}>{label}</span> }
}

fn draft_pitch(draft_json: &Option<String>) -> String {
    let Some(raw) = draft_json else {
        return String::new();
    };
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| {
            v.get("pitch")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    v.get("why_company")
                        .and_then(|x| x.as_str())
                        .map(|s| s.chars().take(140).collect())
                })
        })
        .unwrap_or_default()
}

fn ats_blocked_manual(job: &Job) -> bool {
    let err = job.last_error.as_deref().unwrap_or("").to_ascii_lowercase();
    err.contains("video")
        || err.contains("captcha")
        || err.contains("required file")
        || err.contains("multi_file")
        || err.contains("manual:")
}

fn apply_hint(job: &Job) -> &'static str {
    let u = job.apply_url.as_deref().unwrap_or("");
    let http = crate::sources::web3_career::is_http_auto_applyable_url(u);
    let chrome = crate::sources::web3_career::is_auto_applyable_url(u);
    if job.status == "manual" || ats_blocked_manual(job) {
        if ats_blocked_manual(job) && http {
            "manual — ATS needs video/file (use kit)"
        } else if u.is_empty() || u.contains("web3.career") {
            "manual — download kit.zip"
        } else {
            "manual — kit.zip + paste"
        }
    } else if job.status == "ready" && http {
        "ATS — HTTP auto-apply pending"
    } else if chrome {
        "ATS — needs Chrome local"
    } else if u.is_empty() || u.contains("web3.career") {
        "manual — download kit.zip"
    } else {
        "manual — kit.zip + paste"
    }
}

fn job_row(j: Job, follow_up: bool) -> View {
    let title = j.title.clone();
    let company = if j.company.is_empty() {
        "—".into()
    } else {
        j.company.clone()
    };
    let score = j
        .score
        .map(|x| format!("{x:.0}"))
        .unwrap_or_else(|| "—".into());
    let url = j.apply_url.clone().unwrap_or(j.url.clone());
    let hint = apply_hint(&j);
    let badge = status_badge(&j.status, j.outcome.as_deref());
    let pitch = draft_pitch(&j.draft_json);
    let detail = format!("/jobs/{}", j.id);
    let err = j.last_error.clone().unwrap_or_default();
    let show_mark = matches!(j.status.as_str(), "manual" | "ready" | "failed");
    let id = j.id.to_string();
    let note = if follow_up {
        fit::follow_up_note(&j.title, &j.company, &pitch)
    } else {
        String::new()
    };
    view! {
        <tr>
            <td>{badge}<div class="muted">{j.source}</div></td>
            <td>
                <strong>{title}</strong>
                <div class="muted">{format!("{company} — {}", j.location)}</div>
                <div class="muted">{hint}</div>
                {if follow_up && !note.is_empty() {
                    view! { <pre class="log follow-note">{note}</pre> }
                } else if !pitch.is_empty() {
                    view! { <div class="pitch">{pitch}</div> }
                } else if !err.is_empty() && (j.status == "failed" || j.status == "manual") {
                    view! { <div class="muted">{crate::style::truncate(&err, 120)}</div> }
                } else {
                    View::empty()
                }}
            </td>
            <td>{score}</td>
            <td class="row-actions">
                <a href={detail.clone()}>"draft"</a>
                {" · "}
                <a href={format!("/jobs/{}/kit.zip", j.id)}>"zip"</a>
                {" · "}
                <a href={format!("/jobs/{}/cv.pdf", j.id)}>"cv"</a>
                {" · "}
                <a href={url} target="_blank" rel="noreferrer">"open"</a>
                {if follow_up {
                    view! {
                        {" · "}
                        <Form submit={crate::mark_job_followed_up}>
                            <input type="hidden" name="id" value={id.clone()} />
                            <button class="btn btn-ghost linkish" type="submit">"followed up"</button>
                        </Form>
                    }
                } else {
                    View::empty()
                }}
                {if show_mark && !follow_up {
                    view! {
                        {" · "}
                        <Form submit={crate::mark_job_applied}>
                            <input type="hidden" name="id" value={id} />
                            <button class="btn btn-ghost linkish" type="submit">"mark applied"</button>
                        </Form>
                    }
                } else {
                    View::empty()
                }}
            </td>
        </tr>
    }
}

pub fn page(_req: FlowRequest) -> View {
    load_boundary(
        use_queue_page_load(),
        |snap| render_queue(snap),
        |err| error_page(&FlowError::Loader(err)),
        || View::empty(),
    )
}

fn render_queue(snap: QueueSnapshot) -> View {
    let s = snap.settings;
    let running = s.worker_running != 0;
    let auto_apply = s.auto_apply != 0;
    let chrome_hint = std::env::var("JOBBOT_CHROME_CDP")
        .ok()
        .filter(|u| !u.trim().is_empty());
    let f = snap.funnel;

    let apply_banner = if !auto_apply {
        "Auto-apply is OFF. Turn it on in Settings (or JOBBOT_AUTO_APPLY=true). Recruitee can submit over HTTP without Chrome; Greenhouse/Ashby need local Chrome CDP."
            .to_string()
    } else if chrome_hint.is_none() {
        "Auto-apply ON (HTTP). Recruitee without video/captcha submits from Fly. Today's queue: kit → apply → mark applied."
            .to_string()
    } else {
        "Auto-apply ON — HTTP Recruitee first, then Chrome. Today's queue is the work that still needs you."
            .to_string()
    };

    let live = snap
        .events
        .into_iter()
        .map(|e| {
            let job = e
                .job_id
                .map(|id| format!("#{id} "))
                .unwrap_or_default();
            format!("[{}] {job}{} — {}\n", e.created_at, e.level, e.message)
        })
        .collect::<String>();

    let today_empty = snap.today.is_empty();
    let follow_empty = snap.followups.is_empty();
    let today_rows = snap
        .today
        .into_iter()
        .map(|j| job_row(j, false))
        .collect::<Vec<_>>();
    let follow_rows = snap
        .followups
        .into_iter()
        .map(|j| job_row(j, true))
        .collect::<Vec<_>>();
    let rest_rows = snap
        .rest
        .into_iter()
        .map(|j| job_row(j, false))
        .collect::<Vec<_>>();

    view! {
        <div>
            {loader_poll("/", 8_000)}
            <div class="card">
                <h1>"Application queue"</h1>
                <p class="muted">
                    "Work the 8 jobs below, then paste any URL you find elsewhere. Funnel is recruiter reality, not scrape volume."
                </p>
                <p class="muted">{apply_banner}</p>
                <p class="funnel" aria-label="Application funnel">
                    {format!(
                        "funnel: {d} new · {dr} drafted · {a} applied · {r} replied · {i} interview · {rej} rejected · {g} ghost",
                        d = f.discovered,
                        dr = f.drafted,
                        a = f.applied,
                        r = f.replied,
                        i = f.interview,
                        rej = f.rejected,
                        g = f.ghost,
                    )}
                </p>
                <div class="row">
                    <span class="status-pill">
                        <span class={if running { "dot on" } else { "dot" }}></span>
                        {if running { "Worker running" } else { "Worker paused" }}
                    </span>
                    <span class="muted">
                        {format!(
                            "auto-apply={}",
                            if auto_apply { "on" } else { "off" }
                        )}
                    </span>
                </div>
                <div class="row">
                    {if running {
                        view! {
                            <Form submit={crate::stop_worker}>
                                <button class="btn btn-danger" type="submit">"Stop"</button>
                            </Form>
                        }
                    } else {
                        view! {
                            <Form submit={crate::start_worker}>
                                <button class="btn btn-primary" type="submit">"Run"</button>
                            </Form>
                        }
                    }}
                    <Form submit={crate::discover_now}>
                        <button class="btn" type="submit">"Discover now"</button>
                    </Form>
                    <a class="btn btn-ghost" href="/">"Refresh"</a>
                    <a class="btn" href="/profile">"Profile coach"</a>
                    <a class="btn btn-ghost" href="/settings">"Settings"</a>
                    <a class="btn btn-ghost" href="/logs">"Logs"</a>
                </div>
                <Form submit={crate::import_job_url}>
                    <div class="field">
                        <label for="import-url">"Paste job URL"</label>
                        <p id="import-url-help" class="hint">"LinkedIn, Wellfound, or a careers page. Imports into the queue for scoring/draft."</p>
                        <div class="row">
                            <input
                                id="import-url"
                                name="url"
                                type="url"
                                inputmode="url"
                                autocomplete="url"
                                enterkeyhint="go"
                                required=""
                                aria-describedby="import-url-help"
                                placeholder="https://"
                            />
                            <button class="btn btn-primary" type="submit">"Import URL"</button>
                        </div>
                    </div>
                </Form>
            </div>

            <div class="card">
                <h2>"Today — apply these"</h2>
                <p class="muted">"Top 8 by score. Download kit, submit, mark applied. Leave the rest."</p>
                {if today_empty {
                    view! { <p class="muted">"Nothing waiting — discover, import a URL, or wait for drafts."</p> }
                } else {
                    view! {
                        <table>
                            <thead>
                                <tr>
                                    <th>"Status"</th>
                                    <th>"Role / tailored pitch"</th>
                                    <th>"Score"</th>
                                    <th>"Links"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {today_rows}
                            </tbody>
                        </table>
                    }
                }}
            </div>

            <div class="card">
                <h2>"Follow up (7+ days)"</h2>
                <p class="muted">"Copy the note, send it, then mark followed up. No inbox — you paste."</p>
                {if follow_empty {
                    view! { <p class="muted">"No stale applies yet."</p> }
                } else {
                    view! {
                        <table>
                            <thead>
                                <tr>
                                    <th>"Status"</th>
                                    <th>"Role / follow-up note"</th>
                                    <th>"Score"</th>
                                    <th>"Links"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {follow_rows}
                            </tbody>
                        </table>
                    }
                }}
            </div>

            <details class="activity-fold">
                <summary>"Live activity"</summary>
                <div class="log live">{if live.is_empty() { "Waiting for worker events…".into() } else { live }}</div>
            </details>

            <div class="card">
                <h2>"Rest of queue"</h2>
                <p class="muted">"Applied, skipped, in-flight — kept out of the way."</p>
                <table>
                    <thead>
                        <tr>
                            <th>"Status"</th>
                            <th>"Role / tailored pitch"</th>
                            <th>"Score"</th>
                            <th>"Links"</th>
                        </tr>
                    </thead>
                    <tbody>
                        {rest_rows}
                    </tbody>
                </table>
            </div>
        </div>
    }
}
