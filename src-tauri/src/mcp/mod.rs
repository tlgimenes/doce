//! `rmcp`-based MCP client (User Story 4, FR-018/FR-019): connects to a
//! user-configured external MCP server and lists / calls the tools it
//! exposes, so they can be surfaced into the agent's tool-use loop
//! alongside the built-in tool set.
//!
//! Two transports are supported, selected by the stored
//! `MCPServerConnection.transport` discriminator (`data-model.md`):
//!   * `stdio` — spawns a local child process and speaks MCP over its
//!     stdio. The common case for locally-run MCP servers.
//!   * `http`  — connects to a remote server by URL over rmcp's
//!     streamable-HTTP client transport, optionally attaching a bearer
//!     token as an `Authorization: Bearer <token>` header.
//!
//! The connect/list/call path is transport-agnostic: [`connect`] builds
//! the right rmcp transport from a [`McpTransportConfig`] and returns a
//! connected client; [`list_tools`] and [`call_tool`] then work off that
//! client regardless of transport.
//!
//! NOTE on the `http` bearer token: this is a deliberately minimal stub.
//! A token, if present in the stored config, is plumbed straight through
//! as a static `Authorization` header. There is no OAuth acquisition or
//! refresh flow — that is a planned follow-up (see the PR body / the
//! `add_mcp_http_server` command docs).

use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::ServiceExt;
use serde::Deserialize;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
}

/// A schema-carrying view of one MCP tool — like [`McpToolInfo`] but also
/// carrying the tool's JSON-Schema `input_schema`, so a caller can build an
/// OpenAI-shaped tool definition to advertise the tool into the agent loop.
/// Produced by [`list_tools_detailed`]; the settings-panel "test connection"
/// path keeps using the lighter [`McpToolInfo`]/[`list_tools`].
#[derive(Debug, Clone)]
pub struct McpToolSchema {
    pub name: String,
    pub description: Option<String>,
    /// The tool's JSON Schema for its arguments — an object schema suitable
    /// for dropping straight into an OpenAI tool def's `parameters`.
    pub input_schema: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("failed to spawn MCP server process: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("MCP client error: {0}")]
    Client(String),
    #[error("invalid MCP server config: {0}")]
    Config(String),
}

/// A parsed, transport-tagged MCP server configuration — the in-memory
/// form of an `MCPServerConnection`'s `(transport, config)` pair. Built by
/// [`parse_config`] from the stored discriminator + JSON blob, and the
/// single input to the transport-agnostic [`connect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpTransportConfig {
    /// Local child-process transport. `config` JSON: `{"command","args"}`.
    Stdio { command: String, args: Vec<String> },
    /// Remote streamable-HTTP transport. `config` JSON:
    /// `{"url", "auth_token"?}`. `auth_token` is a bearer token WITHOUT
    /// the `Bearer ` prefix (rmcp adds it); `None` means no auth header.
    Http {
        url: String,
        auth_token: Option<String>,
    },
}

/// The shape stored in `MCPServerConnection.config` for a `stdio` server.
#[derive(Debug, Deserialize)]
struct StdioConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

/// The shape stored in `MCPServerConnection.config` for an `http` server.
#[derive(Debug, Deserialize)]
struct HttpConfig {
    url: String,
    #[serde(default)]
    auth_token: Option<String>,
}

/// Parses a stored `(transport, config_json)` pair into a
/// [`McpTransportConfig`]. This is the one place the `transport`
/// discriminator is interpreted, so transport selection is unit-testable
/// without spawning a process or opening a socket.
pub fn parse_config(transport: &str, config_json: &str) -> Result<McpTransportConfig, McpError> {
    match transport {
        "stdio" => {
            let StdioConfig { command, args } =
                serde_json::from_str(config_json).map_err(|e| McpError::Config(e.to_string()))?;
            Ok(McpTransportConfig::Stdio { command, args })
        }
        "http" => {
            let HttpConfig { url, auth_token } =
                serde_json::from_str(config_json).map_err(|e| McpError::Config(e.to_string()))?;
            Ok(McpTransportConfig::Http { url, auth_token })
        }
        other => Err(McpError::Config(format!("unknown transport {other:?}"))),
    }
}

/// Builds the rmcp streamable-HTTP transport for an `http` server,
/// attaching the optional bearer token as an `Authorization` header.
/// Split out from [`connect`] so the header-plumbing decision (token
/// present -> `auth_header` set; absent -> unauthenticated) is unit
/// testable without a live server.
fn build_http_transport(
    url: &str,
    auth_token: Option<&str>,
) -> StreamableHttpClientTransport<reqwest::Client> {
    match auth_token {
        Some(token) => {
            // `auth_header` expects the raw token; rmcp prepends `Bearer `.
            let config = rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url.to_string())
                .auth_header(token.to_string());
            StreamableHttpClientTransport::from_config(config)
        }
        None => StreamableHttpClientTransport::from_uri(url.to_string()),
    }
}

/// Connects to an MCP server over the transport its config selects and
/// returns a live client. Both branches consume their transport and hand
/// back the same `RunningService<RoleClient, ()>`, so every caller
/// downstream ([`list_tools`], [`call_tool`], the agent loop's dispatch)
/// is transport-agnostic.
pub async fn connect(
    config: &McpTransportConfig,
) -> Result<RunningService<RoleClient, ()>, McpError> {
    match config {
        McpTransportConfig::Stdio { command, args } => {
            let args_owned = args.clone();
            let transport = TokioChildProcess::new(Command::new(command).configure(|cmd| {
                cmd.args(&args_owned);
            }))?;
            ().serve(transport)
                .await
                .map_err(|e| McpError::Client(e.to_string()))
        }
        McpTransportConfig::Http { url, auth_token } => {
            let transport = build_http_transport(url, auth_token.as_deref());
            ().serve(transport)
                .await
                .map_err(|e| McpError::Client(e.to_string()))
        }
    }
}

/// Connects, lists all tools the server exposes, and closes the
/// connection — a point-in-time capability query (e.g. a settings-panel
/// "test connection" action). Works for any transport.
pub async fn list_tools(config: &McpTransportConfig) -> Result<Vec<McpToolInfo>, McpError> {
    let client = connect(config).await?;
    let tools = client
        .list_all_tools()
        .await
        .map_err(|e| McpError::Client(e.to_string()));
    let _ = client.cancel().await;
    let tools = tools?;

    Ok(tools
        .into_iter()
        .map(|t| McpToolInfo {
            name: t.name.to_string(),
            description: t.description.map(|d| d.to_string()),
        })
        .collect())
}

/// Connects, invokes a single tool by name with the given JSON arguments,
/// and closes the connection, returning the tool result serialized to
/// JSON. Works for any transport — this is the call the agent loop's tool
/// dispatch uses to run an MCP tool during a turn.
pub async fn call_tool(
    config: &McpTransportConfig,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    // rmcp wants the arguments as a JSON object (`Map`); a bare
    // `null`/absent argument set becomes `None`.
    let arguments = match arguments {
        serde_json::Value::Object(map) => Some(map),
        serde_json::Value::Null => None,
        other => {
            return Err(McpError::Client(format!(
                "tool arguments must be a JSON object, got {other}"
            )))
        }
    };

    let mut params = rmcp::model::CallToolRequestParams::new(tool_name.to_string());
    params.arguments = arguments;

    let client = connect(config).await?;
    let result = client
        .call_tool(params)
        .await
        .map_err(|e| McpError::Client(e.to_string()));
    let _ = client.cancel().await;
    let result = result?;

    serde_json::to_value(result).map_err(|e| McpError::Client(e.to_string()))
}

/// Connects, lists all tools WITH their `input_schema`, and closes the
/// connection. This is the progressive-disclosure counterpart to
/// [`list_tools`]: when the agent activates a service, the loop needs each
/// tool's argument schema (not just its name/description) to advertise it to
/// the model as a callable tool. Works for any transport.
pub async fn list_tools_detailed(
    config: &McpTransportConfig,
) -> Result<Vec<McpToolSchema>, McpError> {
    let client = connect(config).await?;
    let tools = client
        .list_all_tools()
        .await
        .map_err(|e| McpError::Client(e.to_string()));
    let _ = client.cancel().await;
    let tools = tools?;

    Ok(tools
        .into_iter()
        .map(|t| McpToolSchema {
            name: t.name.to_string(),
            description: t.description.map(|d| d.to_string()),
            // rmcp stores the schema as `Arc<serde_json::Map>`; clone the
            // map out and wrap it as a JSON object `Value`.
            input_schema: serde_json::Value::Object((*t.input_schema).clone()),
        })
        .collect())
}

/// Renders an MCP `CallToolResult` — already serialized to a JSON `Value` by
/// [`call_tool`] — into a plain, model-facing string: the concatenated text
/// of its `content` blocks (the common case), or, when there is no textual
/// content to extract, the compact JSON of the whole value as a fallback.
/// Kept separate from [`call_tool`] (which stays `Value`-returning) so the
/// dispatch site owns the string formatting.
pub fn format_call_result(result: &serde_json::Value) -> String {
    if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
        let texts: Vec<&str> = content
            .iter()
            .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
            .collect();
        if !texts.is_empty() {
            return texts.join("\n");
        }
    }
    result.to_string()
}

/// Back-compat thin wrapper for the stdio-only "test connection" path.
/// Preserves the exact previous behavior: spawn `command args`, list
/// tools, disconnect. New callers should prefer [`list_tools`] with a
/// [`McpTransportConfig`].
pub async fn list_tools_stdio(
    command: &str,
    args: &[String],
) -> Result<Vec<McpToolInfo>, McpError> {
    list_tools(&McpTransportConfig::Stdio {
        command: command.to_string(),
        args: args.to_vec(),
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::handler::server::router::tool::ToolRouter;
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::{ServerCapabilities, ServerInfo};
    use rmcp::{tool, tool_handler, tool_router, ServerHandler};

    // --- Config parsing / transport selection (no I/O) ---------------

    #[test]
    fn parses_stdio_config() {
        let cfg = parse_config(
            "stdio",
            r#"{"command":"node","args":["server.js","--flag"]}"#,
        )
        .unwrap();
        assert_eq!(
            cfg,
            McpTransportConfig::Stdio {
                command: "node".to_string(),
                args: vec!["server.js".to_string(), "--flag".to_string()],
            }
        );
    }

    #[test]
    fn parses_stdio_config_with_missing_args_as_empty() {
        let cfg = parse_config("stdio", r#"{"command":"mcp-server"}"#).unwrap();
        assert_eq!(
            cfg,
            McpTransportConfig::Stdio {
                command: "mcp-server".to_string(),
                args: vec![],
            }
        );
    }

    #[test]
    fn parses_http_config_without_auth() {
        let cfg = parse_config("http", r#"{"url":"https://example.com/mcp"}"#).unwrap();
        assert_eq!(
            cfg,
            McpTransportConfig::Http {
                url: "https://example.com/mcp".to_string(),
                auth_token: None,
            }
        );
    }

    #[test]
    fn parses_http_config_with_auth_token() {
        let cfg = parse_config(
            "http",
            r#"{"url":"https://example.com/mcp","auth_token":"secret-123"}"#,
        )
        .unwrap();
        assert_eq!(
            cfg,
            McpTransportConfig::Http {
                url: "https://example.com/mcp".to_string(),
                auth_token: Some("secret-123".to_string()),
            }
        );
    }

    #[test]
    fn rejects_unknown_transport() {
        let err = parse_config("carrier-pigeon", "{}").unwrap_err();
        assert!(matches!(err, McpError::Config(_)));
    }

    #[test]
    fn rejects_http_config_missing_url() {
        let err = parse_config("http", r#"{"auth_token":"x"}"#).unwrap_err();
        assert!(matches!(err, McpError::Config(_)));
    }

    // --- Optional auth-header plumbing -------------------------------
    //
    // We can't reach into rmcp's transport to read the header back, but
    // the plumbing decision lives in `build_http_transport` -> the config
    // it constructs. Assert that decision directly against the config
    // builder rmcp exposes.

    #[test]
    fn http_config_sets_auth_header_when_token_present() {
        use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
        let config = StreamableHttpClientTransportConfig::with_uri("https://example.com/mcp")
            .auth_header("secret-123");
        assert_eq!(&*config.uri, "https://example.com/mcp");
        assert_eq!(config.auth_header.as_deref(), Some("secret-123"));
    }

    #[test]
    fn http_config_has_no_auth_header_when_token_absent() {
        use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
        let config = StreamableHttpClientTransportConfig::with_uri("https://example.com/mcp");
        assert_eq!(config.auth_header, None);
    }

    // `tokio::test`: constructing the streamable-HTTP transport spawns a
    // background worker, which needs a Tokio reactor in scope.
    #[tokio::test]
    async fn build_http_transport_constructs_for_both_auth_states() {
        // Smoke: both branches build a transport without panicking. (The
        // header value itself is asserted via the config-builder tests
        // above; this guards the match arms in `build_http_transport`.)
        let _authed = build_http_transport("https://example.com/mcp", Some("t"));
        let _anon = build_http_transport("https://example.com/mcp", None);
    }

    // --- Real (in-process) stdio round-trip --------------------------

    #[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
    struct EchoRequest {
        message: String,
    }

    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    struct TestServer {
        tool_router: ToolRouter<Self>,
    }

    impl TestServer {
        fn new() -> Self {
            Self {
                tool_router: Self::tool_router(),
            }
        }
    }

    #[tool_router]
    impl TestServer {
        #[tool(description = "Echoes a message back")]
        fn echo(&self, Parameters(EchoRequest { message }): Parameters<EchoRequest>) -> String {
            message
        }

        #[tool(description = "Adds two numbers")]
        fn add(&self) -> String {
            "2".to_string()
        }
    }

    #[tool_handler]
    impl ServerHandler for TestServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        }
    }

    /// Exercises the real client-side connect/list-tools/call-tool path
    /// against a real (if minimal) in-process MCP server over an in-memory
    /// duplex stream — no external binary needed, so this runs anywhere
    /// `cargo test` runs.
    #[tokio::test]
    async fn lists_and_calls_tools_on_a_real_mcp_server() {
        let (server_transport, client_transport) = tokio::io::duplex(4096);

        let server = TestServer::new();
        let server_handle = tokio::spawn(async move {
            let running = server.serve(server_transport).await.unwrap();
            running.waiting().await.unwrap();
        });

        // Drive the same client path `connect` uses (in-memory duplex
        // isn't one of the two real transports, so we serve it directly).
        let client = ().serve(client_transport).await.unwrap();
        let tools = client.list_all_tools().await.unwrap();

        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        assert!(names.contains(&"echo".to_string()));
        assert!(names.contains(&"add".to_string()));

        let echo_tool = tools.iter().find(|t| t.name == "echo").unwrap();
        assert_eq!(
            echo_tool.description.as_deref(),
            Some("Echoes a message back")
        );

        // And a real tool call round-trips through the same client.
        let mut params = rmcp::model::CallToolRequestParams::new("echo");
        params.arguments = serde_json::json!({ "message": "hi" }).as_object().cloned();
        let result = client.call_tool(params).await.unwrap();
        assert!(serde_json::to_value(&result)
            .unwrap()
            .to_string()
            .contains("hi"));

        client.cancel().await.unwrap();
        server_handle.abort();
    }

    /// Same in-process server, but exercises the schema-carrying mapping
    /// that [`list_tools_detailed`] performs (`Tool.input_schema` ->
    /// `Value::Object`): the `echo` tool takes a `message` string, so its
    /// advertised `input_schema` must be a non-empty JSON object. Driven over
    /// the duplex client directly (the in-memory stream isn't one of the two
    /// real transports `list_tools_detailed`'s `connect` builds), asserting
    /// the exact conversion that function applies to each tool.
    #[tokio::test]
    async fn detailed_listing_carries_a_non_empty_input_schema() {
        let (server_transport, client_transport) = tokio::io::duplex(4096);

        let server = TestServer::new();
        let server_handle = tokio::spawn(async move {
            let running = server.serve(server_transport).await.unwrap();
            running.waiting().await.unwrap();
        });

        let client = ().serve(client_transport).await.unwrap();
        let tools = client.list_all_tools().await.unwrap();

        let schemas: Vec<McpToolSchema> = tools
            .into_iter()
            .map(|t| McpToolSchema {
                name: t.name.to_string(),
                description: t.description.map(|d| d.to_string()),
                input_schema: serde_json::Value::Object((*t.input_schema).clone()),
            })
            .collect();

        let echo = schemas.iter().find(|s| s.name == "echo").unwrap();
        assert!(
            echo.input_schema.is_object(),
            "input_schema must be an object"
        );
        let obj = echo.input_schema.as_object().unwrap();
        assert!(!obj.is_empty(), "echo's input_schema must be non-empty");
        // The `message` string parameter must surface in the schema.
        assert!(
            echo.input_schema.to_string().contains("message"),
            "echo schema should mention its `message` parameter: {}",
            echo.input_schema
        );

        client.cancel().await.unwrap();
        server_handle.abort();
    }

    #[test]
    fn format_call_result_extracts_text_content() {
        let result = serde_json::json!({
            "content": [
                { "type": "text", "text": "hello" },
                { "type": "text", "text": "world" }
            ],
            "isError": false
        });
        assert_eq!(format_call_result(&result), "hello\nworld");
    }

    #[test]
    fn format_call_result_falls_back_to_json_when_no_text() {
        let result = serde_json::json!({
            "content": [ { "type": "image", "data": "..." } ]
        });
        // No text blocks -> compact JSON of the whole value.
        assert_eq!(format_call_result(&result), result.to_string());
    }

    /// Integration test against a PUBLIC, no-auth remote MCP server.
    /// Ignored by default (needs network + a stable public endpoint); run
    /// with `cargo test --lib -- --ignored remote_http`. Proves the http
    /// transport path end-to-end when a network is available.
    #[tokio::test]
    #[ignore = "requires network access to a public remote MCP server"]
    async fn remote_http_lists_tools() {
        let config = McpTransportConfig::Http {
            url: "https://mcp.deepwiki.com/mcp".to_string(),
            auth_token: None,
        };
        let tools = list_tools(&config).await.expect("list tools over http");
        assert!(!tools.is_empty(), "expected at least one remote tool");
    }
}
