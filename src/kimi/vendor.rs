//! Kimi glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::KimiSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::{FetchOutcome, SCHEMA_DRIFT_MESSAGE};

/// Presentation classification for Kimi's legacy `(u16, String)` cached
/// diagnostic. Code zero has never meant HTTP; the stable schema marker lets
/// renderers distinguish an upstream response-shape change from other local
/// failures without changing the on-disk cache format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    Http(u16),
    SchemaDrift,
    Other,
}

pub fn warning_kind(code: u16, message: &str) -> WarningKind {
    if code != 0 {
        WarningKind::Http(code)
    } else if message == SCHEMA_DRIFT_MESSAGE {
        WarningKind::SchemaDrift
    } else {
        WarningKind::Other
    }
}

/// Kimi reports the weekly quota's reset instant but never its length; the
/// subscription bucket rolls every 7 days.
pub const WEEKLY_WINDOW: chrono::Duration = chrono::Duration::days(7);
/// The rolling bucket's length *is* advertised — 300 minutes — and only that
/// spelling is accepted on the way in (`types::is_five_hour_window`).
pub const ROLLING_WINDOW: chrono::Duration = chrono::Duration::hours(5);

pub fn severity(snap: &KimiSnapshot) -> PaceSeverity {
    severity_for(snap.worst_pct())
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Kimi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::{DateTime, Utc};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 7, 12, 0, 0).unwrap()
    }

    fn sample_snap() -> KimiSnapshot {
        KimiSnapshot {
            plan: Some("LEVEL_INTERMEDIATE".into()),
            weekly_limit: 100,
            weekly_used: 26,
            weekly_remaining: 74,
            weekly_reset_at: Some(now() + chrono::Duration::days(4)),
            has_weekly: true,
            monthly_pct: None,
            monthly_reset_at: None,
            window_limit: 100,
            window_used: 15,
            window_remaining: 85,
            window_reset_at: Some(now() + chrono::Duration::hours(2)),
        }
    }

    /// An account on the newer `usages`-map shape: no weekly bucket, a
    /// monthly pool instead.
    fn monthly_snap() -> KimiSnapshot {
        KimiSnapshot {
            plan: Some("Allegretto".into()),
            weekly_limit: 0,
            weekly_used: 0,
            weekly_remaining: 0,
            weekly_reset_at: None,
            has_weekly: false,
            monthly_pct: Some(42),
            monthly_reset_at: Some(now() + chrono::Duration::days(30)),
            window_limit: 100,
            window_used: 15,
            window_remaining: 85,
            window_reset_at: Some(now() + chrono::Duration::hours(2)),
        }
    }

    #[test]
    fn zero_limits_are_low() {
        let snap = KimiSnapshot {
            weekly_limit: 0,
            weekly_used: 0,
            weekly_remaining: 0,
            window_limit: 0,
            window_used: 0,
            window_remaining: 0,
            ..sample_snap()
        };
        assert_eq!(severity(&snap), PaceSeverity::Low);
    }

    #[test]
    fn warning_kind_uses_schema_marker_without_treating_code_zero_as_http() {
        assert_eq!(
            warning_kind(0, SCHEMA_DRIFT_MESSAGE),
            WarningKind::SchemaDrift
        );
        assert_eq!(
            warning_kind(0, "cache lock unavailable"),
            WarningKind::Other
        );
        assert_eq!(warning_kind(503, "unavailable"), WarningKind::Http(503));
    }

    #[test]
    fn fetch_outcome_conversion_preserves_metadata() {
        let snap = sample_snap();
        let fetch = FetchOutcome {
            email: None,
            snapshot: snap.clone(),
            stale: true,
            last_error: Some((401, "bad".into())),
            cache_age: Some(std::time::Duration::from_secs(42)),
        };
        let vendor: VendorOutcome = fetch.into();
        assert!(matches!(
            vendor.snapshot,
            crate::usage::VendorSnapshot::Kimi(_)
        ));
        assert!(vendor.stale);
        assert_eq!(vendor.last_error, Some((401, "bad".into())));
        assert_eq!(vendor.cache_age, Some(std::time::Duration::from_secs(42)));
    }

    #[test]
    fn severity_worst_of_windows() {
        let mut snap = sample_snap();
        snap.weekly_used = 10;
        snap.weekly_remaining = 90;
        snap.window_used = 95;
        snap.window_remaining = 5;
        // 95% window should drive severity to Critical even though weekly is Low.
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }

    #[test]
    fn severity_is_the_max_over_the_present_windows() {
        let mut snap = monthly_snap();
        snap.window_used = 10;
        snap.window_remaining = 90;
        snap.monthly_pct = Some(95);
        // The monthly pool alone can drive severity.
        assert_eq!(severity(&snap), PaceSeverity::Critical);
        snap.monthly_pct = None;
        assert_eq!(severity(&snap), PaceSeverity::Low);
    }
}
