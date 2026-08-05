use crate::db::EventRow;
use resuma::prelude::*;

#[load]
async fn log_events(_req: &FlowRequest) -> Vec<EventRow> {
    crate::db::list_events(120).await.unwrap_or_default()
}

pub fn page(_req: FlowRequest) -> View {
    load_boundary(
        use_log_events_load(),
        |events| {
            let lines = events
                .into_iter()
                .map(|e| {
                    let job = e
                        .job_id
                        .map(|id| format!("job#{id} "))
                        .unwrap_or_default();
                    format!("[{}] {}{} — {}\n", e.created_at, job, e.level, e.message)
                })
                .collect::<String>();

            view! {
                <div class="card">
                    <h1>"Logs"</h1>
                    <p class="muted">"Recent worker events (newest first)."</p>
                    <div class="log">{lines}</div>
                    <div class="row">
                        <a class="btn" href="/logs">"Refresh"</a>
                        <a class="btn btn-ghost" href="/">"Queue"</a>
                    </div>
                </div>
            }
        },
        |err| error_page(&FlowError::Loader(err)),
        || View::empty(),
    )
}
