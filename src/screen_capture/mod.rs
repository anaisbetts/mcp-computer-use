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
/// `width`/`height` are the logical pixel size used by the input
/// coordinate space, while `physical_width`/`physical_height` describe
/// the actual pixels in `png_data`. On Windows and Wayland today these
/// are the same and `scale_factor` is always `1.0`, but the schema keeps
/// room for HiDPI accounting later.
pub struct Screenshot {
    pub png_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f32,
}

/// One-shot screen capture API used by the executor.
pub trait ScreenCapture: Send + Sync {
    fn capture(&self) -> Result<Screenshot, CaptureError>;
}

// -----------------------------------------------------------------------------
// Per-OS dispatch
// -----------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
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
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let backend = unsupported::UnsupportedCapture;
        Ok(Box::new(backend))
    }
}
