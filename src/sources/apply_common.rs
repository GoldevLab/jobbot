//! Shared CDP apply primitives (Rust + in-page JS only — no external ATS SDKs).

use crate::db::Settings;
use anyhow::{anyhow, Context, Result};
use chromiumoxide::page::Page;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub submitted: bool,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtsKind {
    Recruitee,
    Greenhouse,
    Ashby,
    Lever,
    Unknown,
}

impl AtsKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recruitee => "recruitee",
            Self::Greenhouse => "greenhouse",
            Self::Ashby => "ashby",
            Self::Lever => "lever",
            Self::Unknown => "unknown",
        }
    }

    pub fn auto_apply_supported(self) -> bool {
        matches!(self, Self::Recruitee | Self::Greenhouse | Self::Ashby)
    }
}

pub fn classify_ats(url: &str) -> AtsKind {
    let u = url.to_lowercase();
    if u.contains("recruitee") || u.contains("careers.tether") {
        AtsKind::Recruitee
    } else if u.contains("greenhouse") {
        AtsKind::Greenhouse
    } else if u.contains("ashbyhq") || u.contains("ashby.com") {
        AtsKind::Ashby
    } else if u.contains("lever.co") {
        AtsKind::Lever
    } else {
        AtsKind::Unknown
    }
}

pub fn split_name(full: &str) -> (String, String) {
    let t = full.trim();
    if t.is_empty() {
        return (String::new(), String::new());
    }
    if let Some((first, rest)) = t.split_once(' ') {
        (first.to_string(), rest.trim().to_string())
    } else {
        (t.to_string(), String::new())
    }
}

pub async fn page_text(page: &Page, max: usize) -> Result<String> {
    let js = format!(
        r#"(() => (document.body && document.body.innerText) ? document.body.innerText.slice(0, {max}) : "")()"#
    );
    Ok(page
        .evaluate(js)
        .await?
        .into_value::<String>()
        .unwrap_or_default())
}

pub async fn current_url(page: &Page) -> String {
    page.evaluate("(() => location.href)()")
        .await
        .ok()
        .and_then(|v| v.into_value().ok())
        .unwrap_or_default()
}

pub fn looks_already_applied(text: &str) -> bool {
    let l = text.to_lowercase();
    l.contains("already applied")
        || l.contains("you have already submitted")
        || l.contains("application has already been submitted")
}

pub fn looks_success_text(text: &str) -> bool {
    let l = text.to_lowercase();
    l.contains("thank you for applying")
        || l.contains("thanks for applying")
        || l.contains("application has been submitted")
        || l.contains("application submitted")
        || l.contains("successfully submitted")
        || l.contains("we received your application")
        || l.contains("thanks for your application")
        || (l.contains("thank you") && l.contains("appl"))
}

pub fn looks_captcha(text: &str) -> bool {
    let l = text.to_lowercase();
    l.contains("captcha") || l.contains("recaptcha") || l.contains("hcaptcha")
}

pub fn looks_validation_error(text: &str) -> bool {
    let l = text.to_lowercase();
    l.contains("is required")
        || l.contains("please fill")
        || l.contains("please complete")
        || l.contains("fix is invalid")
        || l.contains("fix contains errors")
}

/// Poll until `predicate` JS returns true, or timeout.
pub async fn wait_for_js(page: &Page, predicate_js: &str, timeout_ms: u64) -> Result<bool> {
    let step = 400u64;
    let mut waited = 0u64;
    while waited < timeout_ms {
        let ok = page
            .evaluate(format!("(() => {{ try {{ return !!({predicate_js}); }} catch (_) {{ return false; }} }})()"))
            .await
            .ok()
            .and_then(|v| v.into_value::<bool>().ok())
            .unwrap_or(false);
        if ok {
            return Ok(true);
        }
        tokio::time::sleep(std::time::Duration::from_millis(step)).await;
        waited += step;
    }
    Ok(false)
}

pub async fn fill_common_fields(page: &Page, settings: &Settings, draft: &Value) -> Result<i64> {
    let why = draft
        .get("why_company")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let databases = draft
        .get("databases")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let compliance = draft
        .get("compliance_finance")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let salary = draft
        .get("salary_usd")
        .and_then(|v| v.as_str())
        .unwrap_or(settings.expected_salary_usd.as_str());
    let country = draft
        .get("country")
        .and_then(|v| v.as_str())
        .unwrap_or(settings.country.as_str());
    let node = draft
        .get("node_experience")
        .and_then(|v| v.as_str())
        .unwrap_or("5_plus");
    let p2p = draft.get("p2p").and_then(|v| v.as_str()).unwrap_or("no");
    let (first, last) = split_name(&settings.full_name);

    let payload = serde_json::json!({
        "full_name": settings.full_name,
        "first_name": first,
        "last_name": last,
        "email": settings.email,
        "phone": settings.phone,
        "linkedin": settings.linkedin,
        "github": settings.github,
        "country": country,
        "salary": salary,
        "why": why,
        "databases": databases,
        "compliance": compliance,
        "node": node,
        "p2p": p2p,
    });

    let filled = page
        .evaluate(format!(
            r#"
            (() => {{
              const data = {};
              const norm = (s) => (s || '').toLowerCase().replace(/\s+/g, ' ').trim();
              const fields = Array.from(document.querySelectorAll('input, textarea, select'));
              const setVal = (el, val) => {{
                if (val == null || val === '') return false;
                const tag = el.tagName.toLowerCase();
                if (tag === 'select') {{
                  const opts = Array.from(el.options || []);
                  const n = norm(String(val));
                  let opt = opts.find(o => norm(o.text).includes(n) || norm(o.value).includes(n));
                  if (!opt && n.includes('5_plus')) opt = opts.find(o => /more than 5|5\+|over 5/i.test(o.text));
                  if (!opt && n.includes('3_5')) opt = opts.find(o => /3-5|3 – 5|3 to 5/i.test(o.text));
                  if (!opt && n.includes('1_3')) opt = opts.find(o => /1-3|1 – 3|1 to 3/i.test(o.text));
                  if (!opt && (n === 'yes' || n.startsWith('yes'))) opt = opts.find(o => /^yes$/i.test(o.text.trim()));
                  if (!opt && (n === 'no' || n.startsWith('no'))) opt = opts.find(o => /^no$/i.test(o.text.trim()));
                  if (opt) {{ el.value = opt.value; el.dispatchEvent(new Event('change', {{bubbles:true}})); return true; }}
                  return false;
                }}
                el.focus();
                el.value = String(val);
                el.dispatchEvent(new Event('input', {{bubbles:true}}));
                el.dispatchEvent(new Event('change', {{bubbles:true}}));
                return true;
              }};
              const labelFor = (el) => {{
                let t = '';
                if (el.id) {{
                  const lab = document.querySelector('label[for="' + el.id + '"]');
                  if (lab) t += ' ' + lab.innerText;
                }}
                const wrap = el.closest('label, .field, .form-group, [class*="question"], [class*="field"], [data-testid]');
                if (wrap) t += ' ' + wrap.innerText;
                t += ' ' + (el.name || '') + ' ' + (el.id || '') + ' ' + (el.placeholder || '') + ' ' + (el.getAttribute('aria-label') || '');
                return norm(t);
              }};
              let filled = 0;
              for (const el of fields) {{
                if (el.type === 'hidden' || el.type === 'file' || el.type === 'submit' || el.disabled) continue;
                const L = labelFor(el);
                if (!L) continue;
                if (/first.?name|fname/.test(L) && !/last/.test(L)) {{ if (setVal(el, data.first_name)) filled++; continue; }}
                if (/last.?name|lname|surname|family name/.test(L)) {{ if (setVal(el, data.last_name)) filled++; continue; }}
                if (/(full name|your name|^name$)/.test(L) && !/company|user|first|last/.test(L)) {{ if (setVal(el, data.full_name)) filled++; continue; }}
                if (/e-?mail/.test(L)) {{ if (setVal(el, data.email)) filled++; continue; }}
                if (/phone|mobile|tel/.test(L)) {{ if (setVal(el, data.phone)) filled++; continue; }}
                if (/linkedin/.test(L)) {{ if (setVal(el, data.linkedin)) filled++; continue; }}
                if (/github|portfolio|useful links|website|personal site/.test(L) && !/linkedin/.test(L)) {{ if (setVal(el, data.github)) filled++; continue; }}
                if (/salary|compensation|expected annual|pay expectation/.test(L)) {{ if (setVal(el, data.salary)) filled++; continue; }}
                if (/country|working from|from which country|location \(country\)/.test(L)) {{ if (setVal(el, data.country)) filled++; continue; }}
                if (/why.*interested|why.*working|motivation|why are you|cover letter|additional information/.test(L)) {{ if (setVal(el, data.why)) filled++; continue; }}
                if (/relational|non-relational|database/.test(L)) {{ if (setVal(el, data.databases)) filled++; continue; }}
                if (/compliance|financial|kyc|aml|payments|accounting/.test(L)) {{ if (setVal(el, data.compliance)) filled++; continue; }}
                if (/nodejs|node\.js|node js/.test(L)) {{ if (setVal(el, data.node)) filled++; continue; }}
                if (/peer-to-peer|\bp2p\b/.test(L)) {{
                  const yes = /^yes/i.test(String(data.p2p));
                  if (el.type === 'radio' || el.type === 'checkbox') {{
                    if ((yes && /yes/.test(L)) || (!yes && /\bno\b/.test(L))) {{ el.click(); filled++; }}
                  }} else if (setVal(el, yes ? 'Yes' : 'No')) filled++;
                  continue;
                }}
              }}
              return filled;
            }})()
            "#,
            serde_json::to_string(&payload).unwrap()
        ))
        .await
        .context("fill fields")?
        .into_value::<i64>()
        .unwrap_or(0);

    Ok(filled)
}

pub async fn upload_cv(page: &Page, cv_path: &str) -> Result<()> {
    use chromiumoxide::cdp::browser_protocol::dom::SetFileInputFilesParams;

    let el = page
        .find_element("input[type=file]")
        .await
        .context("file input")?;
    let params = SetFileInputFilesParams::builder()
        .files(vec![cv_path.to_string()])
        .backend_node_id(el.backend_node_id)
        .build()
        .map_err(|e| anyhow!(e))?;
    page.execute(params).await.context("DOM.setFileInputFiles")?;
    Ok(())
}

pub async fn maybe_upload_cv(page: &Page, cv_path: &str) {
    if cv_path.is_empty() || !std::path::Path::new(cv_path).exists() {
        return;
    }
    if let Err(e) = upload_cv(page, cv_path).await {
        log::warn!("CV upload: {e}");
    }
}

pub async fn click_submit(page: &Page) -> Result<bool> {
    let clicked = page
        .evaluate(
            r#"
            (() => {
              const candidates = Array.from(document.querySelectorAll(
                'button, input[type=submit], a[role=button], a.button, #submit_app'
              ));
              const isSubmit = (el) => {
                const t = ((el.innerText || el.value || '') + ' ' + (el.getAttribute('aria-label') || '') + ' ' + (el.id || '')).toLowerCase();
                if (/submit application|send application|submit|apply now|^apply$/.test(t)) return true;
                if (el.id === 'submit_app') return true;
                return false;
              };
              // Prefer real submit over early "Apply" that only opens the form.
              const ranked = candidates.filter(isSubmit).sort((a, b) => {
                const ta = ((a.innerText || a.value || '')).toLowerCase();
                const tb = ((b.innerText || b.value || '')).toLowerCase();
                const score = (t) => (/submit/.test(t) ? 0 : /send/.test(t) ? 1 : 2);
                return score(ta) - score(tb);
              });
              const btn = ranked[0];
              if (!btn) return false;
              btn.click();
              return true;
            })()
            "#,
        )
        .await?
        .into_value::<bool>()
        .unwrap_or(false);
    Ok(clicked)
}

pub async fn click_apply_entry(page: &Page) -> Result<bool> {
    let clicked = page
        .evaluate(
            r#"
            (() => {
              const hasForm = !!document.querySelector(
                '#application_form, #first_name, input[name=first_name], input[type=email], form[action*="apply"]'
              );
              if (hasForm) return false;
              const candidates = Array.from(document.querySelectorAll('a, button'));
              const btn = candidates.find((el) => {
                const t = ((el.innerText || el.value || '') + ' ' + (el.getAttribute('aria-label') || '')).toLowerCase().trim();
                return t === 'apply' || t === 'apply now' || t === 'apply for this job' || /^apply\b/.test(t);
              });
              if (!btn) return false;
              btn.click();
              return true;
            })()
            "#,
        )
        .await?
        .into_value::<bool>()
        .unwrap_or(false);
    Ok(clicked)
}

#[derive(Debug, Clone)]
pub struct ConfirmSignal {
    pub text_ok: bool,
    pub url_changed: bool,
    pub form_gone: bool,
    pub captcha: bool,
    pub validation_error: bool,
}

impl ConfirmSignal {
    pub fn is_confirmed(&self) -> bool {
        if self.captcha || self.validation_error {
            return false;
        }
        self.text_ok || (self.url_changed && self.form_gone)
    }

    pub fn note(&self, clicked: bool) -> String {
        if self.captcha {
            return "captcha blocked submit — finish manually".into();
        }
        if self.validation_error {
            return "form validation errors after submit — check required fields".into();
        }
        if self.text_ok {
            return "submit confirmed by page text".into();
        }
        if self.url_changed && self.form_gone {
            return "submit confirmed (URL changed + form gone)".into();
        }
        if clicked {
            return "clicked submit; confirmation unclear — not marking applied".into();
        }
        "could not find submit control".into()
    }
}

/// Strong confirmation: never treat a bare click as success.
pub async fn confirm_submission(page: &Page, before_url: &str) -> Result<ConfirmSignal> {
    let mut text_ok = false;
    let mut url_changed = false;
    let mut form_gone = false;
    let mut captcha = false;
    let mut validation_error = false;

    for _ in 0..8 {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        let text = page_text(page, 6000).await.unwrap_or_default();
        let url = current_url(page).await;
        if looks_captcha(&text) {
            captcha = true;
        }
        if looks_success_text(&text) || looks_already_applied(&text) {
            text_ok = true;
        }
        if looks_validation_error(&text) && !text_ok {
            validation_error = true;
        }
        if !before_url.is_empty() && !url.is_empty() && url != before_url {
            let bl = before_url.to_lowercase();
            let ul = url.to_lowercase();
            if ul.contains("thank")
                || ul.contains("confirmation")
                || ul.contains("success")
                || ul.contains("submitted")
                || (!ul.contains("/apply") && bl.contains("/apply"))
                || ul != bl
            {
                url_changed = true;
            }
        }
        form_gone = page
            .evaluate(
                r#"(() => {
                  const form = document.querySelector('#application_form, form[action*="application"], form');
                  const submit = document.querySelector('#submit_app, button[type=submit], input[type=submit]');
                  if (!form && !submit) return true;
                  if (form && (form.offsetParent === null || form.getAttribute('hidden') != null)) return true;
                  return false;
                })()"#,
            )
            .await
            .ok()
            .and_then(|v| v.into_value::<bool>().ok())
            .unwrap_or(false);

        if text_ok || captcha || (url_changed && form_gone) {
            break;
        }
    }

    Ok(ConfirmSignal {
        text_ok,
        url_changed,
        form_gone,
        captcha,
        validation_error: validation_error && !text_ok,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_ats_hosts() {
        assert_eq!(
            classify_ats("https://boards.greenhouse.io/acme/jobs/1"),
            AtsKind::Greenhouse
        );
        assert_eq!(
            classify_ats("https://jobs.ashbyhq.com/acme/xyz"),
            AtsKind::Ashby
        );
        assert_eq!(
            classify_ats("https://tether.recruitee.com/o/foo"),
            AtsKind::Recruitee
        );
        assert_eq!(
            classify_ats("https://careers.tether.io/o/foo"),
            AtsKind::Recruitee
        );
        assert_eq!(
            classify_ats("https://jobs.lever.co/acme/1"),
            AtsKind::Lever
        );
    }

    #[test]
    fn confirm_rules() {
        let ok = ConfirmSignal {
            text_ok: true,
            url_changed: false,
            form_gone: false,
            captcha: false,
            validation_error: false,
        };
        assert!(ok.is_confirmed());
        let click_only = ConfirmSignal {
            text_ok: false,
            url_changed: false,
            form_gone: false,
            captcha: false,
            validation_error: false,
        };
        assert!(!click_only.is_confirmed());
    }
}
