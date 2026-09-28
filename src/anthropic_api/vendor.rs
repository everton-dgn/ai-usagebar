//! Anthropic Admin API glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::AnthropicApiSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// Severity keys on the spend-vs-limit %. With no limit there's no signal, so
/// it stays calm (low).
pub fn severity(snap: &AnthropicApiSnapshot) -> PaceSeverity {
    match snap.pct() {
        Some(p) => severity_for(p.min(100)),
        None => PaceSeverity::Low,
    }
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::AnthropicApi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::AnthropicApiSnapshot;

    #[test]
    fn severity_scales_with_spend_pct() {
        assert_eq!(
            severity(&AnthropicApiSnapshot {
                spent: 950.0,
                limit: Some(1000.0)
            }),
            PaceSeverity::Critical
        );
        assert_eq!(
            severity(&AnthropicApiSnapshot {
                spent: 1.0,
                limit: Some(1000.0)
            }),
            PaceSeverity::Low
        );
        assert_eq!(
            severity(&AnthropicApiSnapshot {
                spent: 500.0,
                limit: None
            }),
            PaceSeverity::Low
        );
    }
}
