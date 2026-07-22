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
/// `- <name>` line per connected service (marked `(activated)` when its tools
/// are loaded), then a `Currently activated: ...` line when any are.
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
        if activated_servers.contains(s.name.as_str()) {
            out.push_str(&format!("\n- {} (activated)", s.name));
        } else {
            out.push_str(&format!("\n- {}", s.name));
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
    fn catalog_lists_servers_one_per_line() {
        let snaps = [snapshot("github"), snapshot("gmail")];
        let out = render_catalog(&snaps, &[]);
        assert_eq!(
            out,
            "Connected services — call activate_service to load one before using its tools:\n- github\n- gmail"
        );
    }

    #[test]
    fn catalog_marks_activated_service_and_lists_it() {
        let snaps = [snapshot("github"), snapshot("gmail")];
        let activated = [tool("gmail", "send_email")];
        let out = render_catalog(&snaps, &activated);
        assert!(out.contains("- github\n"));
        assert!(out.contains("- gmail (activated)"));
        assert!(out.ends_with("Currently activated: gmail"));
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
}
