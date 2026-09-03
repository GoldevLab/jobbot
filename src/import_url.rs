//! Paste a job URL (LinkedIn, Wellfound, careers page) into the queue.

use crate::sources::DiscoveredJob;
use anyhow::{anyhow, Context, Result};
use scraper::{Html, Selector};
use std::net::IpAddr;
use url::Url;

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Only public http(s) URLs — no localhost / RFC1918 / link-local.
pub fn public_http_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw.trim()).context("invalid URL")?;
    match url.scheme() {
        "http" | "https" => {}
        _ => anyhow::bail!("only http(s) URLs are allowed"),
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("URL missing host"))?
        .to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host == "metadata.google.internal"
        || host == "0.0.0.0"
    {
        anyhow::bail!("blocked host");
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if ip_is_blocked(ip) {
            anyhow::bail!("blocked address");
        }
    }
    Ok(url)
}

fn ip_is_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_multicast()
                || v.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(v) => {
            v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || v.is_unique_local()
                || v.is_unicast_link_local()
        }
    }
}

pub fn external_id_for_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or("job");
    let path = url.path().trim_end_matches('/');
    let mut id = format!("{host}{path}");
    if id.len() > 180 {
        id.truncate(180);
    }
    if id.is_empty() {
        "imported".into()
    } else {
        id
    }
}

fn meta_content(doc: &Html, key: &str) -> String {
    let sels = [
        format!(r#"meta[property="{key}"]"#),
        format!(r#"meta[name="{key}"]"#),
    ];
    for raw in sels {
        if let Ok(sel) = Selector::parse(&raw) {
            if let Some(el) = doc.select(&sel).next() {
                if let Some(c) = el.value().attr("content") {
                    let t = collapse_ws(c);
                    if !t.is_empty() {
                        return t;
                    }
                }
            }
        }
    }
    String::new()
}

fn first_text(doc: &Html, selector: &str) -> String {
    Selector::parse(selector)
        .ok()
        .and_then(|sel| doc.select(&sel).next())
        .map(|el| collapse_ws(&el.text().collect::<String>()))
        .unwrap_or_default()
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse listing HTML into a queue row (no network).
pub fn parse_imported_job(html: &str, page_url: &Url) -> DiscoveredJob {
    let doc = Html::parse_document(html);
    let mut title = meta_content(&doc, "og:title");
    if title.is_empty() {
        title = meta_content(&doc, "twitter:title");
    }
    if title.is_empty() {
        title = first_text(&doc, "h1");
    }
    if title.is_empty() {
        title = first_text(&doc, "title");
    }
    if title.is_empty() {
        title = "Imported job".into();
    }
    // LinkedIn often prefixes "Company hiring Role"
    let hiring = title.clone();
    if let Some((left, right)) = hiring.split_once(" hiring ") {
        if !right.is_empty() {
            title = right
                .split('|')
                .next()
                .unwrap_or(right)
                .trim()
                .to_string();
            if title.is_empty() {
                title = left.trim().to_string();
            }
        }
    }

    let mut company = meta_content(&doc, "og:site_name");
    if company.is_empty() {
        company = first_text(&doc, "[class*=company], .company, [data-company]");
    }
    if let Ok(sel) = Selector::parse(r#"script[type="application/ld+json"]"#) {
        for el in doc.select(&sel) {
            let raw = el.text().collect::<String>();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                let org = v
                    .pointer("/hiringOrganization/name")
                    .or_else(|| v.pointer("/hiringOrganization"))
                    .and_then(|x| x.as_str());
                if let Some(name) = org {
                    if company.is_empty() {
                        company = collapse_ws(name);
                    }
                }
                if let Some(t) = v.get("title").and_then(|x| x.as_str()) {
                    if title == "Imported job" {
                        title = collapse_ws(t);
                    }
                }
            }
        }
    }

    let mut description = meta_content(&doc, "og:description");
    let body = first_text(&doc, "article, .job-description, [class*=description], main");
    if description.is_empty() {
        description = body;
    } else if !body.is_empty() && body.len() > description.len() {
        description = format!("{description}\n\n{body}");
    }
    if description.len() > 12_000 {
        description.truncate(12_000);
    }

    let location = if description.to_ascii_lowercase().contains("remote")
        || title.to_ascii_lowercase().contains("remote")
    {
        "Remote".into()
    } else {
        first_text(&doc, "[class*=location], .location")
    };

    let href = page_url.as_str().to_string();
    DiscoveredJob {
        source: "url".into(),
        external_id: external_id_for_url(page_url),
        title,
        company,
        location,
        url: href.clone(),
        apply_url: Some(href),
        description,
    }
}

pub async fn fetch_imported_job(raw_url: &str) -> Result<DiscoveredJob> {
    let url = public_http_url(raw_url)?;
    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(6))
        .build()?;
    let resp = client
        .get(url.clone())
        .send()
        .await
        .context("fetch job page")?;
    if !resp.status().is_success() {
        anyhow::bail!("job page returned HTTP {}", resp.status());
    }
    let final_url = resp.url().clone();
    if public_http_url(final_url.as_str()).is_err() {
        anyhow::bail!("redirected to a blocked host");
    }
    let html = resp.text().await.context("read job page")?;
    Ok(parse_imported_job(&html, &final_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_localhost_and_lan() {
        assert!(public_http_url("http://127.0.0.1/job").is_err());
        assert!(public_http_url("http://localhost/job").is_err());
        assert!(public_http_url("http://192.168.1.9/job").is_err());
        assert!(public_http_url("http://10.0.0.2/x").is_err());
        assert!(public_http_url("file:///etc/passwd").is_err());
        assert!(public_http_url("https://web3.career/foo").is_ok());
    }

    #[test]
    fn parses_og_and_h1() {
        let html = r#"
        <html><head>
          <meta property="og:title" content="Senior Backend — Acme hiring Senior Backend">
          <meta property="og:site_name" content="Acme">
          <meta property="og:description" content="Build Node services. Fully remote.">
        </head>
        <body><h1>Senior Backend</h1>
        <article>TypeScript, Postgres, remote worldwide.</article>
        </body></html>
        "#;
        let url = Url::parse("https://jobs.example.com/senior-backend").unwrap();
        let job = parse_imported_job(html, &url);
        assert_eq!(job.source, "url");
        assert!(job.title.to_lowercase().contains("backend"));
        assert_eq!(job.company, "Acme");
        assert!(job.description.to_lowercase().contains("node"));
        assert_eq!(job.location, "Remote");
        assert_eq!(job.apply_url.as_deref(), Some(url.as_str()));
    }

    #[test]
    fn linkedin_hiring_title() {
        let html = r#"<html><head><title>Stripe hiring Backend Engineer | LinkedIn</title></head><body></body></html>"#;
        let url = Url::parse("https://www.linkedin.com/jobs/view/123").unwrap();
        let job = parse_imported_job(html, &url);
        assert!(job.title.to_lowercase().contains("backend"));
        assert!(!job.title.to_lowercase().contains("stripe hiring"));
    }
}
