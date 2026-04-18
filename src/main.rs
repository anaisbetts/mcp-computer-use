use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HelloWorldRequest {
    /// Optional name to greet.
    #[schemars(description = "Optional name to greet")]
    pub name: Option<String>,
}

#[derive(Clone)]
struct HelloWorldServer {
    tool_router: ToolRouter<Self>,
}

impl HelloWorldServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for HelloWorldServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl HelloWorldServer {
    #[tool(description = "Return a hello-world greeting")]
    fn hello_world(&self, Parameters(req): Parameters<HelloWorldRequest>) -> String {
        match req.name {
            Some(ref n) if !n.trim().is_empty() => format!("Hello, {}!", n.trim()),
            _ => "Hello, world!".to_string(),
        }
    }
}

#[tool_handler]
impl ServerHandler for HelloWorldServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Hello world MCP server.".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let service = HelloWorldServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_world_default_greeting() {
        let server = HelloWorldServer::new();
        let out = server.hello_world(Parameters(HelloWorldRequest { name: None }));
        assert_eq!(out, "Hello, world!");
    }

    #[test]
    fn hello_world_named_greeting() {
        let server = HelloWorldServer::new();
        let out = server.hello_world(Parameters(HelloWorldRequest {
            name: Some("Ada".to_string()),
        }));
        assert_eq!(out, "Hello, Ada!");
    }
}
