//! The [`Sink`] trait: where [`RunReport`](crate::RunReport)s go after a run.
//!
//! Default implementations live alongside the CLI: a stdout JSON writer and
//! a file writer. The trait exists so that experiments and tests can route
//! reports elsewhere (e.g., into an in-memory `Vec`) without touching the
//! scheduler or runner.

use std::error::Error as StdError;

use crate::report::RunReport;

/// A destination for [`RunReport`]s.
pub trait Sink: Send + Sync {
    /// Sink-specific error type.
    type Error: StdError + Send + Sync + 'static;

    /// Record one report. Sinks must be idempotent or at least
    /// crash-tolerant: a partial write should not corrupt prior reports.
    fn record(&self, report: &RunReport) -> Result<(), Self::Error>;
}
