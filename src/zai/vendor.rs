//! Z.AI glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::ZaiSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

pub fn severity(snap: &ZaiSnapshot) -> PaceSeverity {
    let session = snap
        .session
        .as_ref()
        .map(|w| w.utilization_pct)
        .unwrap_or(0);
    let weekly = snap.weekly.as_ref().map(|w| w.utilization_pct).unwrap_or(0);
    let mcp = snap.mcp.as_ref().map(|w| w.utilization_pct).unwrap_or(0);
    severity_for([session, weekly, mcp].into_iter().max().unwrap_or(0))
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Zai)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{UsageWindow, ZaiSnapshot};
    use chrono::Utc;

    fn sample_snap() -> ZaiSnapshot {
        let now = Utc::now();
        ZaiSnapshot {
            plan: "GLM Coding Pro".into(),
            session: Some(UsageWindow {
                utilization_pct: 42,
                resets_at: Some(now + chrono::Duration::hours(2)),
                window_duration: chrono::Duration::hours(5),
            }),
            weekly: Some(UsageWindow {
                utilization_pct: 15,
                resets_at: Some(now + chrono::Duration::days(3)),
                window_duration: chrono::Duration::days(7),
            }),
            mcp: None,
        }
    }

    #[test]
    fn severity_picks_worst_window() {
        let mut snap = sample_snap();
        snap.weekly.as_mut().unwrap().utilization_pct = 95;
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }
}
