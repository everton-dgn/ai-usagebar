//! Novita AI glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::usage::NovitaSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// Severity keys on the absolute remaining USD balance (same thresholds as
/// Kilo/DeepSeek): running low = warmer color, empty = critical.
pub fn severity(snap: &NovitaSnapshot) -> PaceSeverity {
    crate::pango::balance_severity(snap.available, "USD")
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Novita)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::NovitaSnapshot;

    #[test]
    fn severity_scales_with_balance() {
        let mk = |b: f64| NovitaSnapshot {
            available: b,
            cash: 0.0,
            credit_limit: 0.0,
            outstanding: 0.0,
        };
        assert_eq!(severity(&mk(0.5)), PaceSeverity::Critical);
        assert_eq!(severity(&mk(3.0)), PaceSeverity::High);
        assert_eq!(severity(&mk(12.0)), PaceSeverity::Mid);
        assert_eq!(severity(&mk(50.0)), PaceSeverity::Low);
    }
}
