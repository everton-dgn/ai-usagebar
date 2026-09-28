//! OpenRouter glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::usage::OpenRouterSnapshot;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;

/// OpenRouter severity is keyed on consumed-percentage (low credit = critical),
/// with one case the percentage cannot express: an account that owes money.
/// `consumed_pct` needs `total_credits` as a denominator and reports 0 without
/// one, so an account that never bought credits but ran up usage would
/// otherwise show a debt in reassuring green.
pub fn severity(snap: &OpenRouterSnapshot) -> PaceSeverity {
    if snap.balance() < 0.0 {
        return PaceSeverity::Critical;
    }
    severity_for(snap.consumed_pct())
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Openrouter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::OpenRouterSnapshot;

    fn sample_snap() -> OpenRouterSnapshot {
        OpenRouterSnapshot {
            label: "OpenRouter — prod".into(),
            total_credits: 100.0,
            total_usage: 25.5,
            usage_daily: 1.0,
            usage_weekly: 7.0,
            usage_monthly: 25.5,
            is_free_tier: false,
            limit: Some(50.0),
            limit_remaining: Some(24.5),
        }
    }

    /// A free-tier account that has spent nothing is not in debt, and must
    /// stay green — the fix keys on the balance, not on "credits are zero".
    #[test]
    fn a_zero_credit_account_with_no_usage_stays_low() {
        let mut snap = sample_snap();
        snap.total_credits = 0.0;
        snap.total_usage = 0.0;
        assert_eq!(snap.balance(), 0.0);
        assert_eq!(severity(&snap), PaceSeverity::Low);
    }

    #[test]
    fn severity_keys_on_consumed_pct() {
        let mut snap = sample_snap();
        snap.total_usage = 92.0; // 92% consumed → critical
        assert_eq!(severity(&snap), PaceSeverity::Critical);
        snap.total_usage = 60.0; // mid
        assert_eq!(severity(&snap), PaceSeverity::Mid);
    }
}
