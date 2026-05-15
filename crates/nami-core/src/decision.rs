//! Scheduler output: a decision to run at a specific time, run immediately,
//! or refuse with a stated reason.
//!
//! Per `CLAUDE.md`, refusing is a first-class outcome — the scheduler must
//! refuse rather than invent numbers when data is missing, stale, sparse,
//! or below the materiality threshold.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::confidence::Confidence;

/// Why the scheduler picked a start time (or chose to run immediately).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartReason {
    /// The selected window has the lowest estimated mean intensity among
    /// candidates and beats run-now by at least the materiality threshold.
    LowestEstimatedIntensity,
    /// Running now is already the cleanest option among candidates.
    RunNowAlreadyCleanest,
    /// The deadline leaves no room to defer.
    DeadlineTooSoon,
    /// A fallback policy (e.g., no provider available) defaulted to
    /// running immediately.
    FallbackPolicyRunImmediately,
    /// The user passed an explicit override flag.
    UserForced,
}

/// Why the scheduler refused to produce a schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum RefuseReason {
    /// No provider declared support for the requested region.
    UnsupportedRegion,
    /// Historical data is required for the forecast model but is missing.
    MissingHistoricalData,
    /// The historical cache is older than the configured staleness bound.
    StaleHistoricalCache,
    /// Too few samples to produce a defensible estimate.
    InsufficientSamples,
    /// All providers (or the only required one) failed.
    ProviderUnavailable,
    /// No candidate window fits before the deadline.
    NoWindowBeforeDeadline,
    /// Best candidate's improvement over run-now is below the materiality
    /// threshold.
    CandidateWindowsBelowMaterialityThreshold,
    /// Forecast confidence is too low to justify a recommendation.
    ForecastTooUncertain,
}

/// The scheduler's verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SchedulingDecision {
    /// Defer the run to `start_time`.
    StartAt {
        /// UTC instant at which the wrapped command should be spawned.
        start_time: OffsetDateTime,
        /// Why this start time was chosen.
        reason: StartReason,
        /// Confidence in the decision.
        confidence: Confidence,
    },
    /// Run immediately. This is a positive outcome (not an error) — it
    /// just means the scheduler couldn't find a materially cleaner window.
    StartImmediately {
        /// Why running immediately is the right call.
        reason: StartReason,
        /// Confidence in the decision (often `Low` here — a fallback to
        /// run-now means the model didn't have enough signal to defer).
        confidence: Confidence,
    },
    /// Refuse to schedule. The caller must surface this loudly to the
    /// user — refusal is the honest answer, not an error to swallow.
    Refuse {
        /// Why no schedule was produced.
        reason: RefuseReason,
    },
}
