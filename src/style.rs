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
- Physics degree (ULA); English advanced; remote from Venezuela, open to Norway/EU timezone overlap
- GitHub GoldevLab; LinkedIn golfredo-perez-fernandez; email golfredo.pf@gmail.com

Never invent employers or bank years. If MongoDB/Rails asked and he lacks depth: say Postgres/SQL + Node is the core, picks up fast.
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
"#,
        system = SYSTEM_HUMAN,
        name = settings.full_name,
        email = settings.email,
        phone = settings.phone,
        linkedin = settings.linkedin,
        github = settings.github,
        country = settings.country,
        salary = settings.expected_salary_usd,
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

/// Profile coach — parallel agent; never invents employers or fake metrics.
pub fn profile_coach_prompt(
    platform: &str,
    settings: &crate::db::Settings,
    snapshot: &str,
) -> String {
    format!(
        r#"{system}

You are improving public professional profiles for job search (backend / Web3 / Norway-EU remote).
Platform to focus on: {platform}

Current settings:
Name: {name}
LinkedIn URL: {linkedin}
GitHub URL: {github}
Country: {country}
Target keywords: {keywords}
Target locations: {locations}
Notes / pasted profile text (may be empty — LinkedIn About, headline, pinned repos notes):
{notes}

Live snapshot / public data for this platform (may be partial):
{snapshot}

Reply with ONLY JSON (no markdown):
{{
  "summary": "<2 short sentences on what to fix first>",
  "suggestions": [
    {{
      "title": "<short label, e.g. GitHub bio>",
      "body": "<ready-to-paste text OR concrete checklist steps>",
      "priority": <1 urgent, 2 normal, 3 nice-to-have>
    }}
  ]
}}

Rules:
- 3 to 6 suggestions max. Ready-to-paste copy when possible (bio, About, README blurb).
- Sound human, not corporate. No emojis. No fake star counts or invented companies.
- For github: bio, README pin order, topics, pin strategy, contribution visibility.
- For linkedin: headline, About, featured, Open to Work framing for Norway/EU backend.
- For general: CV alignment, personal site, consistency across profiles.
- Prefer concrete Norwegian/EU remote + Web3/backend positioning when relevant.
"#,
        system = SYSTEM_HUMAN,
        platform = platform,
        name = settings.full_name,
        linkedin = settings.linkedin,
        github = settings.github,
        country = settings.country,
        keywords = settings.keywords,
        locations = settings.locations,
        notes = truncate(&settings.profile_notes, 2500),
        snapshot = truncate(snapshot, 4500),
    )
}
