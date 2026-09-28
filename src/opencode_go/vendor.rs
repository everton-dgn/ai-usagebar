//! OpenCode Go glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).
use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;
use super::types::Usage;

/// Window lengths for pacing math. The usage endpoint reports only `status`,
/// `percent`, and `resetsAt` — never a duration — so these are constants with
/// a recorded provenance:
/// - `rolling` is the 5-hour limit (`packages/console/app/src/routes/zen/go/v1/usage.ts`
///   `formatUsage` + `packages/console/app/src/routes/zen/util/handler.ts` and
///   `i18n/en.ts` "5-hour usage limit reached", validated live via
///   `GET https://opencode.ai/zen/go/v1/usage`).
/// - `weekly` resets Monday 00:00 UTC (7 days, same response capture).
///
/// There is deliberately no monthly constant: the monthly window follows the
/// subscription cycle (`getMonthlyBounds(now, timeSubscribed)` upstream —
/// 28/29/31-day months depending on the subscriber), so any fixed length
/// would pace against a wrong denominator and publish a wrong `window_secs`.
/// The monthly reset is still shown; only pacing is omitted until the API
/// reports real cycle bounds.
pub const ROLLING_WINDOW: chrono::Duration = chrono::Duration::hours(5);
pub const WEEKLY_WINDOW: chrono::Duration = chrono::Duration::days(7);

impl From<FetchOutcome> for VendorOutcome {
    fn from(outcome: FetchOutcome) -> Self {
        outcome.map(crate::usage::VendorSnapshot::OpenCodeGo)
    }
}

pub fn severity(usage: &Usage) -> PaceSeverity {
    usage
        .rolling
        .iter()
        .chain(usage.weekly.iter())
        .chain(usage.monthly.iter())
        .map(|window| window.percent as i32)
        .max()
        .map(severity_for)
        .unwrap_or(PaceSeverity::Low)
}

#[cfg(test)]
mod tests {}
