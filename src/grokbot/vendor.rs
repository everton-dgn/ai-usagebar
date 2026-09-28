//! Grok Bot glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::GrokbotSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// No included allowance reads as Low: there is no pool to be exhausting.
pub fn severity(snap: &GrokbotSnapshot) -> PaceSeverity {
    if snap.has_included_allowance {
        severity_for(snap.weekly_pct)
    } else {
        PaceSeverity::Low
    }
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Grokbot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::{DateTime, Utc};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap()
    }

    fn sample_snap() -> GrokbotSnapshot {
        GrokbotSnapshot {
            plan: "Grok Bot Plan".into(),
            has_included_allowance: true,
            weekly_pct: 42,
            has_available_usage: true,
            on_demand_enabled: false,
            period_start: Some(now() - chrono::Duration::days(3)),
            reset_at: Some(now() + chrono::Duration::days(4)),
            window: Some(chrono::Duration::days(7)),
        }
    }

    #[test]
    fn fetch_outcome_conversion_preserves_metadata() {
        let snap = sample_snap();
        let fetch = FetchOutcome {
            email: None,
            snapshot: snap,
            stale: true,
            last_error: Some((401, "bad".into())),
            cache_age: Some(std::time::Duration::from_secs(42)),
        };
        let vendor: VendorOutcome = fetch.into();
        assert!(matches!(
            vendor.snapshot,
            crate::usage::VendorSnapshot::Grokbot(_)
        ));
        assert!(vendor.stale);
        assert_eq!(vendor.last_error, Some((401, "bad".into())));
        assert_eq!(vendor.cache_age, Some(std::time::Duration::from_secs(42)));
    }
}
