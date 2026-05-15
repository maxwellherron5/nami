//! The [`Scheduler`] trait.
//!
//! A scheduler converts a [`JobSpec`] plus a slice of [`ForecastPoint`]s
//! into a [`SchedulingDecision`]. It is synchronous: by the time a scheduler
//! is invoked, all necessary I/O has been done.

use crate::carbon::ForecastPoint;
use crate::decision::SchedulingDecision;
use crate::job::JobSpec;
use time::OffsetDateTime;

/// A scheduling policy.
///
/// Phase 0 ships `BestWindowScheduler` (in `nami-scheduler`), which picks the
/// contiguous window with minimum mean forecast intensity that fits before
/// the deadline. Additional policies (e.g., "as late as possible", "first
/// window below threshold X") can be added later.
pub trait Scheduler: Send + Sync {
    /// Decide when to run the job.
    ///
    /// `now` is passed explicitly rather than read from the clock so that
    /// scheduling is testable and deterministic.
    fn decide(
        &self,
        job: &JobSpec,
        forecast: &[ForecastPoint],
        now: OffsetDateTime,
    ) -> SchedulingDecision;
}
