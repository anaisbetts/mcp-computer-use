//! Windows screen capture via DXGI Desktop Duplication.
//!
//! `windows-capture` exposes a small synchronous wrapper that grabs one
//! desktop frame at a time, which is exactly what an MCP screenshot tool
//! needs. We pull a frame, copy it out as RGBA (handling row padding),
//! then PNG-encode using the `image` crate so the rest of the server
//! sees a normalized payload.

use image::{ImageBuffer, ImageFormat, Rgba};
use std::io::Cursor;
use windows_capture::dxgi_duplication_api::{DxgiDuplicationApi, DxgiDuplicationFormat};
use windows_capture::monitor::Monitor;

use super::{CaptureError, Screenshot, ScreenCapture};

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

/// Per-frame timeout. Wide enough that a stalled compositor still completes
/// the call but tight enough that we don't hang the MCP request indefinitely.
const FRAME_TIMEOUT_MS: u32 = 1_000;

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
    fn capture(&self) -> Result<Screenshot, CaptureError> {
        let monitor = Monitor::primary()
            .map_err(|e| CaptureError::Failed(format!("primary monitor lookup failed: {e}")))?;
        let mut dup = DxgiDuplicationApi::new(monitor)
            .map_err(|e| CaptureError::Failed(format!("DxgiDuplicationApi::new: {e}")))?;
        let mut frame = dup
            .acquire_next_frame(FRAME_TIMEOUT_MS)
            .map_err(|e| CaptureError::Failed(format!("acquire_next_frame: {e}")))?;
        let buffer = frame
            .buffer()
            .map_err(|e| CaptureError::Failed(format!("frame.buffer: {e}")))?;

        let width = buffer.width();
        let height = buffer.height();
        let format = buffer.format();

        let mut unpadded: Vec<u8> = Vec::new();
        let pixels = buffer.as_nopadding_buffer(&mut unpadded);

        let png_data = encode_png(pixels, width, height, format)?;

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

// -----------------------------------------------------------------------------
// Pixel conversion
// -----------------------------------------------------------------------------

/// Encode a tightly-packed pixel buffer as PNG.
///
/// The DXGI duplication API hands us BGRA8 by default and may also use
/// RGBA8. Other formats (e.g. 16-bit float HDR) are flagged as unsupported
/// rather than silently producing wrong colors.
fn encode_png(
    pixels: &[u8],
    width: u32,
    height: u32,
    format: DxgiDuplicationFormat,
) -> Result<Vec<u8>, CaptureError> {
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
            CaptureError::Failed(format!(
                "pixel buffer size mismatch ({}x{})",
                width, height
            ))
        })?;

    let mut out = Vec::with_capacity((width * height) as usize);
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|e| CaptureError::Failed(format!("PNG encoding failed: {e}")))?;
    Ok(out)
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
    #[test]
    #[ignore]
    fn captures_one_real_frame() {
        let backend = WindowsCapture::new().expect("capture backend init");
        let shot = backend.capture().expect("capture frame");
        assert!(shot.width > 0 && shot.height > 0);
        assert!(!shot.png_data.is_empty());
        // Standard PNG signature.
        assert_eq!(&shot.png_data[..8], b"\x89PNG\r\n\x1a\n");
    }
}
