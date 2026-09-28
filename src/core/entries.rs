//! Provider entry identity and per-entry state for the app:
//! built-in vendors, configured accounts, saved Claude Desktop profiles, and
//! `[[custom]]` providers.

use std::collections::HashSet;

use crate::config::{Config, CustomProviderConfig};
use crate::vendor::VendorId;

/// What we display per vendor — raw snapshot + fetch metadata for native
/// panel rendering, or an error message when the fetch failed.
///
/// `Ready` is boxed because the snapshot is much larger than the other
/// variant (silences `clippy::large_enum_variant`).
#[derive(Debug, Clone)]
pub enum TabState {
    Ready(Box<ReadyTab>),
    Error {
        message: String,
        /// Vendor plan already known from credentials when the quota fetch
        /// failed (Claude OAuth `subscriptionType`). Absent when the vendor
        /// has no plan without a snapshot.
        plan: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct ReadyTab {
    pub snapshot: crate::usage::VendorSnapshot,
    pub email: Option<crate::identity::AccountEmail>,
    pub stale: bool,
    pub last_error: Option<(u16, String)>,
    /// Absolute moment the cache was written (i.e. the API response landed).
    /// Snapshotted once at TabState build time so the rendered "Updated …"
    /// timestamp stays stable across redraws instead of drifting with the
    /// passing wall clock.
    pub fetched_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Bar-number settings this vendor was configured with — the tank size a
    /// prepaid balance is metered against, and which of the two numbers goes on
    /// the bar. Resolved from config at fetch time rather than stored in the
    /// snapshot, so editing config.toml takes effect on the next redraw instead
    /// of waiting for the cache to expire.
    pub display: crate::balance::DisplayPrefs,
}

/// Where a tab's usage comes from: a built-in vendor, or a user-declared
/// `[[custom]]` HTTP provider. A custom tab carries its own display names so
/// the report never needs the config again just to label it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TabSource {
    Builtin(VendorId),
    Custom {
        id: String,
        name: String,
        short_name: String,
    },
}

impl TabState {
    #[cfg(test)]
    pub fn error(message: impl AsRef<str>) -> Self {
        Self::error_with_plan(message, None)
    }

    pub fn error_with_plan(message: impl AsRef<str>, plan: Option<String>) -> Self {
        let plan = plan.and_then(|plan| {
            let cleaned = crate::display::sanitize_untrusted_field(plan.trim());
            if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("unknown") {
                None
            } else {
                Some(cleaned)
            }
        });
        Self::Error {
            message: message.as_ref().to_string(),
            plan,
        }
    }
}

/// Identity of one provider entry. Usually a whole vendor; Claude and OpenRouter can
/// also name a configured account. `account: None` is a plain vendor tab or
/// that vendor's default account.
/// `desktop` marks an account whose usage comes from the Claude Desktop app's
/// own token rather than a `claude` CLI credential.
/// Custom providers never carry an account or a desktop flag.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TabId {
    pub source: TabSource,
    pub account: Option<String>,
    pub desktop: bool,
}

impl TabId {
    /// A plain vendor tab (default account for Anthropic).
    pub fn vendor(vendor: VendorId) -> Self {
        Self {
            source: TabSource::Builtin(vendor),
            account: None,
            desktop: false,
        }
    }

    /// A named Anthropic account tab (`[[anthropic.accounts]]` label).
    pub fn account(label: impl Into<String>) -> Self {
        Self::account_for(VendorId::Anthropic, label)
    }

    /// A named account for a vendor that supports account arrays.
    pub fn account_for(vendor: VendorId, label: impl Into<String>) -> Self {
        Self {
            source: TabSource::Builtin(vendor),
            account: Some(label.into()),
            desktop: false,
        }
    }

    /// An Anthropic account whose usage is read from the Claude Desktop app's
    /// own token store (a saved `~/.claude-acc/profiles/<label>` account).
    pub fn desktop_account(label: impl Into<String>) -> Self {
        Self {
            source: TabSource::Builtin(VendorId::Anthropic),
            account: Some(label.into()),
            desktop: true,
        }
    }

    /// A user-declared `[[custom]]` provider tab.
    pub fn custom(spec: &CustomProviderConfig) -> Self {
        Self {
            source: TabSource::Custom {
                id: spec.id.clone(),
                name: spec.name.clone(),
                short_name: spec.short_name.clone(),
            },
            account: None,
            desktop: false,
        }
    }

    /// The built-in vendor behind this tab; `None` for a custom provider.
    #[cfg(test)]
    pub fn vendor_id(&self) -> Option<VendorId> {
        match &self.source {
            TabSource::Builtin(vendor) => Some(*vendor),
            TabSource::Custom { .. } => None,
        }
    }
}

/// Expand enabled vendors into the tab list. Claude, OpenRouter, and Codex
/// (OpenAI) yield their default account followed by configured named accounts;
/// every other vendor is a single tab. With no extra accounts the result equals
/// `config.enabled_vendors()`, preserving the historical tab set and order.
///
/// Config-only and pure — no Desktop profiles. Production uses
/// [`tabs_with_desktop`]; this stays for the hermetic unit tests.
#[cfg(test)]
pub fn tabs_from_config(config: &Config) -> Vec<TabId> {
    build_tabs(config, &[], &|_| true)
}

/// The production aggregate-view tab list: configured accounts plus every saved
/// Claude Desktop profile that has usable credentials. Desktop discovery is
/// best-effort and macOS-only; anywhere else this equals [`tabs_from_config`].
pub fn tabs_with_desktop(config: &Config) -> Vec<TabId> {
    build_tabs(config, &desktop_profile_labels(config), &|label| {
        desktop_credential_usable(config, label)
    })
}

/// Core expansion, parameterized on the Desktop account labels so it stays pure
/// and unit-testable. Desktop accounts follow the CLI accounts and count toward
/// "Anthropic has accounts" for the default-tab suppression.
///
/// In aggregate views, a label present in both a `[[anthropic.accounts]]` CLI
/// entry and a Desktop profile gets exactly one source. The same account in two
/// stores means two of them refreshing one rotating refresh token — each
/// rotation invalidates the other's copy — and the CLI copy can even refresh to
/// a stale/wrong identity that still authenticates but reports another
/// account's (often zero) usage, which no credential-health check can catch.
/// The app-maintained Desktop token avoids both, so it wins when
/// `desktop_wins(label)` proves it usable; otherwise the configured CLI entry
/// stays and the Desktop profile gets no tab, so a broken Desktop snapshot
/// never hides a working account. `desktop_wins` is only consulted for
/// colliding labels.
fn build_tabs(
    config: &Config,
    desktop_labels: &[String],
    desktop_wins: &dyn Fn(&str) -> bool,
) -> Vec<TabId> {
    let mut tabs = Vec::new();
    for vendor in config.enabled_vendors() {
        if vendor == VendorId::Anthropic {
            let all_accounts = config.anthropic.all_accounts();
            let cli_labels: HashSet<&str> = all_accounts.iter().map(|a| a.label.as_str()).collect();
            let desktop_labels: Vec<&str> = desktop_labels
                .iter()
                .map(String::as_str)
                .filter(|label| !cli_labels.contains(label) || desktop_wins(label))
                .collect();
            let desktop_set: HashSet<&str> = desktop_labels.iter().copied().collect();
            let accounts: Vec<_> = all_accounts
                .iter()
                .filter(|a| !desktop_set.contains(a.label.as_str()))
                .map(|a| a.label.clone())
                .collect();
            // The default (unnamed) Claude tab is suppressible once every
            // account is named — but never when it would leave Anthropic with
            // no tab at all. Desktop accounts count as named accounts here.
            if config.anthropic.show_default_account
                || (accounts.is_empty() && desktop_labels.is_empty())
            {
                tabs.push(TabId::vendor(vendor));
            }
            for label in accounts {
                tabs.push(TabId::account(label));
            }
            for label in desktop_labels {
                tabs.push(TabId::desktop_account(label));
            }
        } else if vendor == VendorId::Openrouter {
            if config.openrouter.show_default_account || config.openrouter.accounts.is_empty() {
                tabs.push(TabId::vendor(vendor));
            }
            for account in &config.openrouter.accounts {
                tabs.push(TabId::account_for(vendor, account.label.clone()));
            }
        } else if vendor == VendorId::Openai {
            if config.openai.show_default_account || config.openai.accounts.is_empty() {
                tabs.push(TabId::vendor(vendor));
            }
            for account in &config.openai.accounts {
                tabs.push(TabId::account_for(vendor, account.label.clone()));
            }
        } else {
            tabs.push(TabId::vendor(vendor));
        }
    }
    // Custom providers follow every built-in vendor, in config order.
    for spec in config.enabled_custom() {
        tabs.push(TabId::custom(spec));
    }
    tabs
}

/// Labels of saved Claude Desktop profiles with usable credentials. macOS-only
/// (elsewhere there is no Desktop app); best-effort, so an unreadable profile
/// store just yields none rather than failing the whole tab list.
fn desktop_profile_labels(config: &Config) -> Vec<String> {
    let Ok(paths) = crate::claude_desktop::Paths::resolve(&config.anthropic) else {
        return Vec::new();
    };
    if !paths.available() {
        return Vec::new();
    }
    crate::claude_desktop::load_profiles(&paths.profiles_dir)
        .into_iter()
        .filter(|p| p.has_credentials)
        .map(|p| p.label)
        .collect()
}

/// Whether the Desktop credential behind `label` can stand in for a colliding
/// CLI entry. Resolves the same source the fetch would use (the live
/// `config.json` for the active account, the snapshot otherwise), then checks
/// it offline: no network, no refresh, no write-back. Any resolution failure
/// keeps the CLI entry.
fn desktop_credential_usable(config: &Config, label: &str) -> bool {
    match crate::anthropic::desktop_creds::account_target(config, label) {
        Ok((crate::anthropic::creds::CredsTarget::Desktop(source), _)) => {
            desktop_source_usable(&source, chrono::Utc::now().timestamp())
        }
        _ => false,
    }
}

/// Pure half of [`desktop_credential_usable`]: the blob decrypts to an
/// inference-scoped token that is unexpired or that the fetch may refresh.
/// A read-only source (the active account) cannot refresh, so an expired token
/// there cannot report usage. Limit: a present refresh token is not proof the
/// server still accepts it; proving that would mean refreshing here, rotating
/// the token this check exists to protect, so a revoked one still wins and
/// fails at fetch time.
fn desktop_source_usable(
    source: &crate::anthropic::desktop_creds::DesktopCreds,
    now_secs: i64,
) -> bool {
    source.read().is_ok_and(|(creds, _)| {
        let oauth = creds.claude_ai_oauth;
        crate::anthropic::oauth::can_refresh(&oauth.refresh_token)
            || oauth.expires_at_secs() > now_secs
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn config_with_accounts(labels: &[&str]) -> Config {
        let mut config = Config::default();
        // Keep only Anthropic enabled so the test asserts on account expansion,
        // not on the full default vendor set.
        config.openai.enabled = false;
        config.zai.enabled = false;
        config.openrouter.enabled = false;
        config.commandcode.enabled = false;
        config.anthropic.accounts = labels
            .iter()
            .map(|l| crate::config::AnthropicAccount {
                label: (*l).to_string(),
                credentials_path: format!("/creds/{l}.json").into(),
            })
            .collect();
        config
    }

    #[test]
    fn show_default_account_false_hides_the_unnamed_claude_tab() {
        // With named accounts and show_default_account=false, only the named
        // tabs appear — no redundant default "Claude" tab.
        let mut config = config_with_accounts(&["work", "personal"]);
        config.anthropic.show_default_account = false;
        assert_eq!(
            tabs_from_config(&config),
            vec![TabId::account("work"), TabId::account("personal")]
        );

        // But with no named accounts it is kept, so Anthropic never loses its
        // only tab.
        let mut empty = Config::default();
        empty.openai.enabled = false;
        empty.zai.enabled = false;
        empty.openrouter.enabled = false;
        empty.commandcode.enabled = false;
        empty.anthropic.show_default_account = false;
        assert_eq!(
            tabs_from_config(&empty),
            vec![TabId::vendor(VendorId::Anthropic)]
        );
    }

    #[test]
    fn tabs_expand_anthropic_accounts_after_default() {
        // Default Claude tab first, then each account in config order.
        let tabs = tabs_from_config(&config_with_accounts(&["work", "personal"]));
        assert_eq!(
            tabs,
            vec![
                TabId::vendor(VendorId::Anthropic),
                TabId::account("work"),
                TabId::account("personal"),
            ]
        );
    }

    #[test]
    fn tabs_without_accounts_are_just_enabled_vendors() {
        // No [[anthropic.accounts]] → one tab per enabled vendor, unchanged.
        let config = Config::default();
        let tabs = tabs_from_config(&config);
        let vendors: Vec<VendorId> = tabs.iter().filter_map(TabId::vendor_id).collect();
        assert_eq!(vendors, config.enabled_vendors());
        assert!(tabs.iter().all(|t| t.account.is_none()));
    }

    #[test]
    fn tabs_expand_openrouter_accounts_without_changing_other_vendors() {
        let mut config = Config::default();
        config.anthropic.enabled = false;
        config.openai.enabled = false;
        config.zai.enabled = false;
        config.commandcode.enabled = false;
        config.openrouter.accounts = vec![
            crate::config::OpenRouterAccount {
                label: "work".into(),
                api_key_env: Some("OPENROUTER_WORK_API_KEY".into()),
                api_key: None,
            },
            crate::config::OpenRouterAccount {
                label: "personal".into(),
                api_key_env: None,
                api_key: Some("personal-key".into()),
            },
        ];
        assert_eq!(
            tabs_from_config(&config),
            vec![
                TabId::vendor(VendorId::Openrouter),
                TabId::account_for(VendorId::Openrouter, "work"),
                TabId::account_for(VendorId::Openrouter, "personal"),
            ]
        );
    }

    #[test]
    fn openai_named_accounts_get_their_own_tabs_after_the_default() {
        let mut config = Config::default();
        config.anthropic.enabled = false;
        config.zai.enabled = false;
        config.openrouter.enabled = false;
        config.commandcode.enabled = false;
        config.openai.accounts.push(crate::config::OpenAiAccount {
            label: "work".into(),
            codex_auth_path: "/tmp/codex-work/auth.json".into(),
        });
        assert_eq!(
            tabs_from_config(&config),
            vec![
                TabId::vendor(VendorId::Openai),
                TabId::account_for(VendorId::Openai, "work"),
            ]
        );
    }

    #[test]
    fn openrouter_can_hide_default_only_when_named_accounts_exist() {
        let mut config = Config::default();
        config.anthropic.enabled = false;
        config.openai.enabled = false;
        config.zai.enabled = false;
        config.commandcode.enabled = false;
        config.openrouter.show_default_account = false;
        assert_eq!(
            tabs_from_config(&config),
            vec![TabId::vendor(VendorId::Openrouter)]
        );

        config
            .openrouter
            .accounts
            .push(crate::config::OpenRouterAccount {
                label: "work".into(),
                api_key_env: Some("OPENROUTER_WORK_API_KEY".into()),
                api_key: None,
            });
        assert_eq!(
            tabs_from_config(&config),
            vec![TabId::account_for(VendorId::Openrouter, "work")]
        );
    }

    #[test]
    fn codex_can_hide_default_only_when_named_accounts_exist() {
        let mut config = Config::default();
        config.anthropic.enabled = false;
        config.zai.enabled = false;
        config.openrouter.enabled = false;
        config.commandcode.enabled = false;
        config.openai.show_default_account = false;
        assert_eq!(
            tabs_from_config(&config),
            vec![TabId::vendor(VendorId::Openai)]
        );

        config.openai.accounts.push(crate::config::OpenAiAccount {
            label: "work".into(),
            codex_auth_path: "/tmp/codex-work/auth.json".into(),
        });
        assert_eq!(
            tabs_from_config(&config),
            vec![TabId::account_for(VendorId::Openai, "work")]
        );
    }

    #[test]
    fn tabs_include_accounts_auto_discovered_from_accounts_dir() {
        // A CLAUDE_CONFIG_DIR-style directory becomes account tabs with no
        // explicit [[anthropic.accounts]] entry. Hermetic: real TempDir.
        let td = tempfile::tempdir().unwrap();
        for label in ["work", "personal"] {
            let dir = td.path().join(label);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(".credentials.json"), "{}").unwrap();
        }
        let mut config = Config::default();
        config.openai.enabled = false;
        config.zai.enabled = false;
        config.openrouter.enabled = false;
        config.commandcode.enabled = false;
        config.anthropic.accounts_dir = Some(td.path().to_path_buf());

        let tabs = tabs_from_config(&config);
        assert_eq!(
            tabs,
            vec![
                TabId::vendor(VendorId::Anthropic),
                TabId::account("personal"), // sorted by label
                TabId::account("work"),
            ]
        );
    }

    #[test]
    fn desktop_labels_become_account_tabs_after_cli_accounts() {
        // Pure core: desktop accounts follow CLI accounts, in the order given.
        let config = config_with_accounts(&["work"]);
        let tabs = build_tabs(&config, &["gmail".into(), "hotmail".into()], &|_| true);
        assert_eq!(
            tabs,
            vec![
                TabId::vendor(VendorId::Anthropic),
                TabId::account("work"),
                TabId::desktop_account("gmail"),
                TabId::desktop_account("hotmail"),
            ]
        );
    }

    #[test]
    fn a_desktop_profile_wins_a_label_collision_with_a_cli_account() {
        // One tab per label; a Desktop source proven usable wins so the label is
        // never fed from two stores refreshing one rotating token (which
        // invalidate each other and can silently show a wrong account's usage).
        // The CLI entry is dropped; a CLI-only label (work) is untouched.
        let config = config_with_accounts(&["gmail", "work"]);
        let tabs = build_tabs(&config, &["gmail".into(), "hotmail".into()], &|_| true);
        assert_eq!(
            tabs,
            vec![
                TabId::vendor(VendorId::Anthropic),
                TabId::account("work"),
                TabId::desktop_account("gmail"),
                TabId::desktop_account("hotmail"),
            ]
        );
    }

    #[test]
    fn an_unusable_desktop_profile_keeps_the_colliding_cli_account() {
        // Two snapshot files do not prove a working token. When the Desktop
        // credential for a colliding label is unusable, the configured CLI
        // account keeps its tab and the Desktop profile gets none, so the label
        // still has exactly one source. Only the collision is probed.
        let config = config_with_accounts(&["gmail", "work"]);
        let probed = std::cell::RefCell::new(Vec::new());
        let tabs = build_tabs(&config, &["gmail".into(), "hotmail".into()], &|label| {
            probed.borrow_mut().push(label.to_string());
            false
        });
        assert_eq!(
            tabs,
            vec![
                TabId::vendor(VendorId::Anthropic),
                TabId::account("gmail"),
                TabId::account("work"),
                TabId::desktop_account("hotmail"),
            ]
        );
        assert_eq!(probed.into_inner(), vec!["gmail".to_string()]);
    }

    #[test]
    fn desktop_only_labels_are_never_probed() {
        let config = config_with_accounts(&["work"]);
        let tabs = build_tabs(&config, &["gmail".into()], &|label| {
            panic!("probed non-colliding label {label}")
        });
        assert_eq!(
            tabs,
            vec![
                TabId::vendor(VendorId::Anthropic),
                TabId::account("work"),
                TabId::desktop_account("gmail"),
            ]
        );
    }

    mod desktop_probe {
        use super::super::desktop_source_usable;
        use crate::anthropic::desktop_creds::source_for;
        use crate::safe_storage;

        const NOW: i64 = 1_800_000_000;

        fn key() -> [u8; 16] {
            safe_storage::derive_key(b"test-secret")
        }

        fn inference_entry(refresh: &str, expires_ms: Option<i64>) -> serde_json::Value {
            let mut entry = serde_json::json!({
                "token": "access-token-fixture",
                "refreshToken": refresh,
                "subscriptionType": "max",
                "rateLimitTier": "default_claude_max_20x",
            });
            if let Some(ms) = expires_ms {
                entry["expiresAt"] = ms.into();
            }
            serde_json::json!({ "client:org:https://api.anthropic.com:user:inference user:profile": entry })
        }

        /// A temp profile snapshot plus a live `config.json`, both holding
        /// `plain` encrypted under `k`.
        fn fixture(plain: &serde_json::Value, k: &[u8; 16]) -> tempfile::TempDir {
            let root = tempfile::tempdir().unwrap();
            let blob = safe_storage::encrypt(k, &serde_json::to_vec(plain).unwrap());
            let profile = root.path().join("profile");
            std::fs::create_dir_all(&profile).unwrap();
            std::fs::write(profile.join("config-tokenCacheV2"), &blob).unwrap();
            std::fs::write(profile.join("config-tokenCache"), &blob).unwrap();
            let config_json = serde_json::json!({ "oauth:tokenCacheV2": blob });
            std::fs::write(
                root.path().join("config.json"),
                serde_json::to_vec(&config_json).unwrap(),
            )
            .unwrap();
            root
        }

        fn usable(root: &tempfile::TempDir, is_active: bool, k: [u8; 16]) -> bool {
            let source = source_for(
                &root.path().join("config.json"),
                &root.path().join("profile"),
                is_active,
                k,
            );
            desktop_source_usable(&source, NOW)
        }

        const FUTURE_MS: i64 = (NOW + 3600) * 1000;
        const PAST_MS: i64 = (NOW - 3600) * 1000;

        #[test]
        fn a_snapshot_that_does_not_decrypt_is_unusable() {
            let root = fixture(&inference_entry("refresh", Some(FUTURE_MS)), &key());
            let wrong = safe_storage::derive_key(b"other-secret");
            assert!(!usable(&root, false, wrong));
            assert!(!usable(&root, true, wrong));
        }

        #[test]
        fn a_cache_without_an_inference_token_is_unusable() {
            let plain = serde_json::json!({
                "client:org:https://api.anthropic.com:user:profile": {
                    "token": "profile-only", "refreshToken": "r", "expiresAt": FUTURE_MS,
                }
            });
            let root = fixture(&plain, &key());
            assert!(!usable(&root, false, key()));
        }

        #[test]
        fn an_expired_read_only_active_token_is_unusable() {
            // The active account is read-only (refresh blanked), so an expired
            // token cannot recover; a missing expiry counts as expired.
            let root = fixture(&inference_entry("refresh", Some(PAST_MS)), &key());
            assert!(!usable(&root, true, key()));
            let root = fixture(&inference_entry("", None), &key());
            assert!(!usable(&root, false, key()));
        }

        #[test]
        fn a_live_or_refreshable_token_is_usable() {
            let root = fixture(&inference_entry("", Some(FUTURE_MS)), &key());
            assert!(usable(&root, true, key()));
            // An inactive snapshot may refresh an expired token at fetch time.
            let root = fixture(&inference_entry("refresh", Some(PAST_MS)), &key());
            assert!(usable(&root, false, key()));
        }
    }

    #[test]
    fn desktop_accounts_suppress_the_default_tab_like_named_ones() {
        // show_default_account=false + only Desktop accounts => no default tab,
        // exactly as if they were [[anthropic.accounts]] (the Desktop-only user).
        let mut config = config_with_accounts(&[]);
        config.cursor.enabled = false;
        config.anthropic.show_default_account = false;

        // No accounts of either kind: the default tab survives (never leave
        // Anthropic tab-less).
        assert_eq!(
            build_tabs(&config, &[], &|_| true),
            vec![TabId::vendor(VendorId::Anthropic)]
        );
        // A Desktop account is present: default suppressed, only the account.
        assert_eq!(
            build_tabs(&config, &["gmail".into()], &|_| true),
            vec![TabId::desktop_account("gmail")]
        );
    }

    pub(crate) fn custom_spec(id: &str, enabled: bool) -> CustomProviderConfig {
        CustomProviderConfig {
            id: id.into(),
            name: "My Tool".into(),
            short_name: "myt".into(),
            enabled,
            ..Default::default()
        }
    }

    #[test]
    fn custom_providers_get_tabs_after_every_builtin() {
        let config = Config {
            custom: vec![custom_spec("mytool", true), custom_spec("other", true)],
            ..Default::default()
        };
        let tabs = tabs_from_config(&config);
        let builtin_count = config.enabled_vendors().len();
        assert_eq!(tabs.len(), builtin_count + 2);
        assert!(
            tabs[..builtin_count]
                .iter()
                .all(|t| matches!(t.source, TabSource::Builtin(_)))
        );
        assert_eq!(tabs[builtin_count], TabId::custom(&config.custom[0]));
        assert_eq!(tabs[builtin_count + 1], TabId::custom(&config.custom[1]));
        assert_eq!(
            tabs[builtin_count].source,
            TabSource::Custom {
                id: "mytool".into(),
                name: "My Tool".into(),
                short_name: "myt".into(),
            }
        );
        assert!(tabs[builtin_count].account.is_none());
        assert!(!tabs[builtin_count].desktop);
        assert_eq!(tabs[builtin_count].vendor_id(), None);
    }

    #[test]
    fn disabled_custom_provider_has_no_tab() {
        let config = Config {
            custom: vec![custom_spec("mytool", false)],
            ..Default::default()
        };
        let tabs = tabs_from_config(&config);
        assert!(
            tabs.iter()
                .all(|t| matches!(t.source, TabSource::Builtin(_)))
        );
        assert_eq!(tabs.len(), config.enabled_vendors().len());
    }
}
