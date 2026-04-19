//! Desktop input controller — mouse/keyboard simulation via `enigo`.
//!
//! Ported and adapted from `temm1e-gaze::desktop_controller` so this crate
//! does not depend on the external `temm1e-core` error type. Input is the
//! only responsibility here; screen capture lives in `screen_capture`.

use enigo::{
    Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings,
    InputError as EnigoInputError, NewConError,
};
use std::time::Duration;
use thiserror::Error;

use crate::computer_use::{MouseButton, XY};

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

/// Sleep between intermediate moves while playing a drag path.
/// Mirrors the small inter-step delay common in input automation so the
/// underlying widget toolkit gets time to register movement deltas.
const DRAG_STEP_DELAY: Duration = Duration::from_millis(15);

// -----------------------------------------------------------------------------
// Errors
// -----------------------------------------------------------------------------

/// Errors produced by the input controller.
#[derive(Debug, Error)]
pub enum DesktopError {
    #[error("input simulation unavailable: {0}")]
    Unavailable(String),

    #[error("input failed: {0}")]
    Input(String),

    #[error("invalid key combo '{0}'")]
    InvalidKey(String),

    #[error("empty drag path")]
    EmptyDragPath,
}

impl From<NewConError> for DesktopError {
    fn from(e: NewConError) -> Self {
        DesktopError::Unavailable(e.to_string())
    }
}

impl From<EnigoInputError> for DesktopError {
    fn from(e: EnigoInputError) -> Self {
        DesktopError::Input(e.to_string())
    }
}

// -----------------------------------------------------------------------------
// DesktopController
// -----------------------------------------------------------------------------

/// Drives mouse and keyboard via `enigo`.
///
/// A fresh `Enigo` is created per operation. The temm1e-gaze original did this
/// to dodge `Send/Sync` issues with macOS Core Graphics pointers; we keep the
/// pattern because it is cheap and frees us from internal locking.
pub struct DesktopController {
    /// True when an initial probe of `Enigo::new` succeeded. Lets the server
    /// surface a single clear error instead of failing every action.
    input_available: bool,
}

impl DesktopController {
    /// Probe enigo once and remember whether input simulation is usable.
    pub fn new() -> Result<Self, DesktopError> {
        let settings = Settings::default();
        let input_available = match Enigo::new(&settings) {
            Ok(_) => {
                tracing::info!("Desktop input simulation available");
                true
            }
            Err(e) => {
                tracing::warn!(error = %e, "Desktop input simulation unavailable");
                false
            }
        };
        Ok(Self { input_available })
    }

    #[allow(dead_code)]
    pub fn input_available(&self) -> bool {
        self.input_available
    }

    /// Move the cursor without pressing buttons.
    pub fn mouse_move(&self, x: i32, y: i32, modifiers: &[String]) -> Result<(), DesktopError> {
        let mut enigo = self.new_enigo()?;
        with_modifiers(&mut enigo, modifiers, |e| {
            e.move_mouse(x, y, Coordinate::Abs)?;
            Ok(())
        })?;
        tracing::debug!(x, y, ?modifiers, "Desktop mouse_move");
        Ok(())
    }

    /// Move to `(x, y)` and click the requested button.
    pub fn click(
        &self,
        button: MouseButton,
        x: i32,
        y: i32,
        modifiers: &[String],
    ) -> Result<(), DesktopError> {
        let enigo_button = enigo_button(button);
        let mut enigo = self.new_enigo()?;
        with_modifiers(&mut enigo, modifiers, |e| {
            e.move_mouse(x, y, Coordinate::Abs)?;
            e.button(enigo_button, Direction::Click)?;
            Ok(())
        })?;
        tracing::debug!(?button, x, y, ?modifiers, "Desktop click");
        Ok(())
    }

    /// Move to `(x, y)` and left-click twice.
    pub fn double_click(
        &self,
        x: i32,
        y: i32,
        modifiers: &[String],
    ) -> Result<(), DesktopError> {
        let mut enigo = self.new_enigo()?;
        with_modifiers(&mut enigo, modifiers, |e| {
            e.move_mouse(x, y, Coordinate::Abs)?;
            e.button(Button::Left, Direction::Click)?;
            e.button(Button::Left, Direction::Click)?;
            Ok(())
        })?;
        tracing::debug!(x, y, ?modifiers, "Desktop double_click");
        Ok(())
    }

    /// Scroll at `(x, y)` by `(dx, dy)` ticks.
    ///
    /// Positive `dy` scrolls down, positive `dx` scrolls right — matching the
    /// convention used by the rest of the OpenAI computer-use surface.
    pub fn scroll(
        &self,
        x: i32,
        y: i32,
        dx: i32,
        dy: i32,
        modifiers: &[String],
    ) -> Result<(), DesktopError> {
        let mut enigo = self.new_enigo()?;
        with_modifiers(&mut enigo, modifiers, |e| {
            e.move_mouse(x, y, Coordinate::Abs)?;
            if dy != 0 {
                e.scroll(dy, Axis::Vertical)?;
            }
            if dx != 0 {
                e.scroll(dx, Axis::Horizontal)?;
            }
            Ok(())
        })?;
        tracing::debug!(x, y, dx, dy, ?modifiers, "Desktop scroll");
        Ok(())
    }

    /// Type a unicode string by simulating keystrokes.
    pub fn type_text(&self, text: &str) -> Result<(), DesktopError> {
        let mut enigo = self.new_enigo()?;
        enigo.text(text)?;
        tracing::debug!(len = text.len(), "Desktop type_text");
        Ok(())
    }

    /// Press a sequence of keys (modifiers + main key) as a chord.
    ///
    /// The keys list mirrors the OpenAI computer-use convention: every entry
    /// is held down in order, then released in reverse. This produces both
    /// single-key presses (`["enter"]`) and chords (`["ctrl", "shift", "a"]`).
    pub fn keypress(&self, keys: &[String]) -> Result<(), DesktopError> {
        if keys.is_empty() {
            return Err(DesktopError::InvalidKey("(empty)".into()));
        }
        let mapped = parse_keys(keys)?;
        let mut enigo = self.new_enigo()?;
        for key in &mapped {
            enigo.key(*key, Direction::Press)?;
        }
        for key in mapped.iter().rev() {
            enigo.key(*key, Direction::Release)?;
        }
        tracing::debug!(?keys, "Desktop keypress");
        Ok(())
    }

    /// Drag along `path`: move to first point, press left, move through the
    /// rest, then release.
    pub fn drag(&self, path: &[XY], modifiers: &[String]) -> Result<(), DesktopError> {
        if path.is_empty() {
            return Err(DesktopError::EmptyDragPath);
        }
        let mut enigo = self.new_enigo()?;
        with_modifiers(&mut enigo, modifiers, |e| {
            let first = &path[0];
            e.move_mouse(first.x, first.y, Coordinate::Abs)?;
            e.button(Button::Left, Direction::Press)?;
            for point in &path[1..] {
                e.move_mouse(point.x, point.y, Coordinate::Abs)?;
                std::thread::sleep(DRAG_STEP_DELAY);
            }
            e.button(Button::Left, Direction::Release)?;
            Ok(())
        })?;
        tracing::debug!(points = path.len(), ?modifiers, "Desktop drag");
        Ok(())
    }

    fn new_enigo(&self) -> Result<Enigo, DesktopError> {
        if !self.input_available {
            return Err(DesktopError::Unavailable(
                "initial enigo probe failed; check OS-level input permissions".into(),
            ));
        }
        Ok(Enigo::new(&Settings::default())?)
    }
}

// -----------------------------------------------------------------------------
// Mouse button mapping
// -----------------------------------------------------------------------------

/// Map our public [`MouseButton`] variants onto `enigo::Button`.
///
/// `MouseButton::Wheel` collapses to a middle click — the OpenAI spec uses
/// `wheel` to mean a wheel-button press, not a scroll gesture.
fn enigo_button(b: MouseButton) -> Button {
    match b {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Wheel => Button::Middle,
        MouseButton::Back => Button::Back,
        MouseButton::Forward => Button::Forward,
    }
}

// -----------------------------------------------------------------------------
// Modifier orchestration
// -----------------------------------------------------------------------------

/// Press the given modifier keys, run `body`, and release them in reverse
/// order. An empty modifier list runs `body` directly.
fn with_modifiers<F>(
    enigo: &mut Enigo,
    modifiers: &[String],
    body: F,
) -> Result<(), DesktopError>
where
    F: FnOnce(&mut Enigo) -> Result<(), DesktopError>,
{
    if modifiers.is_empty() {
        return body(enigo);
    }
    let keys = parse_keys(modifiers)?;
    for key in &keys {
        enigo.key(*key, Direction::Press)?;
    }
    let result = body(enigo);
    // Always release modifiers, even if the body errored.
    for key in keys.iter().rev() {
        let _ = enigo.key(*key, Direction::Release);
    }
    result
}

// -----------------------------------------------------------------------------
// Key parsing
// -----------------------------------------------------------------------------

/// Parse a list of key names into `enigo::Key` values.
pub fn parse_keys(keys: &[String]) -> Result<Vec<Key>, DesktopError> {
    keys.iter().map(|k| map_key_name(k)).collect()
}

/// Parse a `+`-separated combo string like `"ctrl+shift+a"` into keys.
///
/// Useful when callers prefer a single string over an array.
#[allow(dead_code)]
pub fn parse_key_combo(combo: &str) -> Result<Vec<Key>, DesktopError> {
    let parts: Vec<String> = combo.split('+').map(|s| s.trim().to_string()).collect();
    if parts.iter().all(|p| p.is_empty()) {
        return Err(DesktopError::InvalidKey(combo.to_string()));
    }
    parse_keys(&parts)
}

/// Map a single key name to an `enigo::Key`.
///
/// Names are case-insensitive and accept both human-readable aliases
/// (`cmd`/`command`/`meta`) and short forms.
fn map_key_name(name: &str) -> Result<Key, DesktopError> {
    let lower = name.trim().to_lowercase();
    match lower.as_str() {
        // Modifiers
        "cmd" | "command" | "meta" | "super" | "win" => Ok(Key::Meta),
        "ctrl" | "control" => Ok(Key::Control),
        "alt" | "option" | "opt" => Ok(Key::Alt),
        "shift" => Ok(Key::Shift),

        // Whitespace / editing
        "enter" | "return" => Ok(Key::Return),
        "tab" => Ok(Key::Tab),
        "escape" | "esc" => Ok(Key::Escape),
        "backspace" => Ok(Key::Backspace),
        "delete" | "del" | "forwarddelete" => Ok(Key::Delete),
        "space" | "spacebar" => Ok(Key::Space),

        // Arrows
        "up" | "uparrow" | "arrowup" => Ok(Key::UpArrow),
        "down" | "downarrow" | "arrowdown" => Ok(Key::DownArrow),
        "left" | "leftarrow" | "arrowleft" => Ok(Key::LeftArrow),
        "right" | "rightarrow" | "arrowright" => Ok(Key::RightArrow),

        // Navigation
        "home" => Ok(Key::Home),
        "end" => Ok(Key::End),
        "pageup" | "pgup" => Ok(Key::PageUp),
        "pagedown" | "pgdn" => Ok(Key::PageDown),

        // Function keys F1..F12
        s if s.starts_with('f') && s.len() <= 3 => parse_function_key(s)
            .ok_or_else(|| DesktopError::InvalidKey(name.to_string())),

        // Single character — treat as Unicode keystroke
        s if s.chars().count() == 1 => {
            let ch = s.chars().next().expect("checked length");
            Ok(Key::Unicode(ch))
        }

        _ => Err(DesktopError::InvalidKey(name.to_string())),
    }
}

fn parse_function_key(name: &str) -> Option<Key> {
    let n: u8 = name[1..].parse().ok()?;
    match n {
        1 => Some(Key::F1),
        2 => Some(Key::F2),
        3 => Some(Key::F3),
        4 => Some(Key::F4),
        5 => Some(Key::F5),
        6 => Some(Key::F6),
        7 => Some(Key::F7),
        8 => Some(Key::F8),
        9 => Some(Key::F9),
        10 => Some(Key::F10),
        11 => Some(Key::F11),
        12 => Some(Key::F12),
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifier_aliases() {
        assert!(matches!(map_key_name("cmd").unwrap(), Key::Meta));
        assert!(matches!(map_key_name("CTRL").unwrap(), Key::Control));
        assert!(matches!(map_key_name("Option").unwrap(), Key::Alt));
        assert!(matches!(map_key_name("shift").unwrap(), Key::Shift));
    }

    #[test]
    fn parses_specials_and_arrows() {
        assert!(matches!(map_key_name("enter").unwrap(), Key::Return));
        assert!(matches!(map_key_name("Esc").unwrap(), Key::Escape));
        assert!(matches!(map_key_name("ArrowUp").unwrap(), Key::UpArrow));
        assert!(matches!(map_key_name("PageDown").unwrap(), Key::PageDown));
    }

    #[test]
    fn parses_function_keys() {
        for i in 1..=12u8 {
            let n = format!("f{i}");
            assert!(map_key_name(&n).is_ok(), "{n} should parse");
        }
        assert!(map_key_name("f0").is_err());
        assert!(map_key_name("f13").is_err());
    }

    #[test]
    fn parses_unicode_singleton() {
        assert!(matches!(map_key_name("a").unwrap(), Key::Unicode('a')));
        assert!(matches!(map_key_name("3").unwrap(), Key::Unicode('3')));
    }

    #[test]
    fn rejects_unknown_key() {
        assert!(map_key_name("nonexistent").is_err());
        assert!(map_key_name("").is_err());
    }

    #[test]
    fn parse_key_combo_handles_chord() {
        let keys = parse_key_combo("ctrl+shift+a").unwrap();
        assert_eq!(keys.len(), 3);
    }

    #[test]
    fn parse_key_combo_handles_whitespace() {
        let keys = parse_key_combo("ctrl + shift + a").unwrap();
        assert_eq!(keys.len(), 3);
    }

    #[test]
    fn enigo_button_mapping_is_total() {
        assert!(matches!(enigo_button(MouseButton::Left), Button::Left));
        assert!(matches!(enigo_button(MouseButton::Right), Button::Right));
        assert!(matches!(enigo_button(MouseButton::Wheel), Button::Middle));
        assert!(matches!(enigo_button(MouseButton::Back), Button::Back));
        assert!(matches!(enigo_button(MouseButton::Forward), Button::Forward));
    }

    /// Real `mouse_move` against the active desktop. Ignored by default
    /// since it requires an interactive session and physically moves the
    /// cursor. The Wayland validation pass called out in the plan should
    /// run this with `--ignored` from a wlroots compositor.
    #[test]
    #[ignore]
    fn mouse_move_smoke() {
        let ctrl = DesktopController::new().expect("desktop controller init");
        if !ctrl.input_available() {
            // No display / permission — skip rather than fail.
            eprintln!("skipping: input not available");
            return;
        }
        ctrl.mouse_move(100, 100, &[]).expect("mouse_move should succeed");
    }

    /// Same shape as `mouse_move_smoke` but with a modifier key, to catch
    /// modifier press/release ordering regressions on real hardware.
    #[test]
    #[ignore]
    fn mouse_move_with_modifier_smoke() {
        let ctrl = DesktopController::new().expect("desktop controller init");
        if !ctrl.input_available() {
            eprintln!("skipping: input not available");
            return;
        }
        ctrl.mouse_move(120, 120, &["shift".to_string()])
            .expect("mouse_move with modifier should succeed");
    }
}
