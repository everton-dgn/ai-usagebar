//! Nous OAuth refresh for a sign-in saved earlier. The app has no Nous
//! sign-in flow of its own.

use chrono::{DateTime, Utc};
use serde_json::Value;
use thiserror::Error;

use super::credentials::{CredentialStore, NousCredential};
use super::types::{TokenResponse, parse_token};

pub const CLIENT_ID: &str = "hermes-cli";
pub const TOKEN_URL: &str = "https://portal.nousresearch.com/api/oauth/token";
pub const REFRESH_SKEW_SECONDS: i64 = 120;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum OAuthError {
    #[error("OAuth transport failure")]
    Transport,
    #[error("OAuth HTTP request returned status {0}")]
    HttpStatus(u16),
    #[error("OAuth response schema mismatch")]
    Schema,
    #[error("OAuth server returned an unknown error")]
    UnknownOAuthError,
    #[error("authorization was denied")]
    AccessDenied,
    #[error("device authorization expired")]
    ExpiredToken,
    #[error("device authorization deadline elapsed")]
    Deadline,
    #[error("refresh authorization was rejected; login is required again")]
    RefreshTokenRejected,
    #[error("credential store failure")]
    Credentials,
}

pub async fn refresh_access_token(
    client: &reqwest::Client,
    endpoint: &str,
    refresh_token: &str,
) -> Result<TokenResponse, OAuthError> {
    if refresh_token.trim().is_empty() {
        return Err(OAuthError::Credentials);
    }
    let response = client
        .post(endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-nous-refresh-token", refresh_token)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .map_err(|_| OAuthError::Transport)?;
    let status = response.status();
    if !status.is_success() {
        // Portal uses 400 for an expired, revoked, reused, or otherwise invalid
        // refresh grant. All require a clean login; no body text is surfaced.
        if status.as_u16() == 400 {
            return Err(OAuthError::RefreshTokenRejected);
        }
        return Err(OAuthError::HttpStatus(status.as_u16()));
    }
    let body = crate::vendor::read_body_capped(response, crate::vendor::MAX_BODY_BYTES)
        .await
        .map_err(|_| OAuthError::Transport)?;
    let value: Value = serde_json::from_slice(&body).map_err(|_| OAuthError::Schema)?;
    parse_token(&value).map_err(|_| OAuthError::Schema)
}

pub fn needs_refresh(now: DateTime<Utc>, expires_at: DateTime<Utc>) -> bool {
    now.checked_add_signed(chrono::Duration::seconds(REFRESH_SKEW_SECONDS))
        .is_none_or(|threshold| expires_at <= threshold)
}

/// Lock, re-read, refresh at most once, and persist the complete rotated pair
/// before returning the access credential to an account fetcher.
pub async fn refresh_if_needed(
    client: &reqwest::Client,
    store: &CredentialStore,
    endpoint: &str,
    now: DateTime<Utc>,
) -> Result<NousCredential, OAuthError> {
    let lock = store.acquire_lock().map_err(|_| OAuthError::Credentials)?;
    let document = store
        .read_unlocked()
        .map_err(|_| OAuthError::Credentials)?
        .ok_or(OAuthError::Credentials)?;
    let current = document
        .nous
        .as_ref()
        .ok_or(OAuthError::Credentials)?
        .clone();
    if !needs_refresh(now, current.expires_at) {
        drop(lock);
        return Ok(current);
    }
    let token = refresh_access_token(client, endpoint, &current.refresh_token).await?;
    let expires_at = token_expiration(now, token.expires_in)?;
    let replacement = NousCredential {
        client_id: CLIENT_ID.into(),
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at,
    };
    replacement.validate().map_err(|_| OAuthError::Schema)?;
    let mut replacement_document = document;
    replacement_document.nous = Some(replacement.clone());
    store
        .write_locked(&lock, &replacement_document)
        .map_err(|_| OAuthError::Credentials)?;
    drop(lock);
    Ok(replacement)
}

fn token_expiration(now: DateTime<Utc>, expires_in: u64) -> Result<DateTime<Utc>, OAuthError> {
    let seconds = i64::try_from(expires_in).map_err(|_| OAuthError::Schema)?;
    now.checked_add_signed(chrono::Duration::seconds(seconds))
        .ok_or(OAuthError::Schema)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};

    use super::*;

    #[test]
    fn refresh_threshold_is_exactly_120_seconds() {
        let now = Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
        assert!(!needs_refresh(now, now + ChronoDuration::seconds(121)));
        assert!(needs_refresh(now, now + ChronoDuration::seconds(120)));
        assert!(needs_refresh(now, now + ChronoDuration::seconds(119)));
        assert!(needs_refresh(now, now - ChronoDuration::seconds(1)));
    }

    #[tokio::test]
    async fn refresh_request_uses_header_and_required_form_without_secret_in_url() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/token")
            .match_header("x-nous-refresh-token", "test-old-refresh")
            .match_header(
                "content-type",
                mockito::Matcher::Regex("application/x-www-form-urlencoded.*".into()),
            )
            .match_body(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("grant_type".into(), "refresh_token".into()),
                mockito::Matcher::UrlEncoded("client_id".into(), "hermes-cli".into()),
                mockito::Matcher::UrlEncoded(
                    "refresh_token".into(),
                    "test-old-refresh".into(),
                ),
            ]))
            .with_status(200)
            .with_body(r#"{"access_token":"test-new-access","refresh_token":"test-new-refresh","token_type":"Bearer","expires_in":3600}"#)
            .create_async()
            .await;
        let token = refresh_access_token(
            &reqwest::Client::new(),
            &format!("{}/token", server.url()),
            "test-old-refresh",
        )
        .await
        .unwrap();
        assert_eq!(token.access_token, "test-new-access");
        mock.assert_async().await;
    }

    #[test]
    fn token_expiration_rejects_overflow_instead_of_wrapping_or_panicking() {
        let now = Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
        assert_eq!(
            token_expiration(now, 3600).unwrap(),
            now + ChronoDuration::hours(1)
        );
        assert_eq!(token_expiration(now, u64::MAX), Err(OAuthError::Schema));
        assert_eq!(
            token_expiration(DateTime::<Utc>::MAX_UTC, 1),
            Err(OAuthError::Schema)
        );
    }

    #[tokio::test]
    async fn any_bad_refresh_grant_requires_a_clean_login() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/token")
            .with_status(400)
            .with_body("non-json error body that must not affect classification")
            .create_async()
            .await;

        let error = refresh_access_token(
            &reqwest::Client::new(),
            &format!("{}/token", server.url()),
            "test-old-refresh",
        )
        .await
        .unwrap_err();
        assert_eq!(error, OAuthError::RefreshTokenRejected);
    }
}
