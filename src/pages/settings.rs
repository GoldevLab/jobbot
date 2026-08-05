use crate::db::Settings;
use resuma::prelude::*;

#[load]
async fn settings_page(_req: &FlowRequest) -> Settings {
    crate::db::get_settings()
        .await
        .unwrap_or_else(|_| Settings::fallback())
}

pub fn page(_req: FlowRequest) -> View {
    load_boundary(
        use_settings_page_load(),
        |s| render_settings(s),
        |err| error_page(&FlowError::Loader(err)),
        || View::empty(),
    )
}

fn render_settings(s: Settings) -> View {
    let auto_on = s.auto_apply != 0;
    let auto_input = if auto_on {
        view! {
            <label style="display:flex;gap:0.5rem;align-items:center;margin:0">
                <input type="checkbox" name="auto_apply" value="on" checked="checked" />
                "Auto-apply when draft is ready"
            </label>
        }
    } else {
        view! {
            <label style="display:flex;gap:0.5rem;align-items:center;margin:0">
                <input type="checkbox" name="auto_apply" value="on" />
                "Auto-apply when draft is ready"
            </label>
        }
    };

    view! {
        <div class="card">
            <h1>"Settings"</h1>
            <p class="muted">"Profile used when filling Recruitee forms. CV path must be absolute on this machine."</p>
            <Form submit={crate::save_settings}>
                <div class="grid2">
                    <div>
                        <label>"Full name"</label>
                        <input type="text" name="full_name" value={s.full_name} />
                    </div>
                    <div>
                        <label>"Email"</label>
                        <input type="email" name="email" value={s.email} />
                    </div>
                    <div>
                        <label>"Phone"</label>
                        <input type="text" name="phone" value={s.phone} />
                    </div>
                    <div>
                        <label>"Country (work from)"</label>
                        <input type="text" name="country" value={s.country} />
                    </div>
                    <div>
                        <label>"LinkedIn"</label>
                        <input type="text" name="linkedin" value={s.linkedin} />
                    </div>
                    <div>
                        <label>"GitHub / links"</label>
                        <input type="text" name="github" value={s.github} />
                    </div>
                    <div>
                        <label>"Expected salary USD"</label>
                        <input type="text" name="expected_salary_usd" value={s.expected_salary_usd} />
                    </div>
                    <div>
                        <label>"Rate limit seconds"</label>
                        <input type="text" name="rate_limit_secs" value={s.rate_limit_secs.to_string()} />
                    </div>
                </div>
                <label>"CV path"</label>
                <input type="text" name="cv_path" value={s.cv_path} />
                <label>"Keywords (comma)"</label>
                <input type="text" name="keywords" value={s.keywords} />
                <label>"Locations (comma) — leave empty for all countries"</label>
                <input type="text" name="locations" value={s.locations} placeholder="e.g. remote,norway — or blank = worldwide" />
                <label>"Profile notes (paste LinkedIn About / headline for the coach)"</label>
                <textarea name="profile_notes" rows="6">{s.profile_notes}</textarea>
                <div class="row" style="margin-top:0.9rem">
                    {auto_input}
                </div>
                <div class="row">
                    <button class="btn btn-primary" type="submit">"Save"</button>
                    <a class="btn btn-ghost" href="/profile">"Profile coach"</a>
                    <a class="btn btn-ghost" href="/">"Back"</a>
                </div>
            </Form>
        </div>
    }
}
