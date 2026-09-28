//! Model Studio glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::ModelStudioSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// The API reports no plan name; the label falls back to the vendor display
/// name, the single source frontends are allowed to carry.
pub const PLAN_LABEL: &str = "Model Studio";

/// Worst of the windows the account actually reports.
pub fn severity(snap: &ModelStudioSnapshot) -> PaceSeverity {
    let max = [snap.session.as_ref(), snap.weekly.as_ref()]
        .into_iter()
        .flatten()
        .map(|window| window.utilization_pct)
        .max()
        .unwrap_or(0);
    severity_for(max)
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::ModelStudio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modelstudio::types::FIVE_HOUR_WINDOW;
    use crate::usage::UsageWindow;
    use chrono::TimeZone;
    use chrono::{DateTime, Utc};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 23, 12, 0, 0).unwrap()
    }

    fn window(pct: i32, mins_ahead: i64) -> UsageWindow {
        UsageWindow {
            utilization_pct: pct,
            resets_at: Some(now() + chrono::Duration::minutes(mins_ahead)),
            window_duration: FIVE_HOUR_WINDOW,
        }
    }

    fn snap() -> ModelStudioSnapshot {
        ModelStudioSnapshot {
            session: Some(window(42, 90)),
            weekly: Some(UsageWindow {
                utilization_pct: 74,
                resets_at: Some(now() + chrono::Duration::days(3)),
                window_duration: chrono::Duration::days(7),
            }),
        }
    }

    #[test]
    fn severity_is_the_worst_reported_window() {
        assert_eq!(severity(&snap()), severity_for(74));
        let s = ModelStudioSnapshot {
            session: Some(window(97, 10)),
            weekly: Some(window(12, 300)),
        };
        assert_eq!(severity(&s), severity_for(97));
    }
}
