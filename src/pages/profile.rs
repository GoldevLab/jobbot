use crate::db::{ProfileEventRow, ProfileSuggestion, Settings};
use crate::style;
use resuma::prelude::*;

#[load]
async fn profile_settings(_req: &FlowRequest) -> Settings {
    crate::db::get_settings()
        .await
        .unwrap_or_else(|_| Settings::fallback())
}

#[load]
async fn profile_suggestions(_req: &FlowRequest) -> Vec<ProfileSuggestion> {
    crate::db::list_profile_suggestions(40)
        .await
        .unwrap_or_default()
}

#[load]
async fn profile_events(_req: &FlowRequest) -> Vec<ProfileEventRow> {
    crate::db::list_profile_events(30)
        .await
        .unwrap_or_default()
}

pub fn page(_req: FlowRequest) -> View {
    load_all3(
        use_profile_settings_load(),
        use_profile_suggestions_load(),
        use_profile_events_load(),
        |s, suggestions, events| render_profile(s, suggestions, events),
        |err| error_page(&FlowError::Loader(err)),
        || View::empty(),
    )
}

fn priority_label(p: i64) -> &'static str {
    match p {
        1 => "urgent",
        3 => "nice",
        _ => "normal",
    }
}

fn apply_hint(platform: &str, title: &str) -> (&'static str, &'static str) {
    match style::profile_suggestion_kind(title) {
        Some("bio") | Some("topic") if platform == "github" => ("badge badge-ok", "auto"),
        Some("headline") | Some("about") => ("badge badge-info", "paste"),
        Some("pin") | Some("readme") => ("badge badge-warn", "manual"),
        _ => ("badge", "review"),
    }
}

fn collapse_activity(events: &[ProfileEventRow]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < events.len() {
        let msg = events[i].message.as_str();
        let level = events[i].level.as_str();
        let mut n = 1;
        while i + n < events.len()
            && events[i + n].message == msg
            && events[i + n].level == level
        {
            n += 1;
        }
        let ts = &events[i].created_at;
        if n > 1 {
            out.push_str(&format!("[{ts}] {level} — {msg}  (×{n})\n"));
        } else {
            out.push_str(&format!("[{ts}] {level} — {msg}\n"));
        }
        i += n;
    }
    out
}

fn render_profile(
    s: Settings,
    suggestions: Vec<ProfileSuggestion>,
    events: Vec<ProfileEventRow>,
) -> View {
    let running = s.profile_worker_running != 0;
    let event_count = events.len();
    let live = collapse_activity(&events);
    let blocking = suggestions
        .iter()
        .filter(|sug| {
            style::profile_suggestion_kind(&sug.title).is_some_and(style::is_blocking_kind)
        })
        .count();
    let open_n = suggestions.len();

    let cards = suggestions
        .into_iter()
        .map(|sug| {
            let platform = sug.platform.clone();
            let title = sug.title.clone();
            let body = sug.body.clone();
            let pri = priority_label(sug.priority);
            let (hint_cls, hint) = apply_hint(&platform, &title);
            let id = sug.id.to_string();
            view! {
                <div class="card" style="margin-top:0.75rem">
                    <div class="row" style="justify-content:space-between;align-items:baseline">
                        <h3 style="margin:0">{format!("{platform} · {title}")}</h3>
                        <span class="row" style="margin:0;gap:0.4rem">
                            <span class={hint_cls.to_string()}>{hint}</span>
                            <span class="muted">{pri}</span>
                        </span>
                    </div>
                    <pre class="pitch" style="white-space:pre-wrap;margin:0.75rem 0">{body.clone()}</pre>
                    <div class="row">
                        <Form submit={crate::keep_profile_suggestion}>
                            <input type="hidden" name="id" value={id.clone()} />
                            <button class="btn btn-primary" type="submit">"Keep"</button>
                        </Form>
                        <Form submit={crate::dismiss_profile_suggestion}>
                            <input type="hidden" name="id" value={id} />
                            <button class="btn btn-ghost" type="submit">"Dismiss"</button>
                        </Form>
                        <button
                            type="button"
                            class="btn btn-ghost"
                            data-copy={body}
                            onClick={js! {
                                const btn = event.currentTarget;
                                const t = btn.dataset.copy || "";
                                navigator.clipboard.writeText(t).then(() => {
                                    btn.textContent = "Copied";
                                }).catch(() => {
                                    btn.textContent = "Copy failed";
                                });
                            }}
                        >"Copy"</button>
                    </div>
                </div>
            }
        })
        .collect::<Vec<_>>();

    let queue_hint = if open_n == 0 {
        "Queue clear — coach can analyze again.".to_string()
    } else if blocking > 0 {
        format!(
            "{open_n} open · {blocking} copy card(s) need Keep/Dismiss (headline / About / bio). Pin and README are checklists and no longer freeze the coach."
        )
    } else {
        format!(
            "{open_n} open checklist(s). Coach keeps running — Keep to remember the style, or Dismiss."
        )
    };

    view! {
        <div>
            {loader_poll("/profile", 10_000)}
            <div class="card">
                <h1>"Profile coach"</h1>
                <p class="muted">
                    "Drafts teach the coach; kept bios/headlines shape new pitches. GitHub bio/topics auto-push with GITHUB_TOKEN. LinkedIn About/headline land in Profile notes. Pin order uses live repo names only. Positioning: remote worldwide."
                </p>
                <div class="row">
                    <span class="status-pill">
                        <span class={if running { "dot on" } else { "dot" }}></span>
                        {if running { "Coach running" } else { "Coach paused" }}
                    </span>
                    <span class="muted">
                        {format!("targets: {} · {}", s.github, s.linkedin)}
                    </span>
                </div>
                <div class="row">
                    {if running {
                        view! {
                            <Form submit={crate::stop_profile_worker}>
                                <button class="btn btn-danger" type="submit">"Stop coach"</button>
                            </Form>
                        }
                    } else {
                        view! {
                            <Form submit={crate::start_profile_worker}>
                                <button class="btn btn-primary" type="submit">"Run coach"</button>
                            </Form>
                        }
                    }}
                    <Form submit={crate::analyze_profiles_now}>
                        <button class="btn" type="submit">"Analyze now"</button>
                    </Form>
                    <Form submit={crate::apply_all_profile_suggestions}>
                        <button class="btn btn-primary" type="submit">"Apply all pending"</button>
                    </Form>
                    <Form submit={crate::dismiss_all_profile_suggestions}>
                        <button class="btn btn-ghost" type="submit">"Dismiss all"</button>
                    </Form>
                    <a class="btn btn-ghost" href="/settings">"Settings / notes"</a>
                    <a class="btn btn-ghost" href="/">"Queue"</a>
                </div>
            </div>

            <div>
                <h2 style="margin:1rem 0 0.25rem">"Open suggestions"</h2>
                <p class="muted">{queue_hint}</p>
                {if cards.is_empty() {
                    view! { <div class="card muted">"Nothing waiting — hit Analyze now or let the loop run."</div> }
                } else {
                    view! { <div>{cards}</div> }
                }}
            </div>

            <details class="card activity-fold" open="">
                <summary>
                    <h2 style="display:inline;margin:0">"Coach activity"</h2>
                    <span class="muted">{format!(" · {event_count} recent")}</span>
                </summary>
                <div class="log live">{if live.is_empty() { "No profile events yet — hit Analyze now.".into() } else { live }}</div>
            </details>
        </div>
    }
}
