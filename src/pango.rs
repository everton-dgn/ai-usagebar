//! Severity tiers for usage percentages and prepaid balances, shared by the
//! report projection and notifications.

use crate::pacing::PaceSeverity;

/// Map a usage percentage to a severity tier, matching `color_for`
/// (claudebar:198-205):
///   >= 90 → critical (red); >= 75 → high (orange);
///   >= 50 → mid (yellow); else low (green).
pub fn severity_for(pct: i32) -> PaceSeverity {
    if pct >= 90 {
        PaceSeverity::Critical
    } else if pct >= 75 {
        PaceSeverity::High
    } else if pct >= 50 {
        PaceSeverity::Mid
    } else {
        PaceSeverity::Low
    }
}

/// Map a *remaining balance* to a severity tier — the prepaid-credit analogue
/// of [`severity_for`], which keys on a percentage.
///
/// Balance vendors have no denominator to take a percentage of, so the tiers
/// are absolute amounts and therefore currency-dependent: CNY is scaled ≈7×
/// USD (rough parity). An unknown currency is treated as USD-scale, which is
/// the same choice `format::with_currency` makes for an unknown code.
///
/// Five vendors carried their own copy of this ladder, two of them including
/// the CNY row. A "low balance" must not mean ¥5 in one panel and $5 in
/// another, so the numbers live here once. Callers keep their own
/// *pre*-checks — DeepSeek's `is_available` and Moonshot's `<= 0` shortcut are
/// about the account, not the amount.
pub fn balance_severity(balance: f64, currency: &str) -> PaceSeverity {
    let (critical, high, mid) = match currency {
        "CNY" => (7.0_f64, 35.0, 140.0),
        _ => (1.0_f64, 5.0, 20.0),
    };
    if balance < critical {
        PaceSeverity::Critical
    } else if balance < high {
        PaceSeverity::High
    } else if balance < mid {
        PaceSeverity::Mid
    } else {
        PaceSeverity::Low
    }
}

#[cfg(test)]
mod tests {

    /// Five vendors carried this ladder. Grok, Kilo and Novita bill only in
    /// USD; DeepSeek and Moonshot also sell in CNY, and the two of them had
    /// the scaled row while the other three did not. Pin both rows so a
    /// "running low" balance cannot come to mean two different amounts.
    #[test]
    fn the_balance_ladder_is_one_table_in_both_currencies() {
        use crate::pacing::PaceSeverity::*;
        for (balance, expected) in [
            (-1.0, Critical),
            (0.0, Critical),
            (0.99, Critical),
            (1.0, High),
            (4.99, High),
            (5.0, Mid),
            (19.99, Mid),
            (20.0, Low),
        ] {
            assert_eq!(balance_severity(balance, "USD"), expected, "USD {balance}");
            // An unknown code is USD-scale, matching format::with_currency.
            assert_eq!(balance_severity(balance, "SEK"), expected, "SEK {balance}");
        }
        for (balance, expected) in [
            (6.99, Critical),
            (7.0, High),
            (34.99, High),
            (35.0, Mid),
            (139.99, Mid),
            (140.0, Low),
        ] {
            assert_eq!(balance_severity(balance, "CNY"), expected, "CNY {balance}");
        }
    }
    use super::*;

    #[test]
    fn severity_thresholds_match_claudebar() {
        assert_eq!(severity_for(0), PaceSeverity::Low);
        assert_eq!(severity_for(49), PaceSeverity::Low);
        assert_eq!(severity_for(50), PaceSeverity::Mid);
        assert_eq!(severity_for(74), PaceSeverity::Mid);
        assert_eq!(severity_for(75), PaceSeverity::High);
        assert_eq!(severity_for(89), PaceSeverity::High);
        assert_eq!(severity_for(90), PaceSeverity::Critical);
        assert_eq!(severity_for(100), PaceSeverity::Critical);
    }
}
