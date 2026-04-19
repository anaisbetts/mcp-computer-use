//! Linux Wayland screen capture via `libwayshot`.
//!
//! `libwayshot` speaks the wlroots `wlr-screencopy-unstable-v1` protocol,
//! which works on Sway, Hyprland, River, and other wlroots-based
//! compositors out of the box. GNOME and KDE Wayland do not implement
//! `wlr-screencopy` and would need an XDG Desktop Portal / PipeWire path
//! that is not yet wired up here. The error path documents that.

use image::{ImageFormat, imageops::FilterType};
use libwayshot::WayshotConnection;
use std::io::Cursor;

use super::{CaptureError, Screenshot, ScreenCapture, compute_scale_factor};
use crate::scaling::scaled_dimensions;

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

/// Resampling filter used when downscaling captures. Matches the Windows
/// backend so screenshots look consistent across platforms.
const RESIZE_FILTER: FilterType = FilterType::Triangle;

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
    fn capture(&self, max_dim: Option<u32>) -> Result<Screenshot, CaptureError> {
        let img = self
            .conn
            .screenshot_all(false)
            .map_err(|e| CaptureError::Failed(format!("screenshot_all failed: {e}")))?;
        let desktop_w = img.width();
        let desktop_h = img.height();

        let (target_w, target_h) = scaled_dimensions(desktop_w, desktop_h, max_dim);
        let final_img = if target_w == desktop_w && target_h == desktop_h {
            img
        } else {
            img.resize_exact(target_w, target_h, RESIZE_FILTER)
        };

        let mut png_data = Vec::new();
        final_img
            .write_to(&mut Cursor::new(&mut png_data), ImageFormat::Png)
            .map_err(|e| CaptureError::Failed(format!("PNG encoding failed: {e}")))?;

        Ok(Screenshot {
            png_data,
            width: target_w,
            height: target_h,
            physical_width: desktop_w,
            physical_height: desktop_h,
            scale_factor: compute_scale_factor(target_w, target_h, desktop_w, desktop_h),
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
        let shot = backend.capture(None).expect("capture frame");
        assert!(shot.width > 0 && shot.height > 0);
        assert_eq!(&shot.png_data[..8], b"\x89PNG\r\n\x1a\n");
    }

    /// Real wlr-screencopy capture with a max dimension cap to exercise
    /// the resize path end-to-end.
    #[test]
    #[ignore]
    fn captures_resized_real_frame() {
        let backend = LinuxWaylandCapture::new().expect("wayshot connect");
        let shot = backend.capture(Some(720)).expect("capture frame");
        assert!(shot.width.max(shot.height) <= 720);
        assert!(shot.physical_width >= shot.width);
        assert!(shot.physical_height >= shot.height);
    }
}
