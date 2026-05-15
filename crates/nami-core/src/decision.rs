//! The scheduler's output: a decision to start at a specific time, or a
//! refusal with a stated reason.
//!
//! These types are part of the trait surface between `nami-scheduler` and
//! `nami-cli`. They are intentionally small and explicit: every reason for
//! starting now versus deferring, and every reason for refusing to schedule,
//! is named.

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::carbon::CarbonIntensity;
use crate::region::Region;

/// Why the scheduler decided to start at a particular instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartReason {
    /// The chosen window is the cleanest contiguous window before the deadline.
    CleanestWindow,
    /// No forecast data was available; running immediately is the honest
    /// fallback.
    NoForecastFellBackToNow,
    /// The deadline is so tight that no deferral was possible.
    DeadlineTooTight,
}

/// Why the scheduler refused to produce a schedule at all.
///
/// Refusal is a first-class outcome: per the design principle of not
/// silently estimating, the scheduler must refuse rather than guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum RefuseReason {
    /// The region was unsupported by every available provider.
    RegionUnsupported(Region),
    /// All providers failed and `--strict` (or equivalent) was set, so we
    /// refuse rather than fall back to "run now."
    AllProvidersFailed(String),
    /// The job spec itself was invalid (caught by the scheduler rather than
    /// at parse time).
    InvalidSpec(String),
}

/// The scheduler's verdict on a [`JobSpec`](crate::JobSpec).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SchedulingDecision {
    /// Run the job, starting at `start`.
    Run {
        /// UTC instant at which the wrapped command should be spawned.
        start: OffsetDateTime,
        /// Expected duration; the scheduler echoes back the job spec's
        /// estimate so downstream code does not need to look it up.
        duration: Duration,
        /// Why this start time was chosen.
        reason: StartReason,
        /// The mean expected intensity over the chosen window, if known.
        expected_mean_intensity: Option<CarbonIntensity>,
    },
    /// Do not schedule. The caller must surface this loudly to the user.
    Refuse(RefuseReason),
}
