//! Shared types and stub dispatch for OpenAI-style computer use actions.
//!
//! Real UI automation is intentionally not implemented yet.

use rmcp::schemars;
use serde::{Deserialize, Serialize};

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const STUB_STATUS: &str = "not_implemented";

const STUB_MESSAGE: &str = "Computer use harness not implemented yet.";

// -----------------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------------

/// Startup mode: batched `computer_use` tool vs one MCP tool per action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMode {
    /// Expose `computer_use` with `actions[]` (OpenAI-style batch).
    Batch,
    /// Expose `computer_click`, `computer_type`, etc.
    Split,
}

/// Mouse button for [`ComputerAction::Click`].
#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Right,
    Wheel,
    Back,
    Forward,
}

/// A point in screen coordinates.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct XY {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate.
    pub y: i32,
}

/// One GA-aligned computer action (discriminated by JSON field `type`).
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComputerAction {
    /// Click at `(x, y)` with the given button.
    Click {
        button: MouseButton,
        x: i32,
        y: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    /// Double-click at `(x, y)`.
    DoubleClick {
        x: i32,
        y: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    /// Scroll at `(x, y)` by `(scroll_x, scroll_y)`.
    Scroll {
        x: i32,
        y: i32,
        scroll_x: i32,
        scroll_y: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    /// Type text (JSON `type` is `"type"`).
    #[serde(rename = "type")]
    Type { text: String },
    /// Wait (no-op stub).
    Wait,
    /// Standalone keypress sequence.
    Keypress { keys: Vec<String> },
    /// Drag along `path`.
    Drag {
        path: Vec<XY>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    /// Move pointer to `(x, y)` (JSON `type` is `"move"`).
    #[serde(rename = "move")]
    MouseMove {
        x: i32,
        y: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    /// Request a screenshot (capture is stubbed).
    Screenshot,
}

/// Batched tool input: ordered `actions` matching OpenAI GA computer use.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ComputerUseRequest {
    /// Actions to run in order.
    #[schemars(description = "OpenAI computer-use actions to execute in order")]
    pub actions: Vec<ComputerAction>,
}

/// Stub result for one action.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StubActionResult {
    /// Action discriminator (e.g. `click`, `type`).
    pub action: String,
    /// Always `not_implemented` until the harness exists.
    pub status: String,
    /// Human-readable stub explanation.
    pub message: Option<String>,
}

/// Response for batched `computer_use` (and single-action tools return one element).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ComputerUseResponse {
    /// One stub entry per input action, in order.
    pub results: Vec<StubActionResult>,
}

// --- Split-tool request structs (no `type` field; tool name implies action) ---

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ClickToolRequest {
    pub button: MouseButton,
    pub x: i32,
    pub y: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DoubleClickToolRequest {
    pub x: i32,
    pub y: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ScrollToolRequest {
    pub x: i32,
    pub y: i32,
    pub scroll_x: i32,
    pub scroll_y: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct TypeToolRequest {
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct KeypressToolRequest {
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DragToolRequest {
    pub path: Vec<XY>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct MoveToolRequest {
    pub x: i32,
    pub y: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<String>>,
}

// -----------------------------------------------------------------------------
// Stub dispatch (theme)
// -----------------------------------------------------------------------------

impl ComputerAction {
    /// JSON `type` string for this action (for stub payloads and logging).
    pub fn action_type_str(&self) -> &'static str {
        match self {
            ComputerAction::Click { .. } => "click",
            ComputerAction::DoubleClick { .. } => "double_click",
            ComputerAction::Scroll { .. } => "scroll",
            ComputerAction::Type { .. } => "type",
            ComputerAction::Wait => "wait",
            ComputerAction::Keypress { .. } => "keypress",
            ComputerAction::Drag { .. } => "drag",
            ComputerAction::MouseMove { .. } => "move",
            ComputerAction::Screenshot => "screenshot",
        }
    }
}

/// Run stub logic for each action in order.
pub fn stub_dispatch_batch(actions: &[ComputerAction]) -> ComputerUseResponse {
    ComputerUseResponse {
        results: actions
            .iter()
            .map(|a| StubActionResult {
                action: a.action_type_str().to_string(),
                status: STUB_STATUS.to_string(),
                message: Some(STUB_MESSAGE.to_string()),
            })
            .collect(),
    }
}

/// Serialize [`ComputerUseResponse`] to a JSON string for MCP tool output.
pub fn response_to_json(res: &ComputerUseResponse) -> String {
    serde_json::to_string(res).expect("ComputerUseResponse must serialize to JSON")
}

// -----------------------------------------------------------------------------
// Conversions from split-tool requests
// -----------------------------------------------------------------------------

impl From<ClickToolRequest> for ComputerAction {
    fn from(r: ClickToolRequest) -> Self {
        ComputerAction::Click {
            button: r.button,
            x: r.x,
            y: r.y,
            keys: r.keys,
        }
    }
}

impl From<DoubleClickToolRequest> for ComputerAction {
    fn from(r: DoubleClickToolRequest) -> Self {
        ComputerAction::DoubleClick {
            x: r.x,
            y: r.y,
            keys: r.keys,
        }
    }
}

impl From<ScrollToolRequest> for ComputerAction {
    fn from(r: ScrollToolRequest) -> Self {
        ComputerAction::Scroll {
            x: r.x,
            y: r.y,
            scroll_x: r.scroll_x,
            scroll_y: r.scroll_y,
            keys: r.keys,
        }
    }
}

impl From<TypeToolRequest> for ComputerAction {
    fn from(r: TypeToolRequest) -> Self {
        ComputerAction::Type { text: r.text }
    }
}

impl From<KeypressToolRequest> for ComputerAction {
    fn from(r: KeypressToolRequest) -> Self {
        ComputerAction::Keypress { keys: r.keys }
    }
}

impl From<DragToolRequest> for ComputerAction {
    fn from(r: DragToolRequest) -> Self {
        ComputerAction::Drag {
            path: r.path,
            keys: r.keys,
        }
    }
}

impl From<MoveToolRequest> for ComputerAction {
    fn from(r: MoveToolRequest) -> Self {
        ComputerAction::MouseMove {
            x: r.x,
            y: r.y,
            keys: r.keys,
        }
    }
}

// -----------------------------------------------------------------------------
// Utilities
// -----------------------------------------------------------------------------

/// Parse [`ToolMode`] from process arguments: `--batch` enables batched tools.
pub fn tool_mode_from_args_iter<I>(args: I) -> ToolMode
where
    I: IntoIterator<Item = String>,
{
    if args.into_iter().any(|a| a == "--batch") {
        ToolMode::Batch
    } else {
        ToolMode::Split
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_mode_batch_flag() {
        assert_eq!(
            tool_mode_from_args_iter(vec!["prog".into(), "--batch".into()]),
            ToolMode::Batch
        );
        assert_eq!(
            tool_mode_from_args_iter(vec!["prog".into()]),
            ToolMode::Split
        );
    }

    #[test]
    fn deserialize_each_action_variant() {
        let samples = [
            (r#"{"type":"click","button":"left","x":1,"y":2}"#, "click"),
            (r#"{"type":"double_click","x":1,"y":2}"#, "double_click"),
            (
                r#"{"type":"scroll","x":0,"y":0,"scroll_x":0,"scroll_y":-10}"#,
                "scroll",
            ),
            (r#"{"type":"type","text":"hi"}"#, "type"),
            (r#"{"type":"wait"}"#, "wait"),
            (r#"{"type":"keypress","keys":["CTRL","c"]}"#, "keypress"),
            (
                r#"{"type":"drag","path":[{"x":0,"y":0},{"x":10,"y":10}]}"#,
                "drag",
            ),
            (r#"{"type":"move","x":3,"y":4}"#, "move"),
            (r#"{"type":"screenshot"}"#, "screenshot"),
        ];
        for (json, expected) in samples {
            let a: ComputerAction = serde_json::from_str(json).unwrap_or_else(|e| {
                panic!("deserialize {json}: {e}");
            });
            assert_eq!(a.action_type_str(), expected);
        }
    }

    #[test]
    fn batch_order_preserved() {
        let actions = vec![
            ComputerAction::Screenshot,
            ComputerAction::Wait,
            ComputerAction::Type { text: "x".into() },
        ];
        let out = stub_dispatch_batch(&actions);
        assert_eq!(out.results.len(), 3);
        assert_eq!(out.results[0].action, "screenshot");
        assert_eq!(out.results[1].action, "wait");
        assert_eq!(out.results[2].action, "type");
        assert!(out.results.iter().all(|r| r.status == STUB_STATUS));
    }

    #[test]
    fn split_conversions_hit_same_dispatch() {
        let click: ComputerAction = ClickToolRequest {
            button: MouseButton::Left,
            x: 1,
            y: 2,
            keys: None,
        }
        .into();
        let out = stub_dispatch_batch(std::slice::from_ref(&click));
        assert_eq!(out.results[0].action, "click");
    }
}
