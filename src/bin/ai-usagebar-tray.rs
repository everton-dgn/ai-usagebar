//! macOS menu bar app. The same executable also serves the private account
//! worker the app spawns for account switches.

fn main() {
    #[cfg(target_os = "macos")]
    if let Some(code) = ai_usagebar::tray::run_account_worker() {
        std::process::exit(code);
    }
    std::process::exit(ai_usagebar::tray::run());
}
