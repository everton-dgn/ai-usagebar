//! Fetching one provider entry for the macOS app.

use chrono::Utc;
use reqwest::Client;

use crate::cache::DEFAULT_TTL;
use crate::config::Config;
use crate::core::entries::{ReadyTab, TabId, TabSource, TabState};
use crate::error::{AppError, Result};
use crate::vendor::{VendorId, VendorOutcome};

/// Fetch and render one tab — returns a `TabState`.
pub async fn refresh_one(client: &Client, config: &Config, tab: &TabId) -> TabState {
    match build_outcome(client, config, tab).await {
        Ok(outcome) => {
            // Resolve the cache age (a duration from "now" at fetch time) into an
            // absolute instant ONCE. Without this, sections_for would recompute
            // `Utc::now() - cache_age` on every draw and the displayed time would
            // tick upward in real time instead of holding at the last refresh.
            let now = Utc::now();
            let off_the_wire = outcome.off_the_wire();
            let fetched_at = outcome
                .cache_age
                .map(|age| now - chrono::Duration::from_std(age).unwrap_or_default());
            let state = TabState::Ready(Box::new(ReadyTab {
                snapshot: outcome.snapshot,
                email: outcome.email,
                stale: outcome.stale,
                last_error: outcome.last_error.map(|(code, message)| {
                    (code, crate::display::sanitize_untrusted_field(&message))
                }),
                fetched_at,
                display: match &tab.source {
                    TabSource::Builtin(vendor) => config.display_prefs(*vendor),
                    // A `[[custom]]` provider states its own percentages; it has
                    // no balance to meter and no headline to choose.
                    TabSource::Custom { .. } => crate::balance::DisplayPrefs::default(),
                },
            }));
            // Every provider refresh reaches this notification hook.
            // Wire-fresh outcomes only — never cached, stale, or failed ones —
            // and best-effort by construction: `run` returns nothing and cannot
            // change this function's result or the caller's exit code.
            if off_the_wire
                && config.notifications.enabled
                && let Some(input) = crate::notify::RefreshInput::from_tab(tab, &state, now)
            {
                let _ = crate::notify::run(input, config.notifications.threshold).await;
            }
            state
        }
        Err(e) => TabState::error_with_plan(
            crate::display::sanitize_untrusted_field(&e.user_message()),
            e.plan().map(str::to_string),
        ),
    }
}

async fn build_outcome(client: &Client, config: &Config, tab: &TabId) -> Result<VendorOutcome> {
    let vendor = match &tab.source {
        TabSource::Builtin(vendor) => *vendor,
        TabSource::Custom { id, .. } => {
            // A custom provider owns its TTL and its cache slot; the cache is
            // keyed by the provider id under one shared `custom` vendor dir.
            let spec = config.custom_by_id(id).ok_or_else(|| {
                AppError::Other(format!("custom provider {id} is not configured"))
            })?;
            let api_key = spec.resolve_api_key()?;
            let cache = crate::cache::Cache::for_vendor_account("custom", id)?;
            let outcome =
                crate::custom::fetch_snapshot(client, spec, &api_key, &cache, spec.cache_ttl())
                    .await?;
            return Ok(outcome.into());
        }
    };
    match vendor {
        VendorId::Anthropic => {
            // A named account resolves to its own file + `anthropic/<label>`
            // cache through `account_target`.
            // The default tab keeps the pre-existing resolution: config
            // `credentials_path` is an explicit strict read, and only the
            // platform default gets the macOS Keychain fallback.
            let (creds_target, cache) = match tab.account.as_deref() {
                Some(label) if tab.desktop => {
                    crate::anthropic::desktop_creds::account_target(config, label)?
                }
                Some(label) => config.anthropic.account_target(label)?,
                None => {
                    let target = match config.anthropic.credentials_path.clone() {
                        Some(p) => crate::anthropic::creds::CredsTarget::Explicit(p),
                        None => crate::anthropic::creds::CredsTarget::Default(
                            crate::anthropic::creds::default_path().unwrap_or_default(),
                        ),
                    };
                    (target, crate::cache::Cache::for_vendor("anthropic")?)
                }
            };
            let endpoints = crate::anthropic::fetch::Endpoints::default();
            let outcome = crate::anthropic::fetch_snapshot(
                client,
                &creds_target,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.map(crate::usage::VendorSnapshot::Anthropic))
        }
        VendorId::AnthropicApi => {
            let key = crate::config::resolve_api_key(
                "Anthropic_API",
                &config.anthropic_api.api_key_env,
                config.anthropic_api.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("anthropic_api")?;
            let endpoints = crate::anthropic_api::fetch::Endpoints::default();
            let outcome = crate::anthropic_api::fetch_snapshot(
                client,
                &key,
                &cache,
                &endpoints,
                DEFAULT_TTL,
                config.anthropic_api.monthly_limit,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Openrouter => {
            let (api_key, cache) = openrouter_target(config, tab.account.as_deref())?;
            let endpoints = crate::openrouter::fetch::Endpoints::default();
            let outcome = crate::openrouter::fetch_snapshot(
                client,
                &api_key,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Zai => {
            let api_key = crate::config::resolve_api_key(
                "Zai",
                &config.zai.api_key_env,
                config.zai.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("zai")?;
            let endpoints = crate::zai::fetch::Endpoints::default();
            let outcome = crate::zai::fetch_snapshot(
                client,
                &api_key,
                &cache,
                &endpoints,
                DEFAULT_TTL,
                config.zai.plan_tier.as_deref(),
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Openai => {
            let label = tab.account.as_deref();
            let cache = match label {
                Some(label) => crate::cache::Cache::for_vendor_account("openai", label)?,
                None => crate::cache::Cache::for_vendor("openai")?,
            };
            let route = || config.openai.fetch_auth_path(label);
            let endpoints = crate::openai::fetch::Endpoints::default();
            let outcome = crate::openai::fetch_snapshot_routed(
                client,
                route,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Copilot => {
            let token = config.copilot.resolve_token()?;
            let cache = crate::cache::Cache::for_vendor("copilot")?;
            let endpoints = crate::copilot::fetch::Endpoints::default();
            let outcome =
                crate::copilot::fetch_snapshot(client, &token, &cache, &endpoints, DEFAULT_TTL)
                    .await?;
            Ok(outcome.into())
        }
        VendorId::Deepseek => {
            let api_key = crate::config::resolve_api_key(
                "DeepSeek",
                &config.deepseek.api_key_env,
                config.deepseek.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("deepseek")?;
            let endpoints = crate::deepseek::fetch::Endpoints::default();
            let outcome =
                crate::deepseek::fetch_snapshot(client, &api_key, &cache, &endpoints, DEFAULT_TTL)
                    .await?;
            Ok(outcome.into())
        }
        VendorId::Kimi => {
            let (auth, endpoints) = crate::kimi::resolve_auth(&config.kimi)?;
            let cache = crate::cache::Cache::for_vendor("kimi")?;
            let outcome = crate::kimi::fetch::fetch_snapshot_with_auth(
                client,
                &auth,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Kilo => {
            let api_key = crate::config::resolve_api_key(
                "Kilo",
                &config.kilo.api_key_env,
                config.kilo.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("kilo")?;
            let endpoints = crate::kilo::fetch::Endpoints::default();
            let outcome = crate::kilo::fetch_snapshot(
                client,
                &api_key,
                &cache,
                &endpoints,
                DEFAULT_TTL,
                config.kilo.organization_id.as_deref(),
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Novita => {
            let api_key = crate::config::resolve_api_key(
                "Novita",
                &config.novita.api_key_env,
                config.novita.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("novita")?;
            let endpoints = crate::novita::fetch::Endpoints::default();
            let outcome =
                crate::novita::fetch_snapshot(client, &api_key, &cache, &endpoints, DEFAULT_TTL)
                    .await?;
            Ok(outcome.into())
        }
        VendorId::Moonshot => {
            let api_key = crate::config::resolve_api_key(
                "Moonshot",
                &config.moonshot.api_key_env,
                config.moonshot.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("moonshot")?;
            let (endpoints, currency) =
                crate::moonshot::fetch::Endpoints::for_region(&config.moonshot.region);
            let outcome = crate::moonshot::fetch_snapshot(
                client,
                &api_key,
                &cache,
                &endpoints,
                DEFAULT_TTL,
                currency,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Grok => {
            let key = crate::config::resolve_api_key(
                "Grok",
                &config.grok.api_key_env,
                config.grok.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("grok")?;
            let endpoints = crate::grok::fetch::Endpoints::default();
            let outcome = crate::grok::fetch_snapshot(
                client,
                &key,
                &cache,
                &endpoints,
                DEFAULT_TTL,
                config.grok.team_id.as_deref(),
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Supergrok => {
            let cache = crate::cache::Cache::for_vendor("supergrok")?;
            let scope_paths = crate::supergrok::scope::ScopePaths::with_overrides(
                config.supergrok.auth_path.as_deref(),
                config.supergrok.config_path.as_deref(),
            )?;
            let outcome = crate::supergrok::fetch_snapshot(
                &config.supergrok.grok_binary,
                &scope_paths,
                &cache,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Antigravity => {
            // No API key: the local Antigravity server is the source, and the
            // saved Google session stands in while nothing is running.
            let cache = crate::cache::Cache::for_vendor("antigravity")?;
            let oauth = crate::antigravity::cloud::OauthClient::from_config(
                config.antigravity.oauth_client_id.as_deref(),
                config.antigravity.oauth_client_secret.as_deref(),
            );
            let outcome =
                crate::antigravity::fetch_snapshot(client, &cache, DEFAULT_TTL, oauth.as_ref())
                    .await?;
            Ok(outcome.into())
        }
        VendorId::Grokbot => {
            // The desktop app's own session is the login; refreshed pairs
            // persist only inside the vendor cache.
            let creds = crate::grokbot::resolve_credentials(&config.grokbot)?;
            let cache = crate::cache::Cache::for_vendor("grokbot")?;
            let endpoints = crate::grokbot::fetch::Endpoints::default();
            let outcome = crate::grokbot::fetch::fetch_snapshot_with(
                client,
                &creds,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::ModelStudio => {
            // The bl CLI's own console session is the login; its region/site
            // pair picks the gateway, and only a token fingerprint persists.
            let creds = crate::modelstudio::resolve_credentials(&config.modelstudio)?;
            let cache = crate::cache::Cache::for_vendor("modelstudio")?;
            let endpoints =
                crate::modelstudio::fetch::Endpoints::for_gateway(creds.region, creds.site);
            let outcome = crate::modelstudio::fetch_snapshot_with(
                client,
                &creds,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Minimax => {
            let api_key = crate::config::resolve_api_key(
                "MiniMax",
                &config.minimax.api_key_env,
                config.minimax.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("minimax")?;
            let endpoints = crate::minimax::fetch::Endpoints::for_region(&config.minimax.region);
            let outcome =
                crate::minimax::fetch_snapshot(client, &api_key, &cache, &endpoints, DEFAULT_TTL)
                    .await?;
            Ok(outcome.into())
        }
        VendorId::Cursor => {
            let cache = crate::cache::Cache::for_vendor("cursor")?;
            let db_path = config
                .cursor
                .db_path
                .clone()
                .map(Ok)
                .unwrap_or_else(crate::cursor::db::default_db_path)?;
            let agent_auth_path = config
                .cursor
                .agent_auth_path
                .clone()
                .map(Ok)
                .unwrap_or_else(crate::cursor::db::default_agent_auth_path)?;
            let endpoints = crate::cursor::fetch::Endpoints::default();
            let outcome = crate::cursor::fetch_snapshot(
                client,
                &db_path,
                &agent_auth_path,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Kiro => {
            let cache = crate::cache::Cache::for_vendor("kiro")?;
            let db_path = config
                .kiro
                .db_path
                .clone()
                .map(Ok)
                .unwrap_or_else(crate::kiro::db::default_db_path)?;
            let outcome =
                crate::kiro::fetch_snapshot(client, &db_path, &cache, DEFAULT_TTL).await?;
            Ok(outcome.into())
        }
        VendorId::NousResearch => {
            let store = crate::nous::credentials::CredentialStore::default();
            let endpoints = crate::nous::fetch::Endpoints::default();
            let account = crate::nous::fetch::fetch_account_with_refresh(
                client,
                &store,
                &endpoints,
                Utc::now(),
            )
            .await?;
            // Nous keeps no cache of its own, so every read is a live one.
            let email = account.email.clone();
            Ok(
                crate::outcome::Outcome::fresh(crate::usage::VendorSnapshot::NousResearch(account))
                    .with_email(email),
            )
        }
        VendorId::OpenCodeGo => {
            let api_key = crate::config::resolve_api_key(
                "OpenCode Go",
                &config.opencode_go.api_key_env,
                config.opencode_go.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("opencode-go")?;
            let endpoints = crate::opencode_go::fetch::Endpoints::default();
            let outcome = crate::opencode_go::fetch::fetch_snapshot(
                client,
                &api_key,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::CommandCode => {
            let credential =
                crate::commandcode::creds::resolve(config.commandcode.auth_paths.as_deref())?;
            let cache = crate::cache::Cache::for_vendor("commandcode")?;
            let endpoints = crate::commandcode::fetch::Endpoints::default();
            let outcome = crate::commandcode::fetch::fetch_snapshot(
                client,
                &credential.token,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::Ollama => {
            let api_key = crate::config::resolve_api_key(
                "Ollama",
                &config.ollama.api_key_env,
                config.ollama.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("ollama")?;
            let endpoints = crate::ollama::fetch::Endpoints::default();
            let outcome = crate::ollama::fetch_snapshot(
                client,
                &api_key,
                &config.ollama.plan,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
        VendorId::OrcaRouter => {
            let api_key = crate::config::resolve_api_key(
                "OrcaRouter",
                &config.orcarouter.api_key_env,
                config.orcarouter.api_key.as_deref(),
            )?;
            let cache = crate::cache::Cache::for_vendor("orcarouter")?;
            let endpoints = crate::orcarouter::fetch::Endpoints::default();
            let outcome = crate::orcarouter::fetch_snapshot(
                client,
                &api_key,
                &cache,
                &endpoints,
                DEFAULT_TTL,
            )
            .await?;
            Ok(outcome.into())
        }
    }
}

/// Keep credential and cache selection tied to the same account identity.
fn openrouter_target(
    config: &Config,
    label: Option<&str>,
) -> Result<(String, crate::cache::Cache)> {
    let api_key = config.openrouter.resolve_api_key(label)?;
    let cache = match label {
        Some(label) => crate::cache::Cache::for_vendor_account("openrouter", label)?,
        None => crate::cache::Cache::for_vendor("openrouter")?,
    };
    Ok((api_key, cache))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        let mut config = Config::default();
        config.openrouter.api_key_env.clear();
        config.openrouter.api_key = Some("fixture-default".into());
        config.openrouter.accounts = ["work", "personal"]
            .into_iter()
            .map(|label| crate::config::OpenRouterAccount {
                label: label.into(),
                api_key_env: None,
                api_key: Some(format!("fixture-{label}")),
            })
            .collect();
        config
    }

    #[test]
    fn named_accounts_keep_their_own_key_and_cache() {
        let config = config();
        let (work_key, work_cache) = openrouter_target(&config, Some("work")).unwrap();
        let (personal_key, personal_cache) = openrouter_target(&config, Some("personal")).unwrap();
        assert_eq!(work_key, "fixture-work");
        assert_eq!(personal_key, "fixture-personal");
        assert!(work_cache.dir().ends_with("openrouter/work"));
        assert!(personal_cache.dir().ends_with("openrouter/personal"));
        assert_ne!(work_cache.dir(), personal_cache.dir());
    }

    #[test]
    fn default_account_keeps_the_original_cache_slot() {
        let (key, cache) = openrouter_target(&config(), None).unwrap();
        assert_eq!(key, "fixture-default");
        assert!(cache.dir().ends_with("ai-usagebar/openrouter"));
    }

    #[test]
    fn unknown_account_never_falls_back_to_the_default_key() {
        assert!(openrouter_target(&config(), Some("missing")).is_err());
    }
}
