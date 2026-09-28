//! GitHub Copilot quota fetch with cache isolation and no credential storage.
//!
//! The account email is looked up on every call with the same token that
//! reads the quota, best effort and never written to disk: a failed or slow
//! profile request leaves `email` empty and the quota result untouched.

use std::fmt::Write as _;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::cache::{Cache, acquire_lock_async};
use crate::error::{AppError, Result};
use crate::vendor::{MAX_BODY_BYTES, read_body_capped};

use super::types::{Response, Snapshot, to_snapshot};

pub const USER_URL: &str = "https://api.github.com/copilot_internal/user";
pub const PROFILE_URL: &str = "https://api.github.com/user";
pub const EMAILS_URL: &str = "https://api.github.com/user/emails";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
const SCHEMA_ERROR: &str = "GitHub Copilot quota response schema mismatch";

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub user: String,
    pub profile: String,
    pub emails: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            user: USER_URL.to_string(),
            profile: PROFILE_URL.to_string(),
            emails: EMAILS_URL.to_string(),
        }
    }
}

pub type FetchOutcome = crate::outcome::Outcome<Snapshot>;

pub async fn fetch_snapshot(
    client: &reqwest::Client,
    token: &str,
    cache: &Cache,
    endpoints: &Endpoints,
    ttl: Duration,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;
    let target = target_key(endpoints, token);
    if let Some(bytes) = cache.fresh_payload(ttl)?
        && let Ok(snapshot) = parse_cache(&bytes, &target)
    {
        let email = fetch_email(client, token, endpoints).await;
        return Ok(crate::outcome::Outcome::cached(snapshot, cache, false).with_email(email));
    }
    let (live, email) = tokio::join!(
        fetch_live(client, token, endpoints),
        fetch_email(client, token, endpoints)
    );
    settle(cache, &target, live).map(|outcome| outcome.with_email(email))
}

fn settle(cache: &Cache, target: &str, live: Result<Snapshot>) -> Result<FetchOutcome> {
    match live {
        Ok(snapshot) => {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "target": target,
                "snapshot": snapshot,
            }))?;
            cache.write_payload(&bytes)?;
            Ok(crate::outcome::Outcome::fresh(snapshot))
        }
        Err(error @ AppError::Transport(_)) => fallback_or_error(cache, None, target, error),
        Err(AppError::Http { status, .. }) => {
            let message = status_message(status).to_string();
            cache.mark_stale();
            let diagnostic = cache.write_last_error(status, &message);
            fallback_or_error(
                cache,
                Some(diagnostic),
                target,
                AppError::Http {
                    status,
                    body: message,
                },
            )
        }
        Err(AppError::Schema(_)) => {
            cache.mark_stale();
            let diagnostic = cache.write_last_error(0, SCHEMA_ERROR);
            fallback_or_error(
                cache,
                Some(diagnostic),
                target,
                AppError::Schema(SCHEMA_ERROR.to_string()),
            )
        }
        Err(error) => fallback_or_error(cache, None, target, error),
    }
}

#[derive(serde::Deserialize)]
struct Profile {
    email: Option<String>,
}

#[derive(serde::Deserialize)]
struct ListedEmail {
    email: String,
    primary: bool,
    verified: bool,
}

/// The account email, or `None` when it is private and the token was not
/// already granted access to the address list, or when anything fails.
async fn fetch_email(
    client: &reqwest::Client,
    token: &str,
    endpoints: &Endpoints,
) -> Option<crate::identity::AccountEmail> {
    tokio::time::timeout(PROFILE_TIMEOUT, discover_email(client, token, endpoints))
        .await
        .ok()
        .flatten()
}

async fn discover_email(
    client: &reqwest::Client,
    token: &str,
    endpoints: &Endpoints,
) -> Option<crate::identity::AccountEmail> {
    let (scopes, bytes) = github_get(client, token, &endpoints.profile).await?;
    let profile: Profile = serde_json::from_slice(&bytes).ok()?;
    if let Some(email) = profile
        .email
        .as_deref()
        .and_then(crate::identity::AccountEmail::parse)
    {
        return Some(email);
    }
    if !grants_email_list(scopes.as_deref()) {
        return None;
    }
    let (_, bytes) = github_get(client, token, &endpoints.emails).await?;
    let listed: Vec<ListedEmail> = serde_json::from_slice(&bytes).ok()?;
    listed
        .iter()
        .find(|entry| entry.primary && entry.verified)
        .and_then(|entry| crate::identity::AccountEmail::parse(&entry.email))
}

/// A successful GitHub REST response: its `X-OAuth-Scopes` header and a
/// size-capped body. Bodies are parsed, never logged.
async fn github_get(
    client: &reqwest::Client,
    token: &str,
    url: &str,
) -> Option<(Option<String>, Vec<u8>)> {
    let response = client
        .get(url)
        .header(reqwest::header::AUTHORIZATION, format!("token {token}"))
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header(reqwest::header::USER_AGENT, "ai-usagebar")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let scopes = response
        .headers()
        .get("x-oauth-scopes")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = read_body_capped(response, PROFILE_BODY_BYTES).await.ok()?;
    Some((scopes, bytes))
}

/// `GET /user/emails` needs `user:email` (or the broader `user`) on classic
/// OAuth tokens. Tokens that report no scopes are not probed.
fn grants_email_list(scopes: Option<&str>) -> bool {
    scopes.is_some_and(|scopes| {
        scopes
            .split(',')
            .map(str::trim)
            .any(|scope| scope == "user" || scope == "user:email")
    })
}

async fn fetch_live(
    client: &reqwest::Client,
    token: &str,
    endpoints: &Endpoints,
) -> Result<Snapshot> {
    let response = tokio::time::timeout(
        HTTP_TIMEOUT,
        client
            .get(&endpoints.user)
            .header(reqwest::header::AUTHORIZATION, format!("token {token}"))
            .header(reqwest::header::ACCEPT, "application/json")
            .header("Editor-Version", "vscode/1.96.2")
            .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
            .header(reqwest::header::USER_AGENT, "GitHubCopilotChat/0.26.7")
            .header("X-GitHub-Api-Version", "2025-04-01")
            .send(),
    )
    .await
    .map_err(|_| AppError::Transport("GitHub Copilot request timed out".into()))??;
    let status = response.status();
    let bytes = read_body_capped(response, MAX_BODY_BYTES).await?;
    if !status.is_success() {
        return Err(AppError::Http {
            status: status.as_u16(),
            body: status_message(status.as_u16()).into(),
        });
    }
    let response: Response =
        serde_json::from_slice(&bytes).map_err(|_| AppError::Schema(SCHEMA_ERROR.to_string()))?;
    to_snapshot(response)
}

/// Bind cache reuse to both endpoint and token without writing either raw
/// credential or the full response to disk.
fn target_key(endpoints: &Endpoints, token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut fingerprint = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(fingerprint, "{byte:02x}");
    }
    format!("{}|token:{fingerprint}", endpoints.user)
}

fn parse_cache(bytes: &[u8], target: &str) -> Result<Snapshot> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| AppError::Schema("GitHub Copilot cache is invalid".into()))?;
    if value.get("target").and_then(serde_json::Value::as_str) != Some(target) {
        return Err(AppError::Schema(
            "GitHub Copilot cache belongs to a different account".into(),
        ));
    }
    serde_json::from_value(
        value.get("snapshot").cloned().ok_or_else(|| {
            AppError::Schema("GitHub Copilot cache is missing its snapshot".into())
        })?,
    )
    .map_err(|_| AppError::Schema("GitHub Copilot cache has an invalid snapshot".into()))
}

const PROFILE_TIMEOUT: Duration = Duration::from_secs(3);
const PROFILE_BODY_BYTES: usize = 64 * 1024;

fn fallback_or_error(
    cache: &Cache,
    diagnostic: Option<(u16, String)>,
    target: &str,
    error: AppError,
) -> Result<FetchOutcome> {
    crate::outcome::fallback(cache, diagnostic, error, |bytes| parse_cache(bytes, target))
}

fn status_message(status: u16) -> &'static str {
    match status {
        401 | 403 => crate::error::AUTH_FAILURE_MESSAGE,
        429 => "GitHub Copilot rate limited the quota request",
        500..=599 => "GitHub Copilot quota endpoint is unavailable",
        _ => "GitHub Copilot quota request failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copilot::types::Quota;

    fn cache_in(dir: &std::path::Path) -> Cache {
        Cache::at(dir.join("copilot"))
    }

    fn endpoints_for(server: &mockito::Server) -> Endpoints {
        Endpoints {
            user: format!("{}/copilot_internal/user", server.url()),
            profile: format!("{}/user", server.url()),
            emails: format!("{}/user/emails", server.url()),
        }
    }

    const QUOTA_BODY: &str = r#"{"copilot_plan":"pro","quota_reset_date":"2026-09-15","quota_snapshots":{"premium_interactions":{"entitlement":300,"remaining":45,"percent_remaining":15}}}"#;

    async fn mock_quota(server: &mut mockito::Server, token: &str) -> mockito::Mock {
        server
            .mock("GET", "/copilot_internal/user")
            .match_header("authorization", format!("token {token}").as_str())
            .with_status(200)
            .with_body(QUOTA_BODY)
            .create_async()
            .await
    }

    async fn fetch(
        server: &mockito::Server,
        token: &str,
        cache: &Cache,
        ttl: Duration,
    ) -> FetchOutcome {
        fetch_snapshot(
            &reqwest::Client::new(),
            token,
            cache,
            &endpoints_for(server),
            ttl,
        )
        .await
        .unwrap()
    }

    fn email_of(outcome: &FetchOutcome) -> Option<&str> {
        outcome
            .email
            .as_ref()
            .map(crate::identity::AccountEmail::as_str)
    }

    #[tokio::test]
    async fn public_profile_email_comes_from_the_same_token() {
        let mut server = mockito::Server::new_async().await;
        mock_quota(&mut server, "mock-oauth-token").await;
        let profile = server
            .mock("GET", "/user")
            .match_header("authorization", "token mock-oauth-token")
            .match_header("accept", "application/vnd.github+json")
            .expect(1)
            .with_status(200)
            .with_header("x-oauth-scopes", "repo, user:email")
            .with_body(r#"{"login":"octo","id":1,"email":"octo@example.test"}"#)
            .create_async()
            .await;
        let emails = server
            .mock("GET", "/user/emails")
            .expect(0)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_in(dir.path());

        let outcome = fetch(&server, "mock-oauth-token", &cache, Duration::ZERO).await;

        profile.assert_async().await;
        emails.assert_async().await;
        assert_eq!(email_of(&outcome), Some("octo@example.test"));
        assert_eq!(outcome.snapshot.premium.unwrap().used_pct(), 85);
        let stored = std::fs::read_to_string(cache.payload_path()).unwrap();
        assert!(
            !stored.contains("octo@example.test"),
            "email stays out of the disk cache"
        );
    }

    #[tokio::test]
    async fn private_email_uses_the_primary_verified_address_when_the_scope_is_granted() {
        let mut server = mockito::Server::new_async().await;
        mock_quota(&mut server, "mock-oauth-token").await;
        server
            .mock("GET", "/user")
            .with_status(200)
            .with_header("x-oauth-scopes", "repo, user:email, read:org")
            .with_body(r#"{"login":"octo","id":1,"email":null}"#)
            .create_async()
            .await;
        let emails = server
            .mock("GET", "/user/emails")
            .match_header("authorization", "token mock-oauth-token")
            .expect(1)
            .with_status(200)
            .with_body(
                r#"[{"email":"first@example.test","primary":false,"verified":true,"visibility":null},
                    {"email":"primary@example.test","primary":true,"verified":true,"visibility":"private"}]"#,
            )
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();

        let outcome = fetch(
            &server,
            "mock-oauth-token",
            &cache_in(dir.path()),
            Duration::ZERO,
        )
        .await;

        emails.assert_async().await;
        assert_eq!(email_of(&outcome), Some("primary@example.test"));
    }

    #[tokio::test]
    async fn an_unverified_primary_or_no_primary_yields_no_email() {
        for body in [
            r#"[{"email":"primary@example.test","primary":true,"verified":false,"visibility":null}]"#,
            r#"[{"email":"first@example.test","primary":false,"verified":true,"visibility":null}]"#,
            r#"[]"#,
        ] {
            let mut server = mockito::Server::new_async().await;
            mock_quota(&mut server, "mock-oauth-token").await;
            server
                .mock("GET", "/user")
                .with_status(200)
                .with_header("x-oauth-scopes", "user")
                .with_body(r#"{"email":null}"#)
                .create_async()
                .await;
            server
                .mock("GET", "/user/emails")
                .with_status(200)
                .with_body(body)
                .create_async()
                .await;
            let dir = tempfile::tempdir().unwrap();
            let outcome = fetch(
                &server,
                "mock-oauth-token",
                &cache_in(dir.path()),
                Duration::ZERO,
            )
            .await;
            assert_eq!(email_of(&outcome), None, "{body}");
        }
    }

    #[tokio::test]
    async fn the_email_list_is_not_requested_without_an_existing_email_scope() {
        for scopes in [Some("repo, read:org, gist"), Some(""), None] {
            let mut server = mockito::Server::new_async().await;
            mock_quota(&mut server, "mock-oauth-token").await;
            let mut profile = server
                .mock("GET", "/user")
                .with_status(200)
                .with_body(r#"{"email":null}"#);
            if let Some(scopes) = scopes {
                profile = profile.with_header("x-oauth-scopes", scopes);
            }
            profile.create_async().await;
            let emails = server
                .mock("GET", "/user/emails")
                .expect(0)
                .create_async()
                .await;
            let dir = tempfile::tempdir().unwrap();
            let outcome = fetch(
                &server,
                "mock-oauth-token",
                &cache_in(dir.path()),
                Duration::ZERO,
            )
            .await;
            emails.assert_async().await;
            assert_eq!(email_of(&outcome), None, "{scopes:?}");
        }
    }

    #[tokio::test]
    async fn a_failing_profile_never_fails_the_quota() {
        for (status, body) in [
            (500, r#"{"email":"leak@example.test"}"#),
            (401, r#"{"message":"Bad credentials"}"#),
            (200, "not json"),
            (200, r#"{"email":"not-an-email"}"#),
        ] {
            let mut server = mockito::Server::new_async().await;
            mock_quota(&mut server, "mock-oauth-token").await;
            server
                .mock("GET", "/user")
                .with_status(status)
                .with_body(body)
                .create_async()
                .await;
            let dir = tempfile::tempdir().unwrap();
            let outcome = fetch(
                &server,
                "mock-oauth-token",
                &cache_in(dir.path()),
                Duration::ZERO,
            )
            .await;
            assert_eq!(email_of(&outcome), None, "{status} {body}");
            assert!(outcome.last_error.is_none());
            assert_eq!(outcome.snapshot.premium.unwrap().used_pct(), 85);
        }
    }

    #[tokio::test]
    async fn an_oversized_profile_body_is_ignored() {
        let mut server = mockito::Server::new_async().await;
        mock_quota(&mut server, "mock-oauth-token").await;
        let padding = "x".repeat(PROFILE_BODY_BYTES + 1);
        server
            .mock("GET", "/user")
            .with_status(200)
            .with_body(format!(
                r#"{{"email":"big@example.test","bio":"{padding}"}}"#
            ))
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let outcome = fetch(
            &server,
            "mock-oauth-token",
            &cache_in(dir.path()),
            Duration::ZERO,
        )
        .await;
        assert_eq!(email_of(&outcome), None);
    }

    #[tokio::test]
    async fn a_slow_profile_is_abandoned_without_delaying_the_quota_past_its_budget() {
        let mut server = mockito::Server::new_async().await;
        mock_quota(&mut server, "mock-oauth-token").await;
        // A profile host that accepts the connection and never answers, on
        // its own thread: the quota server is never blocked by it.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let silent = format!("http://{}", listener.local_addr().unwrap());
        let (release, released) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut held = Vec::new();
            while released.try_recv().is_err() && std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((socket, _)) => held.push(socket),
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        let endpoints = Endpoints {
            profile: format!("{silent}/user"),
            emails: format!("{silent}/user/emails"),
            ..endpoints_for(&server)
        };
        let dir = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let outcome = fetch_snapshot(
            &reqwest::Client::new(),
            "mock-oauth-token",
            &cache_in(dir.path()),
            &endpoints,
            Duration::ZERO,
        )
        .await
        .unwrap();
        let elapsed = started.elapsed();
        let _ = release.send(());
        holder.join().unwrap();
        assert!(elapsed >= PROFILE_TIMEOUT, "{elapsed:?}");
        assert!(
            elapsed < PROFILE_TIMEOUT + Duration::from_millis(400),
            "{elapsed:?}"
        );
        assert_eq!(email_of(&outcome), None);
        assert_eq!(outcome.snapshot.premium.unwrap().used_pct(), 85);
    }

    #[tokio::test]
    async fn a_cached_quota_still_reports_the_current_tokens_email() {
        let mut server = mockito::Server::new_async().await;
        let quota = server
            .mock("GET", "/copilot_internal/user")
            .expect(2)
            .with_status(200)
            .with_body(QUOTA_BODY)
            .create_async()
            .await;
        for (token, email) in [("token-a", "a@example.test"), ("token-b", "b@example.test")] {
            server
                .mock("GET", "/user")
                .match_header("authorization", format!("token {token}").as_str())
                .with_status(200)
                .with_body(format!(r#"{{"email":"{email}"}}"#))
                .create_async()
                .await;
        }
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_in(dir.path());
        let ttl = Duration::from_secs(60);

        let first = fetch(&server, "token-a", &cache, ttl).await;
        let cached = fetch(&server, "token-a", &cache, ttl).await;
        let switched = fetch(&server, "token-b", &cache, ttl).await;

        quota.assert_async().await;
        assert_eq!(email_of(&first), Some("a@example.test"));
        assert!(!cached.off_the_wire());
        assert_eq!(email_of(&cached), Some("a@example.test"));
        assert_eq!(email_of(&switched), Some("b@example.test"));
    }

    #[tokio::test]
    async fn requests_vs_code_endpoint_with_only_copilot_token_and_normalizes_quotas() {
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("GET", "/copilot_internal/user")
            .expect(1)
            .match_header("authorization", "token mock-oauth-token")
            .match_header("accept", "application/json")
            .match_header("editor-version", "vscode/1.96.2")
            .match_header("editor-plugin-version", "copilot-chat/0.26.7")
            .match_header("user-agent", "GitHubCopilotChat/0.26.7")
            .match_header("x-github-api-version", "2025-04-01")
            .with_status(200)
            .with_body(
                r#"{"copilot_plan":"pro","quota_reset_date":"2026-09-15","quota_snapshots":{"premium_interactions":{"entitlement":300,"remaining":45,"percent_remaining":15},"chat":{"entitlement":1000,"remaining":250},"completions":{"unlimited":true}}}"#,
            )
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_in(dir.path());
        let endpoints = endpoints_for(&server);
        let outcome = fetch_snapshot(
            &reqwest::Client::new(),
            "mock-oauth-token",
            &cache,
            &endpoints,
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        let cached = fetch_snapshot(
            &reqwest::Client::new(),
            "mock-oauth-token",
            &cache,
            &endpoints,
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        request.assert_async().await;
        assert_eq!(outcome.snapshot.premium.unwrap().used_pct(), 85);
        assert!(!cached.stale);
        assert!(cached.cache_age.is_some());
        assert_eq!(
            outcome.snapshot.chat.unwrap().used_and_entitlement(),
            Some((750, 1000))
        );
        assert!(outcome.snapshot.completions.unwrap().unlimited);
    }

    #[tokio::test]
    async fn rejected_refresh_uses_stale_cache_without_storing_token_or_response() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/copilot_internal/user")
            .with_status(401)
            .with_body(r#"{"message":"contains private account data"}"#)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_in(dir.path());
        let endpoints = endpoints_for(&server);
        let target = target_key(&endpoints, "mock-oauth-token");
        let snapshot = Snapshot {
            plan: "pro".into(),
            premium: Some(Quota {
                percent_remaining: 80,
                entitlement: Some(300),
                remaining: Some(240),
                unlimited: false,
            }),
            chat: None,
            completions: None,
            reset_at: None,
        };
        cache
            .write_payload(
                serde_json::json!({"target": target, "snapshot": snapshot})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();

        let outcome = fetch_snapshot(
            &reqwest::Client::new(),
            "mock-oauth-token",
            &cache,
            &endpoints,
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert!(outcome.stale);
        assert_eq!(outcome.last_error.unwrap().0, 401);
        assert_eq!(outcome.snapshot.premium.unwrap().used_pct(), 20);
        let cached = std::fs::read_to_string(cache.payload_path()).unwrap();
        assert!(!cached.contains("mock-oauth-token"));
        assert!(!cached.contains("private account data"));
    }

    #[tokio::test]
    async fn invalid_success_body_is_schema_error_on_a_cold_cache() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/copilot_internal/user")
            .with_status(200)
            .with_body(r#"{"quota_snapshots":{}}"#)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let error = fetch_snapshot(
            &reqwest::Client::new(),
            "mock-oauth-token",
            &cache_in(dir.path()),
            &endpoints_for(&server),
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, AppError::Schema(message) if message == SCHEMA_ERROR));
    }
}
