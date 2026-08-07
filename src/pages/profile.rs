use crate::db::{ProfileEventRow, ProfileSuggestion, Settings};
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

fn render_profile(
    s: Settings,
    suggestions: Vec<ProfileSuggestion>,
    events: Vec<ProfileEventRow>,
) -> View {
    let running = s.profile_worker_running != 0;
    let live = events
        .into_iter()
        .map(|e| format!("[{}] {} — {}\n", e.created_at, e.level, e.message))
        .collect::<String>();

    let cards = suggestions
        .into_iter()
        .map(|sug| {
            let platform = sug.platform.clone();
            let title = sug.title.clone();
            let body = sug.body.clone();
            let pri = priority_label(sug.priority);
            let status = sug.status.clone();
            let id = sug.id.to_string();
            view! {
                <div class="card" style="margin-top:0.75rem">
                    <div class="row" style="justify-content:space-between;align-items:baseline">
                        <h3 style="margin:0">{format!("{platform} · {title}")}</h3>
                        <span class="muted">{format!("{pri} · {status}")}</span>
                    </div>
                    <pre class="pitch" style="white-space:pre-wrap;margin:0.75rem 0">{body}</pre>
                    <div class="row">
                        <Form submit={crate::keep_profile_suggestion}>
                            <input type="hidden" name="id" value={id.clone()} />
                            <button class="btn btn-primary" type="submit">"Keep"</button>
                        </Form>
                        <Form submit={crate::dismiss_profile_suggestion}>
                            <input type="hidden" name="id" value={id} />
                            <button class="btn btn-ghost" type="submit">"Dismiss"</button>
                        </Form>
                    </div>
                </div>
            }
        })
        .collect::<Vec<_>>();

    view! {
        <div>
            {loader_poll("/profile", 10_000)}
            <div class="card">
                <h1>"Profile coach"</h1>
                <p class="muted">
                    "Connected to the apply queue: drafts teach the coach, kept bios/headlines shape new pitches. Offline: both workers auto-start on Fly. With GITHUB_TOKEN (scopes repo + user), bio and topics auto-push. LinkedIn About/headline land in Profile notes. Positioning: remote worldwide."
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

            <div class="card">
                <h2>"Coach activity"</h2>
                <div class="log live">{if live.is_empty() { "No profile events yet — hit Analyze now.".into() } else { live }}</div>
            </div>

            <div>
                <h2 style="margin:1rem 0 0.25rem">"Open suggestions"</h2>
                <p class="muted">"Only open items appear here. Apply all pending pushes GitHub bio/topics (skips Norway copy), saves LinkedIn into notes, and clears the rest so the coach can continue. Keep = prefer this style; Dismiss = avoid."</p>
                {if cards.is_empty() {
                    view! { <div class="card muted">"Queue clear — coach can analyze again."</div> }
                } else {
                    view! { <div>{cards}</div> }
                }}
            </div>
        </div>
    }
}
