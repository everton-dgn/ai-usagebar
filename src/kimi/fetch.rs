//! Fetch Kimi usage from `/coding/v1/usages`.
//!
//! One endpoint, two credentials: an **API key**, or the **Kimi Code CLI's
//! OAuth session** — the credential a subscriber already has locally, with no
//! key to create or paste. See [`Auth`] and `oauth.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::cache::{Cache, acquire_lock_async};
use crate::error::{AppError, Result};
use crate::identity::AccountEmail;
use crate::usage::KimiSnapshot;

use super::oauth::{self, Region};
use super::types::{UsagesResponse, UserInfoResponse, humanize_membership_level};

pub const BASE_URL: &str = "https://api.kimi.com";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(15);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
/// Stable marker stored alongside code 0 for a successful HTTP response whose
/// payload no longer matches Kimi's undocumented usage schema.
pub const SCHEMA_DRIFT_MESSAGE: &str = "Kimi API schema drift";

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub usages: String,
    /// Profile endpoint, read solely for the subscription's own tier name.
    pub me: String,
    /// OAuth token endpoint for the same deployment. Unused by the API-key
    /// path, which never refreshes anything.
    pub token: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self::for_region(Region::MainlandCn)
    }
}

impl Endpoints {
    pub fn for_region(region: Region) -> Self {
        Self {
            usages: format!("{}/usages", region.api_base()),
            me: format!("{}/me", region.api_base()),
            token: oauth::token_endpoint(region.oauth_host()),
        }
    }
}

/// Where the bearer for `/coding/v1/usages` comes from.
#[derive(Debug, Clone)]
pub enum Auth {
    /// A platform API key (`KIMI_API_KEY` or `[kimi] api_key`).
    ApiKey(String),
    /// The Kimi Code CLI's own OAuth session — a subscription login.
    KimiCode(KimiCodeAuth),
}

/// The two paths inside a kimi-code home this vendor touches: the credential
/// file it reads and rewrites, and the lock target that serializes a refresh
/// against the CLI's own (see `lock.rs`).
#[derive(Debug, Clone)]
pub struct KimiCodeAuth {
    pub credentials_path: PathBuf,
    pub lock_target: PathBuf,
}

impl KimiCodeAuth {
    pub fn in_home(home: &Path) -> Self {
        Self {
            credentials_path: oauth::credentials_path_in(home),
            lock_target: oauth::lock_target_in(home),
        }
    }

    /// A credential file relocated by config (`[kimi] credentials_path`) still
    /// belongs to a kimi-code home; the lock target is derived from that home
    /// so both clients agree on which file the lock protects.
    pub fn with_credentials_path(home: &Path, credentials_path: PathBuf) -> Self {
        Self {
            credentials_path,
            lock_target: oauth::lock_target_in(home),
        }
    }
}

/// This vendor's [`Outcome`](crate::outcome::Outcome) — the shared shape,
/// specialised to its snapshot.
pub type FetchOutcome = crate::outcome::Outcome<KimiSnapshot>;

/// API-key fetch. Kept as-is for existing callers; the OAuth path goes
/// through [`fetch_snapshot_with_auth`].
pub async fn fetch_snapshot(
    client: &reqwest::Client,
    api_key: &str,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
) -> Result<FetchOutcome> {
    fetch_snapshot_with_auth(
        client,
        &Auth::ApiKey(api_key.to_string()),
        cache,
        endpoints,
        cache_ttl,
    )
    .await
}

pub async fn fetch_snapshot_with_auth(
    client: &reqwest::Client,
    auth: &Auth,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
) -> Result<FetchOutcome> {
    fetch_snapshot_at(client, auth, cache, endpoints, cache_ttl, Utc::now()).await
}

/// Clock seam for the OAuth expiry decision, mirroring
/// `kiro::fetch::fetch_snapshot_at`.
async fn fetch_snapshot_at(
    client: &reqwest::Client,
    auth: &Auth,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
    now: DateTime<Utc>,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;

    // The credential as stored, before any refresh: a fresh payload is only
    // reused for the identity that wrote it.
    let stored = stored_identity(endpoints, auth);

    if let Some(bytes) = cache.fresh_payload(cache_ttl)? {
        // Releases before the profile lookup cached `/usages`' internal enum
        // verbatim. Do not let a still-fresh legacy entry postpone `/me` until
        // the normal TTL expires: refresh it once and replace it with Kimi's
        // own tier name. If the network is down, the fallback path below still
        // serves the quota after humanizing the enum.
        if !cache_has_legacy_plan(&bytes)
            && let Some(identity) = stored
            && let Ok(outcome) = reuse_cache(bytes, cache, false, &identity)
        {
            let email = identity_memory().recall(cache, &identity);
            return Ok(outcome.with_email(email));
        }
    }
    // Corrupt, unbound or foreign fresh cache: fall through to live fetch
    // rather than return a fabricated zero snapshot or another key's quota.

    // The identity a fallback may replay: the bearer actually used, or — when
    // none could be resolved, e.g. a refresh that failed offline — the stored
    // credential. A refresh that rotated the token binds to the new token, so
    // the old payload is refused rather than attributed by assumption.
    let (identity, live) = match bearer_token(client, endpoints, auth, now).await {
        Ok(token) => {
            let identity = identity_digest(&endpoints.me, &token);
            let live = fetch_live(client, endpoints, &token).await;
            (Some(identity), live.map(|live| (identity, live)))
        }
        Err(e) => (stored, Err(e)),
    };

    match live {
        Ok((identity, live)) => {
            let mut payload = snap_to_json(&live.snap);
            payload["identity"] = serde_json::Value::String(identity_hex(&identity));
            cache.write_payload(&serde_json::to_vec(&payload)?)?;
            let email = identity_memory().record(cache, identity, live.profile_email);
            Ok(crate::outcome::Outcome::fresh(live.snap).with_email(email))
        }
        Err(e) if e.is_transient() => fallback_silent(cache, identity.as_ref(), e),
        Err(e) => {
            cache.mark_stale();
            if let Some((code, msg)) = error_to_pair(&e) {
                cache.write_last_error(code, &msg);
            }
            fallback_with_error(cache, identity.as_ref(), e)
        }
    }
}

fn fallback_silent(
    cache: &Cache,
    identity: Option<&IdentityDigest>,
    original: AppError,
) -> Result<FetchOutcome> {
    crate::outcome::fallback(cache, None, original, |bytes| {
        parse_bound_cache(bytes, identity)
    })
}

fn fallback_with_error(
    cache: &Cache,
    identity: Option<&IdentityDigest>,
    original: AppError,
) -> Result<FetchOutcome> {
    let last_error = error_to_pair(&original);
    crate::outcome::fallback(cache, last_error, original, |bytes| {
        parse_bound_cache(bytes, identity)
    })
}

/// The cached snapshot, only when the payload was written under `identity`.
/// A payload from before the binding existed, from another key, or with no
/// identity to compare against is not this credential's quota.
fn parse_bound_cache(bytes: &[u8], identity: Option<&IdentityDigest>) -> Result<KimiSnapshot> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    let bound = v.get("identity").and_then(serde_json::Value::as_str);
    if identity.is_none() || bound != identity.map(identity_hex).as_deref() {
        return Err(AppError::Schema(
            "kimi cache belongs to a different credential".into(),
        ));
    }
    parse_cache(bytes)
}

fn error_to_pair(e: &AppError) -> Option<(u16, String)> {
    match e {
        AppError::Http { status, body } => Some((*status, body.clone())),
        // A 2xx response with an unknown shape is not an HTTP 422 response.
        AppError::Schema(_) => Some((0, SCHEMA_DRIFT_MESSAGE.into())),
        e => Some((0, e.to_string())),
    }
}

fn reuse_cache(
    bytes: Vec<u8>,
    cache: &Cache,
    stale: bool,
    identity: &IdentityDigest,
) -> Result<FetchOutcome> {
    let snap = parse_bound_cache(&bytes, Some(identity))?;
    Ok(crate::outcome::Outcome::cached(snap, cache, stale))
}

fn parse_cache(bytes: &[u8]) -> Result<KimiSnapshot> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    let weekly_limit = parse_cache_u64(&v["weekly_limit"], "weekly_limit")?;
    let weekly_used = parse_cache_u64(&v["weekly_used"], "weekly_used")?;
    let weekly_remaining = parse_cache_u64(&v["weekly_remaining"], "weekly_remaining")?;
    // Caches written before the monthly shape have no `has_weekly` key; they
    // all carried weekly counters, so absence means the legacy shape.
    let has_weekly = match &v["has_weekly"] {
        serde_json::Value::Null => true,
        serde_json::Value::Bool(b) => *b,
        _ => return Err(AppError::Schema("kimi cache: invalid has_weekly".into())),
    };
    if !has_weekly && (weekly_limit > 0 || weekly_used > 0 || weekly_remaining > 0) {
        // A combination the fetch path can never produce: the newer shape has
        // no weekly counters, so nonzero ones here are a corrupt cache.
        return Err(AppError::Schema(
            "kimi cache: weekly counters on a monthly-shape snapshot".into(),
        ));
    }
    let monthly_pct = parse_cache_monthly_pct(&v["monthly_pct"])?;
    Ok(KimiSnapshot {
        plan: v["plan"].as_str().map(|plan| {
            if plan.starts_with("LEVEL_") {
                humanize_membership_level(plan)
            } else {
                plan.to_string()
            }
        }),
        weekly_limit,
        weekly_used,
        weekly_remaining,
        weekly_reset_at: parse_cache_datetime(&v["weekly_reset_at"])?,
        has_weekly,
        monthly_pct,
        monthly_reset_at: parse_cache_datetime(&v["monthly_reset_at"])?,
        window_limit: parse_cache_u64(&v["window_limit"], "window_limit")?,
        window_used: parse_cache_u64(&v["window_used"], "window_used")?,
        window_remaining: parse_cache_u64(&v["window_remaining"], "window_remaining")?,
        window_reset_at: parse_cache_datetime(&v["window_reset_at"])?,
    })
}

fn cache_has_legacy_plan(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|value| value["plan"].as_str().map(str::to_owned))
        .is_some_and(|plan| plan.starts_with("LEVEL_"))
}

fn parse_cache_u64(v: &serde_json::Value, name: &str) -> Result<u64> {
    v.as_u64()
        .ok_or_else(|| AppError::Schema(format!("kimi cache: invalid {name}")))
}

fn parse_cache_datetime(v: &serde_json::Value) -> Result<Option<DateTime<Utc>>> {
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) => DateTime::parse_from_rfc3339(s)
            .map(|dt| Some(dt.into()))
            .map_err(|e| AppError::Schema(format!("kimi cache: invalid reset timestamp: {e}"))),
        _ => Err(AppError::Schema(
            "kimi cache: invalid reset timestamp".into(),
        )),
    }
}

/// A percentage the fetch path already clamped to 0..=100; anything else in
/// the cache is corruption, not a quota.
fn parse_cache_monthly_pct(v: &serde_json::Value) -> Result<Option<i32>> {
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(n) => {
            let pct = n
                .as_i64()
                .ok_or_else(|| AppError::Schema("kimi cache: invalid monthly_pct".into()))?;
            if !(0..=100).contains(&pct) {
                return Err(AppError::Schema(
                    "kimi cache: monthly_pct out of range".into(),
                ));
            }
            Ok(Some(pct as i32))
        }
        _ => Err(AppError::Schema("kimi cache: invalid monthly_pct".into())),
    }
}

fn snap_to_json(snap: &KimiSnapshot) -> serde_json::Value {
    serde_json::json!({
        "plan": snap.plan,
        "weekly_limit": snap.weekly_limit,
        "weekly_used": snap.weekly_used,
        "weekly_remaining": snap.weekly_remaining,
        "weekly_reset_at": snap.weekly_reset_at.map(|dt| dt.to_rfc3339()),
        "has_weekly": snap.has_weekly,
        "monthly_pct": snap.monthly_pct,
        "monthly_reset_at": snap.monthly_reset_at.map(|dt| dt.to_rfc3339()),
        "window_limit": snap.window_limit,
        "window_used": snap.window_used,
        "window_remaining": snap.window_remaining,
        "window_reset_at": snap.window_reset_at.map(|dt| dt.to_rfc3339()),
    })
}

/// Resolve the bearer for this fetch. The API key is already one; a Kimi Code
/// login may first need a refresh, which rotates the CLI's stored token pair
/// and is therefore serialized against the CLI itself.
async fn bearer_token(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    auth: &Auth,
    now: DateTime<Utc>,
) -> Result<String> {
    let kimi_code = match auth {
        Auth::ApiKey(key) => return Ok(key.clone()),
        Auth::KimiCode(kimi_code) => kimi_code,
    };

    let creds = oauth::read_from(&kimi_code.credentials_path)?;
    if !oauth::needs_refresh(creds.expires_at, now.timestamp()) {
        return Ok(creds.access_token);
    }

    let _lock = super::lock::acquire(&kimi_code.lock_target).await?;
    // Re-read under the lock: the CLI (or another ai-usagebar process) may
    // have refreshed while we waited, and reusing our pre-lock copy would burn
    // an already-rotated refresh token.
    let creds = oauth::read_from(&kimi_code.credentials_path)?;
    if !oauth::needs_refresh(creds.expires_at, now.timestamp()) {
        return Ok(creds.access_token);
    }

    let refreshed = tokio::time::timeout(
        REFRESH_TIMEOUT,
        oauth::refresh(
            client,
            &endpoints.token,
            oauth::CLIENT_ID,
            &creds.refresh_token,
        ),
    )
    .await
    .map_err(|_| AppError::Transport(format!("kimi token refresh timeout: {}", endpoints.token)))?
    .map_err(|e| match e {
        AppError::Transport(msg) => AppError::Transport(msg),
        e => AppError::Credentials(format!(
            "Kimi Code token refresh failed ({e}). A new Kimi Code sign-in is needed, which this app cannot do."
        )),
    })?;

    let next = oauth::apply_refresh(&creds, refreshed, now.timestamp());
    oauth::write_to(&kimi_code.credentials_path, &next).map_err(|e| {
        // The rotation already happened upstream, so a failed write-back means
        // the CLI is now holding a dead refresh token: say so plainly instead
        // of leaving the user to discover it at their next `kimi` run.
        AppError::Credentials(format!(
            "the refreshed Kimi Code credentials could not be saved ({e}); a new Kimi Code sign-in is needed, which this app cannot do"
        ))
    })?;
    Ok(next.access_token)
}

/// Ask `/coding/v1/me` for the subscription's own tier name ("Allegretto"),
/// which is the only place the vendor spells its plans the way its pricing
/// page does — `/usages` only carries the `LEVEL_*` wire enum — and for the
/// signed-in account's address.
///
/// Best-effort by construction: every failure returns `None` and leaves the
/// humanized enum in place. A missing plan label or address must never cost
/// the user their quota numbers, and this endpoint is documented (by
/// kimi-code's own error text) to 404 for accounts without a coding profile.
async fn fetch_profile(
    client: &reqwest::Client,
    url: &str,
    token: &str,
) -> Option<UserInfoResponse> {
    let resp = tokio::time::timeout(
        HTTP_TIMEOUT,
        client
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send(),
    )
    .await
    .ok()?
    .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = crate::vendor::read_body_capped(resp, crate::vendor::MAX_BODY_BYTES)
        .await
        .ok()?;
    serde_json::from_slice::<UserInfoResponse>(&bytes).ok()
}

/// What one live poll produced: the quota and — when `/me` answered — that
/// profile's address, which may itself be absent.
struct LiveFetch {
    snap: KimiSnapshot,
    profile_email: Option<Option<AccountEmail>>,
}

async fn fetch_live(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    token: &str,
) -> Result<LiveFetch> {
    let url = &endpoints.usages;
    // Concurrent, not sequential: the profile is a second request against
    // the same deployment, and a widget tick should cost one round-trip's
    // latency, not two.
    let (usages, profile) = tokio::join!(
        tokio::time::timeout(
            HTTP_TIMEOUT,
            client
                .get(url)
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/json")
                .send(),
        ),
        fetch_profile(client, &endpoints.me, token),
    );
    let resp = usages.map_err(|_| AppError::Transport(format!("kimi timeout: {url}")))??;

    let status = resp.status();

    if !status.is_success() {
        // Never surface upstream/proxy bodies: they can contain credentials or
        // arbitrary markup. Keep the cached diagnostic useful but generic.
        let body = if matches!(status.as_u16(), 401 | 403) {
            "Kimi authentication failed".into()
        } else {
            format!("Kimi API returned HTTP {}", status.as_u16())
        };
        return Err(AppError::Http {
            status: status.as_u16(),
            body,
        });
    }

    let bytes = crate::vendor::read_body_capped(resp, crate::vendor::MAX_BODY_BYTES).await?;
    let r: UsagesResponse = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Schema(format!("kimi usages response: {e}")))?;
    let mut snap = r.into_snapshot()?;
    if let Some(label) = profile.as_ref().and_then(UserInfoResponse::plan_label) {
        snap.plan = Some(label);
    }
    Ok(LiveFetch {
        snap,
        profile_email: profile.map(|profile| profile.account_email()),
    })
}

/// SHA-256 over the profile endpoint and the bearer, domain-separated. It
/// names "the account behind this credential on this deployment" without
/// keeping the token itself in memory.
type IdentityDigest = [u8; 32];

fn identity_digest(me_url: &str, token: &str) -> IdentityDigest {
    let mut hasher = Sha256::new();
    hasher.update(b"ai-usagebar/kimi/account-identity/v1\0");
    hasher.update(me_url.as_bytes());
    hasher.update(b"\0");
    hasher.update(token.as_bytes());
    let mut digest = [0; 32];
    digest.copy_from_slice(&hasher.finalize());
    digest
}

/// The credential as stored, without the refresh `bearer_token` may perform.
/// An expired CLI token still names the login that wrote the cache, so it
/// attributes a payload; an unreadable or logged-out store names nobody.
fn stored_identity(endpoints: &Endpoints, auth: &Auth) -> Option<IdentityDigest> {
    let token = match auth {
        Auth::ApiKey(key) => key.clone(),
        Auth::KimiCode(kimi_code) => {
            oauth::read_from(&kimi_code.credentials_path)
                .ok()?
                .access_token
        }
    };
    (!token.trim().is_empty()).then(|| identity_digest(&endpoints.me, &token))
}

/// Lowercase hex of an [`IdentityDigest`], as bound into the cache payload.
fn identity_hex(identity: &IdentityDigest) -> String {
    use std::fmt::Write as _;
    identity
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Which credential wrote each cache payload in this process, and the address
/// its `/me` reported. Memory only: the address never reaches the quota cache
/// (which carries only the identity digest), and after a restart the first
/// live poll re-establishes it.
///
/// An address is only ever paired with a payload this process wrote under the
/// *same* identity; a cache inherited from an earlier run shows the quota with
/// no address rather than a guess.
#[derive(Default)]
struct IdentityMemory(Mutex<HashMap<PathBuf, (IdentityDigest, Option<AccountEmail>)>>);

fn identity_memory() -> &'static IdentityMemory {
    static MEMORY: OnceLock<IdentityMemory> = OnceLock::new();
    MEMORY.get_or_init(IdentityMemory::default)
}

impl IdentityMemory {
    fn entries(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<PathBuf, (IdentityDigest, Option<AccountEmail>)>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Bind the payload just written to `identity`. When `/me` failed this
    /// time, an address the same identity reported earlier is kept; a new
    /// identity starts with none.
    fn record(
        &self,
        cache: &Cache,
        identity: IdentityDigest,
        profile_email: Option<Option<AccountEmail>>,
    ) -> Option<AccountEmail> {
        let mut entries = self.entries();
        let key = cache.payload_path();
        let email = profile_email.unwrap_or_else(|| {
            entries
                .get(&key)
                .filter(|(known, _)| *known == identity)
                .and_then(|(_, email)| email.clone())
        });
        entries.insert(key, (identity, email.clone()));
        email
    }

    /// The address for a cache hit, only when this process wrote the payload
    /// under the same identity.
    fn recall(&self, cache: &Cache, identity: &IdentityDigest) -> Option<AccountEmail> {
        self.entries()
            .get(&cache.payload_path())
            .filter(|(known, _)| known == identity)
            .and_then(|(_, email)| email.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cache_fixture() -> (TempDir, Cache) {
        let td = TempDir::new().unwrap();
        let cache = Cache::at(td.path().join("kimi"));
        cache.ensure_dir().unwrap();
        (td, cache)
    }

    /// Both endpoints point at the same mock server, so an OAuth test can
    /// serve `/api/oauth/token` and `/coding/v1/usages` from one mockito.
    fn test_endpoints(base: &str) -> Endpoints {
        Endpoints {
            usages: format!("{base}/coding/v1/usages"),
            me: format!("{base}/coding/v1/me"),
            token: format!("{base}/api/oauth/token"),
        }
    }

    fn sample_json() -> &'static str {
        r#"{
            "user": { "membership": { "level": "LEVEL_INTERMEDIATE" } },
            "usage": { "limit": "100", "used": "26", "remaining": "74", "resetTime": "2026-02-11T17:32:50.757941Z" },
            "limits": [
                {
                    "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                    "detail": { "limit": "100", "used": "15", "remaining": "85", "resetTime": "2026-02-07T12:32:50.757941Z" }
                }
            ]
        }"#
    }

    /// `sample_seed()` as a live poll with `token` against `base` writes it:
    /// bound to that credential's identity.
    fn bound_seed(base: &str, token: &str) -> String {
        let mut seed = sample_seed();
        seed["identity"] = identity_hex(&identity_digest(&test_endpoints(base).me, token)).into();
        seed.to_string()
    }

    fn sample_seed() -> serde_json::Value {
        serde_json::json!({
            "plan": "LEVEL_INTERMEDIATE",
            "weekly_limit": 100,
            "weekly_used": 30,
            "weekly_remaining": 70,
            "weekly_reset_at": "2026-02-11T17:32:50.757941Z",
            "window_limit": 100,
            "window_used": 20,
            "window_remaining": 80,
            "window_reset_at": "2026-02-07T12:32:50.757941Z"
        })
    }

    #[tokio::test]
    async fn live_200_returns_snapshot_and_sends_headers() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .match_header("authorization", "Bearer sk-test")
            .match_header("accept", "application/json")
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = test_endpoints(&server.url());
        let out = fetch_snapshot(
            &client,
            "sk-test",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        m.assert_async().await;
        // No /me mock on this server, so the humanized wire enum stands.
        assert_eq!(out.snapshot.plan, Some("Intermediate".into()));
        assert_eq!(out.snapshot.weekly_limit, 100);
        assert_eq!(out.snapshot.weekly_used, 26);
        assert_eq!(out.snapshot.weekly_remaining, 74);
        assert_eq!(out.snapshot.window_limit, 100);
        assert_eq!(out.snapshot.window_used, 15);
        assert!(!out.stale);
    }

    #[tokio::test]
    async fn http_401_falls_back_to_cache() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(401)
            .with_body(r#"{"error": "invalid api key"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        cache
            .write_payload(bound_seed(&server.url(), "bad-key").as_bytes())
            .unwrap();

        let client = reqwest::Client::new();
        let endpoints = test_endpoints(&server.url());
        let out = fetch_snapshot(
            &client,
            "bad-key",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert!(out.stale);
        assert_eq!(out.snapshot.weekly_used, 30);
        assert_eq!(out.last_error.as_ref().map(|(c, _)| *c), Some(401));
    }

    #[tokio::test]
    async fn http_500_falls_back_to_cache() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(500)
            .with_body(r#"{"error": "internal server error"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        cache
            .write_payload(bound_seed(&server.url(), "sk-test").as_bytes())
            .unwrap();

        let client = reqwest::Client::new();
        let endpoints = test_endpoints(&server.url());
        let out = fetch_snapshot(
            &client,
            "sk-test",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert!(out.stale);
        assert_eq!(out.last_error.as_ref().map(|(c, _)| *c), Some(500));
    }

    #[tokio::test]
    async fn http_401_without_cache_returns_http_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(401)
            .with_body(r#"{"error": "invalid api key"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = test_endpoints(&server.url());
        let err = fetch_snapshot(
            &client,
            "bad-key",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap_err();
        match err {
            AppError::Http { status, .. } => assert_eq!(status, 401),
            other => panic!("expected Http 401, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn malformed_numeric_200_returns_schema_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(r#"{"usage": {"limit": "100", "used": "garbage"}}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = test_endpoints(&server.url());
        let err = fetch_snapshot(
            &client,
            "sk-test",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("used") || err.to_string().contains("Schema"),
            "expected schema error, got {err}"
        );
    }

    #[tokio::test]
    async fn malformed_numeric_200_with_seeded_cache_returns_stale_snapshot_and_preserves_cache() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(r#"{"usage": {"limit": "100", "used": "garbage"}}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let seeded = bound_seed(&server.url(), "sk-test");
        cache.write_payload(seeded.as_bytes()).unwrap();

        let client = reqwest::Client::new();
        let endpoints = test_endpoints(&server.url());
        let out = fetch_snapshot(
            &client,
            "sk-test",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();

        assert!(out.stale);
        assert_eq!(out.snapshot.weekly_used, 30);
        assert_eq!(out.snapshot.window_used, 20);
        assert_eq!(out.last_error, Some((0, SCHEMA_DRIFT_MESSAGE.into())));

        // The payload file must still contain the original seeded snapshot.
        let payload = std::fs::read_to_string(cache.payload_path()).unwrap();
        assert_eq!(payload, seeded);
    }

    #[tokio::test]
    async fn error_object_200_returns_schema_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(r#"{"error": "invalid token"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = test_endpoints(&server.url());
        let err = fetch_snapshot(
            &client,
            "sk-test",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("usage block"), "got {err}");
    }

    #[tokio::test]
    async fn corrupt_fresh_cache_ignored() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        cache.write_payload(b"not valid json".as_slice()).unwrap();

        let client = reqwest::Client::new();
        let endpoints = test_endpoints(&server.url());
        let out = fetch_snapshot(
            &client,
            "sk-test",
            &cache,
            &endpoints,
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        assert_eq!(out.snapshot.weekly_used, 26);
        assert!(!out.stale);
    }

    #[tokio::test]
    async fn a_fresh_legacy_plan_cache_is_upgraded_through_the_profile_endpoint() {
        let mut server = mockito::Server::new_async().await;
        let usages = server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;
        let me = server
            .mock("GET", "/coding/v1/me")
            .with_status(200)
            .with_body(r#"{"user_level_name":"Allegretto"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        cache
            .write_payload(sample_seed().to_string().as_bytes())
            .unwrap();

        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-test",
            &cache,
            &test_endpoints(&server.url()),
            Duration::from_secs(60),
        )
        .await
        .unwrap();

        usages.assert_async().await;
        me.assert_async().await;
        assert_eq!(out.snapshot.plan, Some("Allegretto".into()));
        let cached: serde_json::Value =
            serde_json::from_slice(&std::fs::read(cache.payload_path()).unwrap()).unwrap();
        assert_eq!(cached["plan"], "Allegretto");
    }

    #[test]
    fn a_legacy_plan_is_humanized_when_only_fallback_cache_is_available() {
        let bytes = sample_seed().to_string();
        let snap = parse_cache(bytes.as_bytes()).unwrap();
        assert_eq!(snap.plan, Some("Intermediate".into()));
    }

    #[tokio::test]
    async fn corrupt_stale_cache_returns_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(401)
            .with_body(r#"{"error": "invalid api key"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        cache.write_payload(b"not valid json".as_slice()).unwrap();

        let client = reqwest::Client::new();
        let endpoints = test_endpoints(&server.url());
        let err = fetch_snapshot(
            &client,
            "bad-key",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, AppError::Http { status, .. } if status == 401),
            "expected 401, got {err:?}"
        );
    }

    #[tokio::test]
    async fn transport_error_with_stale_cache_uses_cache() {
        // Use a URL that will not resolve to trigger a transport error.
        let (_td, cache) = cache_fixture();
        cache
            .write_payload(bound_seed("http://localhost:1", "sk-test").as_bytes())
            .unwrap();

        let client = reqwest::Client::new();
        let endpoints = test_endpoints("http://localhost:1");
        let out = fetch_snapshot(
            &client,
            "sk-test",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert!(out.stale);
        assert_eq!(out.snapshot.weekly_used, 30);
    }

    #[tokio::test]
    async fn missing_counters_with_seeded_cache_preserves_snapshot() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(r#"{"usage":{"limit":100}}"#)
            .create_async()
            .await;
        let (_td, cache) = cache_fixture();
        let seeded = bound_seed(&server.url(), "sk-test");
        cache.write_payload(seeded.as_bytes()).unwrap();
        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-test",
            &cache,
            &test_endpoints(&server.url()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert!(out.stale);
        assert_eq!(out.snapshot.weekly_used, 30);
        assert_eq!(
            std::fs::read_to_string(cache.payload_path()).unwrap(),
            seeded
        );
    }

    #[tokio::test]
    async fn unrecognized_window_with_seeded_cache_preserves_snapshot() {
        let mut server = mockito::Server::new_async().await;
        server.mock("GET", "/coding/v1/usages").with_status(200)
            .with_body(r#"{"usage":{"limit":100,"used":10},"limits":[{"window":{"duration":4,"timeUnit":"TIME_UNIT_HOUR"},"detail":{"limit":100,"used":10}}]}"#).create_async().await;
        let (_td, cache) = cache_fixture();
        let seeded = bound_seed(&server.url(), "sk-test");
        cache.write_payload(seeded.as_bytes()).unwrap();
        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-test",
            &cache,
            &test_endpoints(&server.url()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert!(out.stale);
        assert_eq!(out.snapshot.window_used, 20);
        assert_eq!(
            std::fs::read_to_string(cache.payload_path()).unwrap(),
            seeded
        );
    }

    #[tokio::test]
    async fn http_error_body_is_redacted() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(500)
            .with_body("proxy secret: <token>")
            .create_async()
            .await;
        let (_td, cache) = cache_fixture();
        let err = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-test",
            &cache,
            &test_endpoints(&server.url()),
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, AppError::Http { status: 500, ref body } if body == "Kimi API returned HTTP 500")
        );
    }

    // ---- Kimi Code CLI (subscription) credential path ----

    /// A kimi-code home with a stored login. `expires_at` is relative to
    /// `NOW_SECS`, the instant every OAuth test below passes in.
    const NOW_SECS: i64 = 1_800_000_000;

    fn kimi_code_home(td: &TempDir, expires_in: i64) -> (PathBuf, KimiCodeAuth) {
        let home = td.path().join(".kimi-code");
        let auth = KimiCodeAuth::in_home(&home);
        std::fs::create_dir_all(auth.credentials_path.parent().unwrap()).unwrap();
        std::fs::write(
            &auth.credentials_path,
            serde_json::json!({
                "access_token": "cli-at",
                "refresh_token": "cli-rt",
                "expires_at": NOW_SECS + expires_in,
                "expires_in": 900,
                "scope": "kimi-code",
                "token_type": "Bearer",
            })
            .to_string(),
        )
        .unwrap();
        (home, auth)
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(NOW_SECS, 0).unwrap()
    }

    #[tokio::test]
    async fn a_valid_cli_token_is_used_as_is_and_never_refreshed() {
        let mut server = mockito::Server::new_async().await;
        let usages = server
            .mock("GET", "/coding/v1/usages")
            .match_header("authorization", "Bearer cli-at")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;
        let refresh = server
            .mock("POST", "/api/oauth/token")
            .expect(0)
            .create_async()
            .await;

        let (td, cache) = cache_fixture();
        let (_home, auth) = kimi_code_home(&td, 600);
        let out = fetch_snapshot_at(
            &reqwest::Client::new(),
            &Auth::KimiCode(auth.clone()),
            &cache,
            &test_endpoints(&server.url()),
            Duration::ZERO,
            now(),
        )
        .await
        .unwrap();

        usages.assert_async().await;
        refresh.assert_async().await;
        assert_eq!(out.snapshot.weekly_used, 26);
        let stored = std::fs::read_to_string(&auth.credentials_path).unwrap();
        assert!(stored.contains("cli-rt"), "an unused token must not rotate");
    }

    #[tokio::test]
    async fn an_expiring_cli_token_is_refreshed_and_the_rotation_is_written_back() {
        let mut server = mockito::Server::new_async().await;
        let refresh = server
            .mock("POST", "/api/oauth/token")
            .match_body(mockito::Matcher::UrlEncoded(
                "refresh_token".into(),
                "cli-rt".into(),
            ))
            .with_status(200)
            .with_body(
                r#"{"access_token":"fresh-at","refresh_token":"fresh-rt","expires_in":900,
                    "scope":"kimi-code","token_type":"Bearer"}"#,
            )
            .create_async()
            .await;
        let usages = server
            .mock("GET", "/coding/v1/usages")
            .match_header("authorization", "Bearer fresh-at")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;

        let (td, cache) = cache_fixture();
        // Inside the refresh buffer: still valid, but not for long enough.
        let (home, auth) = kimi_code_home(&td, 30);
        let out = fetch_snapshot_at(
            &reqwest::Client::new(),
            &Auth::KimiCode(auth.clone()),
            &cache,
            &test_endpoints(&server.url()),
            Duration::ZERO,
            now(),
        )
        .await
        .unwrap();

        refresh.assert_async().await;
        usages.assert_async().await;
        assert_eq!(out.snapshot.weekly_used, 26);

        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&auth.credentials_path).unwrap()).unwrap();
        assert_eq!(stored["access_token"], "fresh-at");
        assert_eq!(
            stored["refresh_token"], "fresh-rt",
            "the CLI's own store must carry the rotated token, or its next run is dead"
        );
        assert_eq!(stored["expires_at"], NOW_SECS + 900);
        // The lock is the CLI's own target, and it must not be left behind.
        assert!(!super::super::lock::lock_dir_for(&auth.lock_target).exists());
        assert!(home.join("oauth").is_dir());
    }

    #[tokio::test]
    async fn a_rejected_refresh_reports_a_credential_error_naming_the_cli() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/oauth/token")
            .with_status(401)
            .with_body(r#"{"error":"invalid_grant"}"#)
            .create_async()
            .await;

        let (td, cache) = cache_fixture();
        let (_home, auth) = kimi_code_home(&td, -60);
        let err = fetch_snapshot_at(
            &reqwest::Client::new(),
            &Auth::KimiCode(auth),
            &cache,
            &test_endpoints(&server.url()),
            Duration::ZERO,
            now(),
        )
        .await
        .unwrap_err();
        let message = err.to_string();
        assert!(matches!(err, AppError::Credentials(_)), "{err:?}");
        assert!(message.contains("Kimi Code sign-in"), "{message}");
    }

    #[tokio::test]
    async fn a_logged_out_cli_reports_the_credential_error_instead_of_an_unowned_cache() {
        let (td, cache) = cache_fixture();
        let (_home, auth) = kimi_code_home(&td, 600);
        // The quota the login had before logging out: bound to its token.
        let seeded = bound_seed("http://localhost:1", "cli-at");
        cache.write_payload(seeded.as_bytes()).unwrap();
        std::fs::write(
            &auth.credentials_path,
            r#"{"access_token":"","refresh_token":"","expires_at":0}"#,
        )
        .unwrap();

        // A logged-out store names no account, so nothing may be replayed.
        let err = fetch_snapshot_at(
            &reqwest::Client::new(),
            &Auth::KimiCode(auth),
            &cache,
            &test_endpoints("http://localhost:1"),
            Duration::ZERO,
            now(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::Credentials(_)), "{err:?}");
        assert!(err.to_string().contains("logged out"), "{err}");
        let (code, message) = cache.read_last_error().unwrap();
        assert_eq!(code, 0);
        assert!(message.contains("logged out"), "{message}");
        assert_eq!(
            std::fs::read_to_string(cache.payload_path()).unwrap(),
            seeded
        );
    }

    #[tokio::test]
    async fn a_peer_refresh_during_the_wait_is_picked_up_instead_of_rotating_again() {
        let mut server = mockito::Server::new_async().await;
        let refresh = server
            .mock("POST", "/api/oauth/token")
            .expect(0)
            .create_async()
            .await;
        let usages = server
            .mock("GET", "/coding/v1/usages")
            .match_header("authorization", "Bearer peer-at")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;

        let (td, cache) = cache_fixture();
        let (_home, auth) = kimi_code_home(&td, -60);
        // Stand in for the CLI finishing its own refresh while we queued: the
        // re-read under the lock must win over the copy read before it.
        let peer = serde_json::json!({
            "access_token": "peer-at",
            "refresh_token": "peer-rt",
            "expires_at": NOW_SECS + 900,
            "expires_in": 900,
            "scope": "kimi-code",
            "token_type": "Bearer",
        });
        std::fs::write(&auth.credentials_path, peer.to_string()).unwrap();

        let out = fetch_snapshot_at(
            &reqwest::Client::new(),
            &Auth::KimiCode(auth),
            &cache,
            &test_endpoints(&server.url()),
            Duration::ZERO,
            now(),
        )
        .await
        .unwrap();
        refresh.assert_async().await;
        usages.assert_async().await;
        assert_eq!(out.snapshot.weekly_used, 26);
    }

    #[test]
    fn endpoints_follow_the_region() {
        let cn = Endpoints::for_region(Region::MainlandCn);
        assert_eq!(cn.usages, "https://api.kimi.com/coding/v1/usages");
        assert_eq!(cn.token, "https://auth.kimi.com/api/oauth/token");
        let global = Endpoints::for_region(Region::Global);
        assert_eq!(global.usages, "https://api.kimi.ai/coding/v1/usages");
        assert_eq!(global.token, "https://auth.kimi.ai/api/oauth/token");
        // The default stays the endpoint the API-key path has always used.
        assert_eq!(Endpoints::default().usages, cn.usages);
    }

    #[tokio::test]
    async fn the_vendors_own_tier_name_replaces_the_wire_enum() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;
        let me = server
            .mock("GET", "/coding/v1/me")
            .match_header("authorization", "Bearer sk-test")
            .with_status(200)
            .with_body(r#"{"user_id":"u-1","user_level":25,"user_level_name":"Allegretto"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-test",
            &cache,
            &test_endpoints(&server.url()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        me.assert_async().await;
        assert_eq!(out.snapshot.plan, Some("Allegretto".into()));
        // …and it survives the cache round-trip, not just the live fetch.
        let cached: serde_json::Value =
            serde_json::from_slice(&std::fs::read(cache.payload_path()).unwrap()).unwrap();
        assert_eq!(cached["plan"], "Allegretto");
    }

    #[tokio::test]
    async fn a_profile_endpoint_that_fails_costs_the_label_and_nothing_else() {
        // 404 is documented by kimi-code's own error text for accounts with no
        // coding profile; the quota numbers must still come through.
        for status in [404, 401, 500] {
            let mut server = mockito::Server::new_async().await;
            server
                .mock("GET", "/coding/v1/usages")
                .with_status(200)
                .with_body(sample_json())
                .create_async()
                .await;
            server
                .mock("GET", "/coding/v1/me")
                .with_status(status)
                .with_body(r#"{"error":"nope"}"#)
                .create_async()
                .await;

            let (_td, cache) = cache_fixture();
            let out = fetch_snapshot(
                &reqwest::Client::new(),
                "sk-test",
                &cache,
                &test_endpoints(&server.url()),
                Duration::ZERO,
            )
            .await
            .unwrap();
            assert_eq!(out.snapshot.plan, Some("Intermediate".into()), "{status}");
            assert_eq!(out.snapshot.weekly_used, 26, "{status}");
            assert!(out.last_error.is_none(), "{status}: must not warn");
        }
    }

    #[tokio::test]
    async fn an_unreachable_profile_endpoint_does_not_fail_the_fetch() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;
        let mut endpoints = test_endpoints(&server.url());
        endpoints.me = "http://localhost:1/coding/v1/me".into();

        let (_td, cache) = cache_fixture();
        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-test",
            &cache,
            &endpoints,
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(out.snapshot.plan, Some("Intermediate".into()));
        assert!(!out.stale);
    }

    #[test]
    fn a_relocated_credential_file_keeps_the_homes_lock_target() {
        let home = Path::new("/home/u/.kimi-code");
        let auth =
            KimiCodeAuth::with_credentials_path(home, PathBuf::from("/elsewhere/kimi-code.json"));
        assert_eq!(
            auth.credentials_path,
            PathBuf::from("/elsewhere/kimi-code.json")
        );
        assert_eq!(auth.lock_target, oauth::lock_target_in(home));
    }

    fn monthly_shape_snap() -> KimiSnapshot {
        KimiSnapshot {
            plan: Some("Allegretto".into()),
            weekly_limit: 0,
            weekly_used: 0,
            weekly_remaining: 0,
            weekly_reset_at: None,
            has_weekly: false,
            monthly_pct: Some(42),
            monthly_reset_at: Some(
                DateTime::parse_from_rfc3339("2026-10-16T00:00:00Z")
                    .unwrap()
                    .into(),
            ),
            window_limit: 100,
            window_used: 15,
            window_remaining: 85,
            window_reset_at: Some(
                DateTime::parse_from_rfc3339("2026-09-16T20:11:32Z")
                    .unwrap()
                    .into(),
            ),
        }
    }

    #[test]
    fn a_monthly_shape_snapshot_survives_the_cache_round_trip() {
        let bytes = serde_json::to_vec(&snap_to_json(&monthly_shape_snap())).unwrap();
        let snap = parse_cache(&bytes).unwrap();
        assert!(!snap.has_weekly);
        assert_eq!(snap.monthly_pct, Some(42));
        assert_eq!(
            snap.monthly_reset_at.map(|dt| dt.to_rfc3339()),
            Some("2026-10-16T00:00:00+00:00".to_string())
        );
        assert_eq!(snap.weekly_used, 0);
        assert_eq!(snap.window_used, 15);
        assert_eq!(snap, monthly_shape_snap());
    }

    #[test]
    fn a_legacy_cache_without_the_new_keys_still_parses() {
        // sample_seed predates the monthly shape: no has_weekly/monthly keys.
        let bytes = sample_seed().to_string();
        let snap = parse_cache(bytes.as_bytes()).unwrap();
        assert!(snap.has_weekly);
        assert_eq!(snap.monthly_pct, None);
        assert_eq!(snap.monthly_reset_at, None);
        assert_eq!(snap.weekly_used, 30);
    }

    #[test]
    fn a_cache_with_weekly_counters_on_a_monthly_shape_is_rejected() {
        let mut v = sample_seed();
        v["has_weekly"] = serde_json::json!(false);
        let err = parse_cache(v.to_string().as_bytes()).unwrap_err();
        assert!(err.to_string().contains("monthly-shape"), "{err}");
    }

    #[test]
    fn a_cache_with_an_out_of_range_monthly_pct_is_rejected() {
        for bad in [150, -1] {
            let mut v: serde_json::Value = serde_json::from_slice(
                &serde_json::to_vec(&snap_to_json(&monthly_shape_snap())).unwrap(),
            )
            .unwrap();
            v["monthly_pct"] = serde_json::json!(bad);
            let err = parse_cache(v.to_string().as_bytes()).unwrap_err();
            assert!(err.to_string().contains("monthly_pct"), "{bad}: {err}");
        }
    }

    // --- account email ---------------------------------------------------

    fn profile_body(email: &str) -> String {
        serde_json::json!({"user_level_name": "Allegretto", "email": email}).to_string()
    }

    fn email_of(out: &FetchOutcome) -> Option<&str> {
        out.email.as_ref().map(AccountEmail::as_str)
    }

    async fn fetch_with_key(
        server: &mockito::Server,
        cache: &Cache,
        key: &str,
        ttl: Duration,
    ) -> Result<FetchOutcome> {
        fetch_snapshot(
            &reqwest::Client::new(),
            key,
            cache,
            &test_endpoints(&server.url()),
            ttl,
        )
        .await
    }

    #[tokio::test]
    async fn the_profile_email_rides_the_outcome_but_never_the_cache() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;
        let me = server
            .mock("GET", "/coding/v1/me")
            .match_header("authorization", "Bearer sk-test")
            .with_status(200)
            .with_body(profile_body("person@example.test"))
            .expect(1)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let out = fetch_with_key(&server, &cache, "sk-test", Duration::ZERO)
            .await
            .unwrap();

        me.assert_async().await;
        assert!(out.off_the_wire());
        assert_eq!(email_of(&out), Some("person@example.test"));
        assert_eq!(out.snapshot.plan, Some("Allegretto".into()));
        assert!(!format!("{out:?}").contains("person@"), "Debug leaks it");
        let stored = String::from_utf8(std::fs::read(cache.payload_path()).unwrap()).unwrap();
        assert!(!stored.contains("person@"), "{stored}");
        assert!(!stored.contains("sk-test"), "{stored}");
    }

    #[tokio::test]
    async fn a_fresh_cache_hit_reuses_the_address_without_another_profile_call() {
        let mut server = mockito::Server::new_async().await;
        let usages = server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .expect(1)
            .create_async()
            .await;
        let me = server
            .mock("GET", "/coding/v1/me")
            .with_status(200)
            .with_body(profile_body("person@example.test"))
            .expect(1)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        fetch_with_key(&server, &cache, "sk-test", Duration::ZERO)
            .await
            .unwrap();
        let cached = fetch_with_key(&server, &cache, "sk-test", Duration::from_secs(3600))
            .await
            .unwrap();

        usages.assert_async().await;
        me.assert_async().await;
        assert!(!cached.off_the_wire());
        assert_eq!(email_of(&cached), Some("person@example.test"));
    }

    #[tokio::test]
    async fn a_new_key_never_inherits_the_previous_keys_address() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .expect(2)
            .create_async()
            .await;
        server
            .mock("GET", "/coding/v1/me")
            .match_header("authorization", "Bearer key-a")
            .with_status(200)
            .with_body(profile_body("first@example.test"))
            .expect(1)
            .create_async()
            .await;
        let me_b = server
            .mock("GET", "/coding/v1/me")
            .match_header("authorization", "Bearer key-b")
            .with_status(200)
            .with_body(profile_body("second@example.test"))
            .expect(1)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let first = fetch_with_key(&server, &cache, "key-a", Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(email_of(&first), Some("first@example.test"));

        // The fresh payload is bound to key A, so key B refetches its own
        // quota inside the TTL and gets its own address, never A's.
        let live = fetch_with_key(&server, &cache, "key-b", Duration::from_secs(3600))
            .await
            .unwrap();
        me_b.assert_async().await;
        assert!(live.off_the_wire());
        assert_eq!(email_of(&live), Some("second@example.test"));
    }

    #[tokio::test]
    async fn a_failed_profile_keeps_only_the_same_keys_known_address() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;
        let ok = server
            .mock("GET", "/coding/v1/me")
            .with_status(200)
            .with_body(profile_body("person@example.test"))
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let first = fetch_with_key(&server, &cache, "key-a", Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(email_of(&first), Some("person@example.test"));

        ok.remove_async().await;
        server
            .mock("GET", "/coding/v1/me")
            .with_status(500)
            .create_async()
            .await;
        let same_key = fetch_with_key(&server, &cache, "key-a", Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(email_of(&same_key), Some("person@example.test"));
        assert!(same_key.last_error.is_none());

        let other_key = fetch_with_key(&server, &cache, "key-b", Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(other_key.snapshot.weekly_used, 26);
        assert_eq!(email_of(&other_key), None);
    }

    #[tokio::test]
    async fn a_profile_without_a_usable_address_yields_no_email() {
        for body in [
            r#"{"user_level_name":"Allegretto"}"#,
            r#"{"user_level_name":"Allegretto","email":"not-an-address"}"#,
        ] {
            let mut server = mockito::Server::new_async().await;
            server
                .mock("GET", "/coding/v1/usages")
                .with_status(200)
                .with_body(sample_json())
                .create_async()
                .await;
            server
                .mock("GET", "/coding/v1/me")
                .with_status(200)
                .with_body(body)
                .create_async()
                .await;
            let (_td, cache) = cache_fixture();
            let out = fetch_with_key(&server, &cache, "sk-test", Duration::ZERO)
                .await
                .unwrap();
            assert_eq!(out.snapshot.plan, Some("Allegretto".into()), "{body}");
            assert_eq!(email_of(&out), None, "{body}");
        }
    }

    #[tokio::test]
    async fn an_error_fallback_carries_no_email() {
        let mut server = mockito::Server::new_async().await;
        let ok = server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;
        server
            .mock("GET", "/coding/v1/me")
            .with_status(200)
            .with_body(profile_body("person@example.test"))
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        fetch_with_key(&server, &cache, "sk-test", Duration::ZERO)
            .await
            .unwrap();
        ok.remove_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(401)
            .create_async()
            .await;
        // Same key and endpoint, so its own payload is replayed — but a
        // failed poll vouches for no address, even with `/me` still answering.
        let replay = fetch_with_key(&server, &cache, "sk-test", Duration::ZERO)
            .await
            .unwrap();
        assert!(replay.stale);
        assert_eq!(replay.last_error.as_ref().map(|(code, _)| *code), Some(401));
        assert_eq!(email_of(&replay), None);
    }

    #[tokio::test]
    async fn an_expired_cli_token_still_owns_its_cache_and_is_not_refreshed_by_a_hit() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .match_header("authorization", "Bearer cli-at")
            .with_status(200)
            .with_body(sample_json())
            .expect(1)
            .create_async()
            .await;
        server
            .mock("GET", "/coding/v1/me")
            .match_header("authorization", "Bearer cli-at")
            .with_status(200)
            .with_body(profile_body("person@example.test"))
            .expect(1)
            .create_async()
            .await;
        let refresh = server
            .mock("POST", "/api/oauth/token")
            .expect(0)
            .create_async()
            .await;

        let (td, cache) = cache_fixture();
        let (_home, auth) = kimi_code_home(&td, 600);
        let auth = Auth::KimiCode(auth);
        let endpoints = test_endpoints(&server.url());
        let client = reqwest::Client::new();
        let live = fetch_snapshot_at(&client, &auth, &cache, &endpoints, Duration::ZERO, now())
            .await
            .unwrap();
        assert_eq!(email_of(&live), Some("person@example.test"));

        let same_token = fetch_snapshot_at(
            &client,
            &auth,
            &cache,
            &endpoints,
            Duration::from_secs(3600),
            now(),
        )
        .await
        .unwrap();
        assert_eq!(email_of(&same_token), Some("person@example.test"));

        // Past the token's expiry the stored token still names the login that
        // wrote the payload, so the hit is served — without a refresh.
        let later = now() + chrono::Duration::seconds(3600);
        let expired = fetch_snapshot_at(
            &client,
            &auth,
            &cache,
            &endpoints,
            Duration::from_secs(3600),
            later,
        )
        .await
        .unwrap();
        refresh.assert_async().await;
        assert!(!expired.off_the_wire());
        assert_eq!(expired.snapshot.weekly_used, 26);
        assert_eq!(email_of(&expired), Some("person@example.test"));
    }

    // --- credential-bound quota cache ------------------------------------

    #[tokio::test]
    async fn the_cache_carries_the_identity_digest_but_no_credential() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;
        let (_td, cache) = cache_fixture();
        fetch_with_key(&server, &cache, "sk-secret-value", Duration::ZERO)
            .await
            .unwrap();
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(cache.payload_path()).unwrap()).unwrap();
        let expected = identity_hex(&identity_digest(
            &test_endpoints(&server.url()).me,
            "sk-secret-value",
        ));
        assert_eq!(stored["identity"], expected.as_str());
        assert_eq!(expected.len(), 64);
        assert!(!stored.to_string().contains("sk-secret-value"));
        // The snapshot fields are unchanged around the envelope field.
        assert_eq!(
            parse_cache(stored.to_string().as_bytes())
                .unwrap()
                .weekly_used,
            26
        );
    }

    #[tokio::test]
    async fn an_unbound_fresh_cache_from_an_older_release_is_a_miss() {
        let mut server = mockito::Server::new_async().await;
        let usages = server
            .mock("GET", "/coding/v1/usages")
            .with_status(200)
            .with_body(sample_json())
            .expect(1)
            .create_async()
            .await;
        let (_td, cache) = cache_fixture();
        let mut legacy = sample_seed();
        legacy["plan"] = "Allegretto".into();
        cache.write_payload(legacy.to_string().as_bytes()).unwrap();

        let out = fetch_with_key(&server, &cache, "sk-test", Duration::from_secs(3600))
            .await
            .unwrap();
        usages.assert_async().await;
        assert!(out.off_the_wire());
        assert_eq!(out.snapshot.weekly_used, 26);
    }

    #[tokio::test]
    async fn an_unbound_cache_is_never_a_fallback() {
        let (_td, cache) = cache_fixture();
        cache
            .write_payload(sample_seed().to_string().as_bytes())
            .unwrap();
        let err = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-test",
            &cache,
            &test_endpoints("http://localhost:1"),
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::Transport(_)), "{err:?}");
    }

    #[tokio::test]
    async fn another_keys_cache_is_never_served_even_offline() {
        let (_td, cache) = cache_fixture();
        let base = "http://localhost:1";
        cache
            .write_payload(bound_seed(base, "key-a").as_bytes())
            .unwrap();

        let err = fetch_snapshot(
            &reqwest::Client::new(),
            "key-b",
            &cache,
            &test_endpoints(base),
            Duration::from_secs(3600),
        )
        .await
        .unwrap_err();
        // The original transport failure, not key A's quota.
        assert!(matches!(err, AppError::Transport(_)), "{err:?}");

        // The same key still gets its own payload back while offline.
        let own = fetch_snapshot(
            &reqwest::Client::new(),
            "key-a",
            &cache,
            &test_endpoints(base),
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert!(own.stale);
        assert_eq!(own.snapshot.weekly_used, 30);
    }

    #[tokio::test]
    async fn another_region_is_another_identity() {
        let (_td, cache) = cache_fixture();
        cache
            .write_payload(bound_seed("http://localhost:1", "sk-test").as_bytes())
            .unwrap();
        let err = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-test",
            &cache,
            &test_endpoints("http://127.0.0.1:1"),
            Duration::from_secs(3600),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::Transport(_)), "{err:?}");
    }

    #[tokio::test]
    async fn a_rotated_token_whose_poll_fails_does_not_replay_the_old_tokens_payload() {
        let mut server = mockito::Server::new_async().await;
        let refresh = server
            .mock("POST", "/api/oauth/token")
            .with_status(200)
            .with_body(
                r#"{"access_token":"fresh-at","refresh_token":"fresh-rt","expires_in":900,
                    "scope":"kimi-code","token_type":"Bearer"}"#,
            )
            .expect(1)
            .create_async()
            .await;
        server
            .mock("GET", "/coding/v1/usages")
            .match_header("authorization", "Bearer fresh-at")
            .with_status(500)
            .create_async()
            .await;

        let (td, cache) = cache_fixture();
        cache
            .write_payload(bound_seed(&server.url(), "cli-at").as_bytes())
            .unwrap();
        // Expired: the fresh cache still belongs to `cli-at`, but the poll
        // goes live because the TTL is zero, and the refresh rotates it.
        let (_home, auth) = kimi_code_home(&td, -60);
        let err = fetch_snapshot_at(
            &reqwest::Client::new(),
            &Auth::KimiCode(auth),
            &cache,
            &test_endpoints(&server.url()),
            Duration::ZERO,
            now(),
        )
        .await
        .unwrap_err();
        refresh.assert_async().await;
        assert!(matches!(err, AppError::Http { status: 500, .. }), "{err:?}");
    }

    #[tokio::test]
    async fn a_refresh_that_fails_offline_replays_the_stored_tokens_own_payload() {
        let (td, cache) = cache_fixture();
        let base = "http://localhost:1";
        cache
            .write_payload(bound_seed(base, "cli-at").as_bytes())
            .unwrap();
        let (_home, auth) = kimi_code_home(&td, -60);
        let out = fetch_snapshot_at(
            &reqwest::Client::new(),
            &Auth::KimiCode(auth),
            &cache,
            &test_endpoints(base),
            Duration::ZERO,
            now(),
        )
        .await
        .unwrap();
        assert!(out.stale);
        assert_eq!(out.snapshot.weekly_used, 30);
        assert!(out.email.is_none());
    }
}
