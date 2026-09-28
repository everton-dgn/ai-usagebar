//! DeepSeek glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::usage::DeepseekSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

pub fn severity(snap: &DeepseekSnapshot) -> PaceSeverity {
    if !snap.is_available {
        return PaceSeverity::Critical;
    }
    crate::pango::balance_severity(snap.balance, &snap.currency)
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Deepseek)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::money;
    use crate::usage::DeepseekSnapshot;

    fn sample_snap() -> DeepseekSnapshot {
        DeepseekSnapshot {
            is_available: true,
            balance: 5.50,
            granted: 5.00,
            topped_up: 0.50,
            currency: "USD".into(),
        }
    }

    #[test]
    fn unavailable_api_shows_critical_severity() {
        let mut snap = sample_snap();
        snap.is_available = false;
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }

    #[test]
    fn cny_format() {
        let snap = DeepseekSnapshot {
            is_available: true,
            balance: 20.0,
            granted: 20.0,
            topped_up: 0.0,
            currency: "CNY".into(),
        };
        assert_eq!(money(snap.balance, &snap.currency), "¥20.00");
    }
}
