//! Static preset of Google's hosted Workspace MCP servers.
//!
//! Each entry pairs a stable `key` (what a UI / command passes) with the
//! server's `display_name`, hosted `url`, and least-privilege `scopes`. The
//! `display_name` is chosen so it NORMALIZES (via
//! [`crate::agent::service_registry::normalize`]) to the matching curated
//! `service_registry` key/alias — e.g. "Google Calendar" -> `googlecalendar`
//! -> the `gcal` entry — so registering one of these servers lights up its
//! curated skill doc automatically.
//!
//! IMPORTANT: these URLs are Google's DOCUMENTED hosted MCP endpoints (see the
//! OAuth implementation map). Whether a doce-owned desktop OAuth client with a
//! loopback redirect is ACCEPTED by `*.mcp.googleapis.com` is UNVERIFIED — a
//! human spike, not a coding one. Treat the URLs as the documented default,
//! not a proven integration.
//!
//! Google Keep is intentionally absent: it has NO documented hosted MCP
//! endpoint. The `gkeep` curated skill doc stays for a future self-hosted
//! route.
//!
//! This module is pure data — no I/O, no network, no credentials. The actual
//! OAuth account is created separately by `connect_oauth_account`.

use super::google;

/// One hosted Google Workspace MCP server preset.
#[derive(Debug, Clone, Copy)]
pub struct GoogleWorkspaceService {
    /// Stable lookup key a UI / command passes (e.g. `gmail`, `calendar`).
    pub key: &'static str,
    /// The server name written to `mcp_server_connections`. Chosen so it
    /// normalizes to the matching `service_registry` key/alias.
    pub display_name: &'static str,
    /// Google's documented hosted MCP endpoint (loopback acceptance UNVERIFIED).
    pub url: &'static str,
    /// Least-privilege scopes this server needs (from the `google` preset).
    pub scopes: &'static [&'static str],
}

/// The preset table. Gmail, Calendar, Drive — NOT Keep (no hosted endpoint).
pub static SERVICES: &[GoogleWorkspaceService] = &[
    GoogleWorkspaceService {
        key: "gmail",
        display_name: "Gmail",
        url: "https://gmailmcp.googleapis.com/mcp/v1",
        scopes: &[google::SCOPE_GMAIL_READONLY, google::SCOPE_GMAIL_COMPOSE],
    },
    GoogleWorkspaceService {
        key: "calendar",
        display_name: "Google Calendar",
        url: "https://calendarmcp.googleapis.com/mcp/v1",
        scopes: &[
            google::SCOPE_CALENDAR_EVENTS_READONLY,
            google::SCOPE_CALENDAR_EVENTS,
        ],
    },
    GoogleWorkspaceService {
        key: "drive",
        display_name: "Google Drive",
        url: "https://drivemcp.googleapis.com/mcp/v1",
        scopes: &[google::SCOPE_DRIVE_FILE],
    },
];

/// Looks up a preset by its stable `key`, or `None` if unknown.
pub fn lookup(key: &str) -> Option<&'static GoogleWorkspaceService> {
    SERVICES.iter().find(|s| s.key == key)
}

/// The valid preset keys, for surfacing in an "unknown key" error.
pub fn valid_keys() -> Vec<&'static str> {
    SERVICES.iter().map(|s| s.key).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::service_registry;

    #[test]
    fn every_preset_display_name_resolves_to_a_curated_skill() {
        // The whole point of the display_name choice: registering a preset
        // server lights up its curated `service_registry` skill.
        let expected = [
            ("gmail", "gmail"),
            ("calendar", "gcal"),
            ("drive", "gdrive"),
        ];
        for svc in SERVICES {
            let curated = service_registry::lookup(svc.display_name).unwrap_or_else(|| {
                panic!(
                    "preset {:?} (display_name {:?}) must resolve to a curated service",
                    svc.key, svc.display_name
                )
            });
            let want = expected
                .iter()
                .find(|(k, _)| *k == svc.key)
                .map(|(_, curated_key)| *curated_key)
                .unwrap_or_else(|| panic!("unexpected preset key {:?}", svc.key));
            assert_eq!(
                curated.key, want,
                "preset {:?} should resolve to curated {want:?}",
                svc.key
            );
        }
    }

    #[test]
    fn keep_is_not_in_the_preset() {
        assert!(
            lookup("keep").is_none() && lookup("gkeep").is_none(),
            "Keep has no hosted MCP endpoint and must not be a preset"
        );
    }

    #[test]
    fn lookup_and_valid_keys_agree() {
        for key in valid_keys() {
            assert!(lookup(key).is_some());
        }
        assert!(lookup("bogus").is_none());
    }

    #[test]
    fn every_preset_has_at_least_one_scope() {
        for svc in SERVICES {
            assert!(!svc.scopes.is_empty(), "{} needs scopes", svc.key);
        }
    }
}
