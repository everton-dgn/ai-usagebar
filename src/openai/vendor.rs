//! OpenAI glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::OpenAiSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

pub fn severity(snap: &OpenAiSnapshot) -> PaceSeverity {
    let windows = [
        snap.session.as_ref(),
        snap.weekly.as_ref(),
        snap.code_review.as_ref(),
    ];
    let max = windows
        .into_iter()
        .flatten()
        .map(|window| window.utilization_pct)
        .max()
        .unwrap_or(0);
    severity_for(max)
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Openai)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{OpenAiSnapshot, OpenAiSource, UsageWindow};
    use chrono::Utc;

    fn sample() -> OpenAiSnapshot {
        OpenAiSnapshot {
            plan: "ChatGPT Plus".into(),
            session: Some(UsageWindow {
                utilization_pct: 1,
                resets_at: Some(Utc::now() + chrono::Duration::hours(5)),
                window_duration: chrono::Duration::hours(5),
            }),
            weekly: Some(UsageWindow {
                utilization_pct: 0,
                resets_at: Some(Utc::now() + chrono::Duration::days(7)),
                window_duration: chrono::Duration::days(7),
            }),
            code_review: None,
            additional_limits: Vec::new(),
            unavailable_models: Vec::new(),
            credits: None,
            reset_credits: Default::default(),
            source: OpenAiSource::CodexOauth,
        }
    }

    #[test]
    fn severity_picks_worst_window() {
        let mut s = sample();
        s.weekly.as_mut().unwrap().utilization_pct = 95;
        assert_eq!(severity(&s), PaceSeverity::Critical);
    }
}
