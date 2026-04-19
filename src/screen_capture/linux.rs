//! Linux Wayland screen capture via `libwayshot`.
//!
//! `libwayshot` speaks the wlroots `wlr-screencopy-unstable-v1` protocol,
//! which works on Sway, Hyprland, River, and other wlroots-based
//! compositors out of the box. GNOME and KDE Wayland do not implement
//! `wlr-screencopy` and would need an XDG Desktop Portal / PipeWire path
//! that is not yet wired up here. The error path documents that.

use image::ImageFormat;
use libwayshot::WayshotConnection;
use std::io::Cursor;

use super::{CaptureError, Screenshot, ScreenCapture};

// -----------------------------------------------------------------------------
// Backend
// -----------------------------------------------------------------------------

/// Captures all outputs as a single image on a Wayland session.
pub struct LinuxWaylandCapture {
    conn: WayshotConnection,
}

impl LinuxWaylandCapture {
    pub fn new() -> Result<Self, CaptureError> {
        let conn = WayshotConnection::new().map_err(|e| {
            CaptureError::Failed(format!(
                "WayshotConnection::new failed (need a wlroots-based Wayland \
                 compositor like Sway or Hyprland): {e}"
            ))
        })?;
        Ok(Self { conn })
    }
}

impl ScreenCapture for LinuxWaylandCapture {
    fn capture(&self) -> Result<Screenshot, CaptureError> {
        let img = self
            .conn
            .screenshot_all(false)
            .map_err(|e| CaptureError::Failed(format!("screenshot_all failed: {e}")))?;
        let width = img.width();
        let height = img.height();

        let mut png_data = Vec::new();
        img.write_to(&mut Cursor::new(&mut png_data), ImageFormat::Png)
            .map_err(|e| CaptureError::Failed(format!("PNG encoding failed: {e}")))?;

        Ok(Screenshot {
            png_data,
            width,
            height,
            physical_width: width,
            physical_height: height,
            scale_factor: 1.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real wlr-screencopy capture. Ignored by default because it needs a
    /// Wayland session running a wlroots-based compositor (Sway, Hyprland).
    #[test]
    #[ignore]
    fn captures_one_real_frame() {
        let backend = LinuxWaylandCapture::new().expect("wayshot connect");
        let shot = backend.capture().expect("capture frame");
        assert!(shot.width > 0 && shot.height > 0);
        assert_eq!(&shot.png_data[..8], b"\x89PNG\r\n\x1a\n");
    }
}
