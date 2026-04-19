//! macOS screen capture via Apple's ScreenCaptureKit (`screencapturekit` crate).
//!
//! `SCScreenshotManager::capture_image` is a true one-shot capture API
//! introduced in macOS 14.0, which fits the [`ScreenCapture::capture`]
//! trait perfectly — no streaming setup, no async runtime.
//!
//! Coordinate-space caveat: `enigo` on macOS feeds raw `(x, y)` into
//! `CGPoint::new`, which is interpreted by Core Graphics as **logical
//! display points**, not physical pixels. The PNG we hand back is in
//! pixels, so [`Screenshot::physical_width`]/`physical_height` are
//! reported in *points* (pixels divided by `point_pixel_scale`) so the
//! executor's `image -> desktop` remap lands in enigo's coordinate
//! space.

use image::{ImageBuffer, ImageFormat, Rgba, imageops::FilterType};
use screencapturekit::screenshot_manager::SCScreenshotManager;
use screencapturekit::shareable_content::SCShareableContent;
use screencapturekit::stream::configuration::{PixelFormat, SCStreamConfiguration};
use screencapturekit::stream::content_filter::SCContentFilter;
use std::io::Cursor;

use super::{CaptureError, ScreenCapture, Screenshot, compute_scale_factor};
use crate::scaling::scaled_dimensions;

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

/// Resampling filter used when downscaling captures. Matches the other
/// backends so screenshots look consistent across platforms.
const RESIZE_FILTER: FilterType = FilterType::Triangle;

// -----------------------------------------------------------------------------
// Backend
// -----------------------------------------------------------------------------

/// Captures the primary display on macOS via ScreenCaptureKit.
///
/// We re-resolve the primary display inside `capture()` rather than
/// caching it so the backend is trivially `Send + Sync` and so a
/// display reconfiguration between calls (resolution change, monitor
/// hot-plug) can't leave us holding a stale `SCDisplay`.
pub struct MacOsCapture;

impl MacOsCapture {
    /// Probe ScreenCaptureKit once at startup so missing Screen
    /// Recording permission surfaces immediately as a clear error
    /// rather than only on the first screenshot request.
    pub fn new() -> Result<Self, CaptureError> {
        let content = SCShareableContent::get().map_err(|e| {
            CaptureError::Failed(format!(
                "SCShareableContent::get failed (grant Screen Recording \
                 permission to the host process under System Settings → \
                 Privacy & Security → Screen Recording): {e}"
            ))
        })?;
        if content.displays().is_empty() {
            return Err(CaptureError::Failed("no displays available".into()));
        }
        Ok(Self)
    }
}

impl ScreenCapture for MacOsCapture {
    fn capture(&self, max_dim: Option<u32>) -> Result<Screenshot, CaptureError> {
        let content = SCShareableContent::get()
            .map_err(|e| CaptureError::Failed(format!("SCShareableContent::get failed: {e}")))?;
        let displays = content.displays();
        let display = displays
            .first()
            .ok_or_else(|| CaptureError::Failed("no displays available".into()))?;

        let pixel_w = display.width();
        let pixel_h = display.height();
        if pixel_w == 0 || pixel_h == 0 {
            return Err(CaptureError::Failed(format!(
                "display reports zero dimensions ({pixel_w}x{pixel_h})"
            )));
        }

        let filter = SCContentFilter::create()
            .with_display(display)
            .with_excluding_windows(&[])
            .build();

        // `point_pixel_scale` is typically 2.0 on Retina, 1.0 elsewhere.
        // Clamp at 1.0 so a degenerate 0.0/NaN can't blow up the
        // points calculation below.
        let scale = filter.point_pixel_scale().max(1.0);

        let config = SCStreamConfiguration::new()
            .with_width(pixel_w)
            .with_height(pixel_h)
            .with_pixel_format(PixelFormat::BGRA)
            .with_shows_cursor(true);

        let cg_image = SCScreenshotManager::capture_image(&filter, &config).map_err(|e| {
            CaptureError::Failed(format!("SCScreenshotManager::capture_image: {e}"))
        })?;
        let rgba = cg_image
            .rgba_data()
            .map_err(|e| CaptureError::Failed(format!("CGImage::rgba_data: {e}")))?;

        // Use the CGImage's own dimensions (post-capture), which match
        // the configured width/height but defend against any rounding
        // SCK might apply.
        let img_w = cg_image.width() as u32;
        let img_h = cg_image.height() as u32;

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(img_w, img_h, rgba)
            .ok_or_else(|| {
                CaptureError::Failed(format!("pixel buffer size mismatch ({img_w}x{img_h})"))
            })?;

        let (target_w, target_h) = scaled_dimensions(img_w, img_h, max_dim);
        let final_img = if target_w == img_w && target_h == img_h {
            img
        } else {
            image::imageops::resize(&img, target_w, target_h, RESIZE_FILTER)
        };

        let mut png_data = Vec::new();
        final_img
            .write_to(&mut Cursor::new(&mut png_data), ImageFormat::Png)
            .map_err(|e| CaptureError::Failed(format!("PNG encoding failed: {e}")))?;

        let point_w = ((img_w as f32) / scale).round().max(1.0) as u32;
        let point_h = ((img_h as f32) / scale).round().max(1.0) as u32;

        Ok(Screenshot {
            png_data,
            width: target_w,
            height: target_h,
            physical_width: point_w,
            physical_height: point_h,
            scale_factor: compute_scale_factor(target_w, target_h, point_w, point_h),
        })
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Real ScreenCaptureKit capture against the active desktop. Ignored
    /// by default because it needs an interactive macOS session and
    /// Screen Recording permission granted to the host process.
    #[test]
    #[ignore]
    fn captures_one_real_frame() {
        let backend = MacOsCapture::new().expect("capture backend init");
        let shot = backend.capture(None).expect("capture frame");
        assert!(shot.width > 0 && shot.height > 0);
        assert!(!shot.png_data.is_empty());
        assert_eq!(&shot.png_data[..8], b"\x89PNG\r\n\x1a\n");

        let decoded = image::load_from_memory_with_format(&shot.png_data, ImageFormat::Png)
            .expect("decode captured PNG")
            .to_rgba8();
        let any_non_black = decoded
            .pixels()
            .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0);
        assert!(any_non_black, "captured frame is entirely black pixels");
    }

    /// Real ScreenCaptureKit capture with a max dimension cap to exercise
    /// the resize path end-to-end.
    #[test]
    #[ignore]
    fn captures_resized_real_frame() {
        let backend = MacOsCapture::new().expect("capture backend init");
        let shot = backend.capture(Some(720)).expect("capture frame");
        assert!(shot.width.max(shot.height) <= 720);
        assert!(shot.physical_width >= 1);
        assert!(shot.physical_height >= 1);
    }
}
