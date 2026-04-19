//! MCP server: dual-mode computer use (`--batch` for batched OpenAI-style tool).
//!
//! Both server types share a single [`Backend`] via `Arc`. Batch mode exposes
//! one `computer_use` tool that runs an array of actions; split mode exposes
//! one MCP tool per action, each routed through the same dispatcher.

use std::sync::Arc;

use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};

mod computer_use;
mod desktop;
mod scaling;
mod screen_capture;

use computer_use::{
    Backend, ClickToolRequest, ComputerUseRequest, DoubleClickToolRequest, DragToolRequest,
    KeypressToolRequest, MoveToolRequest, ScrollToolRequest, TypeToolRequest, response_to_json,
};

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const INSTRUCTIONS_BATCH: &str =
    "MCP server with batched computer_use (OpenAI-style actions[]). Drives the local desktop.";

const INSTRUCTIONS_SPLIT: &str =
    "MCP server with per-action computer_* tools. Drives the local desktop.";

// -----------------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------------

#[derive(Clone)]
struct BatchComputerServer {
    backend: Arc<Backend>,
    tool_router: ToolRouter<Self>,
}

#[derive(Clone)]
struct SplitComputerServer {
    backend: Arc<Backend>,
    tool_router: ToolRouter<Self>,
}

// -----------------------------------------------------------------------------
// Batch mode server
// -----------------------------------------------------------------------------

impl BatchComputerServer {
    fn new(backend: Arc<Backend>) -> Self {
        Self {
            backend,
            tool_router: Self::tool_router(),
        }
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
        description = "Execute OpenAI-style computer use actions in order (batched)."
    )]
    fn computer_use(&self, Parameters(req): Parameters<ComputerUseRequest>) -> String {
        response_to_json(&self.backend.execute_batch(&req.actions))
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
    fn new(backend: Arc<Backend>) -> Self {
        Self {
            backend,
            tool_router: Self::tool_router(),
        }
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
    #[tool(description = "Computer use: click")]
    fn computer_click(&self, Parameters(req): Parameters<ClickToolRequest>) -> String {
        response_to_json(&self.backend.execute_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: double_click")]
    fn computer_double_click(&self, Parameters(req): Parameters<DoubleClickToolRequest>) -> String {
        response_to_json(&self.backend.execute_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: scroll")]
    fn computer_scroll(&self, Parameters(req): Parameters<ScrollToolRequest>) -> String {
        response_to_json(&self.backend.execute_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: type")]
    fn computer_type(&self, Parameters(req): Parameters<TypeToolRequest>) -> String {
        response_to_json(&self.backend.execute_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: wait")]
    fn computer_wait(&self) -> String {
        response_to_json(
            &self
                .backend
                .execute_batch(&[computer_use::ComputerAction::Wait]),
        )
    }

    #[tool(description = "Computer use: keypress")]
    fn computer_keypress(&self, Parameters(req): Parameters<KeypressToolRequest>) -> String {
        response_to_json(&self.backend.execute_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: drag")]
    fn computer_drag(&self, Parameters(req): Parameters<DragToolRequest>) -> String {
        response_to_json(&self.backend.execute_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: move")]
    fn computer_move(&self, Parameters(req): Parameters<MoveToolRequest>) -> String {
        response_to_json(&self.backend.execute_batch(&[req.into()]))
    }

    #[tool(description = "Computer use: screenshot")]
    fn computer_screenshot(&self) -> String {
        response_to_json(
            &self
                .backend
                .execute_batch(&[computer_use::ComputerAction::Screenshot]),
        )
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
    let config = computer_use::server_config_from_args_iter(std::env::args());
    let backend = Backend::new(config.max_image_dimension)?;

    match config.mode {
        computer_use::ToolMode::Batch => {
            let service = BatchComputerServer::new(backend).serve(stdio()).await?;
            service.waiting().await?;
        }
        computer_use::ToolMode::Split => {
            let service = SplitComputerServer::new(backend).serve(stdio()).await?;
            service.waiting().await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_backend() -> Arc<Backend> {
        Backend::new(Some(computer_use::DEFAULT_MAX_IMAGE_DIMENSION))
            .expect("backend init should not fail in tests")
    }

    #[test]
    fn batch_server_exposes_only_computer_use() {
        let s = BatchComputerServer::new(test_backend());
        let names = s.tool_names_sorted();
        assert_eq!(names, vec!["computer_use".to_string()]);
    }

    #[test]
    fn split_server_exposes_per_action_tools() {
        let s = SplitComputerServer::new(test_backend());
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
    fn batch_wait_action_returns_ok() {
        // `wait` does not need any OS permissions and is the safest action
        // to exercise end-to-end through the real backend in CI.
        let s = BatchComputerServer::new(test_backend());
        let json = s.computer_use(Parameters(ComputerUseRequest {
            actions: vec![computer_use::ComputerAction::Wait],
        }));
        assert!(json.contains("\"action\":\"wait\""));
        assert!(json.contains("\"status\":\"ok\""));
    }
}
