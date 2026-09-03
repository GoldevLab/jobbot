use crate::db::Job;
use resuma::prelude::*;

#[load]
async fn job_detail(req: &FlowRequest) -> Option<Job> {
    let id = req.param("id").and_then(|s| s.parse::<i64>().ok())?;
    crate::db::get_job(id).await.ok().flatten()
}

pub fn page(_req: FlowRequest) -> View {
    load_boundary(
        use_job_detail_load(),
        |job| match job {
            Some(j) => render_job(j),
            None => view! {
                <div class="card">
                    <h1>"Job not found"</h1>
                    <a href="/">"Back"</a>
                </div>
            },
        },
        |err| error_page(&FlowError::Loader(err)),
        || View::empty(),
    )
}

fn draft_field(draft: &serde_json::Value, key: &str) -> String {
    draft
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn render_job(j: Job) -> View {
    let draft: serde_json::Value = j
        .draft_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::json!({}));

    let pitch = draft_field(&draft, "pitch");
    let why = draft_field(&draft, "why_company");
    let cover = draft_field(&draft, "cover_note");
    let node = draft_field(&draft, "node_experience");
    let databases = draft_field(&draft, "databases");
    let compliance = draft_field(&draft, "compliance_finance");
    let p2p = draft_field(&draft, "p2p");
    let salary = draft_field(&draft, "salary_usd");
    let country = draft_field(&draft, "country");
    let emphasize = draft
        .get("emphasize")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();
    let bullets = draft
        .get("cv_bullets")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|b| format!("• {b}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let url = j.apply_url.clone().unwrap_or(j.url.clone());
    let score = j
        .score
        .map(|x| format!("{x:.0}"))
        .unwrap_or_else(|| "—".into());
    let packet_href = format!("/jobs/{}/packet.txt", j.id);
    let cv_href = format!("/jobs/{}/cv.pdf", j.id);
    let zip_href = format!("/jobs/{}/kit.zip", j.id);
    let job_id = j.id.to_string();
    let is_manual = matches!(j.status.as_str(), "manual" | "ready" | "failed");
    let can_mark = matches!(j.status.as_str(), "manual" | "ready" | "failed");
    let err = j.last_error.clone().unwrap_or_default();
    let outcome_label = j
        .outcome
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_default();
    let show_follow = j.status == "applied" && j.followed_up_at.is_none();

    view! {
        <div class="card">
            <a class="btn btn-ghost" href="/">"← Queue"</a>
            <h1>{j.title.clone()}</h1>
            <p class="muted">{format!("{} — {} · status {} · score {}", j.company, j.location, j.status, score)}</p>

            {if !outcome_label.is_empty() {
                view! { <p class="muted">{format!("Outcome: {outcome_label}")}</p> }
            } else {
                View::empty()
            }}

            <div class="card outcome-card">
                <h2 style="margin-top:0">"After you applied"</h2>
                <p class="muted">"Track recruiter reality so scoring learns. Follow-up is for +7 days with no reply."</p>
                <div class="row">
                    <Form submit={crate::set_job_outcome}>
                        <input type="hidden" name="id" value={job_id.clone()} />
                        <input type="hidden" name="outcome" value="replied" />
                        <button class="btn" type="submit">"Replied"</button>
                    </Form>
                    <Form submit={crate::set_job_outcome}>
                        <input type="hidden" name="id" value={job_id.clone()} />
                        <input type="hidden" name="outcome" value="interview" />
                        <button class="btn btn-primary" type="submit">"Interview"</button>
                    </Form>
                    <Form submit={crate::set_job_outcome}>
                        <input type="hidden" name="id" value={job_id.clone()} />
                        <input type="hidden" name="outcome" value="rejected" />
                        <button class="btn btn-danger" type="submit">"Rejected"</button>
                    </Form>
                    <Form submit={crate::set_job_outcome}>
                        <input type="hidden" name="id" value={job_id.clone()} />
                        <input type="hidden" name="outcome" value="ghost" />
                        <button class="btn btn-ghost" type="submit">"Ghost"</button>
                    </Form>
                    {if show_follow {
                        view! {
                            <Form submit={crate::mark_job_followed_up}>
                                <input type="hidden" name="id" value={job_id.clone()} />
                                <button class="btn btn-ghost" type="submit">"Mark follow-up sent"</button>
                            </Form>
                        }
                    } else {
                        View::empty()
                    }}
                </div>
            </div>

            {if is_manual {
                view! {
                    <div class="card" style="margin:1rem 0;background:rgba(0,0,0,0.04)">
                        <h2 style="margin-top:0">"Manual apply kit"</h2>
                        <p class="muted">
                            "Bot cannot auto-submit this one. Download the ZIP (CV + paste kit), apply by hand, then mark applied."
                        </p>
                        <div class="row">
                            <a class="btn btn-primary" href={zip_href.clone()} download="kit.zip">"Download kit (.zip)"</a>
                            <a class="btn" href={cv_href.clone()} download="cv.pdf">"CV PDF"</a>
                            <a class="btn" href={packet_href.clone()} download="packet.txt">"Paste kit (.txt)"</a>
                            <a class="btn btn-ghost" href={url.clone()} target="_blank" rel="noreferrer">"Open apply page"</a>
                        </div>
                        {if can_mark {
                            view! {
                                <Form submit={crate::mark_job_applied}>
                                    <input type="hidden" name="id" value={job_id.clone()} />
                                    <button class="btn" type="submit">"Mark as applied"</button>
                                </Form>
                            }
                        } else {
                            View::empty()
                        }}
                        {if !err.is_empty() {
                            view! { <p class="muted">{format!("Note: {err}")}</p> }
                        } else {
                            View::empty()
                        }}
                    </div>
                }
            } else {
                view! {
                    <div class="row">
                        <a class="btn btn-primary" href={url.clone()} target="_blank" rel="noreferrer">"Open apply page"</a>
                        <a class="btn" href={zip_href.clone()} download="kit.zip">"Kit ZIP"</a>
                        <a class="btn" href={packet_href.clone()} download="packet.txt">"Paste kit"</a>
                        <a class="btn btn-ghost" href={cv_href.clone()} download="cv.pdf">"CV PDF"</a>
                    </div>
                }
            }}

            <h2>"Pitch (tailored)"</h2>
            <pre class="log">{if pitch.is_empty() { "Still drafting…".into() } else { pitch }}</pre>

            <h2>"Emphasize"</h2>
            <p class="muted">{if emphasize.is_empty() { "—".into() } else { emphasize }}</p>

            <h2>"CV bullets for this role"</h2>
            <pre class="log">{if bullets.is_empty() { "Still drafting…".into() } else { bullets }}</pre>

            <h2>"Why this company"</h2>
            <pre class="log">{if why.is_empty() { "—".into() } else { why }}</pre>

            {if !cover.is_empty() {
                view! {
                    <>
                        <h2>"Cover note"</h2>
                        <pre class="log">{cover}</pre>
                    </>
                }
            } else {
                View::empty()
            }}

            <h2>"Screening answers (paste)"</h2>
            <pre class="log">{format!(
                "Node experience: {}\nDatabases: {}\nCompliance/finance: {}\nP2P: {}\nCountry: {}\nSalary USD: {}",
                if node.is_empty() { "—" } else { &node },
                if databases.is_empty() { "—" } else { &databases },
                if compliance.is_empty() { "—" } else { &compliance },
                if p2p.is_empty() { "—" } else { &p2p },
                if country.is_empty() { "—" } else { &country },
                if salary.is_empty() { "—" } else { &salary },
            )}</pre>

            <p class="muted">
                {format!(
                    "Files: {} · {}",
                    packet_href, cv_href
                )}
            </p>
        </div>
    }
}
