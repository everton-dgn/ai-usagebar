//! Command Code glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).
use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;
use super::types::Snapshot;

impl From<FetchOutcome> for VendorOutcome {
    fn from(outcome: FetchOutcome) -> Self {
        outcome.map(crate::usage::VendorSnapshot::CommandCode)
    }
}

pub fn severity(snap: &Snapshot) -> PaceSeverity {
    severity_for(snap.worst_pct())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commandcode::types::{Credits, Snapshot, SpendWindow};
    use chrono::{DateTime, Utc};

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().expect("RFC3339 timestamp")
    }

    fn sample() -> Snapshot {
        Snapshot {
            plan: Some("GOAT".into()),
            five_hour: Some(SpendWindow {
                used: 1.23,
                cap: 14.0,
                resets_at: Some(at("2026-08-27T04:40:19Z")),
            }),
            weekly: Some(SpendWindow {
                used: 5.24,
                cap: 35.0,
                resets_at: Some(at("2026-09-02T18:36:12Z")),
            }),
            credits: Some(Credits {
                monthly: 49.28,
                purchased: 0.0,
                free: 0.0,
            }),
            credit_pool: Some(70.0),
            period_end: Some(at("2026-09-17T14:28:52Z")),
        }
    }

    #[test]
    fn monthly_window_needs_ledger_and_a_recognised_plan() {
        // No ledger: nothing to derive the spend from.
        let no_ledger = Snapshot {
            credits: None,
            credit_pool: Some(70.0),
            period_end: Some(at("2026-09-17T14:28:52Z")),
            ..sample()
        };
        assert!(no_ledger.monthly_window().is_none());
        assert_eq!(no_ledger.worst_pct(), 15);

        // No pool: no denominator.
        let no_pool = Snapshot {
            credit_pool: None,
            ..sample()
        };
        assert!(no_pool.monthly_window().is_none());
    }

    #[test]
    fn severity_follows_the_window_closest_to_its_cap() {
        let mut snapshot = sample();
        assert_eq!(severity(&snapshot), severity_for(15));

        snapshot.weekly = Some(SpendWindow {
            used: 34.0,
            cap: 35.0,
            resets_at: None,
        });
        assert_eq!(severity(&snapshot), severity_for(97));
    }
}
