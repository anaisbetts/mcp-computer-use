//! Per-platform screen capture.
//!
//! Each supported OS provides one [`ScreenCapture`] implementation that
//! returns a normalized [`Screenshot`]. The shared executor in
//! `computer_use` consumes [`Screenshot`] without caring about the platform.
//!
//! Currently supported: Windows (DXGI Desktop Duplication) and Linux Wayland
//! (wlroots wlr-screencopy via `libwayshot`). Other platforms get a stub
//! backend that returns a clear error so the rest of the server still runs.

use thiserror::Error;

// -----------------------------------------------------------------------------
// Errors and shared types
// -----------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum CaptureError {
    /// Constructed only by the fallback backend on platforms with no real
    /// implementation; included so error mapping in the executor stays total.
    #[allow(dead_code)]
    #[error("screen capture unavailable on this platform: {0}")]
    Unsupported(String),

    #[error("screen capture failed: {0}")]
    Failed(String),
}

/// One captured screen, encoded as PNG bytes plus pixel metadata.
///
/// `width`/`height` describe the pixel dimensions of the PNG actually
/// returned to the client — they are the coordinate space the client
/// (and any downstream model) sees in the screenshot. When the
/// executor caps screenshot size, those are smaller than the original
/// capture.
///
/// `physical_width`/`physical_height` are the original capture
/// dimensions, i.e. the absolute desktop coordinate space the input
/// controller writes into. `scale_factor` is `image / desktop` for the
/// longest axis, matching the ratio needed to map model-emitted
/// coordinates back to the desktop.
pub struct Screenshot {
    pub png_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f32,
}

/// One-shot screen capture API used by the executor.
///
/// `max_dim`, when `Some(n)` with `n > 0`, caps the longest pixel
/// dimension of the returned image to `n`, preserving aspect ratio.
/// Backends never upscale.
pub trait ScreenCapture: Send + Sync {
    fn capture(&self, max_dim: Option<u32>) -> Result<Screenshot, CaptureError>;
}

// -----------------------------------------------------------------------------
// Per-OS dispatch
// -----------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod unsupported;

/// Construct the platform-default capture backend.
pub fn new_default() -> Result<Box<dyn ScreenCapture>, CaptureError> {
    #[cfg(target_os = "windows")]
    {
        let backend = windows::WindowsCapture::new()?;
        Ok(Box::new(backend))
    }
    #[cfg(target_os = "linux")]
    {
        let backend = linux::LinuxWaylandCapture::new()?;
        Ok(Box::new(backend))
    }
    #[cfg(target_os = "macos")]
    {
        let backend = macos::MacOsCapture::new()?;
        Ok(Box::new(backend))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let backend = unsupported::UnsupportedCapture;
        Ok(Box::new(backend))
    }
}

// -----------------------------------------------------------------------------
// Helpers shared across backends
// -----------------------------------------------------------------------------

/// Compute the `scale_factor` reported on a [`Screenshot`].
///
/// Defined as `image_dim / desktop_dim` on the longest desktop axis,
/// which is the ratio the executor needs to remap model coordinates
/// back to absolute desktop space. Falls back to `1.0` for degenerate
/// inputs.
#[allow(dead_code)] // Used by per-OS backends behind cfg gates.
pub(crate) fn compute_scale_factor(
    image_w: u32,
    image_h: u32,
    desktop_w: u32,
    desktop_h: u32,
) -> f32 {
    let longest_desktop = desktop_w.max(desktop_h);
    if longest_desktop == 0 {
        return 1.0;
    }
    let longest_image = if desktop_w >= desktop_h {
        image_w
    } else {
        image_h
    };
    longest_image as f32 / longest_desktop as f32
}
