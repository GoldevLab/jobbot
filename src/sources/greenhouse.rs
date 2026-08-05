//! Greenhouse boards via Chrome CDP (no Greenhouse SDK).

use super::apply_common::{
    self, click_apply_entry, click_submit, confirm_submission, current_url, fill_common_fields,
    looks_already_applied, maybe_upload_cv, page_text, wait_for_js, ApplyResult,
};
use crate::browser::ChromeSession;
use crate::db::Settings;
use anyhow::Result;
use serde_json::Value;

pub async fn apply_with_draft(
    chrome: &ChromeSession,
    apply_url: &str,
    settings: &Settings,
    draft: &Value,
    cv_path: &str,
) -> Result<ApplyResult> {
    let page = chrome.new_page(apply_url).await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let body = page_text(&page, 8000).await.unwrap_or_default();
    if looks_already_applied(&body) || apply_common::looks_success_text(&body) {
        return Ok(ApplyResult {
            submitted: true,
            note: "greenhouse page says already applied / submitted".into(),
        });
    }

    if click_apply_entry(&page).await? {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    let ready = wait_for_js(
        &page,
        r#"document.querySelector('#first_name, #application_form, input[name=first_name], input[id*=first], input[type=email]')"#,
        12_000,
    )
    .await?;
    if !ready {
        return Ok(ApplyResult {
            submitted: false,
            note: "greenhouse form did not appear (iframe/login?)".into(),
        });
    }

    // Prefer Greenhouse ids when present, then generic heuristics.
    let (first, last) = apply_common::split_name(&settings.full_name);
    let _ = page
        .evaluate(format!(
            r#"
            (() => {{
              const set = (sel, val) => {{
                const el = document.querySelector(sel);
                if (!el || !val) return;
                el.focus();
                el.value = String(val);
                el.dispatchEvent(new Event('input', {{bubbles:true}}));
                el.dispatchEvent(new Event('change', {{bubbles:true}}));
              }};
              set('#first_name', {first});
              set('#last_name', {last});
              set('#email', {email});
              set('#phone', {phone});
            }})()
            "#,
            first = serde_json::to_string(&first).unwrap(),
            last = serde_json::to_string(&last).unwrap(),
            email = serde_json::to_string(&settings.email).unwrap(),
            phone = serde_json::to_string(&settings.phone).unwrap(),
        ))
        .await;

    let filled = fill_common_fields(&page, settings, draft).await?;
    crate::db::log_event(None, "info", format!("greenhouse filled ~{filled} fields")).await;

    maybe_upload_cv(&page, cv_path).await;
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;

    let before = current_url(&page).await;
    let clicked = click_submit(&page).await?;
    let confirm = confirm_submission(&page, &before).await?;

    Ok(ApplyResult {
        submitted: confirm.is_confirmed(),
        note: format!("greenhouse: {}", confirm.note(clicked)),
    })
}
