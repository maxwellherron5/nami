//! The end-of-run report.
//!
//! [`RunReport`] is the durable, auditable record of one `nami` invocation:
//! the inputs, the scheduling decision, what data was used, the
//! methodology version, the materiality threshold in effect, and what
//! actually happened when (or if) the child process ran.

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::carbon::CarbonIntensity;
use crate::confidence::{Confidence, DataFreshness};
use crate::decision::SchedulingDecision;
use crate::provider::ProviderInfo;
use crate::region::Region;

/// A captured estimate of intensity over a specific window, used for both
/// the "run now" baseline and the "selected window" outcome in a report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowEstimate {
    /// UTC start of the window.
    #[serde(with = "time::serde::rfc3339")]
    pub start: OffsetDateTime,
    /// Length of the window.
    #[serde(with = "crate::duration_secs")]
    pub duration: Duration,
    /// Duration-weighted mean intensity across the window.
    pub mean_intensity: CarbonIntensity,
    /// Confidence in this window estimate.
    pub confidence: Confidence,
}

/// The auditable record of one `nami` invocation.
///
/// Every number that ends up in a report must be traceable: the
/// methodology label says which version of the math produced it; the
/// provider info says where the underlying data came from; the freshness
/// field says whether the data was live, stale, or fallback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    // -- Inputs --
    /// The command path (`command[0]`).
    pub command: String,
    /// Arguments after the command path.
    pub args: Vec<String>,
    /// Grid region used.
    pub region: Region,
    /// User-supplied deadline for the job to finish.
    #[serde(with = "time::serde::rfc3339")]
    pub deadline: OffsetDateTime,
    /// User-supplied estimated duration of the job.
    #[serde(with = "crate::duration_secs")]
    pub estimated_duration: Duration,

    // -- Decision --
    /// Full scheduling decision, including reason and confidence.
    pub decision: SchedulingDecision,
    /// Estimate of running immediately (the baseline).
    pub run_now_estimate: Option<WindowEstimate>,
    /// Estimate for the selected window, if a window was selected.
    pub selected_window_estimate: Option<WindowEstimate>,
    /// Estimated relative improvement vs. run-now, in percent
    /// (`(run_now - selected) / run_now × 100`). `None` if no window
    /// was selected.
    pub estimated_improvement_pct: Option<f64>,
    /// Materiality threshold (improvement percent) in effect for this
    /// decision.
    pub materiality_threshold_pct: f64,

    // -- Provenance --
    /// Provider that produced the underlying data.
    pub provider: ProviderInfo,
    /// Freshness state of the data used.
    pub data_freshness: DataFreshness,
    /// Methodology label tying the report to a specific version of the
    /// derivation and forecasting code.
    pub methodology_version: String,
    /// Any warnings the scheduler or runner wants to surface to the user
    /// (e.g., "static fallback used", "missing hours in cache").
    pub warnings: Vec<String>,

    // -- Execution outcome (None if the run did not occur) --
    /// UTC instant `nami` was invoked.
    #[serde(with = "time::serde::rfc3339")]
    pub submitted_at: OffsetDateTime,
    /// When the child process actually started, if it did.
    #[serde(with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    /// When the child process exited, if it did.
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
    /// Wall-clock duration the child ran.
    #[serde(with = "crate::duration_secs::option")]
    pub wall_duration: Option<Duration>,
    /// The child's exit code, if it terminated normally.
    pub exit_code: Option<i32>,
}
