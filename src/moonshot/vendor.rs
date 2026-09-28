//! Moonshot glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::usage::MoonshotSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// `available_balance <= 0` blocks the inference API, so that's critical.
/// Otherwise scale the low/high/mid thresholds by currency (CNY ≈ 7× USD),
/// mirroring DeepSeek.
pub fn severity(snap: &MoonshotSnapshot) -> PaceSeverity {
    if snap.available <= 0.0 {
        return PaceSeverity::Critical;
    }
    crate::pango::balance_severity(snap.available, &snap.currency)
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Moonshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::MoonshotSnapshot;

    fn sample_snap() -> MoonshotSnapshot {
        MoonshotSnapshot {
            available: 49.58,
            voucher: 46.58,
            cash: 3.0,
            currency: "USD".into(),
        }
    }

    #[test]
    fn zero_balance_is_critical() {
        let mut snap = sample_snap();
        snap.available = 0.0;
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }
}
