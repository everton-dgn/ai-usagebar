//! Kiro CLI glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::KiroSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

pub fn severity(snap: &KiroSnapshot) -> PaceSeverity {
    severity_for(snap.pct())
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Kiro)
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

    fn sample_snap() -> KiroSnapshot {
        KiroSnapshot {
            plan: "KIRO POWER".into(),
            used: 9943.38,
            limit: 10000.0,
            reset_at: Some(now() + chrono::Duration::days(1)),
        }
    }

    #[test]
    fn severity_tracks_the_credit_percentage() {
        let mut snap = sample_snap();
        snap.used = 99.0;
        snap.limit = 100.0;
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
            crate::usage::VendorSnapshot::Kiro(_)
        ));
        assert!(vendor.stale);
    }
}
