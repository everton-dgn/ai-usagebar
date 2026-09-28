//! Cursor glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::CursorSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// Severity keys on the binding pool. An unlimited plan has no cap, so it stays
/// calm regardless of the (meaningless) percentages.
pub fn severity(snap: &CursorSnapshot) -> PaceSeverity {
    if snap.unlimited {
        PaceSeverity::Low
    } else {
        severity_for(snap.worst_pct())
    }
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::{DateTime, Utc};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 26, 12, 0, 0).unwrap()
    }

    fn sample_snap() -> CursorSnapshot {
        CursorSnapshot {
            plan: "Ultra".into(),
            auto_pct: 98,
            api_pct: 100,
            total_pct: 99,
            unlimited: false,
            on_demand_enabled: false,
            on_demand_used_cents: None,
            on_demand_limit_cents: None,
            reset_at: Some(now() + chrono::Duration::days(9)),
            cycle_start: None,
        }
    }

    #[test]
    fn severity_keys_on_the_worst_pool() {
        let mut snap = sample_snap();
        snap.auto_pct = 10;
        snap.api_pct = 95;
        // Other Models at 95% must drive severity Critical even though Cursor
        // Models is calm.
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }

    #[test]
    fn fetch_outcome_conversion_preserves_metadata() {
        let fetch = FetchOutcome {
            email: None,
            snapshot: sample_snap(),
            stale: true,
            last_error: Some((401, "bad".into())),
            cache_age: Some(std::time::Duration::from_secs(42)),
        };
        let vendor: VendorOutcome = fetch.into();
        assert!(matches!(
            vendor.snapshot,
            crate::usage::VendorSnapshot::Cursor(_)
        ));
        assert!(vendor.stale);
    }
}
