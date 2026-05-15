//! End-of-run report.
//!
//! [`RunReport`] is the durable record of one `nami` invocation: what was
//! scheduled, what actually happened, and whether any data was missing or
//! degraded. Sinks ([`crate::Sink`]) consume reports.

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::carbon::CarbonIntensity;
use crate::decision::SchedulingDecision;
use crate::region::Region;

/// Aggregate carbon outcome for the run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CarbonOutcome {
    /// Time-weighted mean intensity sampled during the run.
    pub mean_intensity: CarbonIntensity,
    /// Hypothetical mean intensity if the job had run immediately at submit
    /// time. Used to compute deferral savings.
    pub baseline_mean_intensity: Option<CarbonIntensity>,
}

/// A flag indicating that some piece of data was missing or degraded.
///
/// Per the "refuse to estimate" principle, every fallback is recorded so the
/// user can see exactly where uncertainty entered the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum DataGap {
    /// Forecast unavailable; scheduler used the fallback path.
    ForecastUnavailable(String),
    /// Real-time sampling failed at least once during the run.
    SampleFailed(String),
    /// Region was inferred rather than user-supplied.
    RegionInferred(Region),
}

/// The record of one scheduled-and-executed (or refused) job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    /// UTC instant `nami` was invoked.
    pub submitted_at: OffsetDateTime,
    /// The decision the scheduler returned.
    pub decision: SchedulingDecision,
    /// When the child process actually started, if it did.
    pub started_at: Option<OffsetDateTime>,
    /// When the child process exited, if it did.
    pub finished_at: Option<OffsetDateTime>,
    /// Wall-clock duration the child ran.
    pub wall_duration: Option<Duration>,
    /// The child's exit code, if it terminated normally.
    pub exit_code: Option<i32>,
    /// The grid region used.
    pub region: Region,
    /// Carbon outcome, if intensity sampling succeeded.
    pub carbon: Option<CarbonOutcome>,
    /// Every gap or degraded-data event encountered.
    pub data_gaps: Vec<DataGap>,
}
