//! Read-only view of the Claude Desktop app's saved account profiles, and the
//! paths the inactive-profile OAuth refresh shares with them.
//!
//! The Claude Desktop internals this relies on (the data-directory layout, the
//! `oauth:tokenCache` / `oauth:tokenCacheV2` / `lastKnownAccountUuid` fields in
//! `config.json` and the profile store) were reverse-engineered by
//! **claude-acc** (<https://github.com/ohmaseclaro/claude-acc>, MIT). This
//! module reads its profile store so the two tools stay interchangeable;
//! capturing and switching profiles belong to claude-acc.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::AnthropicConfig;
use crate::error::Result;

/// Claude Desktop's own state file: OAuth token caches plus the account pointer.
const CONFIG_JSON: &str = "config.json";

/// Profile-store filenames, owned by claude-acc's layout.
const TOKEN_CACHE: &str = "config-tokenCache";
const TOKEN_CACHE_V2: &str = "config-tokenCacheV2";
const DESKTOP_STATE: &str = "desktop-state";
const META_JSON: &str = "meta.json";

/// Where the Claude Desktop app and the saved profiles live.
///
/// Constructed with [`Paths::at`] in tests so nothing reads a real `$HOME`.
#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub profiles_dir: PathBuf,
    pub backups_dir: PathBuf,
}

impl Paths {
    /// Test seam: every root explicit.
    pub fn at(data_dir: PathBuf, profiles_dir: PathBuf, backups_dir: PathBuf) -> Self {
        Self {
            data_dir,
            profiles_dir,
            backups_dir,
        }
    }

    /// Production paths. `desktop_profiles_dir` overrides the claude-acc
    /// default; rollback archives land beside the profile store.
    pub fn resolve(anthropic: &AnthropicConfig) -> Result<Self> {
        let home = crate::cache::home_dir()?;
        let profiles_dir = anthropic
            .desktop_profiles_dir
            .clone()
            .unwrap_or_else(|| home.join(".claude-acc").join("profiles"));
        let backups_dir = profiles_dir
            .parent()
            .map_or_else(|| home.join(".claude-acc"), Path::to_path_buf)
            .join("backups");
        Ok(Self {
            data_dir: home.join("Library/Application Support/Claude"),
            profiles_dir,
            backups_dir,
        })
    }

    /// Whether there is a Claude Desktop app installation to act on at all.
    /// False on Linux, and on a Mac where the app has never run.
    pub fn available(&self) -> bool {
        self.data_dir.is_dir()
    }

    pub fn config_json(&self) -> PathBuf {
        self.data_dir.join(CONFIG_JSON)
    }

    pub fn profile_dir(&self, label: &str) -> PathBuf {
        self.profiles_dir.join(label)
    }

    /// Shared by Desktop account switching and inactive-profile OAuth refresh.
    /// Both operations can rotate or install the same saved credential.
    pub fn account_switch_lock(&self) -> PathBuf {
        self.backups_dir.join(".account-switch.lock")
    }
}

/// One saved account in the profile store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileMeta {
    /// The profile directory's name — authoritative, so a hand-edited
    /// `meta.json` can never make a profile answer to the wrong label.
    pub label: String,
    pub email: Option<String>,
    pub account_uuid: String,
    /// Absent until the app has created a session folder for the account;
    /// without it the history merges are skipped but the credential swap
    /// still works.
    pub org_uuid: Option<String>,
    pub has_credentials: bool,
    pub has_desktop_state: bool,
}

#[derive(Debug, Deserialize)]
struct RawMeta {
    email: Option<String>,
    #[serde(rename = "accountUuid")]
    account_uuid: Option<String>,
    #[serde(rename = "orgUuid")]
    org_uuid: Option<String>,
}

/// Every saved profile, sorted by label. Best-effort by design: one unreadable
/// or hand-mangled `meta.json` is skipped rather than failing `account status`
/// for every other account.
pub fn load_profiles(profiles_dir: &Path) -> Vec<ProfileMeta> {
    let Ok(entries) = std::fs::read_dir(profiles_dir) else {
        return Vec::new();
    };
    let mut profiles: Vec<ProfileMeta> = entries
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let label = dir.file_name()?.to_str()?.to_string();
            let raw: RawMeta =
                serde_json::from_slice(&std::fs::read(dir.join(META_JSON)).ok()?).ok()?;
            let account_uuid = raw.account_uuid.filter(|uuid| !uuid.is_empty())?;
            Some(ProfileMeta {
                label,
                email: raw.email.filter(|email| !email.is_empty()),
                account_uuid,
                org_uuid: raw.org_uuid.filter(|uuid| !uuid.is_empty()),
                has_credentials: dir.join(TOKEN_CACHE).is_file()
                    && dir.join(TOKEN_CACHE_V2).is_file(),
                has_desktop_state: dir.join(DESKTOP_STATE).is_dir(),
            })
        })
        .collect();
    profiles.sort_by(|a, b| a.label.cmp(&b.label));
    profiles
}

/// Which account the Desktop app currently believes it is. This is the app's
/// own pointer, not a guess from file timestamps.
pub fn active_account_uuid(config_json: &Path) -> Option<String> {
    let bytes = std::fs::read(config_json).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("lastKnownAccountUuid")?
        .as_str()
        .filter(|uuid| !uuid.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _root: tempfile::TempDir,
        paths: Paths,
    }

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// Two accounts: `here` (active) and `there` (fully captured).
    fn fixture() -> Fixture {
        let root = tempfile::TempDir::new().unwrap();
        let data = root.path().join("data");
        let profiles = root.path().join("profiles");
        let backups = root.path().join("backups");

        write(
            &data.join(CONFIG_JSON),
            r#"{"lastKnownAccountUuid":"uuid-here","oauth:tokenCache":"live-a",
                "oauth:tokenCacheV2":"live-b","dxt:allowlistEnabled:org-1":true}"#,
        );

        write(
            &profiles.join("here/meta.json"),
            r#"{"label":"here","email":"here@example.com","accountUuid":"uuid-here","orgUuid":"org-1"}"#,
        );
        write(
            &profiles.join("there/meta.json"),
            r#"{"label":"there","email":"there@example.com","accountUuid":"uuid-there","orgUuid":"org-2"}"#,
        );
        write(&profiles.join("there").join(TOKEN_CACHE), "saved-a");
        write(&profiles.join("there").join(TOKEN_CACHE_V2), "saved-b");
        write(
            &profiles.join("there").join(DESKTOP_STATE).join("Cookies"),
            "there-cookies",
        );
        write(
            &profiles
                .join("there")
                .join(DESKTOP_STATE)
                .join("Local Storage/leveldb/CURRENT"),
            "there-ldb",
        );

        Fixture {
            paths: Paths::at(data, profiles, backups),
            _root: root,
        }
    }

    #[test]
    fn profiles_load_sorted_with_their_capture_state() {
        let fixture = fixture();
        let profiles = load_profiles(&fixture.paths.profiles_dir);

        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].label, "here");
        assert_eq!(profiles[0].email.as_deref(), Some("here@example.com"));
        assert!(!profiles[0].has_credentials);
        assert_eq!(profiles[1].label, "there");
        assert!(profiles[1].has_credentials);
        assert!(profiles[1].has_desktop_state);
    }

    #[test]
    fn a_malformed_profile_is_skipped_not_fatal() {
        let fixture = fixture();
        write(
            &fixture.paths.profiles_dir.join("broken/meta.json"),
            "{ not json",
        );
        write(
            &fixture.paths.profiles_dir.join("no-uuid/meta.json"),
            r#"{"label":"x"}"#,
        );

        let profiles = load_profiles(&fixture.paths.profiles_dir);
        let labels: Vec<&str> = profiles.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["here", "there"]);
    }

    /// Notes are printed verbatim by `account`, so a path entering one must go
    /// through `sanitize_untrusted_path` — `Display for Path` escapes nothing.
    ///
    /// Scoped to `notes.push` rather than to the file, because elsewhere in
    /// this module a bare `.display()` is the *correct* call:
    /// `collect_registry_members` builds tar arguments, and sanitizing one
    /// would corrupt the filename actually handed to `tar`. The distinction is
    /// whether the string is read by a person or by a program.
    #[test]
    fn no_note_interpolates_an_unsanitized_path() {
        let mut sites = Vec::new();
        for file in crate::guard::rs_files_in("src") {
            let source = std::fs::read_to_string(&file).expect("readable module");
            let body = crate::guard::production_code(&source);
            let mut rest = body.as_str();
            while let Some(at) = rest.find("notes.push(") {
                let call = &rest[at..];
                let mut depth = 0usize;
                let mut end = call.len();
                for (i, ch) in call.char_indices() {
                    match ch {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = i;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if call[..end].contains(".display()") {
                    sites.push(format!("{}: {}", file.display(), &call[..end]));
                }
                rest = &call[end.max(1)..];
            }
        }
        assert!(
            sites.is_empty(),
            "a note reaches the terminal verbatim; render its path with \
             `sanitize_untrusted_path`. Found: {sites:#?}"
        );
    }
}
