//! Discover jobs from web3.career (HTML scrape).

use super::DiscoveredJob;
use anyhow::{Context, Result};
use regex::Regex;
use scraper::{Html, Selector};

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

#[allow(dead_code)]
pub async fn discover(keywords: &str, locations: &str) -> Result<Vec<DiscoveredJob>> {
    discover_skipping_known(keywords, locations, &std::collections::HashSet::new()).await
}

/// Like [`discover`], but only detail-scrapes jobs whose `external_id` is not already known.
/// Avoids hammering web3.career when the board set is stable (sterile discovers).
pub async fn discover_skipping_known(
    keywords: &str,
    locations: &str,
    known_ids: &std::collections::HashSet<String>,
) -> Result<Vec<DiscoveredJob>> {
    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Listings. Tag remote seeds so we don't drop them when location text is empty.
    let seeds: Vec<(String, bool)> = vec![
        ("https://web3.career/remote-jobs".into(), true),
        ("https://web3.career/backend+remote-jobs".into(), true),
        ("https://web3.career/?query=backend".into(), false),
        ("https://web3.career/?query=nodejs".into(), false),
        ("https://web3.career/?query=typescript".into(), false),
        (
            format!(
                "https://web3.career/?query={}",
                urlencoding_encode(keywords.split(',').next().unwrap_or("backend").trim())
            ),
            false,
        ),
    ];

    for (seed, remote_tag) in seeds {
        match fetch_listing(&client, &seed, remote_tag).await {
            Ok(jobs) => {
                for j in jobs {
                    if seen.insert(j.external_id.clone()) {
                        out.push(j);
                    }
                }
            }
            Err(e) => log::warn!("web3.career list failed ({seed}): {e}"),
        }
    }

    let keywords_l: Vec<String> = keywords
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let locs_l: Vec<String> = locations
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    // Soft filter on title/url only (description empty until enrich).
    // Prefer backend/Node/Web3 signals; do not treat every "engineer" as a match alone
    // when settings keywords are set (cuts sales/compliance noise).
    out.retain(|j| {
        let hay = format!(
            "{} {} {}",
            j.title.to_lowercase(),
            j.url.to_lowercase(),
            j.location.to_lowercase()
        );
        let backendish = hay.contains("backend")
            || hay.contains("back-end")
            || hay.contains("back end")
            || hay.contains("nodejs")
            || hay.contains("node.js")
            || hay.contains("typescript")
            || hay.contains("rust")
            || hay.contains("web3")
            || hay.contains("solidity")
            || hay.contains("blockchain");
        let kw_ok = if keywords_l.is_empty() {
            backendish || hay.contains("engineer") || hay.contains("developer")
        } else {
            keywords_l.iter().any(|k| hay.contains(k)) || backendish
        };
        let loc_ok = locs_l.is_empty()
            || j.location.to_lowercase().contains("remote")
            || locs_l.iter().any(|l| hay.contains(l));
        kw_ok && loc_ok
    });

    // Detail-scrape only unknowns (or everything if known set empty on first run).
    let mut unknown: Vec<_> = out
        .iter_mut()
        .filter(|j| known_ids.is_empty() || !known_ids.contains(&j.external_id))
        .collect();
    let enrich_n = unknown.len().min(24);
    for job in unknown.iter_mut().take(enrich_n) {
        if let Ok(detail) = fetch_detail(&client, &job.url).await {
            if let Some(apply) = detail.apply_url.as_deref() {
                match resolve_apply_url(&client, apply).await {
                    Ok(resolved) => job.apply_url = Some(resolved),
                    Err(_) if !apply.contains("web3.career") => {
                        job.apply_url = Some(apply.to_string())
                    }
                    Err(_) => {}
                }
            }
            if !detail.description.is_empty() {
                job.description = detail.description;
            }
            if job.company.is_empty() && !detail.company.is_empty() {
                job.company = detail.company;
            }
            if !detail.location.is_empty() && job.location.is_empty() {
                job.location = detail.location;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }

    Ok(out)
}

/// Fetch job page and return a resolved external ATS apply URL when possible.
pub async fn resolve_external_apply(
    client: &reqwest::Client,
    job_page_url: &str,
) -> Result<Option<String>> {
    let detail = fetch_detail(client, job_page_url).await?;
    let Some(apply) = detail.apply_url else {
        return Ok(None);
    };
    match resolve_apply_url(client, &apply).await {
        Ok(resolved) => Ok(Some(resolved)),
        Err(_) if !apply.contains("web3.career") => Ok(Some(apply)),
        Err(_) => Ok(None),
    }
}

/// True when auto-apply (HTTP or Chrome) can target this URL.
pub fn is_auto_applyable_url(url: &str) -> bool {
    let u = url.to_lowercase();
    u.contains("recruitee")
        || u.contains("careers.tether")
        || u.contains("greenhouse")
        || u.contains("ashbyhq")
        || u.contains("lever.co")
}

/// HTTP (no Chrome) auto-apply — Recruitee Careers Site API only today.
/// Greenhouse Job Board POST needs each company's board API key.
pub fn is_http_auto_applyable_url(url: &str) -> bool {
    let u = url.to_lowercase();
    u.contains("recruitee") || u.contains("careers.tether")
}

async fn resolve_apply_url(client: &reqwest::Client, url: &str) -> Result<String> {
    let resp = client
        .get(url)
        .header("User-Agent", UA)
        .send()
        .await?;
    let final_url = resp.url().clone();
    // Prefer Location chain final URL
    let s = final_url.as_str().to_string();
    // If still a web3.career tracker, not useful for external apply
    if s.contains("web3.career") {
        anyhow::bail!("apply stayed on web3.career");
    }
    Ok(s)
}

/// Known high-value board entry for smoke / Norway backend apply path.
pub fn seed_tether_norway() -> DiscoveredJob {
    DiscoveredJob {
        source: "manual".into(),
        external_id: "tether-senior-backend-norway".into(),
        title: "Senior Backend Developer".into(),
        company: "Tether Operations Limited".into(),
        location: "Oslo, Norway / Remote".into(),
        url: "https://careers.tether.io/o/senior-backend-developer-norway".into(),
        apply_url: Some(
            "https://careers.tether.io/o/senior-backend-developer-norway".into(),
        ),
        description: "Senior Backend Developer (Node.js) remote / Oslo. Improve core NodeJS services, RPC between services, complex SQL for compliance/finance, microservices, security.".into(),
    }
}

fn urlencoding_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

async fn fetch_listing(
    client: &reqwest::Client,
    url: &str,
    remote_tag: bool,
) -> Result<Vec<DiscoveredJob>> {
    let html = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(parse_listing(&html, remote_tag))
}

/// web3.career job URLs look like `/senior-full-stack-engineer-bcbgroup/152288`
fn parse_listing(html: &str, remote_tag: bool) -> Vec<DiscoveredJob> {
    let doc = Html::parse_document(html);
    let mut jobs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let a_sel = Selector::parse("a[href]").unwrap();
    let job_re = Regex::new(
        r"(?i)^(?:https?://web3\.career)?/([a-z0-9][a-z0-9\-]*)/(\d{5,})(?:\?[^#]*)?(?:#.*)?$",
    )
    .unwrap();

    let skip_slugs = [
        "assets", "font", "users", "hire", "ads", "learn-web3", "post-web3-job",
        "web3-salaries", "intern-jobs", "top-web3-internships", "web3-jobs-api",
    ];

    for a in doc.select(&a_sel) {
        let href = a.value().attr("href").unwrap_or("");
        let abs = absolutize(href);
        let path = abs.split('?').next().unwrap_or(&abs);
        let Some(caps) = job_re.captures(path) else {
            continue;
        };
        let slug = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if skip_slugs.iter().any(|p| slug == *p || slug.starts_with(&format!("{p}-"))) {
            continue;
        }
        let external_id = caps
            .get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        if external_id.is_empty() || !seen.insert(external_id.clone()) {
            continue;
        }

        let mut text = collapse_ws(&a.text().collect::<String>());
        if text.len() < 4 || text.len() > 160 {
            text = humanize_slug(slug);
        }

        let lower = text.to_lowercase();
        if lower.contains("sign in") || lower.contains("sign up") || lower == "remote" {
            continue;
        }

        jobs.push(DiscoveredJob {
            source: "web3.career".into(),
            external_id,
            title: text,
            company: company_from_slug(slug),
            location: if remote_tag {
                "Remote".into()
            } else {
                String::new()
            },
            url: abs.split('?').next().unwrap_or(&abs).to_string(),
            apply_url: None,
            description: String::new(),
        });
    }

    jobs
}

fn humanize_slug(slug: &str) -> String {
    slug.split('-')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => format!("{}{}", f.to_uppercase(), c.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn company_from_slug(slug: &str) -> String {
    // Often last token is company: senior-full-stack-engineer-bcbgroup
    slug.rsplit('-').next().map(humanize_slug).unwrap_or_default()
}

struct Detail {
    apply_url: Option<String>,
    description: String,
    company: String,
    location: String,
}

async fn fetch_detail(client: &reqwest::Client, url: &str) -> Result<Detail> {
    let html = client
        .get(url)
        .send()
        .await?
        .error_for_status()
        .context("detail")?
        .text()
        .await?;
    Ok(parse_detail(&html, url))
}

fn ats_priority(url: &str) -> i32 {
    let u = url.to_ascii_lowercase();
    if u.contains("recruitee.com") || u.contains("careers.tether") {
        100
    } else if u.contains("greenhouse.io")
        || u.contains("ashbyhq.com")
        || u.contains("jobs.lever.co")
        || u.contains("lever.co")
    {
        95
    } else if u.contains("myworkdayjobs.com")
        || u.contains("workday.com")
        || u.contains("smartrecruiters.com")
        || u.contains("workable.com")
        || u.contains("breezy.hr")
        || u.contains("jobvite.com")
        || u.contains("wellfound.com")
        || u.contains("angel.co")
    {
        85
    } else if !u.contains("web3.career") && (u.contains("http://") || u.contains("https://")) {
        50
    } else {
        0
    }
}

fn parse_detail(html: &str, _page_url: &str) -> Detail {
    let doc = Html::parse_document(html);
    let a_sel = Selector::parse("a[href]").unwrap();
    // Prefer ATS apply links over web3.career /i/ trackers
    let mut apply_url = None;
    let mut apply_priority = 0i32;

    let consider = |abs: &str, text_hint: &str, apply_url: &mut Option<String>, apply_priority: &mut i32| {
        let mut prio = ats_priority(abs);
        if prio == 0 {
            return;
        }
        if text_hint.contains("apply") {
            prio += 5;
        }
        if abs.contains("web3.career") {
            if abs.contains("/i/") {
                prio = 20;
            } else {
                return;
            }
        }
        if prio > *apply_priority {
            *apply_priority = prio;
            *apply_url = Some(abs.to_string());
        }
    };

    for a in doc.select(&a_sel) {
        let href = a.value().attr("href").unwrap_or("");
        let t = a.text().collect::<String>().to_lowercase();
        let abs = if href.starts_with("http") {
            href.to_string()
        } else {
            absolutize(href)
        };
        consider(&abs, &t, &mut apply_url, &mut apply_priority);
    }

    // Scan raw HTML for ATS hosts even when not in <a href>.
    let ats_re = Regex::new(
        r#"https?://[^\s"'<>]+(?:recruitee\.com|careers\.tether\.io|greenhouse\.io|ashbyhq\.com|lever\.co|myworkdayjobs\.com|smartrecruiters\.com|workable\.com)[^\s"'<>]*"#,
    )
    .unwrap();
    for m in ats_re.find_iter(html) {
        let abs = m.as_str().trim_end_matches(['\\', ')', ']', ',', ';']).to_string();
        consider(&abs, "apply", &mut apply_url, &mut apply_priority);
    }

    // JSON-LD applicationUrl / url
    if let Ok(script_sel) = Selector::parse(r#"script[type="application/ld+json"]"#) {
        for el in doc.select(&script_sel) {
            let raw = el.text().collect::<String>();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                for key in ["applicationUrl", "url", "sameAs"] {
                    if let Some(u) = v.get(key).and_then(|x| x.as_str()) {
                        consider(u, "apply", &mut apply_url, &mut apply_priority);
                    }
                }
            }
        }
    }

    // Don't fall back to the job page itself as apply_url (useless for auto-apply)
    let apply_url = apply_url;

    let body_sel = Selector::parse("article, .job-description, main, body").unwrap();
    let mut description = String::new();
    if let Some(el) = doc.select(&body_sel).next() {
        description = collapse_ws(&el.text().collect::<Vec<_>>().join(" "));
    }

    let company = Selector::parse("h1, h2, .company, [class*=company]")
        .ok()
        .and_then(|sel| {
            doc.select(&sel)
                .nth(1)
                .map(|e| collapse_ws(&e.text().collect::<String>()))
        })
        .unwrap_or_default();

    let location = if description.to_lowercase().contains("remote") {
        "Remote".into()
    } else {
        String::new()
    };

    Detail {
        apply_url,
        description,
        company,
        location,
    }
}

fn absolutize(href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if href.starts_with("//") {
        return format!("https:{href}");
    }
    if href.starts_with('/') {
        return format!("https://web3.career{href}");
    }
    format!("https://web3.career/{href}")
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[allow(dead_code)]
pub fn is_recruitee_url(url: &str) -> bool {
    let u = url.to_lowercase();
    u.contains("recruitee.com") || u.contains("careers.tether.io")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_slash_id_urls() {
        let html = r#"
        <a href="/senior-full-stack-engineer-bcbgroup/152288">Senior Full Stack Engineer</a>
        <a href="/product-designer-jito/152289">Product Designer</a>
        <a href="/assets/application-abc123/99999">Asset</a>
        "#;
        let jobs = parse_listing(html, true);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].external_id, "152288");
        assert!(jobs[0].url.contains("/152288"));
        assert_eq!(jobs[0].location, "Remote");
        assert!(jobs[0].title.to_lowercase().contains("full stack"));
    }
}
