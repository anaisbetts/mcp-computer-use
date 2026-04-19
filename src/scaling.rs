//! Dimension and coordinate math for screenshot downscaling.
//!
//! Screenshots returned to MCP clients can be capped to a maximum
//! pixel dimension on their longest side. When that happens every
//! subsequent mouse coordinate the model emits is in that scaled
//! image space and must be remapped back to absolute desktop
//! coordinates before it reaches the input controller.

use crate::computer_use::XY;

// -----------------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------------

/// Mapping from a (possibly downscaled) image's coordinate space to the
/// absolute desktop coordinate space the input controller expects.
///
/// `image_*` are the pixel dimensions actually returned to the client.
/// `desktop_*` are the original capture dimensions — i.e. the absolute
/// coordinate space `enigo` writes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinateMap {
    pub image_width: u32,
    pub image_height: u32,
    pub desktop_width: u32,
    pub desktop_height: u32,
}

// -----------------------------------------------------------------------------
// Scaled dimensions (theme of this file)
// -----------------------------------------------------------------------------

/// Compute the scaled `(width, height)` for an original image, capping
/// the longest side to `max_dim` while preserving aspect ratio.
///
/// `None` or a `max_dim` of `0` disables scaling. Images whose longest
/// side is already at or below the cap are returned unchanged — we
/// never upscale.
pub fn scaled_dimensions(orig_w: u32, orig_h: u32, max_dim: Option<u32>) -> (u32, u32) {
    let max = match max_dim {
        Some(n) if n > 0 => n,
        _ => return (orig_w, orig_h),
    };
    let longest = orig_w.max(orig_h);
    if longest == 0 || longest <= max {
        return (orig_w, orig_h);
    }
    let scale = max as f64 / longest as f64;
    let w = ((orig_w as f64) * scale).round().max(1.0) as u32;
    let h = ((orig_h as f64) * scale).round().max(1.0) as u32;
    (w, h)
}

// -----------------------------------------------------------------------------
// Coordinate remapping
// -----------------------------------------------------------------------------

impl CoordinateMap {
    /// Remap a single point from image space to desktop space.
    ///
    /// Returns the input unchanged when the map is a no-op (image and
    /// desktop dimensions match) or when either image dimension is
    /// zero, which would otherwise divide by zero.
    pub fn remap_point(&self, x: i32, y: i32) -> (i32, i32) {
        if self.is_identity() || self.image_width == 0 || self.image_height == 0 {
            return (x, y);
        }
        let sx = self.desktop_width as f64 / self.image_width as f64;
        let sy = self.desktop_height as f64 / self.image_height as f64;
        let mx = ((x as f64) * sx).round() as i32;
        let my = ((y as f64) * sy).round() as i32;
        (mx, my)
    }

    /// Remap every point in a drag path from image to desktop space.
    pub fn remap_path(&self, path: &[XY]) -> Vec<XY> {
        path.iter()
            .map(|p| {
                let (x, y) = self.remap_point(p.x, p.y);
                XY { x, y }
            })
            .collect()
    }

    /// `true` when the map is a 1:1 passthrough.
    pub fn is_identity(&self) -> bool {
        self.image_width == self.desktop_width && self.image_height == self.desktop_height
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_max_dim_returns_original() {
        assert_eq!(scaled_dimensions(1920, 1080, None), (1920, 1080));
        assert_eq!(scaled_dimensions(1920, 1080, Some(0)), (1920, 1080));
    }

    #[test]
    fn smaller_than_cap_is_unchanged() {
        assert_eq!(scaled_dimensions(640, 480, Some(720)), (640, 480));
        assert_eq!(scaled_dimensions(720, 405, Some(720)), (720, 405));
    }

    #[test]
    fn landscape_is_capped_on_width() {
        assert_eq!(scaled_dimensions(1920, 1080, Some(720)), (720, 405));
    }

    #[test]
    fn portrait_is_capped_on_height() {
        let (w, h) = scaled_dimensions(1080, 1920, Some(720));
        assert_eq!((w, h), (405, 720));
    }

    #[test]
    fn square_caps_both_axes_equally() {
        assert_eq!(scaled_dimensions(2000, 2000, Some(720)), (720, 720));
    }

    #[test]
    fn never_upscales() {
        assert_eq!(scaled_dimensions(100, 50, Some(720)), (100, 50));
    }

    #[test]
    fn identity_map_is_passthrough() {
        let map = CoordinateMap {
            image_width: 1920,
            image_height: 1080,
            desktop_width: 1920,
            desktop_height: 1080,
        };
        assert!(map.is_identity());
        assert_eq!(map.remap_point(123, 456), (123, 456));
    }

    #[test]
    fn scaled_map_remaps_origin_and_corner() {
        let map = CoordinateMap {
            image_width: 720,
            image_height: 405,
            desktop_width: 1920,
            desktop_height: 1080,
        };
        assert!(!map.is_identity());
        assert_eq!(map.remap_point(0, 0), (0, 0));
        let (x, y) = map.remap_point(360, 202);
        // 360/720 * 1920 = 960; 202/405 * 1080 ≈ 538.67 -> 539
        assert_eq!(x, 960);
        assert_eq!(y, 539);
    }

    #[test]
    fn remap_path_applies_per_point() {
        let map = CoordinateMap {
            image_width: 720,
            image_height: 405,
            desktop_width: 1440,
            desktop_height: 810,
        };
        let path = vec![XY { x: 0, y: 0 }, XY { x: 360, y: 202 }];
        let remapped = map.remap_path(&path);
        assert_eq!(remapped.len(), 2);
        assert_eq!((remapped[0].x, remapped[0].y), (0, 0));
        assert_eq!((remapped[1].x, remapped[1].y), (720, 404));
    }

    #[test]
    fn zero_image_dimensions_passes_through() {
        let map = CoordinateMap {
            image_width: 0,
            image_height: 0,
            desktop_width: 1920,
            desktop_height: 1080,
        };
        assert_eq!(map.remap_point(10, 20), (10, 20));
    }
}
