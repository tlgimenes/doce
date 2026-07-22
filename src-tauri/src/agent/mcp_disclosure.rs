//! Phase 1 of tool progressive disclosure for connected MCP services.
//!
//! A local 4B model cannot use Anthropic's server-side tool search, so the
//! agent loop discloses connected MCP services CLIENT-side and EXPLICITLY:
//!
//!   1. It shows a compact *catalog* of connected services (one line each,
//!      NOT their tool schemas) in a per-turn tail — [`render_catalog`].
//!   2. The model calls the `activate_service` meta-tool to load ONE
//!      service's tools into the loop.
//!   3. Once activated, that service's tools are advertised as ordinary
//!      OpenAI tools ([`build_tools_array`]) and dispatched via
//!      [`crate::mcp::call_tool`].
//!
//! THE CARDINAL INVARIANT: with zero connected MCP servers this whole module
//! is inert. [`build_tools_array`] with `has_servers == false` returns
//! exactly `tools_array(base)`, and [`render_catalog`] over no servers
//! returns the empty string — so the agent loop is byte-for-byte identical
//! to the no-MCP loop (the benchmark's gate). Everything here is purely
//! additive and gated on "≥1 enabled MCP server exists".

use crate::mcp::{McpToolSchema, McpTransportConfig};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

/// One of the user's ENABLED MCP servers, snapshotted once at the start of a
/// top-level turn (`send_agent_message`). Carries the parsed transport
/// config so the loop can connect/list/call without re-reading the DB.
#[derive(Debug, Clone)]
pub struct McpServerSnapshot {
    pub id: String,
    pub name: String,
    pub config: McpTransportConfig,
}

/// One MCP tool the model has ACTIVATED (via `activate_service`) and can now
/// call. `advertised_name` is the sanitized, collision-namespaced name the
/// model sees and calls by; `raw_name` is the untouched tool name sent back
/// to the MCP server; `config` is its owning server's transport; `def_json`
/// is the ready-to-advertise OpenAI tool definition.
#[derive(Debug, Clone)]
pub struct ActivatedTool {
    pub advertised_name: String,
    pub server_name: String,
    pub raw_name: String,
    pub config: McpTransportConfig,
    pub def_json: Value,
}

/// Per-conversation live set of activated MCP tools, keyed by
/// `conversation_id`. Managed Tauri state (registered in `lib.rs` alongside
/// `ActiveGenerations`), mirroring that precedent: it persists across user
/// messages within a conversation, so a service activated in one turn stays
/// activated for the next.
#[derive(Default)]
pub struct ActivatedServices(pub Mutex<HashMap<String, Vec<ActivatedTool>>>);

/// Lowercases `server_name` and maps every character outside `[a-z0-9_-]` to
/// `_`, so it can safely form the prefix of an advertised tool name.
pub fn sanitize(server_name: &str) -> String {
    server_name
        .chars()
        .map(|c| {
            let lc = c.to_ascii_lowercase();
            if lc.is_ascii_lowercase() || lc.is_ascii_digit() || lc == '_' || lc == '-' {
                lc
            } else {
                '_'
            }
        })
        .collect()
}

/// Builds the advertised name for an MCP tool: `sanitize(server) + "__" +
/// raw`, then coerced into the `^[a-zA-Z0-9_-]{1,64}$` envelope the tool
/// schema requires (any other char -> `_`, truncated to 64, never empty).
/// The raw tool name's case is preserved (only the server prefix is
/// lowercased); the exact string this returns is what the model calls by and
/// what `ActivatedTool::advertised_name` stores for exact-match routing.
pub fn advertised_name(server_name: &str, raw_name: &str) -> String {
    let combined = format!("{}__{}", sanitize(server_name), raw_name);
    let mut name: String = combined
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Every retained char is ASCII, so a byte truncate is char-boundary safe.
    if name.len() > 64 {
        name.truncate(64);
    }
    if name.is_empty() {
        name.push('_');
    }
    name
}

/// Builds an [`ActivatedTool`] from a server name + config and one of that
/// server's [`McpToolSchema`]s, including the OpenAI-wrapper `def_json` the
/// loop advertises (`{"type":"function","function":{name,description,
/// parameters}}`, where `parameters` is the tool's `input_schema`).
pub fn make_activated_tool(
    server_name: &str,
    config: &McpTransportConfig,
    schema: &McpToolSchema,
) -> ActivatedTool {
    let advertised = advertised_name(server_name, &schema.name);
    let def_json = serde_json::json!({
        "type": "function",
        "function": {
            "name": advertised,
            "description": schema.description.clone().unwrap_or_default(),
            "parameters": schema.input_schema,
        }
    });
    ActivatedTool {
        advertised_name: advertised,
        server_name: server_name.to_string(),
        raw_name: schema.name.clone(),
        config: config.clone(),
        def_json,
    }
}

/// The advertised tools array for a turn: the base built-in tools, PLUS —
/// only when the user has at least one connected MCP server — the
/// `activate_service` meta-tool and every currently-activated MCP tool.
///
/// BYTE-INVARIANCE: with `has_servers == false` this is exactly
/// `tools_array(base)` — see the `byte_invariance_*` tests.
pub fn build_tools_array(
    base: &[&str],
    has_servers: bool,
    activated: &[ActivatedTool],
) -> Vec<Value> {
    let mut tools = crate::inference::http::tools_array(base);
    if has_servers {
        tools.push(crate::inference::http::activate_service_def());
        tools.extend(activated.iter().map(|t| t.def_json.clone()));
    }
    tools
}

/// Renders the connected-services catalog tail: an instruction line, then one
/// line per connected service (marked `(activated)` when its tools are
/// loaded), then a `Currently activated: ...` line when any are.
///
/// Phase 2: each KNOWN service (one doce has a curated entry for — see
/// [`crate::agent::service_registry`]) shows a one-line description after its
/// name, so the small model can pick the right service to activate WITHOUT a
/// per-turn network round-trip (this runs every turn, so it must NOT connect).
/// Unknown servers keep showing just their name.
///
/// BYTE-INVARIANCE: EMPTY string when there are no servers — hosts must skip
/// pushing an empty tail (same discipline as `PlanState::state_tail`).
pub fn render_catalog(snapshots: &[McpServerSnapshot], activated: &[ActivatedTool]) -> String {
    if snapshots.is_empty() {
        return String::new();
    }
    let activated_servers: std::collections::HashSet<&str> =
        activated.iter().map(|t| t.server_name.as_str()).collect();

    let mut out = String::from(
        "Connected services — call activate_service to load one before using its tools:",
    );
    for s in snapshots {
        let marker = if activated_servers.contains(s.name.as_str()) {
            " (activated)"
        } else {
            ""
        };
        // Curated one-liner if doce knows this service; bare name otherwise.
        match crate::agent::service_registry::lookup(&s.name) {
            Some(curated) => {
                out.push_str(&format!(
                    "\n- {}{}: {}",
                    s.name, marker, curated.catalog_description
                ));
            }
            None => out.push_str(&format!("\n- {}{}", s.name, marker)),
        }
    }
    let active_names: Vec<&str> = snapshots
        .iter()
        .map(|s| s.name.as_str())
        .filter(|n| activated_servers.contains(n))
        .collect();
    if !active_names.is_empty() {
        out.push_str(&format!(
            "\nCurrently activated: {}",
            active_names.join(", ")
        ));
    }
    out
}

/// The usage-guidance section appended to a service's activation result
/// (Phase 2, Level-2 "load skill on activation" disclosure), chosen in
/// priority: (a) doce's curated `skill` if the service is KNOWN
/// ([`crate::agent::service_registry::lookup`]); else (b) the server's own
/// handshake `instructions` if it advertised any; else (c) nothing.
///
/// Returns "" when there's nothing to add, so the caller can append
/// unconditionally. When non-empty it's a clearly-delimited section the model
/// reads right after activating: `\n\nHow to use <name>:\n<guidance>`.
pub fn activation_guidance(server_name: &str, instructions: Option<&str>) -> String {
    let curated = crate::agent::service_registry::lookup(server_name).map(|c| c.skill);
    let guidance = curated
        .or(instructions)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match guidance {
        Some(text) => format!("\n\nHow to use {server_name}:\n{text}"),
        None => String::new(),
    }
}

/// Builds the full activation result string the `activate_service` handler
/// returns: the "Activated … you can now call …" acknowledgement, plus the
/// [`activation_guidance`] section. Pure (no live server), so the exact
/// model-facing string is unit-testable; the handler feeds it the loaded
/// tools' advertised names and the server's `instructions`.
pub fn build_activation_result(
    server_name: &str,
    advertised_names: &[String],
    instructions: Option<&str>,
) -> String {
    let mut out = if advertised_names.is_empty() {
        format!("Activated {server_name:?}, but it exposes no tools.")
    } else {
        format!(
            "Activated {server_name:?}. You can now call: {}.",
            advertised_names.join(", ")
        )
    };
    out.push_str(&activation_guidance(server_name, instructions));
    out
}

// Phase 4 note — BM25 `search_tools` is DEFERRED, not omitted. Anthropic's
// server-side tool search (and a local BM25/embedding equivalent) earns its
// keep only at hundreds of tools. doce discloses at the SERVICE level: a
// handful of connected services, each a single eyeballable catalog line
// (`render_catalog`), and the small model picks one to `activate_service`.
// With that small N a ranked search index would add dependencies and a build
// step for no current benefit — the catalog IS the index. Revisit only if a
// single service's per-tool count explodes past what one catalog tail can
// reasonably carry; until then fuzzy `resolve_service` + conservative
// `services_to_autoactivate` are the whole retrieval story.

/// The outcome of resolving a model-supplied service name against the
/// connected servers — the pure core of `activate_service`'s fuzzy matching
/// (Phase 4), factored out so every resolution rule is unit-testable without
/// a live server.
#[derive(Debug)]
pub enum ServiceMatch<'a> {
    /// Exactly one server resolved — activate it.
    Found(&'a McpServerSnapshot),
    /// Nothing resolved — the handler emits its "no such service" error.
    NotFound,
    /// The request was too vague to pick safely: it resolved to more than one
    /// connected server. Carries their names so the handler can ask the model
    /// to disambiguate rather than guess.
    Ambiguous(Vec<String>),
}

/// Fuzzily resolves the `service` name the model passed to `activate_service`
/// to one connected server. Resolution order, FIRST hit wins (so a stricter
/// rule is never overridden by a looser one):
///   a. exact `name` match;
///   b. `service_registry::normalize`-equal match (case/space/punct-insensitive);
///   c. registry-alias match — the request resolves via
///      `service_registry::lookup` to a curated key, and exactly one connected
///      server's name resolves to that SAME key (>1 → `Ambiguous`);
///   d. unique substring — the normalized request is a substring of exactly
///      ONE connected server's normalized name (>1 → `Ambiguous`, 0 →
///      `NotFound`).
/// Ambiguity is never resolved by guessing: rules (c)/(d) yield `Ambiguous`
/// rather than picking one of several candidates.
pub fn resolve_service<'a>(requested: &str, servers: &'a [McpServerSnapshot]) -> ServiceMatch<'a> {
    // a. exact name match (the original, strictest behavior).
    if let Some(s) = servers.iter().find(|s| s.name == requested) {
        return ServiceMatch::Found(s);
    }
    let req_norm = crate::agent::service_registry::normalize(requested);
    if req_norm.is_empty() {
        return ServiceMatch::NotFound;
    }
    // b. normalized-equal match — "gmail" / "Gmail" / "G Mail" all resolve.
    if let Some(s) = servers
        .iter()
        .find(|s| crate::agent::service_registry::normalize(&s.name) == req_norm)
    {
        return ServiceMatch::Found(s);
    }
    // c. registry match — the request resolves to a curated key, by name/alias
    //    ([`lookup`], e.g. "Google Mail" -> gmail) OR by intent keyword
    //    ([`key_for_keyword`], e.g. "email" -> gmail); then match a connected
    //    server that resolves to that same key. This is the only rule that can
    //    see through a synonym the server's own name doesn't contain.
    let req_key = crate::agent::service_registry::lookup(requested)
        .map(|c| c.key)
        .or_else(|| crate::agent::service_registry::key_for_keyword(requested));
    if let Some(key) = req_key {
        let by_key: Vec<&McpServerSnapshot> = servers
            .iter()
            .filter(|s| crate::agent::service_registry::lookup(&s.name).map(|c| c.key) == Some(key))
            .collect();
        match by_key.as_slice() {
            [one] => return ServiceMatch::Found(one),
            [_, _, ..] => {
                return ServiceMatch::Ambiguous(by_key.iter().map(|s| s.name.clone()).collect())
            }
            [] => {}
        }
    }
    // d. unique substring — the normalized request appears inside exactly one
    //    connected server's normalized name.
    let by_substr: Vec<&McpServerSnapshot> = servers
        .iter()
        .filter(|s| crate::agent::service_registry::normalize(&s.name).contains(&req_norm))
        .collect();
    match by_substr.as_slice() {
        [one] => ServiceMatch::Found(one),
        [] => ServiceMatch::NotFound,
        _ => ServiceMatch::Ambiguous(by_substr.iter().map(|s| s.name.clone()).collect()),
    }
}

/// Splits a user message into lowercase alphanumeric tokens for keyword
/// scoring — every run of non-alphanumerics is a separator, so "what's on my
/// calendar?" -> ["what", "s", "on", "my", "calendar"].
fn tokenize(message: &str) -> Vec<String> {
    message
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Conservative doce-side pre-activation (Phase 4): given the user's latest
/// message and the connected servers, returns AT MOST ONE server name to
/// pre-activate so the small model often skips the explicit `activate_service`
/// hop. Deliberately timid — a wrong pre-activation spends a needless connect
/// and clutters the tool list, so the bar is high:
///   * a CURATED server scores one point per curated keyword present as a
///     whole token in the message, PLUS one if its own normalized name appears
///     in the message; it's a candidate only with ≥1 keyword hit (a bare
///     name mention is not enough on its own);
///   * an UNKNOWN server (no curated entry) is a candidate only when its
///     literal normalized name appears in the message, scoring one point;
///   * of the candidates, only the single top scorer is returned, and a TIE
///     for top yields NOTHING (never guess between equally-likely services).
///
/// Empty when there are no servers, so the no-server path is untouched.
pub fn services_to_autoactivate(user_message: &str, servers: &[McpServerSnapshot]) -> Vec<String> {
    let tokens = tokenize(user_message);
    let token_set: std::collections::HashSet<&str> = tokens.iter().map(String::as_str).collect();
    let msg_norm = crate::agent::service_registry::normalize(user_message);

    let mut scored: Vec<(u32, &McpServerSnapshot)> = Vec::new();
    for s in servers {
        let name_norm = crate::agent::service_registry::normalize(&s.name);
        let name_hit = !name_norm.is_empty() && msg_norm.contains(&name_norm);
        let (keyword_hits, confident) = match crate::agent::service_registry::lookup(&s.name) {
            Some(curated) => {
                let hits = curated
                    .keywords
                    .iter()
                    .filter(|kw| token_set.contains(**kw))
                    .count() as u32;
                (hits, hits >= 1)
            }
            // Unknown server: only its own name appearing counts as a signal.
            None => (0, name_hit),
        };
        if confident {
            scored.push((keyword_hits + u32::from(name_hit), s));
        }
    }
    // Cap at 1: the single top scorer, but a tie for top resolves to nothing.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    match scored.as_slice() {
        [only] => vec![only.1.name.clone()],
        [top, second, ..] if top.0 > second.0 => vec![top.1.name.clone()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(name: &str) -> McpServerSnapshot {
        McpServerSnapshot {
            id: format!("id-{name}"),
            name: name.to_string(),
            config: McpTransportConfig::Stdio {
                command: "x".to_string(),
                args: vec![],
            },
        }
    }

    fn tool(server: &str, raw: &str) -> ActivatedTool {
        make_activated_tool(
            server,
            &McpTransportConfig::Stdio {
                command: "x".to_string(),
                args: vec![],
            },
            &McpToolSchema {
                name: raw.to_string(),
                description: Some(format!("does {raw}")),
                input_schema: serde_json::json!({"type":"object","properties":{}}),
            },
        )
    }

    // --- BYTE-INVARIANCE (the benchmark gate) ------------------------

    #[test]
    fn byte_invariance_tools_array_equals_base_with_no_servers() {
        let base = crate::agent::plan::PlanState::default().single_mode_tool_names(true);
        let expected = crate::inference::http::tools_array(base);
        // No servers -> no `activate_service`, no MCP tools, even if the
        // activated slice is somehow non-empty (it can't be, but prove the
        // gate is the `has_servers` flag, not the slice).
        assert_eq!(build_tools_array(base, false, &[]), expected);
        assert_eq!(
            build_tools_array(base, false, &[tool("svc", "echo")]),
            expected
        );
    }

    #[test]
    fn byte_invariance_catalog_is_empty_with_no_servers() {
        assert_eq!(render_catalog(&[], &[]), "");
        assert_eq!(render_catalog(&[], &[tool("svc", "echo")]), "");
    }

    #[test]
    fn with_servers_appends_activate_service_and_activated_defs() {
        let base = crate::agent::plan::PlanState::default().single_mode_tool_names(true);
        let plain = crate::inference::http::tools_array(base);
        let t = tool("svc", "echo");
        let with = build_tools_array(base, true, std::slice::from_ref(&t));
        // base + activate_service + one activated tool
        assert_eq!(with.len(), plain.len() + 2);
        assert_eq!(
            with[plain.len()]["function"]["name"],
            serde_json::json!("activate_service")
        );
        assert_eq!(with[plain.len() + 1], t.def_json);
    }

    // --- Catalog rendering -------------------------------------------

    #[test]
    fn catalog_shows_curated_description_for_known_and_bare_name_for_unknown() {
        // "github" is unknown (bare name); "Gmail" is curated (name + blurb).
        let snaps = [snapshot("github"), snapshot("Gmail")];
        let out = render_catalog(&snaps, &[]);
        assert_eq!(
            out,
            "Connected services — call activate_service to load one before using its tools:\n- github\n- Gmail: search, read & draft email"
        );
    }

    #[test]
    fn catalog_marks_activated_service_and_lists_it() {
        let snaps = [snapshot("github"), snapshot("Gmail")];
        let activated = [tool("Gmail", "send_email")];
        let out = render_catalog(&snaps, &activated);
        assert!(out.contains("- github\n"));
        // Known service keeps its curated blurb even when activated.
        assert!(out.contains("- Gmail (activated): search, read & draft email"));
        assert!(out.ends_with("Currently activated: Gmail"));
    }

    // --- Phase 2: activation guidance --------------------------------

    #[test]
    fn activation_result_appends_curated_skill_for_known_service() {
        let names = vec!["gmail__search".to_string(), "gmail__draft".to_string()];
        // A server's own instructions must NOT override a curated skill.
        let out = build_activation_result("Gmail", &names, Some("ignored server instructions"));
        assert!(
            out.starts_with("Activated \"Gmail\". You can now call: gmail__search, gmail__draft.")
        );
        assert!(out.contains("How to use Gmail:"));
        // The curated gmail skill's hard guardrail rides along.
        assert!(out.contains("without the user's explicit confirmation"));
        assert!(!out.contains("ignored server instructions"));
    }

    #[test]
    fn activation_result_falls_back_to_server_instructions_when_unknown() {
        let names = vec!["github__create_issue".to_string()];
        let out =
            build_activation_result("github", &names, Some("Call create_issue to file bugs."));
        assert!(out.contains("How to use github:\nCall create_issue to file bugs."));
    }

    #[test]
    fn activation_result_adds_nothing_extra_for_unknown_without_instructions() {
        let names = vec!["github__create_issue".to_string()];
        let out = build_activation_result("github", &names, None);
        assert_eq!(
            out,
            "Activated \"github\". You can now call: github__create_issue."
        );
        // Blank/whitespace-only instructions are treated as absent too.
        let blank = build_activation_result("github", &names, Some("   \n  "));
        assert_eq!(blank, out);
    }

    #[test]
    fn activation_result_reports_no_tools_but_still_adds_skill() {
        let out = build_activation_result("Gmail", &[], None);
        assert!(out.starts_with("Activated \"Gmail\", but it exposes no tools."));
        assert!(out.contains("How to use Gmail:"));
    }

    // --- Naming / round-trip -----------------------------------------

    #[test]
    fn sanitize_lowercases_and_replaces_illegal_chars() {
        assert_eq!(sanitize("My Server!"), "my_server_");
        assert_eq!(sanitize("git-hub_1"), "git-hub_1");
    }

    #[test]
    fn advertised_name_namespaces_and_stays_in_envelope() {
        let name = advertised_name("My Server", "sendEmail");
        assert_eq!(name, "my_server__sendEmail");
        assert!(
            in_tool_name_envelope(&name),
            "{name} must match the tool-name envelope"
        );
    }

    /// `^[a-zA-Z0-9_-]{1,64}$` without pulling in a regex dependency.
    fn in_tool_name_envelope(name: &str) -> bool {
        (1..=64).contains(&name.len())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }

    #[test]
    fn advertised_name_truncates_to_64() {
        let long = "a".repeat(200);
        let name = advertised_name("svc", &long);
        assert_eq!(name.len(), 64);
    }

    #[test]
    fn advertised_name_round_trips_exact_match_lookup() {
        let t = tool("GitHub", "create_issue");
        assert_eq!(t.advertised_name, "github__create_issue");
        let activated = [tool("GitHub", "create_issue"), tool("GitHub", "list_prs")];
        let found = activated
            .iter()
            .find(|a| a.advertised_name == t.advertised_name)
            .unwrap();
        // The raw name (what we actually send to the MCP server) survives the
        // advertised-name mangling.
        assert_eq!(found.raw_name, "create_issue");
    }

    #[test]
    fn activated_tool_def_json_is_openai_wrapper_shape() {
        let t = tool("svc", "echo");
        assert_eq!(t.def_json["type"], serde_json::json!("function"));
        assert_eq!(
            t.def_json["function"]["name"],
            serde_json::json!("svc__echo")
        );
        assert_eq!(
            t.def_json["function"]["description"],
            serde_json::json!("does echo")
        );
        assert!(t.def_json["function"]["parameters"].is_object());
    }

    // --- Phase 4: fuzzy activate_service resolution ------------------

    fn found_name<'a>(m: &ServiceMatch<'a>) -> Option<&'a str> {
        match m {
            ServiceMatch::Found(s) => Some(s.name.as_str()),
            _ => None,
        }
    }

    #[test]
    fn resolve_prefers_exact_name_match() {
        let snaps = [snapshot("Gmail"), snapshot("github")];
        assert_eq!(found_name(&resolve_service("Gmail", &snaps)), Some("Gmail"));
        assert_eq!(
            found_name(&resolve_service("github", &snaps)),
            Some("github")
        );
    }

    #[test]
    fn resolve_matches_case_space_and_punct_insensitively() {
        let snaps = [snapshot("Gmail")];
        // Normalized-equal (rule b): case/space/punct differences resolve.
        for req in ["gmail", "GMAIL", "G Mail", "g-mail"] {
            assert_eq!(
                found_name(&resolve_service(req, &snaps)),
                Some("Gmail"),
                "{req:?} should resolve to the Gmail server"
            );
        }
    }

    #[test]
    fn resolve_matches_via_registry_alias_synonym() {
        // "email" / "Google Mail" resolve to the gmail curated key, and the
        // connected server also resolves there — a synonym its name lacks.
        let snaps = [snapshot("Gmail"), snapshot("github")];
        assert_eq!(found_name(&resolve_service("email", &snaps)), Some("Gmail"));
        assert_eq!(
            found_name(&resolve_service("Google Mail", &snaps)),
            Some("Gmail")
        );
    }

    #[test]
    fn resolve_unique_substring_matches() {
        // "cal" is a substring of exactly one connected server's normalized name.
        let snaps = [snapshot("My Calendar Thing"), snapshot("github")];
        assert_eq!(
            found_name(&resolve_service("calendar", &snaps)),
            Some("My Calendar Thing")
        );
    }

    #[test]
    fn resolve_ambiguous_substring_does_not_guess() {
        let snaps = [snapshot("Work Gmail"), snapshot("Home Gmail")];
        // Both normalized names contain "gmail" — ambiguous, name the candidates.
        match resolve_service("gmail", &snaps) {
            ServiceMatch::Ambiguous(names) => {
                assert!(names.contains(&"Work Gmail".to_string()));
                assert!(names.contains(&"Home Gmail".to_string()));
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn resolve_ambiguous_when_two_servers_share_a_curated_key() {
        // "email" resolves to the gmail key, and two connected servers do too.
        let snaps = [snapshot("Gmail"), snapshot("Google Mail")];
        match resolve_service("email", &snaps) {
            ServiceMatch::Ambiguous(names) => assert_eq!(names.len(), 2),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn resolve_not_found_for_unrelated_or_empty() {
        let snaps = [snapshot("Gmail"), snapshot("github")];
        assert!(matches!(
            resolve_service("dropbox", &snaps),
            ServiceMatch::NotFound
        ));
        assert!(matches!(
            resolve_service("", &snaps),
            ServiceMatch::NotFound
        ));
    }

    // --- Phase 4: conservative auto-activation -----------------------

    #[test]
    fn autoactivate_empty_with_no_servers() {
        assert!(services_to_autoactivate("anything at all", &[]).is_empty());
    }

    #[test]
    fn autoactivate_matches_inbox_to_gmail_server() {
        let snaps = [snapshot("Gmail"), snapshot("github")];
        assert_eq!(
            services_to_autoactivate("triage my inbox please", &snaps),
            vec!["Gmail".to_string()]
        );
    }

    #[test]
    fn autoactivate_matches_calendar_message_to_calendar_server() {
        let snaps = [snapshot("Google Calendar"), snapshot("github")];
        assert_eq!(
            services_to_autoactivate("what's on my calendar tomorrow", &snaps),
            vec!["Google Calendar".to_string()]
        );
    }

    #[test]
    fn autoactivate_no_match_for_generic_message() {
        let snaps = [snapshot("Gmail"), snapshot("Google Calendar")];
        assert!(services_to_autoactivate("help me think through this", &snaps).is_empty());
    }

    #[test]
    fn autoactivate_caps_at_one_service() {
        // A message hitting BOTH services: only the stronger scorer is returned.
        let snaps = [snapshot("Gmail"), snapshot("Google Calendar")];
        let picked = services_to_autoactivate(
            "reply to that email and check my inbox, then my calendar",
            &snaps,
        );
        assert_eq!(picked.len(), 1);
        assert_eq!(picked, vec!["Gmail".to_string()]);
    }

    #[test]
    fn autoactivate_tie_resolves_to_nothing() {
        // One keyword hit each -> tie for top -> never guess.
        let snaps = [snapshot("Gmail"), snapshot("Google Calendar")];
        assert!(services_to_autoactivate("send an email about the meeting", &snaps).is_empty());
    }

    #[test]
    fn autoactivate_unknown_server_matches_only_on_its_name() {
        let snaps = [snapshot("Linear")];
        // No curated keywords for "Linear": only its literal name fires.
        assert!(services_to_autoactivate("close this ticket", &snaps).is_empty());
        assert_eq!(
            services_to_autoactivate("open the linear board", &snaps),
            vec!["Linear".to_string()]
        );
    }
}
