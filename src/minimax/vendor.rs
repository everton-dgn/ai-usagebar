//! MiniMax glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::MinimaxSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// The text & coding pool (`general` on the wire) — what the bars represent.
pub const POOL_GENERAL: &str = "Text";
/// The video-generation pool (`video` on the wire), shown when the plan has it.
pub const POOL_VIDEO: &str = "Video";

/// Worst of the two text-pool windows. The video pool deliberately does not
/// drive the bar color: running out of video quota should not paint the coding
/// bar red.
pub fn severity(snap: &MinimaxSnapshot) -> PaceSeverity {
    severity_for(
        snap.session
            .utilization_pct
            .max(snap.weekly.utilization_pct),
    )
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Minimax)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::UsageWindow;
    use chrono::TimeZone;
    use chrono::{DateTime, Utc};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 27, 12, 0, 0).unwrap()
    }

    fn window(pct: i32, mins_ahead: i64, dur: chrono::Duration) -> UsageWindow {
        UsageWindow {
            utilization_pct: pct,
            resets_at: Some(now() + chrono::Duration::minutes(mins_ahead)),
            window_duration: dur,
        }
    }

    fn snap() -> MinimaxSnapshot {
        MinimaxSnapshot {
            plan: "MiniMax Token Plan".to_string(),
            session: window(31, 45, chrono::Duration::hours(5)),
            weekly: window(62, 3000, chrono::Duration::days(7)),
            video_session: Some(window(5, 200, chrono::Duration::hours(24))),
            video_weekly: Some(window(9, 3000, chrono::Duration::days(7))),
        }
    }

    /// The video pool must not drag the coding bar into red.
    #[test]
    fn severity_ignores_the_video_pool() {
        let mut s = snap();
        s.session.utilization_pct = 10;
        s.weekly.utilization_pct = 10;
        s.video_session = Some(window(99, 10, chrono::Duration::hours(24)));
        s.video_weekly = Some(window(99, 10, chrono::Duration::days(7)));
        assert_eq!(severity(&s), severity_for(10));
    }
}
