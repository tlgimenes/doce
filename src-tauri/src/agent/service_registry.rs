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
    /// Additional NORMALIZED names this service matches on, so a server named
    /// with its natural product name resolves too — e.g. "Google Calendar"
    /// normalizes to `googlecalendar`, which aliases to the `gcal` entry. Each
    /// alias MUST already be normalized (see the `aliases_are_normalized` test).
    pub aliases: &'static [&'static str],
    /// One-line, per-turn catalog description (no trailing period needed).
    pub catalog_description: &'static str,
    /// The usage doc appended to the activation result — recipes + guardrail.
    pub skill: &'static str,
    /// Phase 4: lowercase, single-word intent tokens that, when they appear
    /// in the user's message, signal this service is likely relevant — the
    /// scoring input to
    /// [`crate::agent::mcp_disclosure::services_to_autoactivate`]. Matched by
    /// exact whole-token equality against the tokenized message (NOT
    /// substrings), so each entry should be a bare noun/verb the user would
    /// actually type (e.g. `inbox`, `calendar`), not a phrase.
    pub keywords: &'static [&'static str],
}

/// The curated table. Seeded with the four Google services; grows as doce
/// authors more skill docs.
static SERVICES: &[CuratedService] = &[
    CuratedService {
        key: "gmail",
        aliases: &["googlemail"],
        catalog_description: "search, read & draft email",
        skill: include_str!("skills/gmail.md"),
        keywords: &["email", "inbox", "mail", "reply", "gmail", "message"],
    },
    CuratedService {
        key: "gcal",
        aliases: &["googlecalendar", "calendar"],
        catalog_description: "check availability & propose calendar events",
        skill: include_str!("skills/gcal.md"),
        keywords: &[
            "calendar",
            "event",
            "schedule",
            "meeting",
            "availability",
            "invite",
        ],
    },
    CuratedService {
        key: "gkeep",
        aliases: &["googlekeep", "keep"],
        catalog_description: "read & draft notes and lists",
        skill: include_str!("skills/gkeep.md"),
        keywords: &["note", "notes", "keep", "list", "reminder"],
    },
    CuratedService {
        key: "gdrive",
        aliases: &["googledrive", "drive"],
        catalog_description: "search, read & organize files",
        skill: include_str!("skills/gdrive.md"),
        keywords: &[
            "drive",
            "file",
            "files",
            "document",
            "doc",
            "folder",
            "spreadsheet",
        ],
    },
];

/// Normalizes a server name for matching: lowercases it and keeps only ASCII
/// alphanumerics, dropping spaces, punctuation, and separators. This is the
/// spirit of [`crate::agent::mcp_disclosure::sanitize`] (lowercase + collapse
/// noise) but tuned for exact-key matching, so "Gmail", "gmail", "G Mail",
/// and "  GMAIL  " all normalize to `gmail`.
pub fn normalize(server_name: &str) -> String {
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
    SERVICES
        .iter()
        .find(|s| s.key == normalized || s.aliases.contains(&normalized.as_str()))
}

/// Resolves a free-form requested service string to a curated `key` by intent
/// KEYWORD (Phase 4) — the looser sibling of [`lookup`], which matches only on
/// a service's name/aliases. This lets `resolve_service`'s fuzzy activation
/// see through a synonym the server's own name lacks: e.g. `activate_service`
/// with `"email"` resolves to the `gmail` key even though no server is named
/// "email". Matches the WHOLE normalized request against each service's
/// keywords (keywords are disjoint across services — see the
/// `keywords_are_disjoint_across_services` test — so the first hit is
/// unambiguous). `None` when the request matches no keyword.
pub fn key_for_keyword(requested: &str) -> Option<&'static str> {
    let normalized = normalize(requested);
    if normalized.is_empty() {
        return None;
    }
    SERVICES
        .iter()
        .find(|s| s.keywords.contains(&normalized.as_str()))
        .map(|s| s.key)
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
    fn lookup_resolves_natural_google_names_via_aliases() {
        assert_eq!(lookup("Google Calendar").map(|c| c.key), Some("gcal"));
        assert_eq!(lookup("Google Drive").map(|c| c.key), Some("gdrive"));
        assert_eq!(lookup("Google Keep").map(|c| c.key), Some("gkeep"));
        // The key itself still resolves for the natural "Gmail" name.
        assert_eq!(lookup("Gmail").map(|c| c.key), Some("gmail"));
        assert_eq!(lookup("Google Mail").map(|c| c.key), Some("gmail"));
        // Bare product-word aliases resolve too.
        assert_eq!(lookup("Calendar").map(|c| c.key), Some("gcal"));
        assert_eq!(lookup("Drive").map(|c| c.key), Some("gdrive"));
        assert_eq!(lookup("Keep").map(|c| c.key), Some("gkeep"));
    }

    #[test]
    fn aliases_are_themselves_normalized() {
        for svc in SERVICES {
            for alias in svc.aliases {
                assert_eq!(
                    &normalize(alias),
                    alias,
                    "{}'s alias {alias:?} must already be normalized (no spaces/caps)",
                    svc.key
                );
            }
        }
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
    fn keywords_are_disjoint_across_services() {
        // `key_for_keyword` returns the FIRST service whose keywords contain
        // the request, so a keyword shared by two services would resolve
        // ambiguously by table order. Guard that they never overlap.
        let mut seen: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
        for svc in SERVICES {
            for kw in svc.keywords {
                if let Some(other) = seen.insert(kw, svc.key) {
                    panic!("keyword {kw:?} is shared by {other} and {}", svc.key);
                }
            }
        }
    }

    #[test]
    fn key_for_keyword_resolves_synonyms_and_ignores_unknown() {
        assert_eq!(key_for_keyword("email"), Some("gmail"));
        assert_eq!(key_for_keyword("Inbox"), Some("gmail"));
        assert_eq!(key_for_keyword("meeting"), Some("gcal"));
        assert_eq!(key_for_keyword("spreadsheet"), Some("gdrive"));
        assert_eq!(key_for_keyword("reminder"), Some("gkeep"));
        assert_eq!(key_for_keyword("wombat"), None);
        assert_eq!(key_for_keyword(""), None);
    }

    #[test]
    fn every_service_has_lowercase_intent_keywords() {
        // Phase 4: `services_to_autoactivate` matches these by exact
        // whole-token equality against a lowercase-tokenized message, so a
        // capitalized or multi-word entry could never match.
        for svc in SERVICES {
            assert!(
                !svc.keywords.is_empty(),
                "{} needs at least one intent keyword",
                svc.key
            );
            for kw in svc.keywords {
                assert_eq!(
                    &kw.to_ascii_lowercase(),
                    kw,
                    "{}'s keyword {kw:?} must be lowercase",
                    svc.key
                );
                assert!(
                    kw.chars().all(|c| c.is_ascii_alphanumeric()),
                    "{}'s keyword {kw:?} must be a single alphanumeric token",
                    svc.key
                );
            }
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
