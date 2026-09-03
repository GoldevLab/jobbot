//! Prompting helpers — keep answers sounding like a real engineer, not a brochure.

pub const SYSTEM_HUMAN: &str = r#"
You help fill job applications for Golfredo Pérez Fernández, a backend/Web3 engineer from Venezuela.

Write like a real person typing answers in a hurry — short sentences, first person, concrete facts.
Do NOT sound like ChatGPT. Avoid: leverage, passionate, synergy, cutting-edge, excited to join,
"as a seasoned professional", bullet-point poetry, emojis, markdown headings.

Facts you can use (only if relevant to THIS job):
- JS/TypeScript and Node backends since ~2017; remote with Gravitad (Spain) since 2022 on Koolinart/Orionchain
- Postgres block explorer indexer talking to Geth JSON-RPC; Uniswap-V3-style DEX; NFT marketplace on Base; trading bots that resume after restart
- SQL (Postgres, Turso/libSQL + Drizzle), Express/Node APIs, Docker/Fly.io, CI deploys
- Author of Resuma (Rust SSR framework) — systems thinking, but day-to-day product backends are Node/TS
- Physics degree (ULA); English advanced; remote from Venezuela, open to roles worldwide (any country / timezone overlap)
- GitHub GoldevLab; LinkedIn golfredo-perez-fernandez; email golfredo.pf@gmail.com

Never invent employers or bank years. If MongoDB/Rails asked and he lacks depth: say Postgres/SQL + Node is the core, picks up fast.
Positioning is remote worldwide. Only mention a country if THIS job posting requires it.
"#;

pub fn score_prompt(title: &str, company: &str, location: &str, description: &str) -> String {
    format!(
        r#"{system}

Job:
Title: {title}
Company: {company}
Location: {location}

Description (may be truncated):
{desc}

Reply with ONLY JSON (no markdown):
{{"score": <0-100 number>, "reason": "<one short sentence>", "skip": <true if designer/analyst/compliance/support/SRE-network or clearly not backend/Node/Web3>}}
"#,
        system = SYSTEM_HUMAN,
        title = title,
        company = company,
        location = location,
        desc = truncate(description, 3500)
    )
}

pub fn draft_prompt(
    title: &str,
    company: &str,
    location: &str,
    description: &str,
    settings: &crate::db::Settings,
    profile_memory: &str,
) -> String {
    format!(
        r#"{system}

Tailor the application + a mini CV pitch for THIS specific role (not a generic template).
Pick the 3–5 facts from the profile that best match the JD. Drop unrelated stuff.

Profile:
Name: {name}
Email: {email}
Phone: {phone}
LinkedIn: {linkedin}
GitHub: {github}
Working from: {country}
Expected salary USD: {salary}

Shared memory (kept bios/headlines + apply outcomes). Match kept voice. If apply_fail mentions a missing field, include that field. If apply_manual, write a tight paste-ready cover_note:
{memory}

Job: {title} @ {company} ({location})
Description:
{desc}

Reply with ONLY a compact JSON object (keep strings short; max 3 cv_bullets of ~20 words each):
{{
  "pitch": "<2 short sentences>",
  "cv_bullets": ["<bullet>", "<bullet>", "<bullet>"],
  "emphasize": ["<skill>", "<skill>", "<skill>"],
  "why_company": "<3 short sentences>",
  "node_experience": "<none_willing|1_3|3_5|5_plus>",
  "databases": "<2 sentences>",
  "compliance_finance": "<1-2 sentences>",
  "p2p": "<yes/no + one line>",
  "country": "{country}",
  "salary_usd": "{salary}",
  "cover_note": "<3-5 short lines or empty string>"
}}

Finish the full JSON. Do not truncate.
Frame availability as remote worldwide / any timezone overlap unless the JD itself names a required country.
"#,
        system = SYSTEM_HUMAN,
        name = settings.full_name,
        email = settings.email,
        phone = settings.phone,
        linkedin = settings.linkedin,
        github = settings.github,
        country = settings.country,
        salary = settings.expected_salary_usd,
        memory = truncate(
            if profile_memory.trim().is_empty() {
                "(none yet)"
            } else {
                profile_memory
            },
            1800
        ),
        title = title,
        company = company,
        location = location,
        desc = truncate(description, 2200)
    )
}

pub fn truncate(s: &str, max: usize) -> String {
    let mut t = s.chars().take(max).collect::<String>();
    if s.chars().count() > max {
        t.push_str("…");
    }
    t
}

/// Old Norway/EU-only targeting list (or equivalent). Treat as worldwide.
pub fn is_stale_location_filter(raw: &str) -> bool {
    let parts: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return false;
    }
    let stale = ["norway", "oslo", "europe", "eu", "eea", "nordic", "remote"];
    parts.iter().all(|p| {
        stale.iter().any(|s| p == *s || p.contains("norway") || p.contains("oslo"))
    })
}

/// Empty / * / all / worldwide / old Norway list → no country filter.
pub fn normalize_search_locations(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty()
        || t == "*"
        || t.eq_ignore_ascii_case("all")
        || t.eq_ignore_ascii_case("worldwide")
        || is_stale_location_filter(t)
    {
        String::new()
    } else {
        t.to_string()
    }
}

/// Leftover Norway/EU-only framing in bios, headlines, drafts.
pub fn stale_geo_pitch(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    l.contains("norway")
        || l.contains("oslo")
        || (l.contains("eu") && l.contains("overlap") && !l.contains("worldwide"))
}

pub fn jd_is_norway_specific(location: &str, description: &str, title: &str) -> bool {
    let hay = format!("{location} {description} {title}").to_ascii_lowercase();
    hay.contains("norway") || hay.contains("oslo")
}

fn replace_ci(hay: &str, from: &str, to: &str) -> String {
    let lower = hay.to_ascii_lowercase();
    let from_l = from.to_ascii_lowercase();
    let mut out = String::new();
    let mut i = 0;
    while let Some(pos) = lower[i..].find(&from_l) {
        let abs = i + pos;
        out.push_str(&hay[i..abs]);
        out.push_str(to);
        i = abs + from.len();
    }
    out.push_str(&hay[i..]);
    out
}

/// Rewrite stale geo phrases to worldwide remote.
pub fn scrub_stale_geo_text(text: &str) -> String {
    let mut out = text.to_string();
    let replacements = [
        ("EU/Norway overlap", "open worldwide"),
        ("Norway / EU timezone overlap", "any timezone overlap"),
        ("Norway/EU timezone overlap", "any timezone overlap"),
        ("ready for EU/Norway overlap", "open to worldwide remote"),
        ("ready for EU overlap", "open to worldwide remote"),
        ("open to Norway / EU", "open worldwide"),
        ("Remote (Venezuela) | EU/Norway overlap", "Remote worldwide (Venezuela)"),
        ("Remote from Venezuela, EU/Norway overlap", "Remote from Venezuela, open worldwide"),
        ("Remote from Venezuela, EU overlap", "Remote from Venezuela, open worldwide"),
        (" overlap for Oslo / EU hours", " — flexible timezone overlap"),
        ("EU/Norway", "worldwide"),
        ("EU overlap", "timezone overlap"),
        ("Norway / EU", "worldwide"),
        ("Oslo / EU", "worldwide"),
        ("Norway", "worldwide"),
        ("Oslo", "remote"),
    ];
    for (from, to) in replacements {
        out = replace_ci(&out, from, to);
    }
    out
}

/// Scrub pitch/why/cover fields on a draft JSON value when the JD is not Norway-specific.
pub fn scrub_draft_geo(draft: &mut serde_json::Value, location: &str, description: &str, title: &str) {
    if jd_is_norway_specific(location, description, title) {
        return;
    }
    for key in ["pitch", "why_company", "cover_note"] {
        if let Some(s) = draft.get(key).and_then(|v| v.as_str()) {
            if stale_geo_pitch(s) {
                draft[key] = serde_json::Value::String(scrub_stale_geo_text(s));
            }
        }
    }
}

/// Fake repo slugs the LLM keeps inventing from project *descriptions*.
pub const INVENTED_REPO_SLUGS: &[&str] = &[
    "block-explorer-indexer",
    "dex-backend",
    "nft-marketplace",
    "trading-bot",
];

/// Kinds we keep at most one open card for (SQL `LIKE %kind%` fragments).
pub const PROFILE_COPY_KINDS: &[&str] =
    &["headline", "about", "bio", "overview", "topic", "readme", "pin"];

pub fn mentions_invented_repo(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    INVENTED_REPO_SLUGS.iter().any(|s| l.contains(*s))
}

/// Classify a suggestion title into a single slot.
pub fn profile_suggestion_kind(title: &str) -> Option<&'static str> {
    let t = title.to_ascii_lowercase();
    if t.contains("headline") || t.contains("open to work") {
        Some("headline")
    } else if t.contains("about") {
        Some("about")
    } else if t.contains("bio") {
        Some("bio")
    } else if t.contains("overview") {
        Some("overview")
    } else if t.contains("topic") {
        Some("topic")
    } else if t.contains("readme") {
        Some("readme")
    } else if title_looks_like_pin(&t) {
        Some("pin")
    } else {
        None
    }
}

fn title_looks_like_pin(t: &str) -> bool {
    t.contains("pinned")
        || t.contains("pin order")
        || t.contains("pin repo")
        || t
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| w == "pin" || w == "pins")
}

pub fn canonical_suggestion_title(title: &str) -> String {
    match profile_suggestion_kind(title) {
        Some("headline") => "LinkedIn headline".into(),
        Some("about") => "LinkedIn About".into(),
        Some("bio") => "GitHub bio".into(),
        Some("topic") => "GitHub topics".into(),
        Some("readme") => "README blurb".into(),
        Some("pin") => "GitHub pin order".into(),
        Some("overview") => "Overview".into(),
        _ => title.trim().to_string(),
    }
}

pub fn canonical_platform(title: &str, current: &str) -> String {
    match profile_suggestion_kind(title) {
        Some("headline") | Some("about") => "linkedin".into(),
        Some("bio") | Some("topic") | Some("readme") | Some("pin") => "github".into(),
        _ => {
            let t = title.to_ascii_lowercase();
            if t.contains("linkedin") {
                "linkedin".into()
            } else if t.contains("github") {
                "github".into()
            } else {
                current.to_string()
            }
        }
    }
}

/// One-shot checklists — do not re-queue after Keep/Dismiss.
/// Topics stay refreshable (repos change); pin/README do not.
pub fn is_checklist_kind(kind: &str) -> bool {
    matches!(kind, "pin" | "readme")
}

/// Insert or replace a `Label:\nbody` block in Profile notes (no duplicate sections).
pub fn upsert_labeled_block(notes: &str, label: &str, body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return notes.to_string();
    }
    if notes.contains(body) {
        return notes.to_string();
    }
    let marker = format!("{label}:");
    let lower = notes.to_ascii_lowercase();
    let marker_l = marker.to_ascii_lowercase();
    if let Some(idx) = lower.find(&marker_l) {
        let rest = &notes[idx + marker.len()..];
        let end_rel = rest.find("\n\n").unwrap_or(rest.len());
        let end = idx + marker.len() + end_rel;
        let mut out = notes.to_string();
        out.replace_range(idx..end, &format!("{marker}\n{body}"));
        return out.trim().to_string();
    }
    let mut out = notes.trim().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(&format!("{marker}\n{body}"));
    out
}

/// Copy the user should Keep/Dismiss to train voice (blocks the coach).
pub fn is_blocking_kind(kind: &str) -> bool {
    matches!(kind, "headline" | "about" | "bio")
}

pub fn pin_order_from_repos(repos: &[String]) -> String {
    repos
        .iter()
        .take(6)
        .enumerate()
        .map(|(i, name)| format!("{}. {name}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Profile coach — parallel agent; never invents employers or fake metrics.
pub fn profile_coach_prompt(
    platform: &str,
    settings: &crate::db::Settings,
    snapshot: &str,
    learning: &str,
    live_repos: &str,
) -> String {
    let locations = {
        let l = settings.locations.trim();
        if l.is_empty() || l == "*" || l.eq_ignore_ascii_case("all") || l.eq_ignore_ascii_case("worldwide")
        {
            "worldwide remote — any country / any timezone overlap"
                .to_string()
        } else {
            l.to_string()
        }
    };
    format!(
        r#"{system}

You are improving public professional profiles for job search (backend / Web3, remote worldwide).
Platform to focus on: {platform}

JobBot WILL auto-apply GitHub `actions` (bio + repo topics) via API without asking the user.
LinkedIn cannot be auto-edited — only ready-to-paste suggestions.

Current settings:
Name: {name}
LinkedIn URL: {linkedin}
GitHub URL: {github}
Country (lives in): {country}
Target keywords: {keywords}
Target locations: {locations}
Notes / pasted profile text (may be empty — LinkedIn About, headline, pinned repos notes):
{notes}

Live snapshot / public data for this platform (may be partial):
{snapshot}

Live GitHub repo names (pins/topics: ONLY these exact names, never invent slugs):
{live_repos}

Memory from prior coaching + the apply-queue agent (learn from this; prefer kept styles, avoid dismissed):
{learning}

Reply with ONLY JSON (no markdown):
{{
  "summary": "<2 short sentences on what to fix first>",
  "suggestions": [
    {{
      "title": "<short label, e.g. GitHub bio>",
      "body": "<ready-to-paste text OR concrete checklist steps>",
      "priority": <1 urgent, 2 normal, 3 nice-to-have>
    }}
  ],
  "actions": [
    {{
      "type": "set_bio",
      "value": "<GitHub bio ≤160 chars, no email>"
    }},
    {{
      "type": "set_topics",
      "repo": "<exact repo name from the snapshot only>",
      "topics": ["nodejs", "typescript", "web3"]
    }}
  ]
}}

Rules:
- 2 to 4 suggestions max. At most ONE item per title. Ready-to-paste copy when possible.
- Titles MUST be exactly one of: "GitHub bio", "GitHub pin order", "GitHub topics", "README blurb", "LinkedIn headline", "LinkedIn About". Never invent alternate titles for the same slot.
- Sound human, not corporate. No emojis. No fake star counts or invented companies/repos.
- Positioning: remote worldwide from Venezuela. Prefer "remote worldwide" / "open to any timezone overlap". Only name a country if the live snapshot or notes require it.
- Experience: Node/TS backends since ~2017 (~8 years), Gravitad since 2022. Never invent "3+ years".
- For github: ONLY those GitHub titles. ALWAYS include actions when bio/topics should change. Prefer short bio without email.
- For github pins/topics: ONLY names from the live repo list. NEVER invent slugs (block-explorer-indexer, dex-backend, nft-marketplace, trading-bot are descriptions, not repos).
- Pin order and README are one-shot checklists — skip them if Memory already kept or dismissed that kind.
- Prefer evolving headline / About / bio over repeating pin/README.
- For linkedin: ONLY "LinkedIn headline" + "LinkedIn About". No overview. No GitHub bio. No actions.
- For general: at most 2 ready-to-paste items (never an "overview" title).
- Evolve copy using Memory: if a style was kept, reuse it; if dismissed, do not repeat.
- Mirror winning apply-agent pitches from Memory when writing About/headline/bio.
"#,
        system = SYSTEM_HUMAN,
        platform = platform,
        name = settings.full_name,
        linkedin = settings.linkedin,
        github = settings.github,
        country = settings.country,
        keywords = settings.keywords,
        locations = locations,
        notes = truncate(&settings.profile_notes, 2500),
        snapshot = truncate(snapshot, 4500),
        live_repos = if live_repos.trim().is_empty() {
            "(none parsed — do not invent repo names)"
        } else {
            live_repos
        },
        learning = truncate(if learning.trim().is_empty() {
            "(no lessons yet)"
        } else {
            learning
        }, 3500),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_duplicate_pin_and_readme_titles() {
        assert_eq!(profile_suggestion_kind("Pinned repo order"), Some("pin"));
        assert_eq!(
            profile_suggestion_kind("GitHub Pinned Repos Order"),
            Some("pin")
        );
        assert_eq!(profile_suggestion_kind("GitHub pin order"), Some("pin"));
        assert_eq!(
            profile_suggestion_kind("README blurb (main repo)"),
            Some("readme")
        );
        assert_eq!(
            profile_suggestion_kind("Main Repo README Blurb"),
            Some("readme")
        );
        assert_eq!(
            canonical_suggestion_title("GitHub Pinned Repos Order"),
            "GitHub pin order"
        );
        assert_eq!(
            profile_suggestion_kind("LinkedIn Open to Work"),
            Some("headline")
        );
    }

    #[test]
    fn spots_invented_repo_slugs() {
        assert!(mentions_invented_repo(
            "1. block-explorer-indexer\n2. dex-backend"
        ));
        assert!(!mentions_invented_repo("1. resuma\n2. jobbot"));
    }

    #[test]
    fn pin_list_uses_live_names() {
        let body = pin_order_from_repos(&["resuma".into(), "jobbot".into()]);
        assert_eq!(body, "1. resuma\n2. jobbot");
    }

    #[test]
    fn pin_kind_does_not_match_opinion() {
        assert_eq!(profile_suggestion_kind("Opinion on remote work"), None);
        assert_eq!(profile_suggestion_kind("Pin repos"), Some("pin"));
    }

    #[test]
    fn upsert_replaces_existing_headline() {
        let notes = "Headline:\nold line\n\nAbout:\nkeep me";
        let out = upsert_labeled_block(notes, "Headline", "Backend · Remote worldwide");
        assert!(out.contains("Backend · Remote worldwide"));
        assert!(!out.contains("old line"));
        assert!(out.contains("About:\nkeep me"));
    }

    #[test]
    fn old_norway_location_filter_becomes_worldwide() {
        assert_eq!(
            normalize_search_locations("norway,oslo,remote,europe"),
            ""
        );
        assert_eq!(normalize_search_locations("*"), "");
        assert_eq!(normalize_search_locations("united states,remote"), "united states,remote");
    }

    #[test]
    fn scrubs_norway_headline_to_worldwide() {
        let raw = "Backend Engineer | Remote (Venezuela) | EU/Norway overlap";
        let out = scrub_stale_geo_text(raw);
        assert!(!stale_geo_pitch(&out), "{out}");
        assert!(out.to_ascii_lowercase().contains("worldwide"));
    }
}
