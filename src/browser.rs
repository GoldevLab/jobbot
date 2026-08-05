//! Chrome DevTools Protocol via chromiumoxide (attach to existing Chrome).

use anyhow::{anyhow, Context, Result};
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::{Page, ScreenshotParams};
use futures::StreamExt;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ChromeSession {
    browser: Browser,
    _handler: tokio::task::JoinHandle<()>,
}

impl ChromeSession {
    /// Connect to an already-running Chrome with remote debugging.
    pub async fn connect(cdp_http: &str) -> Result<Self> {
        let ws = resolve_ws_url(cdp_http).await?;
        let (browser, mut handler) = Browser::connect(ws)
            .await
            .context("connect to Chrome CDP")?;
        let handle = tokio::spawn(async move {
            while let Some(_ev) = handler.next().await {}
        });
        Ok(Self {
            browser,
            _handler: handle,
        })
    }

    pub async fn new_page(&self, url: &str) -> Result<Page> {
        let page = self.browser.new_page(url).await.context("new_page")?;
        page.wait_for_navigation().await.ok();
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        Ok(page)
    }

    #[allow(dead_code)]
    pub async fn screenshot_png(&self, page: &Page, path: &Path) -> Result<()> {
        let params = ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .build();
        let data = page.screenshot(params).await.context("screenshot")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, data)?;
        Ok(())
    }
}

async fn resolve_ws_url(cdp_http: &str) -> Result<String> {
    let base = cdp_http.trim_end_matches('/');
    let url = format!("{base}/json/version");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| {
            format!("Chrome not reachable at {url}. Run scripts/chrome-cdp.sh first.")
        })?;
    if !resp.status().is_success() {
        return Err(anyhow!("Chrome /json/version HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await?;
    let ws = v
        .get("webSocketDebuggerUrl")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("no webSocketDebuggerUrl in /json/version"))?
        .to_string();
    Ok(ws)
}

/// Shared optional session for the worker.
pub type SharedChrome = Arc<Mutex<Option<ChromeSession>>>;

pub fn shared_chrome() -> SharedChrome {
    Arc::new(Mutex::new(None))
}

pub async fn ensure_chrome(shared: &SharedChrome) -> Result<()> {
    let mut g = shared.lock().await;
    if g.is_some() {
        return Ok(());
    }
    let cdp = std::env::var("JOBBOT_CHROME_CDP")
        .unwrap_or_else(|_| "http://127.0.0.1:9222".into());
    let session = ChromeSession::connect(&cdp).await?;
    *g = Some(session);
    Ok(())
}
