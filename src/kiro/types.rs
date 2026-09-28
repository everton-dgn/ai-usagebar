//! Wire schema for `AmazonCodeWhispererService.GetUsageLimits` — the exact
//! call kiro-cli's own `/usage` slash command makes (confirmed by tracing
//! `kiro-cli chat --no-interactive -vvv "/usage"`, which logs the operation
//! name and the `management.<region>.kiro.dev` endpoint it resolves to for an
//! IAM Identity Center account). Verified live against
//! `https://codewhisperer.us-east-1.amazonaws.com/` with `x-amz-target:
//! AmazonCodeWhispererService.GetUsageLimits` and the account's own cached
//! bearer token:
//!
//! ```json
//! {
//!   "nextDateReset": 1785542400.0,
//!   "subscriptionInfo": { "subscriptionTitle": "KIRO POWER" },
//!   "usageBreakdownList": [{
//!     "resourceType": "CREDIT",
//!     "displayName": "Credit",
//!     "currentUsageWithPrecision": 9943.38,
//!     "usageLimitWithPrecision": 10000.0
//!   }]
//! }
//! ```
//!
//! **Reverse-engineered, not documented** — CodeWhisperer/Q Developer has no
//! public API reference (AWS's own docs say as much). Community reference
//! implementations exist (`Finesssee/ProxyPilot`, `HsnSaboor/CLIProxyAPIPlus`)
//! confirming the same request/response shape against `codewhisperer.*.amazonaws.com`.
//!
//! The optional top-level `userInfo` object (`userId`, `email`) is part of the
//! public Smithy client AWS ships in `aws/amazon-q-developer-cli`
//! (`crates/amzn-codewhisperer-client`, `shape_get_usage_limits.rs` and
//! `shape_user_info.rs`). Only its email is read, as display identity.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};

use crate::error::{AppError, Result};
use crate::identity::AccountEmail;
use crate::usage::KiroSnapshot;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimitsResponse {
    #[serde(default)]
    pub subscription_info: Option<SubscriptionInfo>,
    #[serde(default)]
    pub usage_breakdown_list: Vec<UsageBreakdown>,
    #[serde(default)]
    pub next_date_reset: Option<f64>,
    /// `userInfo.email` of this response, when it holds a valid address.
    /// Anything else in `userInfo`, such as `userId`, is not retained.
    #[serde(default, rename = "userInfo", deserialize_with = "user_info_email")]
    pub email: Option<AccountEmail>,
}

/// Identity is best-effort: a missing, null, malformed or invalid `userInfo`
/// yields no email and never fails the usage response.
fn user_info_email<'de, D>(deserializer: D) -> std::result::Result<Option<AccountEmail>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .as_ref()
        .and_then(|info| info.get("email"))
        .and_then(serde_json::Value::as_str)
        .and_then(AccountEmail::parse))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInfo {
    #[serde(default)]
    pub subscription_title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBreakdown {
    #[serde(default)]
    pub resource_type: Option<String>,
    #[serde(default)]
    pub current_usage_with_precision: Option<f64>,
    #[serde(default)]
    pub usage_limit_with_precision: Option<f64>,
}

/// The list can carry more than one resource bucket; the credit pool
/// (`resourceType: "CREDIT"`) is what kiro-cli's own `/usage` renders, so it
/// wins when present. A single-entry list with no `resourceType` (seen on
/// some plans) is accepted as-is rather than rejected on a technicality.
fn credit_breakdown(list: &[UsageBreakdown]) -> Result<&UsageBreakdown> {
    if let Some(credit) = list
        .iter()
        .find(|b| b.resource_type.as_deref() == Some("CREDIT"))
    {
        return Ok(credit);
    }
    if let [only] = list
        && only.resource_type.is_none()
    {
        return Ok(only);
    }
    if list.is_empty() {
        Err(AppError::Schema(
            "kiro: `usageBreakdownList` is empty".into(),
        ))
    } else {
        Err(AppError::Schema(
            "kiro: no unambiguous `CREDIT` usage bucket".into(),
        ))
    }
}

pub fn to_snapshot(resp: UsageLimitsResponse) -> Result<KiroSnapshot> {
    let plan = resp
        .subscription_info
        .as_ref()
        .and_then(|s| s.subscription_title.as_deref())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            AppError::Schema("kiro: missing `subscriptionInfo.subscriptionTitle`".into())
        })?
        .to_string();

    let breakdown = credit_breakdown(&resp.usage_breakdown_list)?;

    let used = finite(
        "currentUsageWithPrecision",
        breakdown.current_usage_with_precision,
    )?;
    let limit = finite(
        "usageLimitWithPrecision",
        breakdown.usage_limit_with_precision,
    )?;

    let reset_at = resp
        .next_date_reset
        .map(|secs| seconds_to_datetime("nextDateReset", secs))
        .transpose()?;

    Ok(KiroSnapshot {
        plan,
        used,
        limit,
        reset_at,
    })
}

fn finite(field: &str, v: Option<f64>) -> Result<f64> {
    let v = v.ok_or_else(|| AppError::Schema(format!("kiro: missing `{field}`")))?;
    if !v.is_finite() || v < 0.0 {
        return Err(AppError::Schema(format!(
            "kiro: `{field}` is not a non-negative finite number ({v})"
        )));
    }
    Ok(v)
}

fn seconds_to_datetime(field: &str, secs: f64) -> Result<DateTime<Utc>> {
    if !secs.is_finite() || secs < 0.0 {
        return Err(AppError::Schema(format!(
            "kiro: `{field}` is not a valid Unix timestamp ({secs})"
        )));
    }
    DateTime::from_timestamp(secs as i64, 0)
        .ok_or_else(|| AppError::Schema(format!("kiro: `{field}` is out of range ({secs})")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> UsageLimitsResponse {
        serde_json::from_str(
            r#"{
                "daysUntilReset": 0,
                "nextDateReset": 1785542400.0,
                "subscriptionInfo": { "subscriptionTitle": "KIRO POWER" },
                "usageBreakdownList": [{
                    "resourceType": "CREDIT",
                    "displayName": "Credit",
                    "currentUsageWithPrecision": 9943.38,
                    "usageLimitWithPrecision": 10000.0
                }]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn parses_the_verified_live_shape() {
        let snap = to_snapshot(sample()).unwrap();
        assert_eq!(snap.plan, "KIRO POWER");
        assert_eq!(snap.used, 9943.38);
        assert_eq!(snap.limit, 10000.0);
        assert_eq!(
            snap.reset_at,
            Some(DateTime::from_timestamp(1785542400, 0).unwrap())
        );
    }

    #[test]
    fn picks_the_credit_bucket_when_multiple_are_present() {
        let mut resp = sample();
        resp.usage_breakdown_list.insert(
            0,
            UsageBreakdown {
                resource_type: Some("OTHER".into()),
                current_usage_with_precision: Some(1.0),
                usage_limit_with_precision: Some(2.0),
            },
        );
        let snap = to_snapshot(resp).unwrap();
        assert_eq!(snap.used, 9943.38);
    }

    #[test]
    fn falls_back_to_the_first_entry_with_no_resource_type() {
        let resp: UsageLimitsResponse = serde_json::from_str(
            r#"{
                "subscriptionInfo": { "subscriptionTitle": "KIRO POWER" },
                "usageBreakdownList": [{
                    "currentUsageWithPrecision": 5.0,
                    "usageLimitWithPrecision": 10.0
                }]
            }"#,
        )
        .unwrap();
        let snap = to_snapshot(resp).unwrap();
        assert_eq!(snap.used, 5.0);
        assert_eq!(snap.limit, 10.0);
    }

    #[test]
    fn explicit_non_credit_single_bucket_is_schema_drift() {
        let mut resp = sample();
        resp.usage_breakdown_list[0].resource_type = Some("OTHER".into());
        assert!(matches!(to_snapshot(resp), Err(AppError::Schema(_))));
    }

    #[test]
    fn multiple_unknown_buckets_are_schema_drift() {
        let mut resp = sample();
        resp.usage_breakdown_list = vec![
            UsageBreakdown {
                resource_type: None,
                current_usage_with_precision: Some(1.0),
                usage_limit_with_precision: Some(2.0),
            },
            UsageBreakdown {
                resource_type: Some("OTHER".into()),
                current_usage_with_precision: Some(3.0),
                usage_limit_with_precision: Some(4.0),
            },
        ];
        assert!(matches!(to_snapshot(resp), Err(AppError::Schema(_))));
    }

    #[test]
    fn missing_reset_is_none_not_an_error() {
        let mut resp = sample();
        resp.next_date_reset = None;
        let snap = to_snapshot(resp).unwrap();
        assert_eq!(snap.reset_at, None);
    }

    #[test]
    fn missing_plan_is_schema_drift() {
        let mut resp = sample();
        resp.subscription_info = None;
        assert!(matches!(to_snapshot(resp), Err(AppError::Schema(_))));
    }

    #[test]
    fn empty_breakdown_list_is_schema_drift() {
        let mut resp = sample();
        resp.usage_breakdown_list.clear();
        assert!(matches!(to_snapshot(resp), Err(AppError::Schema(_))));
    }

    #[test]
    fn non_finite_usage_is_schema_drift() {
        let mut resp = sample();
        resp.usage_breakdown_list[0].current_usage_with_precision = Some(f64::NAN);
        assert!(matches!(to_snapshot(resp), Err(AppError::Schema(_))));
    }

    #[test]
    fn user_info_email_is_read_without_the_user_id() {
        let resp: UsageLimitsResponse = serde_json::from_str(
            r#"{
                "subscriptionInfo": { "subscriptionTitle": "KIRO POWER" },
                "usageBreakdownList": [{
                    "resourceType": "CREDIT",
                    "currentUsageWithPrecision": 1.0,
                    "usageLimitWithPrecision": 2.0
                }],
                "userInfo": { "userId": "test-user-id", "email": "person@example.test" }
            }"#,
        )
        .unwrap();
        assert_eq!(
            resp.email.as_ref().map(AccountEmail::as_str),
            Some("person@example.test")
        );
        let debug = format!("{resp:?}");
        assert!(!debug.contains("person@example.test"));
        assert!(!debug.contains("test-user-id"));
        assert_eq!(to_snapshot(resp).unwrap().plan, "KIRO POWER");
    }

    #[test]
    fn unusable_user_info_never_fails_the_usage_response() {
        for info in [
            "null",
            "\"person@example.test\"",
            "[]",
            "{}",
            r#"{"email": null}"#,
            r#"{"email": 7}"#,
            r#"{"email": ""}"#,
            r#"{"email": "not-an-address"}"#,
            r#"{"userId": "test-user-id"}"#,
        ] {
            let body = format!(
                r#"{{"subscriptionInfo": {{"subscriptionTitle": "KIRO POWER"}},
                    "usageBreakdownList": [{{"resourceType": "CREDIT",
                        "currentUsageWithPrecision": 1.0, "usageLimitWithPrecision": 2.0}}],
                    "userInfo": {info}}}"#
            );
            let resp: UsageLimitsResponse = serde_json::from_str(&body).unwrap();
            assert_eq!(resp.email, None, "{info}");
            assert_eq!(to_snapshot(resp).unwrap().used, 1.0, "{info}");
        }
        // Absent entirely, as in the verified live shape.
        assert_eq!(sample().email, None);
    }

    #[test]
    fn email_is_not_inferred_from_a_top_level_or_nested_lookalike() {
        let resp: UsageLimitsResponse = serde_json::from_str(
            r#"{
                "email": "top@example.test",
                "subscriptionInfo": { "subscriptionTitle": "KIRO POWER", "email": "sub@example.test" },
                "usageBreakdownList": [{
                    "resourceType": "CREDIT",
                    "currentUsageWithPrecision": 1.0,
                    "usageLimitWithPrecision": 2.0
                }]
            }"#,
        )
        .unwrap();
        assert_eq!(resp.email, None);
    }

    #[test]
    fn negative_usage_is_schema_drift() {
        let mut resp = sample();
        resp.usage_breakdown_list[0].current_usage_with_precision = Some(-1.0);
        assert!(matches!(to_snapshot(resp), Err(AppError::Schema(_))));
    }
}
