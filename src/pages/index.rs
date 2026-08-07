use crate::db::{EventRow, Job, Settings};
use resuma::prelude::*;

#[load]
async fn queue_settings(_req: &FlowRequest) -> Settings {
    crate::db::get_settings()
        .await
        .unwrap_or_else(|_| Settings::fallback())
}

#[load]
async fn queue_jobs(_req: &FlowRequest) -> Vec<Job> {
    crate::db::list_jobs(80).await.unwrap_or_default()
}

#[load]
async fn queue_events(_req: &FlowRequest) -> Vec<EventRow> {
    crate::db::list_events(25).await.unwrap_or_default()
}

fn status_badge(status: &str) -> View {
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

fn is_actionable(job: &Job) -> bool {
    matches!(job.status.as_str(), "manual" | "ready" | "ready_draft")
}

fn job_row(j: Job) -> View {
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
    let badge = status_badge(&j.status);
    let pitch = draft_pitch(&j.draft_json);
    let detail = format!("/jobs/{}", j.id);
    let err = j.last_error.clone().unwrap_or_default();
    let show_mark = matches!(j.status.as_str(), "manual" | "ready");
    let id = j.id.to_string();
    view! {
        <tr>
            <td>{badge}<div class="muted">{j.source}</div></td>
            <td>
                <strong>{title}</strong>
                <div class="muted">{format!("{company} — {}", j.location)}</div>
                <div class="muted">{hint}</div>
                {if !pitch.is_empty() {
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
                {if show_mark {
                    view! {
                        {" · "}
                        <Form submit={crate::mark_job_applied}>
                            <input type="hidden" name="id" value={id} />
                            <button class="btn btn-ghost" type="submit" style="display:inline;padding:0;border:0;background:none;color:inherit;text-decoration:underline;cursor:pointer;font:inherit">"mark applied"</button>
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
    load_all3(
        use_queue_settings_load(),
        use_queue_jobs_load(),
        use_queue_events_load(),
        |s, jobs, events| render_queue(s, jobs, events),
        |err| error_page(&FlowError::Loader(err)),
        || View::empty(),
    )
}

fn render_queue(s: Settings, jobs: Vec<Job>, events: Vec<EventRow>) -> View {
    let running = s.worker_running != 0;
    let auto_apply = s.auto_apply != 0;
    let chrome_hint = std::env::var("JOBBOT_CHROME_CDP").ok().filter(|u| !u.trim().is_empty());
    let discovered = jobs.iter().filter(|j| j.status == "discovered").count();
    let scoring = jobs
        .iter()
        .filter(|j| matches!(j.status.as_str(), "scoring" | "drafting" | "applying" | "ready_draft"))
        .count();
    let ready = jobs
        .iter()
        .filter(|j| matches!(j.status.as_str(), "ready" | "manual"))
        .count();
    // Honest counts: HTTP only while still `ready` (not blocked manuals).
    let http_ready = jobs
        .iter()
        .filter(|j| {
            j.status == "ready"
                && crate::sources::web3_career::is_http_auto_applyable_url(
                    j.apply_url.as_deref().unwrap_or(""),
                )
                && !ats_blocked_manual(j)
        })
        .count();
    let kit_only = ready.saturating_sub(http_ready);
    let applied = jobs.iter().filter(|j| j.status == "applied").count();
    let skipped = jobs.iter().filter(|j| j.status == "skipped").count();

    let apply_banner = if !auto_apply {
        "Auto-apply is OFF. Turn it on in Settings (or JOBBOT_AUTO_APPLY=true). Recruitee can submit over HTTP without Chrome; Greenhouse/Ashby need local Chrome CDP."
            .to_string()
    } else if chrome_hint.is_none() {
        "Auto-apply ON (HTTP). Recruitee without video/captcha submits from Fly. Manual jobs: open draft → Download CV PDF + paste kit, then Mark applied."
            .to_string()
    } else {
        "Auto-apply ON — HTTP Recruitee first, then Chrome. Manual jobs have CV + paste kit on the draft page."
            .to_string()
    };

    let live = events
        .into_iter()
        .map(|e| {
            let job = e
                .job_id
                .map(|id| format!("#{id} "))
                .unwrap_or_default();
            format!("[{}] {job}{} — {}\n", e.created_at, e.level, e.message)
        })
        .collect::<String>();

    let actionable: Vec<Job> = jobs.iter().filter(|j| is_actionable(j)).cloned().collect();
    let rest: Vec<Job> = jobs.into_iter().filter(|j| !is_actionable(&j)).collect();

    let action_rows = actionable.into_iter().map(job_row).collect::<Vec<_>>();
    let rest_rows = rest.into_iter().take(40).map(job_row).collect::<Vec<_>>();

    view! {
        <div>
            {loader_poll("/", 8_000)}
            <div class="card">
                <h1>"Application queue"</h1>
                <p class="muted">
                    "Live worker feed below. Scores/drafts poll via Resuma loader_poll while this tab is open. Each ready job gets a tailored pitch + CV bullets."
                </p>
                <p class="muted">{apply_banner}</p>
                <div class="row">
                    <span class="status-pill">
                        <span class={if running { "dot on" } else { "dot" }}></span>
                        {if running { "Worker running" } else { "Worker paused" }}
                    </span>
                    <span class="muted">
                        {format!(
                            "queue: {discovered} new · {scoring} in-flight · {ready} ready/manual ({http_ready} HTTP pending · {kit_only} kit) · {applied} applied · {skipped} skipped · auto-apply={}",
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
            </div>

            <div class="card">
                <h2>"Live activity"</h2>
                <div class="log live">{if live.is_empty() { "Waiting for worker events…".into() } else { live }}</div>
            </div>

            <div class="card">
                <h2>"Needs you"</h2>
                <p class="muted">"Manual / ready jobs — download kit, apply, then mark applied. HTTP-pending stays here until the worker submits."</p>
                {if action_rows.is_empty() {
                    view! { <p class="muted">"Nothing waiting — discover or wait for new matches."</p> }
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
                                {action_rows}
                            </tbody>
                        </table>
                    }
                }}
            </div>

            <div class="card">
                <h2>"Rest of queue"</h2>
                <p class="muted">"Applied, skipped, failed — noise kept out of the way."</p>
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
