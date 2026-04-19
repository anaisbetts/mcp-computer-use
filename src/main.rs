//! MCP server: dual-mode computer-use stubs (`--batch` for batched OpenAI-style tool).

use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};

mod computer_use;

use computer_use::{
    ClickToolRequest, ComputerUseRequest, DoubleClickToolRequest, DragToolRequest,
    KeypressToolRequest, MoveToolRequest, ScrollToolRequest, TypeToolRequest, response_to_json,
    stub_dispatch_batch,
};

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const INSTRUCTIONS_BATCH: &str =
    "MCP server with batched computer_use (OpenAI-style actions[]). Computer actions are stubs.";

const INSTRUCTIONS_SPLIT: &str =
    "MCP server with per-action computer_* tools. Computer actions are stubs.";

// -----------------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------------

#[derive(Clone)]
struct BatchComputerServer {
    tool_router: ToolRouter<Self>,
}

#[derive(Clone)]
struct SplitComputerServer {
    tool_router: ToolRouter<Self>,
}

// -----------------------------------------------------------------------------
// Batch mode server
// -----------------------------------------------------------------------------

impl BatchComputerServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for BatchComputerServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl BatchComputerServer {
    fn tool_names_sorted(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.into_owned())
            .collect();
        names.sort();
        names
    }
}

#[tool_router(router = tool_router)]
impl BatchComputerServer {
    #[tool(
        name = "computer_use",
        description = "Execute OpenAI-style computer use actions in order (batched). Stub only."
    )]
    fn computer_use(&self, Parameters(req): Parameters<ComputerUseRequest>) -> String {
        response_to_json(&stub_dispatch_batch(&req.actions))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BatchComputerServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(INSTRUCTIONS_BATCH.into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

// -----------------------------------------------------------------------------
// Split mode server
// -----------------------------------------------------------------------------

impl SplitComputerServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for SplitComputerServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl SplitComputerServer {
    fn tool_names_sorted(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.into_owned())
            .collect();
        names.sort();
        names
    }
}

#[tool_router(router = tool_router)]
impl SplitComputerServer {
    #[tool(description = "Computer use: click (stub)")]
    fn computer_click(&self, Parameters(req): Parameters<ClickToolRequest>) -> String {
        response_to_json(&stub_dispatch_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: double_click (stub)")]
    fn computer_double_click(&self, Parameters(req): Parameters<DoubleClickToolRequest>) -> String {
        response_to_json(&stub_dispatch_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: scroll (stub)")]
    fn computer_scroll(&self, Parameters(req): Parameters<ScrollToolRequest>) -> String {
        response_to_json(&stub_dispatch_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: type (stub)")]
    fn computer_type(&self, Parameters(req): Parameters<TypeToolRequest>) -> String {
        response_to_json(&stub_dispatch_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: wait (stub)")]
    fn computer_wait(&self) -> String {
        response_to_json(&stub_dispatch_batch(&[computer_use::ComputerAction::Wait]))
    }

    #[tool(description = "Computer use: keypress (stub)")]
    fn computer_keypress(&self, Parameters(req): Parameters<KeypressToolRequest>) -> String {
        response_to_json(&stub_dispatch_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: drag (stub)")]
    fn computer_drag(&self, Parameters(req): Parameters<DragToolRequest>) -> String {
        response_to_json(&stub_dispatch_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: move (stub)")]
    fn computer_move(&self, Parameters(req): Parameters<MoveToolRequest>) -> String {
        response_to_json(&stub_dispatch_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: screenshot (stub)")]
    fn computer_screenshot(&self) -> String {
        response_to_json(&stub_dispatch_batch(&[
            computer_use::ComputerAction::Screenshot,
        ]))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SplitComputerServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(INSTRUCTIONS_SPLIT.into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

// -----------------------------------------------------------------------------
// Entry point
// -----------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let mode = computer_use::tool_mode_from_args_iter(std::env::args());

    match mode {
        computer_use::ToolMode::Batch => {
            let service = BatchComputerServer::new().serve(stdio()).await?;
            service.waiting().await?;
        }
        computer_use::ToolMode::Split => {
            let service = SplitComputerServer::new().serve(stdio()).await?;
            service.waiting().await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_server_exposes_only_computer_use() {
        let s = BatchComputerServer::new();
        let names = s.tool_names_sorted();
        assert_eq!(names, vec!["computer_use".to_string()]);
    }

    #[test]
    fn split_server_exposes_per_action_tools() {
        let s = SplitComputerServer::new();
        let names = s.tool_names_sorted();
        let expected = vec![
            "computer_click",
            "computer_double_click",
            "computer_drag",
            "computer_keypress",
            "computer_move",
            "computer_screenshot",
            "computer_scroll",
            "computer_type",
            "computer_wait",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
        assert_eq!(names, expected);
    }

    #[test]
    fn batch_tool_routes_stub_json() {
        let s = BatchComputerServer::new();
        let json = s.computer_use(Parameters(ComputerUseRequest {
            actions: vec![computer_use::ComputerAction::Wait],
        }));
        assert!(json.contains("not_implemented"));
        assert!(json.contains("wait"));
    }
}
