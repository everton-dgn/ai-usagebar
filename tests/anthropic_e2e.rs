//! End-to-end integration test for the Anthropic vendor.
//!
//! Stands up a mockito server that pretends to be the Anthropic OAuth +
//! usage endpoints, drives the full `fetch_snapshot` pipeline against canned
//! fixtures, and asserts the snapshot the report projects. Catches schema
//! drift in the wire types and the stale-cache fallback.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use ai_usagebar::anthropic::{self, fetch::Endpoints};
use ai_usagebar::cache::Cache;
use chrono::{TimeZone, Utc};
use tempfile::{NamedTempFile, TempDir};

fn write_creds() -> NamedTempFile {
    // Token expires far in the future → no refresh needed during the test.
    let expires_ms = chrono::Utc::now().timestamp_millis() + 3_600_000;
    let body = format!(
        r#"{{"claudeAiOauth":{{
            "accessToken":"AT","refreshToken":"RT",
            "expiresAt": {expires_ms},
            "subscriptionType":"max","rateLimitTier":"default_claude_max_5x"
        }}}}"#
    );
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(body.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

fn cache_in(td: &TempDir) -> Cache {
    let c = Cache::at(td.path().join("anthropic"));
    c.ensure_dir().unwrap();
    c
}

fn read_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("missing fixture {}: {e}", path.display());
    })
}

#[tokio::test]
async fn full_response_yields_every_window_and_extra_usage() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/api/oauth/usage")
        .with_status(200)
        .with_body(read_fixture("anthropic_usage_full.json"))
        .create_async()
        .await;

    let td = TempDir::new().unwrap();
    let cache = cache_in(&td);
    let creds = write_creds();
    let client = reqwest::Client::new();
    let endpoints = Endpoints {
        usage: format!("{}/api/oauth/usage", server.url()),
        token: format!("{}/v1/oauth/token", server.url()),
    };
    let outcome = anthropic::fetch_snapshot(
        &client,
        &anthropic::creds::CredsTarget::Explicit(creds.path().to_path_buf()),
        &cache,
        &endpoints,
        Duration::from_secs(0),
    )
    .await
    .unwrap();

    let snap = &outcome.snapshot;
    assert!(!outcome.stale);
    assert!(outcome.last_error.is_none());
    assert_eq!(snap.plan, "Max 5x");
    assert_eq!(snap.session.utilization_pct, 62);
    assert_eq!(
        snap.session.resets_at,
        Some(Utc.with_ymd_and_hms(2026, 5, 23, 13, 30, 0).unwrap())
    );
    assert_eq!(snap.weekly.utilization_pct, 27);
    assert_eq!(
        snap.weekly.resets_at,
        Some(Utc.with_ymd_and_hms(2026, 5, 27, 13, 0, 0).unwrap())
    );
    let sonnet = snap.sonnet.as_ref().expect("sonnet window");
    assert_eq!(sonnet.utilization_pct, 4);
    let extra = snap.extra.as_ref().expect("extra usage");
    assert_eq!(extra.spent.0, 250);
    assert_eq!(extra.limit.map(|limit| limit.0), Some(5000));
}

#[tokio::test]
async fn no_sonnet_no_extra_yields_only_the_two_core_windows() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/api/oauth/usage")
        .with_status(200)
        .with_body(read_fixture("anthropic_usage_minimal.json"))
        .create_async()
        .await;

    let td = TempDir::new().unwrap();
    let cache = cache_in(&td);
    let creds = write_creds();
    let client = reqwest::Client::new();
    let endpoints = Endpoints {
        usage: format!("{}/api/oauth/usage", server.url()),
        token: format!("{}/v1/oauth/token", server.url()),
    };
    let outcome = anthropic::fetch_snapshot(
        &client,
        &anthropic::creds::CredsTarget::Explicit(creds.path().to_path_buf()),
        &cache,
        &endpoints,
        Duration::from_secs(0),
    )
    .await
    .unwrap();
    let snap = &outcome.snapshot;
    assert_eq!(snap.session.utilization_pct, 15);
    assert_eq!(snap.weekly.utilization_pct, 8);
    assert!(snap.sonnet.is_none());
    assert!(snap.extra.is_none());
}

#[tokio::test]
async fn http_429_falls_back_to_stale_cache_with_the_error_kept() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/api/oauth/usage")
        .with_status(429)
        .with_body(r#"{"error":{"type":"rate_limit_error","message":"slow down"}}"#)
        .create_async()
        .await;

    let td = TempDir::new().unwrap();
    let cache = cache_in(&td);
    // Seed cache so fallback has something to serve.
    cache
        .write_payload(read_fixture("anthropic_usage_full.json").as_bytes())
        .unwrap();
    let creds = write_creds();
    let client = reqwest::Client::new();
    let endpoints = Endpoints {
        usage: format!("{}/api/oauth/usage", server.url()),
        token: format!("{}/v1/oauth/token", server.url()),
    };
    let outcome = anthropic::fetch_snapshot(
        &client,
        &anthropic::creds::CredsTarget::Explicit(creds.path().to_path_buf()),
        &cache,
        &endpoints,
        Duration::from_secs(0),
    )
    .await
    .unwrap();
    assert!(outcome.stale);
    assert_eq!(outcome.last_error.as_ref().map(|(c, _)| *c), Some(429));
    assert!(
        outcome
            .last_error
            .as_ref()
            .is_some_and(|(_, message)| message.contains("slow down"))
    );
    assert_eq!(outcome.snapshot.session.utilization_pct, 62);
}
