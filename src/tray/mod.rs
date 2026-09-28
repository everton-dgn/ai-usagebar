//! macOS menu-bar popover over the usage report.
//!
//! View-model helpers are plain Rust with no AppKit dependency. The
//! NSStatusItem + WebView event loop is macOS-only.

#[cfg(target_os = "macos")]
mod account_worker;
mod browse;
pub mod hotkey;
mod icon;
#[cfg(any(target_os = "macos", test))]
mod ipc;
#[cfg(any(target_os = "macos", test))]
mod menu_bar;
mod panel;
mod payload;
mod strip;

#[cfg(target_os = "macos")]
mod assets;
#[cfg(target_os = "macos")]
mod host_macos;
#[cfg(target_os = "macos")]
mod menu_space;
#[cfg(target_os = "macos")]
#[path = "startup_macos.rs"]
mod startup;
#[cfg(target_os = "macos")]
mod status_items;

#[doc(hidden)]
#[cfg(target_os = "macos")]
pub use account_worker::run_if_requested as run_account_worker;
pub use browse::http_url;
pub use icon::{Severity, tray_icon_rgba};
pub use payload::{POLL_INTERVAL, host_payload, worst_severity, wrap_report};
pub use strip::{
    BARS_PIXEL_SIDE, StripContent, StripStyle, bars_rgba, content_from_payload, parse_strip_ipc,
    parse_strip_names, parse_strip_thresholds,
};

/// Process entry for `ai-usagebar-tray`.
pub fn run() -> i32 {
    #[cfg(target_os = "macos")]
    {
        host_macos::run()
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("ai-usagebar-tray is the macOS menu-bar app; it is not used on this OS.");
        1
    }
}
