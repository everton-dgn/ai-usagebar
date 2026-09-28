//! Kilo Code glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::usage::KiloSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// Kilo has no purchased-total on this endpoint, so severity keys on the
/// absolute remaining USD balance (mirrors DeepSeek's USD thresholds): running
/// low = warmer color, empty = critical (the `402` boundary).
pub fn severity(snap: &KiloSnapshot) -> PaceSeverity {
    crate::pango::balance_severity(snap.balance, "USD")
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Kilo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::KiloSnapshot;

    #[test]
    fn severity_scales_with_balance() {
        assert_eq!(
            severity(&KiloSnapshot {
                label: "".into(),
                balance: 0.5
            }),
            PaceSeverity::Critical
        );
        assert_eq!(
            severity(&KiloSnapshot {
                label: "".into(),
                balance: 3.0
            }),
            PaceSeverity::High
        );
        assert_eq!(
            severity(&KiloSnapshot {
                label: "".into(),
                balance: 12.0
            }),
            PaceSeverity::Mid
        );
        assert_eq!(
            severity(&KiloSnapshot {
                label: "".into(),
                balance: 50.0
            }),
            PaceSeverity::Low
        );
    }
}
