//! Ashby jobs via Chrome CDP (no Ashby SDK).

use super::apply_common::{
    click_apply_entry, click_submit, confirm_submission, current_url, fill_common_fields,
    looks_already_applied, looks_success_text, maybe_upload_cv, page_text, wait_for_js, ApplyResult,
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
    if looks_already_applied(&body) || looks_success_text(&body) {
        return Ok(ApplyResult {
            submitted: true,
            note: "ashby page says already applied / submitted".into(),
        });
    }

    if click_apply_entry(&page).await? {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // Ashby is a SPA — wait for hydrated inputs.
    let ready = wait_for_js(
        &page,
        r#"document.querySelector('input[type=email], textarea, input[type=text], input:not([type])')"#,
        15_000,
    )
    .await?;
    if !ready {
        return Ok(ApplyResult {
            submitted: false,
            note: "ashby form did not hydrate".into(),
        });
    }

    let filled = fill_common_fields(&page, settings, draft).await?;
    crate::db::log_event(None, "info", format!("ashby filled ~{filled} fields")).await;

    maybe_upload_cv(&page, cv_path).await;
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;

    let before = current_url(&page).await;
    let clicked = click_submit(&page).await?;
    let confirm = confirm_submission(&page, &before).await?;

    Ok(ApplyResult {
        submitted: confirm.is_confirmed(),
        note: format!("ashby: {}", confirm.note(clicked)),
    })
}
