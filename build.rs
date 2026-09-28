//! Build the tray WebView (Vite) before compiling the macOS host.
//!
//! Fail-closed: the build never installs frontend dependencies, never reuses
//! the frontend's `dist/` or a previous `OUT_DIR` build, and never stages a
//! placeholder. Missing Node, missing `node_modules`, a failed Vite run, or an
//! absent/empty asset fails the Cargo build. See `src/build_support.rs`.

// Lives under `src/` (not compiled into the library) so every source fileset
// that already ships `build.rs` + `src/` also carries it.
#[path = "src/build_support.rs"]
mod build_support;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Overrides the Node executable; defaults to `node` on `PATH`.
const NODE_ENV: &str = "AI_USAGEBAR_NODE";

/// The WebView frontend, relative to the manifest. Also named in the Makefile
/// (`FRONTEND_DIR`) and `.github/workflows/ci.yml`.
const FRONTEND_DIR: &str = "frontend";

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let popover = manifest.join(FRONTEND_DIR);
    for input in [
        "src",
        "index.html",
        "package.json",
        "package-lock.json",
        "vite.config.ts",
        "tsconfig.json",
        "components.json",
    ] {
        println!("cargo:rerun-if-changed={}", popover.join(input).display());
    }

    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os != "macos" {
        return;
    }

    // Only where the frontend is built: a watched path that does not exist
    // reruns the script (and recompiles the crate) on every build.
    println!(
        "cargo:rerun-if-changed={}",
        popover
            .join("node_modules")
            .join(".package-lock.json")
            .display()
    );
    println!("cargo:rerun-if-env-changed={NODE_ENV}");
    let node = std::env::var_os(NODE_ENV).unwrap_or_else(|| OsString::from("node"));

    // The hosts `include_str!` from OUT_DIR/popover, never from the source tree.
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    if let Err(error) = build_support::build_popover(&popover, &out_dir, &node) {
        panic!("tray frontend build failed: {error}");
    }
}
