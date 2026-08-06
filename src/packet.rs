//! Manual-apply kit: pasteable answers + CV download for jobs the bot cannot auto-submit.

use crate::db::{Job, Settings};
use serde_json::Value;
use std::path::Path;

fn draft_str(draft: &Value, key: &str) -> String {
    draft
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn draft_list(draft: &Value, key: &str) -> Vec<String> {
    draft
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Build a plain-text application kit ready to paste into any ATS form.
pub fn build_packet_text(job: &Job, settings: &Settings, draft: &Value) -> String {
    let apply = job
        .apply_url
        .as_deref()
        .filter(|u| !u.is_empty())
        .unwrap_or(job.url.as_str());
    let score = job
        .score
        .map(|s| format!("{s:.0}"))
        .unwrap_or_else(|| "—".into());
    let bullets = draft_list(draft, "cv_bullets");
    let emphasize = draft_list(draft, "emphasize");

    let mut out = String::new();
    out.push_str("════════════════════════════════════════\n");
    out.push_str("JOBBOT — MANUAL APPLICATION KIT\n");
    out.push_str("════════════════════════════════════════\n\n");

    out.push_str(&format!("Role: {}\n", job.title));
    out.push_str(&format!("Company: {}\n", job.company));
    out.push_str(&format!("Location: {}\n", job.location));
    out.push_str(&format!("Status: {} · score {}\n", job.status, score));
    out.push_str(&format!("Apply URL: {apply}\n"));
    out.push_str(&format!("Listing: {}\n\n", job.url));

    out.push_str("── CONTACT (paste into form) ──\n");
    out.push_str(&format!("Full name: {}\n", settings.full_name));
    out.push_str(&format!("Email: {}\n", settings.email));
    out.push_str(&format!("Phone: {}\n", settings.phone));
    out.push_str(&format!("Country / working from: {}\n", settings.country));
    out.push_str(&format!("LinkedIn: {}\n", settings.linkedin));
    out.push_str(&format!("GitHub: {}\n", settings.github));
    out.push_str(&format!(
        "Expected salary (USD): {}\n\n",
        draft_str(draft, "salary_usd")
            .if_empty(&settings.expected_salary_usd)
    ));

    out.push_str("── PITCH / SHORT INTRO ──\n");
    out.push_str(&or_dash(&draft_str(draft, "pitch")));
    out.push_str("\n\n");

    out.push_str("── WHY THIS COMPANY ──\n");
    out.push_str(&or_dash(&draft_str(draft, "why_company")));
    out.push_str("\n\n");

    if !draft_str(draft, "cover_note").is_empty() {
        out.push_str("── COVER NOTE ──\n");
        out.push_str(&draft_str(draft, "cover_note"));
        out.push_str("\n\n");
    }

    out.push_str("── EMPHASIZE / KEYWORDS ──\n");
    if emphasize.is_empty() {
        out.push_str("—\n\n");
    } else {
        out.push_str(&emphasize.join(" · "));
        out.push_str("\n\n");
    }

    out.push_str("── CV BULLETS (tailored — paste into summary / experience) ──\n");
    if bullets.is_empty() {
        out.push_str("—\n\n");
    } else {
        for b in &bullets {
            out.push_str(&format!("• {b}\n"));
        }
        out.push('\n');
    }

    out.push_str("── COMMON SCREENING ANSWERS ──\n");
    out.push_str(&format!(
        "Node.js experience: {}\n",
        or_dash(&draft_str(draft, "node_experience"))
    ));
    out.push_str(&format!(
        "Databases: {}\n",
        or_dash(&draft_str(draft, "databases"))
    ));
    out.push_str(&format!(
        "Compliance / finance: {}\n",
        or_dash(&draft_str(draft, "compliance_finance"))
    ));
    out.push_str(&format!("P2P: {}\n", or_dash(&draft_str(draft, "p2p"))));
    out.push_str(&format!(
        "Country (form): {}\n\n",
        draft_str(draft, "country").if_empty(&settings.country)
    ));

    if !settings.profile_notes.trim().is_empty() {
        out.push_str("── PROFILE NOTES (headline / About / bio paste) ──\n");
        out.push_str(settings.profile_notes.trim());
        out.push_str("\n\n");
    }

    out.push_str("── CV FILE ──\n");
    out.push_str(&format!(
        "Download from JobBot: /jobs/{}/cv.pdf\n",
        job.id
    ));
    out.push_str(&format!("Server path: {}\n", settings.cv_path));
    if Path::new(&settings.cv_path).is_file() {
        out.push_str("CV file: present on server\n");
    } else {
        out.push_str("WARNING: CV file missing on server — upload your PDF manually.\n");
    }

    out.push_str("\n── HOW TO APPLY ──\n");
    out.push_str("1. Open the Apply URL above.\n");
    out.push_str("2. Upload the CV PDF from /jobs/<id>/cv.pdf.\n");
    out.push_str("3. Paste contact + pitch + screening answers from this kit.\n");
    out.push_str("4. If the form asks for video, record a short answer yourself.\n");
    out.push_str("════════════════════════════════════════\n");
    out
}

fn or_dash(s: &str) -> String {
    if s.trim().is_empty() {
        "—".into()
    } else {
        s.to_string()
    }
}

trait IfEmpty {
    fn if_empty(&self, fallback: &str) -> String;
}
impl IfEmpty for String {
    fn if_empty(&self, fallback: &str) -> String {
        if self.trim().is_empty() {
            fallback.to_string()
        } else {
            self.clone()
        }
    }
}

/// Persist kit next to the markdown draft for offline use.
pub fn save_packet_file(job_id: i64, text: &str) -> anyhow::Result<()> {
    let dir = Path::new("data/drafts");
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(format!("{job_id}-packet.txt")), text)?;
    Ok(())
}

/// ZIP: packet.txt + CV PDF (when present on disk).
pub fn build_kit_zip(packet_txt: &str, cv_path: &str) -> anyhow::Result<Vec<u8>> {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("application-kit.txt", opts)?;
        zip.write_all(packet_txt.as_bytes())?;

        let cv = Path::new(cv_path);
        if cv.is_file() {
            let bytes = std::fs::read(cv)?;
            let name = cv
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("cv.pdf");
            zip.start_file(name, opts)?;
            zip.write_all(&bytes)?;
        } else {
            zip.start_file("CV-MISSING.txt", opts)?;
            zip.write_all(
                format!("CV not found at {cv_path}. Add JOBBOT_CV_PATH / Settings CV path.\n")
                    .as_bytes(),
            )?;
        }
        zip.finish()?;
    }
    Ok(buf.into_inner())
}

pub fn safe_filename(title: &str, company: &str, ext: &str) -> String {
    let raw = format!("{company}-{title}");
    let slug: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug
        .split('-')
        .filter(|s| !s.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("-");
    format!("jobbot-{slug}.{ext}")
}
