//! Fallback capture backend for platforms with no implementation.
//!
//! Keeps the rest of the server compiling and running so input actions
//! still work; only screenshot calls return a clear error.

use super::{CaptureError, Screenshot, ScreenCapture};

pub struct UnsupportedCapture;

impl ScreenCapture for UnsupportedCapture {
    fn capture(&self) -> Result<Screenshot, CaptureError> {
        Err(CaptureError::Unsupported(
            "no screen capture backend compiled for this OS".into(),
        ))
    }
}
