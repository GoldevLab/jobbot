//! Recruitee / careers.* apply flow via Chrome CDP.

use super::apply_common::{
    click_submit, confirm_submission, current_url, fill_common_fields, looks_already_applied,
    looks_success_text, maybe_upload_cv, page_text, ApplyResult,
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

    let body_text = page_text(&page, 8000).await.unwrap_or_default();
    if looks_already_applied(&body_text) || looks_success_text(&body_text) {
        return Ok(ApplyResult {
            submitted: true,
            note: "recruitee page says already applied / submitted".into(),
        });
    }

    let filled = fill_common_fields(&page, settings, draft).await?;
    crate::db::log_event(None, "info", format!("recruitee filled ~{filled} fields")).await;

    maybe_upload_cv(&page, cv_path).await;
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;

    let before = current_url(&page).await;
    let clicked = click_submit(&page).await?;
    let confirm = confirm_submission(&page, &before).await?;

    Ok(ApplyResult {
        submitted: confirm.is_confirmed(),
        note: format!("recruitee: {}", confirm.note(clicked)),
    })
}
