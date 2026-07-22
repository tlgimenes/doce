//! Phase 2 of tool progressive disclosure: a static, doce-authored registry
//! of CURATED usage docs ("skills") for KNOWN MCP services.
//!
//! A local 4B model rarely knows how to drive a given MCP service well from
//! its tool schemas alone. For services doce recognizes, this table supplies:
//!   1. a one-line `catalog_description` — shown per-turn in the connected-
//!      services catalog ([`crate::agent::mcp_disclosure::render_catalog`]),
//!      so the model can pick the right service WITHOUT a network round-trip;
//!   2. a concise `skill` markdown doc — a few common recipes plus a hard
//!      guardrail, appended to the activation result the moment the model
//!      calls `activate_service` (Anthropic's "load skill on activation"
//!      Level-2 disclosure). The doc is spent as prompt tokens on activation,
//!      so it is kept tight (well under ~250 words).
//!
//! Everything here is a compile-time constant — NO I/O, NO network. Lookup
//! is by a NORMALIZED server name (see [`normalize`]), matched against a
//! service's `key`, so a server the user names "Gmail", "gmail", or "G Mail"
//! all resolve to the `gmail` entry.
//!
//! The four Google services are curated now even though Google isn't wired
//! until Phase 3 (which needs OAuth): the markdown is transport-independent,
//! and the match fires as soon as a server with a matching name is connected.

/// One doce-curated MCP service: its match `key`, the one-line catalog blurb,
/// and the full skill doc shown on activation.
#[derive(Debug, Clone, Copy)]
pub struct CuratedService {
    /// The NORMALIZED name this service matches on (see [`normalize`]) —
    /// lowercase, alphanumerics only.
    pub key: &'static str,
    /// One-line, per-turn catalog description (no trailing period needed).
    pub catalog_description: &'static str,
    /// The usage doc appended to the activation result — recipes + guardrail.
    pub skill: &'static str,
}

/// The curated table. Seeded with the four Google services; grows as doce
/// authors more skill docs.
static SERVICES: &[CuratedService] = &[
    CuratedService {
        key: "gmail",
        catalog_description: "search, read & draft email",
        skill: include_str!("skills/gmail.md"),
    },
    CuratedService {
        key: "gcal",
        catalog_description: "check availability & propose calendar events",
        skill: include_str!("skills/gcal.md"),
    },
    CuratedService {
        key: "gkeep",
        catalog_description: "read & draft notes and lists",
        skill: include_str!("skills/gkeep.md"),
    },
    CuratedService {
        key: "gdrive",
        catalog_description: "search, read & organize files",
        skill: include_str!("skills/gdrive.md"),
    },
];

/// Normalizes a server name for matching: lowercases it and keeps only ASCII
/// alphanumerics, dropping spaces, punctuation, and separators. This is the
/// spirit of [`crate::agent::mcp_disclosure::sanitize`] (lowercase + collapse
/// noise) but tuned for exact-key matching, so "Gmail", "gmail", "G Mail",
/// and "  GMAIL  " all normalize to `gmail`.
fn normalize(server_name: &str) -> String {
    server_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Looks up the curated service for a connected server by its (normalized)
/// name, or `None` for a server doce has no curated doc for.
pub fn lookup(server_name: &str) -> Option<&'static CuratedService> {
    let normalized = normalize(server_name);
    if normalized.is_empty() {
        return None;
    }
    SERVICES.iter().find(|s| s.key == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_matches_known_services_case_and_space_insensitively() {
        for name in ["Gmail", "gmail", "GMAIL", "  G Mail  "] {
            assert_eq!(
                lookup(name).map(|c| c.key),
                Some("gmail"),
                "{name:?} should resolve to the gmail entry"
            );
        }
        assert_eq!(lookup("G Cal").map(|c| c.key), Some("gcal"));
        assert_eq!(lookup("gcal").map(|c| c.key), Some("gcal"));
        assert_eq!(lookup("GKeep").map(|c| c.key), Some("gkeep"));
        assert_eq!(lookup("gDrive").map(|c| c.key), Some("gdrive"));
    }

    #[test]
    fn lookup_returns_none_for_unknown_or_empty() {
        assert!(lookup("github").is_none());
        assert!(lookup("some random server").is_none());
        assert!(lookup("").is_none());
        assert!(lookup("   ").is_none());
    }

    #[test]
    fn all_four_google_skills_are_present_with_a_catalog_line() {
        for key in ["gmail", "gcal", "gkeep", "gdrive"] {
            let svc = SERVICES.iter().find(|s| s.key == key).unwrap();
            assert!(
                !svc.catalog_description.is_empty(),
                "{key} needs a catalog description"
            );
            assert!(!svc.skill.trim().is_empty(), "{key} needs a skill doc");
        }
    }

    #[test]
    fn every_skill_carries_the_never_send_without_user_guardrail() {
        // The hard guardrail: no irreversible action without the user saying so.
        const GUARDRAIL: &str = "without the user's explicit confirmation";
        for svc in SERVICES {
            assert!(
                svc.skill.contains(GUARDRAIL),
                "{}'s skill doc must contain the guardrail {GUARDRAIL:?}",
                svc.key
            );
        }
    }

    #[test]
    fn skill_docs_stay_tight() {
        // These are spent as prompt tokens on activation — keep them short.
        for svc in SERVICES {
            let words = svc.skill.split_whitespace().count();
            assert!(
                words < 250,
                "{}'s skill doc is {words} words — keep it under ~250",
                svc.key
            );
        }
    }
}
