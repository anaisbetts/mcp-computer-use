//! Build script. Almost everything is figured out at the dependency level
//! by `screencapturekit`'s own `build.rs`, but on macOS we need to add a
//! fallback rpath for the Swift 5 runtime libraries (`libswift_Concurrency`
//! et al.).
//!
//! `screencapturekit`'s build script adds rpaths under `xcode-select -p`,
//! which on machines without a full Xcode install (only Command Line
//! Tools) points at `/Applications/Xcode.app/...` — a path that does not
//! exist there. The Swift runtime libs actually live under
//! `/Library/Developer/CommandLineTools/usr/lib/swift/macosx`, so we
//! teach the linker to look there too.
//!
//! On Linux/Windows this is a no-op.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // Both the modern `swift/macosx` and legacy `swift-5.5/macosx`
        // layouts ship with Command Line Tools across recent macOS
        // versions; adding the missing one is harmless.
        for p in [
            "/Library/Developer/CommandLineTools/usr/lib/swift/macosx",
            "/Library/Developer/CommandLineTools/usr/lib/swift-5.5/macosx",
        ] {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{p}");
        }
    }
}
