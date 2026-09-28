//! GitHub Copilot glue for the shared report: severity, projection constants and the
//! conversion of its fetch outcome into a [`VendorOutcome`](crate::vendor::VendorOutcome).

use crate::pacing::PaceSeverity;
use crate::pango::severity_for;
use crate::vendor::VendorOutcome;

use super::fetch::FetchOutcome;
use super::types::Snapshot;

impl From<FetchOutcome> for VendorOutcome {
    fn from(outcome: FetchOutcome) -> Self {
        outcome.map(crate::usage::VendorSnapshot::Copilot)
    }
}

pub fn severity(snap: &Snapshot) -> PaceSeverity {
    severity_for(snap.worst_pct())
}

#[cfg(test)]
mod tests {}
