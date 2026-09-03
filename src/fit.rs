//! Cheap skip rules so we do not draft jobs Golfredo cannot take.

/// Visa / onsite gates that waste a Venezuela remote backend apply.
pub fn visa_or_onsite_skip(title: &str, location: &str, description: &str) -> Option<&'static str> {
    let hay = format!("{title}\n{location}\n{description}").to_ascii_lowercase();
    if hay.trim().is_empty() {
        return None;
    }

    const VISA_HARD: &[&str] = &[
        "no visa sponsorship",
        "cannot sponsor",
        "can't sponsor",
        "unable to sponsor",
        "will not sponsor",
        "won't sponsor",
        "sponsorship is not available",
        "sponsorship not available",
        "not able to sponsor",
        "does not sponsor",
        "do not sponsor work visas",
        "we do not offer sponsorship",
        "must be a us citizen",
        "must be a u.s. citizen",
        "united states citizen only",
        "us citizenship required",
        "u.s. citizenship required",
        "must have a green card",
        "must possess a green card",
        "h1b transfer only",
        "no sponsorship available",
    ];
    for p in VISA_HARD {
        if hay.contains(p) {
            return Some("quick-skip: visa/citizenship required (VE remote)");
        }
    }

    let onsite_only = (hay.contains("on-site only")
        || hay.contains("onsite only")
        || hay.contains("in-office only")
        || hay.contains("must be located in")
        || hay.contains("must live in")
        || hay.contains("must reside in")
        || hay.contains("relocation required")
        || hay.contains("this role is not remote"))
        && !hay.contains("remote worldwide")
        && !hay.contains("remote-first")
        && !hay.contains("fully remote")
        && !hay.contains("remote ok");

    if onsite_only {
        return Some("quick-skip: onsite / location lock (not remote VE)");
    }

    None
}

pub fn valid_outcome(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "replied" => Some("replied"),
        "interview" => Some("interview"),
        "rejected" => Some("rejected"),
        "ghost" => Some("ghost"),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn is_actionable_status(status: &str) -> bool {
    matches!(status, "manual" | "ready" | "ready_draft" | "failed")
}

pub fn follow_up_note(title: &str, company: &str, pitch: &str) -> String {
    let company = if company.trim().is_empty() {
        "your team"
    } else {
        company.trim()
    };
    let mut note = format!(
        "Hi — following up on my application for {title} at {company}. Happy to share more or jump on a short call.\n"
    );
    let pitch = pitch.trim();
    if !pitch.is_empty() {
        note.push('\n');
        note.push_str(pitch);
        if pitch.len() > 280 {
            note.truncate(280);
            note.push('…');
        }
    }
    note
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_no_sponsorship() {
        assert!(visa_or_onsite_skip(
            "Backend",
            "NYC",
            "We are unable to sponsor visas at this time."
        )
        .is_some());
        assert!(visa_or_onsite_skip("Backend", "", "Must be a US citizen.").is_some());
    }

    #[test]
    fn keeps_remote_worldwide() {
        assert!(visa_or_onsite_skip(
            "Backend",
            "Remote",
            "Fully remote. We hire worldwide. Visa not required."
        )
        .is_none());
    }

    #[test]
    fn skips_onsite_only() {
        assert!(visa_or_onsite_skip(
            "Backend",
            "San Francisco",
            "On-site only. Relocation required. Must live in California."
        )
        .is_some());
    }

    #[test]
    fn outcomes_whitelist() {
        assert_eq!(valid_outcome("Interview"), Some("interview"));
        assert!(valid_outcome("hired").is_none());
    }

    #[test]
    fn actionable_pipeline_statuses() {
        assert!(is_actionable_status("manual"));
        assert!(!is_actionable_status("applied"));
    }

    #[test]
    fn follow_up_includes_role() {
        let n = follow_up_note("Rust engineer", "Acme", "I built payment rails.");
        assert!(n.contains("Rust engineer"));
        assert!(n.contains("Acme"));
        assert!(n.contains("payment rails"));
    }
}
