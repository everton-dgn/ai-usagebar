//! SuperGrok glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::SuperGrokSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

pub fn severity(snap: &SuperGrokSnapshot) -> PaceSeverity {
    severity_for(snap.weekly_pct)
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::SuperGrok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::SuperGrokPeriod;
    use chrono::TimeZone;
    use chrono::{DateTime, Utc};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 5, 12, 0, 0).unwrap()
    }

    fn sample_snap() -> SuperGrokSnapshot {
        SuperGrokSnapshot {
            plan: "SuperGrok".into(),
            account: "user-1".into(),
            weekly_pct: 34,
            period: SuperGrokPeriod::Weekly,
            reset_at: Some(now() + chrono::Duration::hours(20)),
            prepaid_balance: Some(0.0),
            reset_credits: Default::default(),
            products: Vec::new(),
        }
    }

    #[test]
    fn high_usage_is_critical() {
        let mut snap = sample_snap();
        snap.weekly_pct = 95;
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }
}
