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

fn render_job(j: Job) -> View {
    let draft: serde_json::Value = j
        .draft_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::json!({}));

    let pitch = draft
        .get("pitch")
        .and_then(|v| v.as_str())
        .unwrap_or("(no pitch yet — wait for drafting)")
        .to_string();
    let why = draft
        .get("why_company")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let cover = draft
        .get("cover_note")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
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
    let file_hint = format!("data/drafts/{}.md", j.id);

    view! {
        <div class="card">
            <a class="btn btn-ghost" href="/">"← Queue"</a>
            <h1>{j.title.clone()}</h1>
            <p class="muted">{format!("{} — {} · status {} · score {}", j.company, j.location, j.status, score)}</p>
            <div class="row">
                <a class="btn btn-primary" href={url} target="_blank" rel="noreferrer">"Open apply page"</a>
            </div>

            <h2>"Pitch (tailored)"</h2>
            <p>{pitch}</p>

            <h2>"Emphasize"</h2>
            <p class="muted">{if emphasize.is_empty() { "—".into() } else { emphasize }}</p>

            <h2>"CV bullets for this role"</h2>
            <pre class="log">{if bullets.is_empty() { "Still drafting…".into() } else { bullets }}</pre>

            <h2>"Why this company"</h2>
            <p>{if why.is_empty() { "—".into() } else { why }}</p>

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

            <p class="muted">{format!("Also saved on disk: {file_hint}")}</p>
        </div>
    }
}
