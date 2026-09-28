//! Antigravity glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::usage::{AntigravitySnapshot, UsageWindow};

/// Model-group names, as Antigravity's own Model Quota screen labels them.
/// Both surfaces put these under a "Session"/"Weekly" heading, so the rows
/// carry no window suffix of their own.
pub const GROUP_PRIMARY: &str = "Gemini";
pub const GROUP_THIRD_PARTY: &str = "Claude & GPT OSS";

/// Every window this vendor reports, in dropdown order: the two 5-hour windows
/// under "Session", then the two weekly ones under "Weekly".
fn windows(snap: &AntigravitySnapshot) -> [(&'static str, Option<&UsageWindow>); 4] {
    [
        (GROUP_PRIMARY, snap.session.as_ref()),
        (GROUP_THIRD_PARTY, snap.third_party_session.as_ref()),
        (GROUP_PRIMARY, snap.weekly.as_ref()),
        (GROUP_THIRD_PARTY, snap.third_party_weekly.as_ref()),
    ]
}

/// Worst of **all four** windows. Grading on the Gemini pool alone would show a
/// calm panel while the Claude & GPT OSS pool is exhausted.
pub fn severity(snap: &AntigravitySnapshot) -> PaceSeverity {
    let worst = windows(snap)
        .iter()
        .filter_map(|(_, w)| w.map(|w| w.utilization_pct))
        .max()
        .unwrap_or(0);
    if worst >= 90 {
        PaceSeverity::Critical
    } else if worst >= 75 {
        PaceSeverity::High
    } else if worst >= 50 {
        PaceSeverity::Mid
    } else {
        PaceSeverity::Low
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::*;

    fn at(s: &str) -> Option<DateTime<Utc>> {
        Some(DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc))
    }

    fn window(pct: i32, resets: &str, weekly: bool) -> UsageWindow {
        UsageWindow {
            utilization_pct: pct,
            resets_at: at(resets),
            window_duration: if weekly {
                chrono::Duration::days(7)
            } else {
                chrono::Duration::hours(5)
            },
        }
    }

    fn snapshot() -> AntigravitySnapshot {
        AntigravitySnapshot {
            plan: "Google AI Pro".into(),
            account: "acct:test".into(),
            source: crate::usage::AntigravitySource::Local,
            session: Some(window(43, "2026-07-22T14:00:00Z", false)),
            weekly: Some(window(8, "2026-07-28T17:39:58Z", true)),
            third_party_session: Some(window(75, "2026-07-22T16:30:00Z", false)),
            third_party_weekly: Some(window(0, "2026-07-29T12:47:00Z", true)),
        }
    }

    #[test]
    fn severity_tracks_the_worst_of_all_four_windows() {
        let mut snap = snapshot();
        snap.session.as_mut().unwrap().utilization_pct = 0;
        snap.weekly.as_mut().unwrap().utilization_pct = 0;
        snap.third_party_session = Some(window(0, "2026-07-22T16:30:00Z", false));
        snap.third_party_weekly = Some(window(0, "2026-07-29T12:47:00Z", true));
        assert_eq!(severity(&snap), PaceSeverity::Low);

        // A third-party pool running dry must still raise the panel.
        snap.third_party_weekly = Some(window(95, "2026-07-29T12:47:00Z", true));
        assert_eq!(severity(&snap), PaceSeverity::Critical);

        snap.third_party_weekly = Some(window(60, "2026-07-29T12:47:00Z", true));
        assert_eq!(severity(&snap), PaceSeverity::Mid);
    }
}
