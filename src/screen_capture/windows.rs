//! Windows screen capture via DXGI Desktop Duplication.
//!
//! `windows-capture` exposes a small synchronous wrapper that grabs one
//! desktop frame at a time, which is exactly what an MCP screenshot tool
//! needs. We pull a frame, copy it out as RGBA (handling row padding),
//! optionally downscale to the executor's max image dimension, then
//! PNG-encode using the `image` crate so the rest of the server sees a
//! normalized payload.

use image::{ImageBuffer, ImageFormat, Rgba, imageops::FilterType};
use std::io::Cursor;
use std::time::{Duration, Instant};
use windows_capture::dxgi_duplication_api::{
    DxgiDuplicationApi, DxgiDuplicationFormat, Error as DxgiError,
};
use windows_capture::monitor::Monitor;

use super::{CaptureError, ScreenCapture, Screenshot, compute_scale_factor};
use crate::scaling::scaled_dimensions;

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

/// Per-attempt timeout for `AcquireNextFrame`. Kept short so the outer loop
/// can rotate quickly when the duplication API hands us metadata-only frames
/// (see [`PRESENTED_FRAME_DEADLINE`]).
const FRAME_ATTEMPT_TIMEOUT_MS: u32 = 250;

/// Hard upper bound on how long a single `capture` call will spin waiting
/// for a frame whose `LastPresentTime` is non-zero. The DXGI Desktop
/// Duplication API frequently returns one or more metadata-only frames
/// immediately after `DuplicateOutput` (and again when the desktop is
/// idle); their texture contents are undefined and very commonly come back
/// as all-black pixels, which is the symptom we are guarding against.
///
/// On a normally interactive desktop the cursor blink and other compositor
/// updates produce a real present within a few hundred ms, so this only
/// matters for completely frozen desktops.
const PRESENTED_FRAME_DEADLINE: Duration = Duration::from_secs(5);

/// Resampling filter used when downscaling captures. `Triangle` (bilinear)
/// is fast and produces clean results for the screenshots a vision model
/// consumes; `Lanczos3` would be sharper but several times slower.
const RESIZE_FILTER: FilterType = FilterType::Triangle;

// -----------------------------------------------------------------------------
// Backend
// -----------------------------------------------------------------------------

/// Captures the primary monitor on Windows.
///
/// `Monitor` wraps a raw `HMONITOR` (`*mut c_void`) and is therefore `Send`
/// but not `Sync`. We sidestep that by re-resolving the primary monitor
/// inside `capture()` rather than caching it on the struct, which keeps
/// the backend trivially shareable across MCP tool invocations.
pub struct WindowsCapture;

impl WindowsCapture {
    pub fn new() -> Result<Self, CaptureError> {
        // Probe once so callers get a clear error at startup if no display
        // is available, rather than only on the first screenshot request.
        let _: Monitor = Monitor::primary()
            .map_err(|e| CaptureError::Failed(format!("primary monitor lookup failed: {e}")))?;
        Ok(Self)
    }
}

impl ScreenCapture for WindowsCapture {
    fn capture(&self, max_dim: Option<u32>) -> Result<Screenshot, CaptureError> {
        let monitor = Monitor::primary()
            .map_err(|e| CaptureError::Failed(format!("primary monitor lookup failed: {e}")))?;
        let mut dup = DxgiDuplicationApi::new(monitor)
            .map_err(|e| CaptureError::Failed(format!("DxgiDuplicationApi::new: {e}")))?;

        let (desktop_w, desktop_h, image_w, image_h, png_data) =
            capture_presented_frame(&mut dup, max_dim)?;

        Ok(Screenshot {
            png_data,
            width: image_w,
            height: image_h,
            physical_width: desktop_w,
            physical_height: desktop_h,
            scale_factor: compute_scale_factor(image_w, image_h, desktop_w, desktop_h),
        })
    }
}

/// Pull frames from `dup` until we get one that actually represents a desktop
/// update (`LastPresentTime != 0`), then map it to CPU memory and PNG-encode
/// it. Returns the desktop dimensions, source pixel format, final image
/// dimensions and PNG bytes.
///
/// The DXGI Desktop Duplication API will happily hand back frames with
/// `LastPresentTime == 0` after `DuplicateOutput` and on idle desktops; the
/// underlying texture for those frames is documented as undefined and on
/// many systems shows up as solid black, which is the bug this works around.
///
/// We rotate `acquire_next_frame` (which internally releases the previous
/// frame at the start of each call) until either we get a presented frame
/// or [`PRESENTED_FRAME_DEADLINE`] elapses. `DxgiError::Timeout` from a
/// single attempt is normal on idle desktops and is treated as "try again".
///
/// All buffer extraction happens inside the loop so the borrowed
/// [`DxgiDuplicationFrame`] never has to escape it.
fn capture_presented_frame(
    dup: &mut DxgiDuplicationApi,
    max_dim: Option<u32>,
) -> Result<(u32, u32, u32, u32, Vec<u8>), CaptureError> {
    let deadline = Instant::now() + PRESENTED_FRAME_DEADLINE;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(CaptureError::Failed(
                "no desktop update arrived within deadline".to_string(),
            ));
        }
        let remaining_ms = (deadline - now).as_millis().min(u128::from(u32::MAX)) as u32;
        let attempt_ms = remaining_ms.min(FRAME_ATTEMPT_TIMEOUT_MS).max(1);

        let mut frame = match dup.acquire_next_frame(attempt_ms) {
            Ok(f) => f,
            Err(DxgiError::Timeout) => continue,
            Err(e) => return Err(CaptureError::Failed(format!("acquire_next_frame: {e}"))),
        };
        if frame.frame_info().LastPresentTime == 0 {
            continue;
        }

        let buffer = frame
            .buffer()
            .map_err(|e| CaptureError::Failed(format!("frame.buffer: {e}")))?;

        let desktop_w = buffer.width();
        let desktop_h = buffer.height();
        let format = buffer.format();

        let mut unpadded: Vec<u8> = Vec::new();
        let pixels = buffer.as_nopadding_buffer(&mut unpadded);

        let (image_w, image_h, png_data) =
            encode_png(pixels, desktop_w, desktop_h, format, max_dim)?;

        return Ok((desktop_w, desktop_h, image_w, image_h, png_data));
    }
}

// -----------------------------------------------------------------------------
// Pixel conversion
// -----------------------------------------------------------------------------

/// Encode a tightly-packed pixel buffer as PNG, optionally downscaling
/// to fit within `max_dim` on the longest side.
///
/// Returns the final image dimensions alongside the PNG bytes so the
/// caller can record the coordinate map between image and desktop
/// space.
///
/// The DXGI duplication API hands us BGRA8 by default and may also use
/// RGBA8. Other formats (e.g. 16-bit float HDR) are flagged as unsupported
/// rather than silently producing wrong colors.
fn encode_png(
    pixels: &[u8],
    width: u32,
    height: u32,
    format: DxgiDuplicationFormat,
    max_dim: Option<u32>,
) -> Result<(u32, u32, Vec<u8>), CaptureError> {
    let rgba = match format {
        DxgiDuplicationFormat::Bgra8 | DxgiDuplicationFormat::Bgra8Srgb => bgra_to_rgba(pixels),
        DxgiDuplicationFormat::Rgba8 | DxgiDuplicationFormat::Rgba8Srgb => pixels.to_vec(),
        other => {
            return Err(CaptureError::Failed(format!(
                "unsupported DXGI pixel format: {other:?}"
            )));
        }
    };

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, rgba)
        .ok_or_else(|| {
            CaptureError::Failed(format!("pixel buffer size mismatch ({width}x{height})"))
        })?;

    let (target_w, target_h) = scaled_dimensions(width, height, max_dim);
    let final_img = if target_w == width && target_h == height {
        img
    } else {
        image::imageops::resize(&img, target_w, target_h, RESIZE_FILTER)
    };

    let mut out = Vec::with_capacity((target_w * target_h) as usize);
    final_img
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|e| CaptureError::Failed(format!("PNG encoding failed: {e}")))?;
    Ok((target_w, target_h, out))
}

fn bgra_to_rgba(pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len());
    for chunk in pixels.chunks_exact(4) {
        out.push(chunk[2]);
        out.push(chunk[1]);
        out.push(chunk[0]);
        out.push(chunk[3]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_to_rgba_swaps_channels() {
        let bgra = [10u8, 20, 30, 40];
        let rgba = bgra_to_rgba(&bgra);
        assert_eq!(rgba, vec![30, 20, 10, 40]);
    }

    #[test]
    fn bgra_to_rgba_handles_multiple_pixels() {
        let bgra = [1, 2, 3, 4, 5, 6, 7, 8];
        let rgba = bgra_to_rgba(&bgra);
        assert_eq!(rgba, vec![3, 2, 1, 4, 7, 6, 5, 8]);
    }

    /// Real DXGI capture against the active desktop. Ignored by default
    /// because it needs an interactive Windows session.
    ///
    /// Also acts as a regression test for the "all-black frame" bug: the
    /// previous implementation would routinely return a frame whose pixels
    /// were entirely zero, so we decode the PNG and assert at least one
    /// non-black pixel exists.
    #[test]
    #[ignore]
    fn captures_one_real_frame() {
        let backend = WindowsCapture::new().expect("capture backend init");
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

    /// Real DXGI capture with a small max dimension to exercise the
    /// resize path end-to-end. Ignored alongside the unscaled capture.
    #[test]
    #[ignore]
    fn captures_resized_real_frame() {
        let backend = WindowsCapture::new().expect("capture backend init");
        let shot = backend.capture(Some(720)).expect("capture frame");
        assert!(shot.width.max(shot.height) <= 720);
        assert!(shot.physical_width >= shot.width);
        assert!(shot.physical_height >= shot.height);
    }
}
