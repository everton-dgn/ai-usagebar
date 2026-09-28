//! Ollama Cloud glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::OllamaSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

pub fn severity(snap: &OllamaSnapshot) -> PaceSeverity {
    // Worst of the three windows — session fills fastest and is the one
    // that actually interrupts a chat mid-stream.
    let session = snap
        .session
        .as_ref()
        .map(|w| w.utilization_pct)
        .unwrap_or(0);
    let weekly = snap.weekly.as_ref().map(|w| w.utilization_pct).unwrap_or(0);
    let monthly = snap
        .monthly
        .as_ref()
        .map(|w| w.utilization_pct)
        .unwrap_or(0);
    severity_for(session.max(weekly).max(monthly))
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Ollama)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{OllamaModelUsage, OllamaSnapshot, UsageWindow};

    fn sample_snap() -> OllamaSnapshot {
        OllamaSnapshot {
            plan: "pro".into(),
            session: Some(UsageWindow {
                utilization_pct: 82,
                resets_at: None,
                window_duration: chrono::Duration::hours(5),
            }),
            weekly: Some(UsageWindow {
                utilization_pct: 23,
                resets_at: None,
                window_duration: chrono::Duration::days(7),
            }),
            monthly: None,
            session_models: vec![OllamaModelUsage {
                name: "kimi-k3".into(),
                request_count: 180,
            }],
            weekly_models: vec![
                OllamaModelUsage {
                    name: "kimi-k3".into(),
                    request_count: 180,
                },
                OllamaModelUsage {
                    name: "minimax-m3".into(),
                    request_count: 554,
                },
            ],
            monthly_models: vec![],
            activity_cost: Some("0.00000".into()),
            activity_period: Some("last_4_weeks".into()),
        }
    }

    #[test]
    fn severity_tracks_worst_window() {
        assert_eq!(severity(&sample_snap()), severity_for(82));
    }
}
