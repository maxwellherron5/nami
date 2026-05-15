//! The [`Scheduler`] trait.
//!
//! A scheduler converts a [`JobSpec`] plus a slice of [`ForecastPoint`]s
//! into a [`SchedulingDecision`]. It is synchronous: all I/O happens
//! upstream in the provider layer; by the time the scheduler runs, the
//! decision is a pure function of its inputs.

use time::OffsetDateTime;

use crate::decision::SchedulingDecision;
use crate::job::JobSpec;
use crate::observation::ForecastPoint;

/// A scheduling policy.
///
/// `now` is passed explicitly rather than read from the clock so that
/// scheduling is testable and deterministic. The materiality threshold
/// and any other policy knobs belong to the scheduler implementation,
/// not this trait.
pub trait Scheduler: Send + Sync {
    /// Decide when (or whether) to run the job.
    fn decide(
        &self,
        job: &JobSpec,
        forecast: &[ForecastPoint],
        now: OffsetDateTime,
    ) -> SchedulingDecision;
}
