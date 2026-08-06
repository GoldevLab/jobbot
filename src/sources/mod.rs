pub mod apply_common;
pub mod ashby;
pub mod greenhouse;
pub mod recruitee;
pub mod recruitee_http;
pub mod web3_career;

use crate::browser::ChromeSession;
use crate::db::Settings;
use anyhow::Result;
use apply_common::{classify_ats, ApplyResult, AtsKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredJob {
    pub source: String,
    pub external_id: String,
    pub title: String,
    pub company: String,
    pub location: String,
    pub url: String,
    pub apply_url: Option<String>,
    pub description: String,
}

/// HTTP-first apply (Fly-safe). Returns Some when handled (applied / needs manual / error note).
/// None means caller should try Chrome CDP.
pub async fn apply_http_first(
    apply_url: &str,
    settings: &Settings,
    draft: &Value,
    cv_path: &str,
) -> Option<Result<ApplyResult>> {
    match classify_ats(apply_url) {
        AtsKind::Recruitee => Some(
            recruitee_http::apply_http(apply_url, settings, draft, cv_path).await,
        ),
        _ => None,
    }
}

/// Route to the right ATS applier (Chrome CDP).
pub async fn apply_with_draft(
    chrome: &ChromeSession,
    apply_url: &str,
    settings: &Settings,
    draft: &Value,
    cv_path: &str,
) -> Result<ApplyResult> {
    match classify_ats(apply_url) {
        AtsKind::Recruitee => {
            recruitee::apply_with_draft(chrome, apply_url, settings, draft, cv_path).await
        }
        AtsKind::Greenhouse => {
            greenhouse::apply_with_draft(chrome, apply_url, settings, draft, cv_path).await
        }
        AtsKind::Ashby => {
            ashby::apply_with_draft(chrome, apply_url, settings, draft, cv_path).await
        }
        AtsKind::Lever => Ok(ApplyResult {
            submitted: false,
            note: "manual: Lever auto-apply not implemented yet".into(),
        }),
        AtsKind::Unknown => Ok(ApplyResult {
            submitted: false,
            note: "manual: unsupported ATS host".into(),
        }),
    }
}
