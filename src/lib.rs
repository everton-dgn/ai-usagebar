//! ai-usagebar library: provider collection, report projection and the macOS
//! menu bar app behind the single `ai-usagebar-tray` binary.

pub mod anthropic;
pub mod anthropic_api;
pub mod antigravity;
pub mod balance;
pub mod cache;
pub mod claude_desktop;
pub mod commandcode;
pub mod config;
pub mod copilot;
pub(crate) mod core;
pub mod countdown;
pub mod cursor;
pub mod custom;
pub mod deepseek;
pub mod detect;
pub mod display;
pub mod error;
pub mod format;
pub mod grok;
pub mod grokbot;
/// Source-scanning helpers for structural guard tests. Test-only.
#[cfg(test)]
pub(crate) mod guard;
pub mod identity;
pub mod jwt;
pub mod kilo;
pub mod kimi;
pub mod kiro;
pub mod minimax;
pub mod modelstudio;
pub mod moonshot;
pub mod notify;
pub mod nous;
pub mod novita;
pub mod ollama;
pub mod openai;
pub mod opencode_go;
pub mod openrouter;
pub mod orcarouter;
pub mod outcome;
pub mod pacing;
pub mod pango;
pub mod report;
pub mod safe_storage;
pub mod serde_helpers;
pub mod supergrok;
pub mod tray;
pub mod usage;
pub mod vendor;
pub mod zai;

pub use error::{AppError, Result};
