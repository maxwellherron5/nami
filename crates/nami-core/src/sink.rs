//! The [`Sink`] trait: where [`RunReport`](crate::RunReport)s go.
//!
//! Default implementations (stdout JSON, file) live in `nami-cli`. The
//! trait exists so tests and experiments can capture reports without
//! touching the scheduler or runner.

use std::error::Error as StdError;

use crate::report::RunReport;

/// A destination for [`RunReport`]s.
pub trait Sink: Send + Sync {
    /// Sink-specific error type.
    type Error: StdError + Send + Sync + 'static;

    /// Record one report. Sinks should be crash-tolerant: a partial write
    /// must not corrupt prior reports.
    fn record(&self, report: &RunReport) -> Result<(), Self::Error>;
}
