//! xAI (Grok) glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::usage::GrokSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// Prepaid credit: running low = warmer, empty/negative = critical.
pub fn severity(snap: &GrokSnapshot) -> PaceSeverity {
    crate::pango::balance_severity(snap.balance, "USD")
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Grok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::GrokSnapshot;

    fn outcome(balance: f64) -> (GrokSnapshot, VendorOutcome) {
        let snap = GrokSnapshot { balance };
        let o = VendorOutcome {
            email: None,
            snapshot: crate::usage::VendorSnapshot::Grok(snap.clone()),
            stale: false,
            last_error: None,
            cache_age: Some(std::time::Duration::from_secs(10)),
        };
        (snap, o)
    }

    #[test]
    fn low_balance_is_critical() {
        let (snap, _) = outcome(0.5);
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }
}
