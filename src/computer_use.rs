//! Schemas for OpenAI-style computer use plus the executor that runs them.
//!
//! The action types and request/response shapes here are stable and
//! shared between the batched `computer_use` tool and the per-action
//! `computer_*` tools. The [`Backend`] type wires those schemas to the
//! real input controller and screen-capture backend.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use rmcp::schemars;
use serde::{Deserialize, Serialize};

use crate::desktop::{DesktopController, DesktopError};
use crate::screen_capture::{self, CaptureError, Screenshot, ScreenCapture};

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const STATUS_OK: &str = "ok";
const STATUS_ERROR: &str = "error";

/// How long the `wait` action sleeps when no caller-specific value is set.
const DEFAULT_WAIT: Duration = Duration::from_millis(1_000);

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
    pub x: i32,
    pub y: i32,
}

/// One GA-aligned computer action (discriminated by JSON field `type`).
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComputerAction {
    Click {
        button: MouseButton,
        x: i32,
        y: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    DoubleClick {
        x: i32,
        y: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    Scroll {
        x: i32,
        y: i32,
        scroll_x: i32,
        scroll_y: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    #[serde(rename = "type")]
    Type { text: String },
    Wait,
    Keypress { keys: Vec<String> },
    Drag {
        path: Vec<XY>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    #[serde(rename = "move")]
    MouseMove {
        x: i32,
        y: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    Screenshot,
}

/// Batched tool input: ordered `actions` matching OpenAI GA computer use.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ComputerUseRequest {
    #[schemars(description = "OpenAI computer-use actions to execute in order")]
    pub actions: Vec<ComputerAction>,
}

/// One captured screen, attached to action results that produce images.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ScreenshotPayload {
    /// Always `"image/png"` today.
    pub media_type: String,
    /// Base64-encoded PNG bytes.
    pub data: String,
    pub width: u32,
    pub height: u32,
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f32,
}

/// Result of a single action.
///
/// Replaces the original `StubActionResult`. `status` is `"ok"` on
/// success and `"error"` otherwise; `message` is human-readable; `image`
/// is populated only by actions that capture pixels (currently just
/// `screenshot`).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ActionResult {
    pub action: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ScreenshotPayload>,
}

/// Response for batched `computer_use` (and single-action tools return one element).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ComputerUseResponse {
    pub results: Vec<ActionResult>,
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
// Backend (theme of this file)
// -----------------------------------------------------------------------------

/// Runtime state shared by every MCP tool handler.
///
/// Holds the input controller and the platform screen-capture backend.
/// Both fields are constructed best-effort: if the screen-capture backend
/// fails to initialize (no Wayland session, missing protocol, etc.) we
/// remember the error and surface it only when a `screenshot` action is
/// actually requested. That keeps the rest of the server usable.
pub struct Backend {
    desktop: DesktopController,
    capture: CaptureSlot,
}

enum CaptureSlot {
    Ready(Box<dyn ScreenCapture>),
    Failed(String),
}

impl Backend {
    /// Build a backend, swallowing screen-capture init errors.
    pub fn new() -> Result<Arc<Self>, DesktopError> {
        let desktop = DesktopController::new()?;
        let capture = match screen_capture::new_default() {
            Ok(b) => CaptureSlot::Ready(b),
            Err(e) => {
                tracing::warn!(error = %e, "Screen capture backend unavailable");
                CaptureSlot::Failed(e.to_string())
            }
        };
        Ok(Arc::new(Self { desktop, capture }))
    }

    /// Run every action in order and return one [`ActionResult`] per input.
    ///
    /// Errors on individual actions become `status: "error"` results so a
    /// batch is not aborted by a single failure — the caller decides
    /// whether to keep going.
    pub fn execute_batch(&self, actions: &[ComputerAction]) -> ComputerUseResponse {
        let results = actions.iter().map(|a| self.execute_one(a)).collect();
        ComputerUseResponse { results }
    }

    fn execute_one(&self, action: &ComputerAction) -> ActionResult {
        let action_name = action.action_type_str();
        match self.dispatch(action) {
            Ok(image) => ActionResult {
                action: action_name.to_string(),
                status: STATUS_OK.to_string(),
                message: None,
                image,
            },
            Err(msg) => ActionResult {
                action: action_name.to_string(),
                status: STATUS_ERROR.to_string(),
                message: Some(msg),
                image: None,
            },
        }
    }

    /// Run a single action, returning an optional screenshot payload.
    ///
    /// Returning `Result<Option<ScreenshotPayload>, String>` keeps every
    /// action through one common error path while letting `screenshot`
    /// attach its image without a special return type.
    fn dispatch(&self, action: &ComputerAction) -> Result<Option<ScreenshotPayload>, String> {
        match action {
            ComputerAction::Click { button, x, y, keys } => self
                .desktop
                .click(*button, *x, *y, keys.as_deref().unwrap_or(&[]))
                .map(|_| None)
                .map_err(|e| e.to_string()),
            ComputerAction::DoubleClick { x, y, keys } => self
                .desktop
                .double_click(*x, *y, keys.as_deref().unwrap_or(&[]))
                .map(|_| None)
                .map_err(|e| e.to_string()),
            ComputerAction::Scroll {
                x,
                y,
                scroll_x,
                scroll_y,
                keys,
            } => self
                .desktop
                .scroll(*x, *y, *scroll_x, *scroll_y, keys.as_deref().unwrap_or(&[]))
                .map(|_| None)
                .map_err(|e| e.to_string()),
            ComputerAction::Type { text } => self
                .desktop
                .type_text(text)
                .map(|_| None)
                .map_err(|e| e.to_string()),
            ComputerAction::Wait => {
                std::thread::sleep(DEFAULT_WAIT);
                Ok(None)
            }
            ComputerAction::Keypress { keys } => self
                .desktop
                .keypress(keys)
                .map(|_| None)
                .map_err(|e| e.to_string()),
            ComputerAction::Drag { path, keys } => self
                .desktop
                .drag(path, keys.as_deref().unwrap_or(&[]))
                .map(|_| None)
                .map_err(|e| e.to_string()),
            ComputerAction::MouseMove { x, y, keys } => self
                .desktop
                .mouse_move(*x, *y, keys.as_deref().unwrap_or(&[]))
                .map(|_| None)
                .map_err(|e| e.to_string()),
            ComputerAction::Screenshot => self.capture_screenshot().map(Some),
        }
    }

    fn capture_screenshot(&self) -> Result<ScreenshotPayload, String> {
        match &self.capture {
            CaptureSlot::Ready(b) => b
                .capture()
                .map(screenshot_to_payload)
                .map_err(|e: CaptureError| e.to_string()),
            CaptureSlot::Failed(msg) => Err(format!("screen capture unavailable: {msg}")),
        }
    }
}

// -----------------------------------------------------------------------------
// Action dispatch helpers
// -----------------------------------------------------------------------------

impl ComputerAction {
    /// JSON `type` string for this action (for response payloads and logging).
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

/// Serialize [`ComputerUseResponse`] to a JSON string for MCP tool output.
pub fn response_to_json(res: &ComputerUseResponse) -> String {
    serde_json::to_string(res).expect("ComputerUseResponse must serialize to JSON")
}

fn screenshot_to_payload(s: Screenshot) -> ScreenshotPayload {
    let data = base64::engine::general_purpose::STANDARD.encode(&s.png_data);
    ScreenshotPayload {
        media_type: "image/png".to_string(),
        data,
        width: s.width,
        height: s.height,
        physical_width: s.physical_width,
        physical_height: s.physical_height,
        scale_factor: s.scale_factor,
    }
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
    fn click_modifier_keys_round_trip() {
        let json = r#"{"type":"click","button":"left","x":1,"y":2,"keys":["ctrl","shift"]}"#;
        let a: ComputerAction = serde_json::from_str(json).unwrap();
        match a {
            ComputerAction::Click { keys, .. } => {
                assert_eq!(keys.as_deref(), Some(&["ctrl".to_string(), "shift".to_string()][..]));
            }
            _ => panic!("expected click"),
        }
    }

    #[test]
    fn response_serializes_with_optional_image() {
        let res = ComputerUseResponse {
            results: vec![
                ActionResult {
                    action: "click".into(),
                    status: STATUS_OK.into(),
                    message: None,
                    image: None,
                },
                ActionResult {
                    action: "screenshot".into(),
                    status: STATUS_OK.into(),
                    message: None,
                    image: Some(ScreenshotPayload {
                        media_type: "image/png".into(),
                        data: "AAAA".into(),
                        width: 10,
                        height: 5,
                        physical_width: 10,
                        physical_height: 5,
                        scale_factor: 1.0,
                    }),
                },
            ],
        };
        let json = response_to_json(&res);
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"action\":\"click\""));
        assert!(json.contains("\"action\":\"screenshot\""));
        assert!(json.contains("\"image\":{"));
        assert!(json.contains("\"media_type\":\"image/png\""));
        // No image on the click result should mean no `image` key for it.
        assert_eq!(json.matches("\"image\":").count(), 1);
    }

    #[test]
    fn split_request_into_action_preserves_modifiers() {
        let click: ComputerAction = ClickToolRequest {
            button: MouseButton::Right,
            x: 5,
            y: 6,
            keys: Some(vec!["alt".into()]),
        }
        .into();
        match click {
            ComputerAction::Click {
                button: MouseButton::Right,
                x: 5,
                y: 6,
                keys: Some(k),
            } => assert_eq!(k, vec!["alt".to_string()]),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
