//! OrcaRouter glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::usage::OrcaRouterSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// Severity keys on the remaining credit (the actionable number, mirroring
/// the balance vendors); an unlimited key has no remaining figure, so it
/// stays calm — the same treatment a missing monthly limit gets.
pub fn severity(snap: &OrcaRouterSnapshot) -> PaceSeverity {
    match snap.remaining_usd() {
        Some(remaining) => crate::pango::balance_severity(remaining, "USD"),
        None => PaceSeverity::Low,
    }
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::OrcaRouter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::OrcaRouterSnapshot;

    #[test]
    fn severity_scales_with_remaining() {
        assert_eq!(
            severity(&OrcaRouterSnapshot {
                spent_cents: 11950,
                limit_cents: Some(12000),
                access_until: None
            }),
            PaceSeverity::Critical
        );
        assert_eq!(
            severity(&OrcaRouterSnapshot {
                spent_cents: 11600,
                limit_cents: Some(12000),
                access_until: None
            }),
            PaceSeverity::High
        );
        assert_eq!(
            severity(&OrcaRouterSnapshot {
                spent_cents: 0,
                limit_cents: Some(12000),
                access_until: None
            }),
            PaceSeverity::Low
        );
    }
}
