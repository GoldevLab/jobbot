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
Do NOT lock messaging to Norway/EU only — he targets remote jobs globally unless the JD itself is location-specific.
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

Shared memory from the profile coach (kept bios/headlines — match this voice when it fits):
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
Prefer remote worldwide framing unless the JD is location-specific — do not default to Norway/EU-only.
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

/// Norway/EU-only framing left over from earlier positioning (unless JD is Norway-specific).
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

/// Rewrite stale geo phrases to worldwide remote.
pub fn scrub_stale_geo_text(text: &str) -> String {
    let mut out = text.to_string();
    let replacements = [
        ("EU/Norway overlap", "open worldwide"),
        ("eu/norway overlap", "open worldwide"),
        ("Norway / EU timezone overlap", "any timezone overlap"),
        ("Norway/EU timezone overlap", "any timezone overlap"),
        ("ready for EU/Norway overlap", "open to worldwide remote"),
        ("ready for EU overlap", "open to worldwide remote"),
        ("EU/Norway", "worldwide"),
        ("EU overlap", "timezone overlap"),
        (", EU overlap", ", open worldwide"),
        (" overlap for Oslo / EU hours", " — flexible timezone overlap"),
        ("open to Norway / EU", "open worldwide"),
        ("Remote (Venezuela) | EU/Norway overlap", "Remote worldwide (Venezuela)"),
        ("Remote from Venezuela, EU/Norway overlap", "Remote from Venezuela, open worldwide"),
        ("Remote from Venezuela, EU overlap", "Remote from Venezuela, open worldwide"),
    ];
    for (from, to) in replacements {
        out = out.replace(from, to);
        // case-insensitive light pass for common leftover
        let lower = out.to_ascii_lowercase();
        let from_l = from.to_ascii_lowercase();
        if lower.contains(&from_l) && from_l != from {
            // already handled exact; skip messy CI replace
        }
        let _ = lower;
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

/// Profile coach — parallel agent; never invents employers or fake metrics.
pub fn profile_coach_prompt(
    platform: &str,
    settings: &crate::db::Settings,
    snapshot: &str,
    learning: &str,
) -> String {
    let locations = {
        let l = settings.locations.trim();
        if l.is_empty() || l == "*" || l.eq_ignore_ascii_case("all") || l.eq_ignore_ascii_case("worldwide")
        {
            "worldwide — any country / remote (NOT Norway-only; do not push Oslo/EU as the default pitch)"
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
- 3 to 6 suggestions max. Ready-to-paste copy when possible (bio, About, README blurb).
- Sound human, not corporate. No emojis. No fake star counts or invented companies/repos.
- Positioning: remote worldwide from Venezuela. Do NOT default to "Norway/EU only" or "Oslo" unless the user notes demand it. Prefer "remote worldwide" / "open to any timezone overlap".
- Experience: Node/TS backends since ~2017 (~8 years), Gravitad since 2022. Never invent "3+ years".
- For github: ONLY GitHub-related suggestion titles (bio, pin order, topics, README). ALWAYS include actions when bio/topics should change. Prefer short bio without email.
- For github topics: ONLY repos that appear in the snapshot (exact names like resuma, jobbot). NEVER invent repos (block-explorer-indexer, dex-backend, etc.). Max 8 topics per repo; prefer 1–2 real repos.
- For linkedin: ONLY LinkedIn titles (headline, About). No overview cards. No GitHub bio under linkedin. No actions. At most one headline + one About ready-to-paste.
- For general: at most 2 ready-to-paste items (never an "overview" title). Prefer concrete bio/headline/About text over checklists.
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
        learning = truncate(if learning.trim().is_empty() {
            "(no lessons yet)"
        } else {
            learning
        }, 3500),
    )
}
