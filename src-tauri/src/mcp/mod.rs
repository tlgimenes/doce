//! `rmcp`-based MCP client (User Story 4, FR-018/FR-019): connects to a
//! user-configured external MCP server over stdio and lists the tools it
//! exposes, so they can be surfaced into the agent's tool-use loop
//! alongside the built-in tool set. HTTP transport (`data-model.md`'s
//! `MCPServerConnection.transport = 'http'`) is not implemented in this
//! pass — only `stdio`, the more common case for locally-run MCP servers.

use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::ServiceExt;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("failed to spawn MCP server process: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("MCP client error: {0}")]
    Client(String),
}

/// Spawns `command` (with `args`) as a child process, speaks MCP over its
/// stdio, and returns the list of tools it exposes. The connection is
/// closed before returning — this is a point-in-time capability query
/// (e.g. for a settings-panel "test connection" action), not a
/// long-lived session; wiring a persistent connection into the agent
/// loop's tool dispatch is a further step (FR-019) not built in this pass.
pub async fn list_tools_stdio(
    command: &str,
    args: &[String],
) -> Result<Vec<McpToolInfo>, McpError> {
    let args_owned = args.to_vec();
    let transport = TokioChildProcess::new(Command::new(command).configure(|cmd| {
        cmd.args(&args_owned);
    }))?;

    let client = ().serve(transport).await.map_err(|e| McpError::Client(e.to_string()))?;
    let tools = client
        .list_all_tools()
        .await
        .map_err(|e| McpError::Client(e.to_string()))?;
    let _ = client.cancel().await;

    Ok(tools
        .into_iter()
        .map(|t| McpToolInfo {
            name: t.name.to_string(),
            description: t.description.map(|d| d.to_string()),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::handler::server::router::tool::ToolRouter;
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::{ServerCapabilities, ServerInfo};
    use rmcp::{tool, tool_handler, tool_router, ServerHandler};

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

    /// Exercises the real client-side connect/list-tools path against a
    /// real (if minimal) in-process MCP server over an in-memory duplex
    /// stream — no external binary needed, so this runs anywhere `cargo
    /// test` runs, unlike a test that shells out to `npx` or `python`.
    #[tokio::test]
    async fn lists_tools_from_a_real_mcp_server() {
        let (server_transport, client_transport) = tokio::io::duplex(4096);

        let server = TestServer::new();
        let server_handle = tokio::spawn(async move {
            let running = server.serve(server_transport).await.unwrap();
            running.waiting().await.unwrap();
        });

        let client = ().serve(client_transport).await.unwrap();
        let tools = client.list_all_tools().await.unwrap();
        client.cancel().await.unwrap();

        let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        assert!(names.contains(&"echo".to_string()));
        assert!(names.contains(&"add".to_string()));

        let echo_tool = tools.iter().find(|t| t.name == "echo").unwrap();
        assert_eq!(
            echo_tool.description.as_deref(),
            Some("Echoes a message back")
        );

        server_handle.abort();
    }
}
