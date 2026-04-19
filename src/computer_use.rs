//! Schemas for OpenAI-style computer use plus the executor that runs them.
//!
//! The action types and request/response shapes here are stable and
//! shared between the batched `computer_use` tool and the per-action
//! `computer_*` tools. The [`Backend`] type wires those schemas to the
//! real input controller and screen-capture backend.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use rmcp::schemars;
use serde::{Deserialize, Serialize};

use crate::desktop::{DesktopController, DesktopError};
use crate::scaling::CoordinateMap;
use crate::screen_capture::{self, CaptureError, Screenshot, ScreenCapture};

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const STATUS_OK: &str = "ok";
const STATUS_ERROR: &str = "error";

/// How long the `wait` action sleeps when no caller-specific value is set.
const DEFAULT_WAIT: Duration = Duration::from_millis(1_000);

/// Pause between consecutive actions in non-release builds (`debug_assertions`).
const DEBUG_BATCH_ACTION_DELAY: Duration = Duration::from_secs(3);

/// Default cap on the longest pixel dimension of returned screenshots.
///
/// 720 keeps full-screen captures small enough that a vision model can
/// ingest them comfortably while still leaving enough resolution to
/// identify UI elements. Override at startup with
/// `--max-image-dimension=<n>` (use `0` to disable).
pub const DEFAULT_MAX_IMAGE_DIMENSION: u32 = 720;

/// CLI flag that overrides the screenshot dimension cap.
const MAX_IMAGE_DIMENSION_FLAG: &str = "--max-image-dimension=";

/// CLI flag that selects batched tool exposure.
const BATCH_FLAG: &str = "--batch";

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

/// Resolved server configuration parsed from process args.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerConfig {
    pub mode: ToolMode,
    /// Cap on the longest pixel dimension of returned screenshots.
    /// `None` disables downscaling and returns native resolution.
    pub max_image_dimension: Option<u32>,
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
///
/// When the executor is downscaling screenshots (the default), `x` and
/// `y` are interpreted in the returned image's coordinate space and
/// remapped to absolute desktop coordinates before reaching the input
/// controller. When downscaling is disabled, image and desktop space
/// are 1:1 and no remapping happens.
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
///
/// `width`/`height` are the pixel dimensions of the PNG actually
/// returned to the client — i.e. the coordinate space subsequent mouse
/// actions should target. `physical_width`/`physical_height` describe
/// the underlying desktop in absolute coordinates. `scale_factor` is
/// `image / desktop` along the longest desktop axis.
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
/// `status` is `"ok"` on success and `"error"` otherwise; `message` is
/// human-readable; `image` is populated only by actions that capture
/// pixels (currently just `screenshot`).
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
/// Holds the input controller, the platform screen-capture backend,
/// and the most-recent coordinate map between the returned screenshot
/// and absolute desktop space. The screen-capture backend is
/// constructed best-effort: if it fails to initialize (no Wayland
/// session, missing protocol, etc.) we remember the error and surface
/// it only when a `screenshot` action is actually requested. That keeps
/// the rest of the server usable.
pub struct Backend {
    desktop: DesktopController,
    capture: CaptureSlot,
    /// Cap on the longest pixel dimension of returned screenshots.
    /// `None` disables downscaling.
    max_image_dimension: Option<u32>,
    /// Latest screenshot's image-to-desktop coordinate map.
    ///
    /// Mutated on every successful `screenshot` action so subsequent
    /// mouse actions remap their coordinates from image space back to
    /// the absolute desktop space `enigo` writes into.
    coordinate_map: Mutex<Option<CoordinateMap>>,
}

enum CaptureSlot {
    Ready(Box<dyn ScreenCapture>),
    Failed(String),
}

impl Backend {
    /// Build a backend, swallowing screen-capture init errors.
    pub fn new(max_image_dimension: Option<u32>) -> Result<Arc<Self>, DesktopError> {
        let desktop = DesktopController::new()?;
        let capture = match screen_capture::new_default() {
            Ok(b) => CaptureSlot::Ready(b),
            Err(e) => {
                tracing::warn!(error = %e, "Screen capture backend unavailable");
                CaptureSlot::Failed(e.to_string())
            }
        };
        Ok(Arc::new(Self {
            desktop,
            capture,
            max_image_dimension: normalize_max_dim(max_image_dimension),
            coordinate_map: Mutex::new(None),
        }))
    }

    /// Run every action in order and return one [`ActionResult`] per input.
    ///
    /// Errors on individual actions become `status: "error"` results so a
    /// batch is not aborted by a single failure — the caller decides
    /// whether to keep going.
    ///
    /// In non-release builds (`cfg!(debug_assertions)`), sleeps
    /// [`DEBUG_BATCH_ACTION_DELAY`] between consecutive actions.
    pub fn execute_batch(&self, actions: &[ComputerAction]) -> ComputerUseResponse {
        let mut results = Vec::with_capacity(actions.len());
        for (i, action) in actions.iter().enumerate() {
            log_action_stderr(action);
            results.push(self.execute_one(action));
            if cfg!(debug_assertions) && i + 1 < actions.len() {
                std::thread::sleep(DEBUG_BATCH_ACTION_DELAY);
            }
        }
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
    /// Coordinate-bearing actions (`click`, `double_click`, `scroll`,
    /// `move`, `drag`) are remapped through the latest screenshot's
    /// coordinate map before reaching the input controller. Actions
    /// without coordinates pass straight through.
    fn dispatch(&self, action: &ComputerAction) -> Result<Option<ScreenshotPayload>, String> {
        match action {
            ComputerAction::Click { button, x, y, keys } => {
                let (x, y) = self.remap_xy(*x, *y);
                self.desktop
                    .click(*button, x, y, keys.as_deref().unwrap_or(&[]))
                    .map(|_| None)
                    .map_err(|e| e.to_string())
            }
            ComputerAction::DoubleClick { x, y, keys } => {
                let (x, y) = self.remap_xy(*x, *y);
                self.desktop
                    .double_click(x, y, keys.as_deref().unwrap_or(&[]))
                    .map(|_| None)
                    .map_err(|e| e.to_string())
            }
            ComputerAction::Scroll {
                x,
                y,
                scroll_x,
                scroll_y,
                keys,
            } => {
                let (x, y) = self.remap_xy(*x, *y);
                self.desktop
                    .scroll(x, y, *scroll_x, *scroll_y, keys.as_deref().unwrap_or(&[]))
                    .map(|_| None)
                    .map_err(|e| e.to_string())
            }
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
            ComputerAction::Drag { path, keys } => {
                let remapped = self.remap_path(path);
                self.desktop
                    .drag(&remapped, keys.as_deref().unwrap_or(&[]))
                    .map(|_| None)
                    .map_err(|e| e.to_string())
            }
            ComputerAction::MouseMove { x, y, keys } => {
                let (x, y) = self.remap_xy(*x, *y);
                self.desktop
                    .mouse_move(x, y, keys.as_deref().unwrap_or(&[]))
                    .map(|_| None)
                    .map_err(|e| e.to_string())
            }
            ComputerAction::Screenshot => self.capture_screenshot().map(Some),
        }
    }

    fn capture_screenshot(&self) -> Result<ScreenshotPayload, String> {
        match &self.capture {
            CaptureSlot::Ready(b) => {
                let shot = b
                    .capture(self.max_image_dimension)
                    .map_err(|e: CaptureError| e.to_string())?;
                self.update_coordinate_map(&shot);
                Ok(screenshot_to_payload(shot))
            }
            CaptureSlot::Failed(msg) => Err(format!("screen capture unavailable: {msg}")),
        }
    }

    fn update_coordinate_map(&self, shot: &Screenshot) {
        let map = CoordinateMap {
            image_width: shot.width,
            image_height: shot.height,
            desktop_width: shot.physical_width,
            desktop_height: shot.physical_height,
        };
        let mut guard = self
            .coordinate_map
            .lock()
            .expect("coordinate_map mutex poisoned");
        *guard = Some(map);
    }

    fn remap_xy(&self, x: i32, y: i32) -> (i32, i32) {
        match self.current_map() {
            Some(map) => map.remap_point(x, y),
            None => (x, y),
        }
    }

    fn remap_path(&self, path: &[XY]) -> Vec<XY> {
        match self.current_map() {
            Some(map) => map.remap_path(path),
            None => path.to_vec(),
        }
    }

    fn current_map(&self) -> Option<CoordinateMap> {
        *self
            .coordinate_map
            .lock()
            .expect("coordinate_map mutex poisoned")
    }

    #[cfg(test)]
    pub(crate) fn install_test_coordinate_map(&self, map: CoordinateMap) {
        *self
            .coordinate_map
            .lock()
            .expect("coordinate_map mutex poisoned") = Some(map);
    }

    #[cfg(test)]
    pub(crate) fn current_coordinate_map(&self) -> Option<CoordinateMap> {
        self.current_map()
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
    if args.into_iter().any(|a| a == BATCH_FLAG) {
        ToolMode::Batch
    } else {
        ToolMode::Split
    }
}

/// Parse the screenshot dimension cap from `--max-image-dimension=<n>`.
///
/// Returns `Some(n)` when the flag is present and parses, `None` when
/// it is absent or malformed. `n == 0` means "disable downscaling".
pub fn max_image_dimension_from_args_iter<I>(args: I) -> Option<u32>
where
    I: IntoIterator<Item = String>,
{
    for a in args {
        if let Some(rest) = a.strip_prefix(MAX_IMAGE_DIMENSION_FLAG) {
            return rest.parse::<u32>().ok();
        }
    }
    None
}

/// Parse the full server config from process args.
///
/// Defaults to `--split` mode and a 720-pixel screenshot cap.
pub fn server_config_from_args_iter<I>(args: I) -> ServerConfig
where
    I: IntoIterator<Item = String>,
{
    let collected: Vec<String> = args.into_iter().collect();
    let mode = tool_mode_from_args_iter(collected.iter().cloned());
    let raw = max_image_dimension_from_args_iter(collected.iter().cloned())
        .unwrap_or(DEFAULT_MAX_IMAGE_DIMENSION);
    ServerConfig {
        mode,
        max_image_dimension: normalize_max_dim(Some(raw)),
    }
}

/// Translate a raw dimension cap into the executor's normalized form,
/// where `Some(n)` always has `n > 0` and `None` disables downscaling.
fn normalize_max_dim(raw: Option<u32>) -> Option<u32> {
    match raw {
        Some(n) if n > 0 => Some(n),
        _ => None,
    }
}

/// Emit one MCP-safe diagnostic line to stderr (JSON-RPC uses stdout).
fn log_action_stderr(action: &ComputerAction) {
    let json = serde_json::to_string(action).expect("ComputerAction serializes to JSON");
    eprintln!("mcp-computer-use: action {json}");
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
    fn max_image_dimension_flag_parses() {
        assert_eq!(
            max_image_dimension_from_args_iter(vec![
                "prog".into(),
                "--max-image-dimension=1280".into(),
            ]),
            Some(1280)
        );
        assert_eq!(
            max_image_dimension_from_args_iter(vec!["prog".into()]),
            None
        );
        assert_eq!(
            max_image_dimension_from_args_iter(vec![
                "prog".into(),
                "--max-image-dimension=notanumber".into(),
            ]),
            None
        );
    }

    #[test]
    fn server_config_defaults_to_split_and_default_cap() {
        let cfg = server_config_from_args_iter(vec!["prog".into()]);
        assert_eq!(
            cfg,
            ServerConfig {
                mode: ToolMode::Split,
                max_image_dimension: Some(DEFAULT_MAX_IMAGE_DIMENSION),
            }
        );
    }

    #[test]
    fn server_config_honors_flags() {
        let cfg = server_config_from_args_iter(vec![
            "prog".into(),
            "--batch".into(),
            "--max-image-dimension=1024".into(),
        ]);
        assert_eq!(
            cfg,
            ServerConfig {
                mode: ToolMode::Batch,
                max_image_dimension: Some(1024),
            }
        );
    }

    #[test]
    fn server_config_disables_cap_with_zero() {
        let cfg = server_config_from_args_iter(vec![
            "prog".into(),
            "--max-image-dimension=0".into(),
        ]);
        assert_eq!(cfg.max_image_dimension, None);
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
                assert_eq!(
                    keys.as_deref(),
                    Some(&["ctrl".to_string(), "shift".to_string()][..])
                );
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
                        width: 720,
                        height: 405,
                        physical_width: 1920,
                        physical_height: 1080,
                        scale_factor: 0.375,
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
        assert!(json.contains("\"width\":720"));
        assert!(json.contains("\"physical_width\":1920"));
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

    #[test]
    fn normalize_max_dim_treats_zero_as_disabled() {
        assert_eq!(normalize_max_dim(None), None);
        assert_eq!(normalize_max_dim(Some(0)), None);
        assert_eq!(normalize_max_dim(Some(720)), Some(720));
    }

    /// Sanity-check that combining `scaled_dimensions` with
    /// [`CoordinateMap::remap_point`] round-trips coordinates close to
    /// where the model intended on the desktop. This covers the
    /// executor-level contract end-to-end without needing a real
    /// capture backend.
    #[test]
    fn screenshot_scale_round_trips_to_desktop_space() {
        let (img_w, img_h) = crate::scaling::scaled_dimensions(1920, 1080, Some(720));
        let map = CoordinateMap {
            image_width: img_w,
            image_height: img_h,
            desktop_width: 1920,
            desktop_height: 1080,
        };
        let (dx, dy) = map.remap_point(360, 200);
        assert_eq!((dx, dy), (960, 533));
    }

    /// Backend without a stored screenshot map should pass coordinates
    /// through unchanged. This preserves today's behavior for tools
    /// that act before any screenshot has been requested.
    #[test]
    fn backend_without_map_passes_coordinates_through() {
        let backend = Backend::new(Some(720)).expect("backend init");
        assert!(backend.current_coordinate_map().is_none());
        assert_eq!(backend.remap_xy(123, 456), (123, 456));
        let path = vec![XY { x: 1, y: 2 }, XY { x: 3, y: 4 }];
        let remapped = backend.remap_path(&path);
        assert_eq!(remapped.len(), 2);
        assert_eq!((remapped[0].x, remapped[0].y), (1, 2));
        assert_eq!((remapped[1].x, remapped[1].y), (3, 4));
    }

    /// Once the executor has cached a downscaled screenshot's
    /// coordinate map, subsequent `dispatch` calls remap incoming
    /// image-space coordinates back to absolute desktop space.
    #[test]
    fn backend_with_installed_map_remaps_dispatch_coordinates() {
        let backend = Backend::new(Some(720)).expect("backend init");
        backend.install_test_coordinate_map(CoordinateMap {
            image_width: 720,
            image_height: 405,
            desktop_width: 1920,
            desktop_height: 1080,
        });
        assert_eq!(backend.remap_xy(360, 202), (960, 539));

        let path = vec![XY { x: 0, y: 0 }, XY { x: 720, y: 405 }];
        let remapped = backend.remap_path(&path);
        assert_eq!(remapped.len(), 2);
        assert_eq!((remapped[0].x, remapped[0].y), (0, 0));
        assert_eq!((remapped[1].x, remapped[1].y), (1920, 1080));
    }
}
