//! Errors emitted by `nami-scheduler`.

use thiserror::Error;

/// A scheduler error.
///
/// Note: a legitimate "we refuse to schedule" outcome is **not** an error —
/// it is encoded as [`nami_core::SchedulingDecision::Refuse`]. The variants
/// here cover only unrecoverable bugs in scheduler implementations.
#[derive(Debug, Error)]
pub enum Error {
    /// A forecast slice was malformed in a way that violated scheduler
    /// invariants (e.g., unsorted timestamps when sorting was required).
    #[error("malformed forecast: {0}")]
    MalformedForecast(String),
}

/// Result alias for the scheduler.
pub type Result<T> = std::result::Result<T, Error>;
