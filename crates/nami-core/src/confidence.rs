//! Confidence, data freshness, and granularity — first-class types.
//!
//! Per `CLAUDE.md`, uncertainty is structurally part of every estimate
//! `nami` produces. The types here are intentionally explicit so a downstream
//! reader of a [`RunReport`](crate::RunReport) can see exactly why a
//! recommendation was rated `High` vs `VeryLow` and what data was available.

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

/// Qualitative confidence level for a single estimate or recommendation.
///
/// Confidence degrades along several axes: small sample count, high
/// variance, stale data, sparse coverage, far-horizon forecast. A
/// [`Confidence`] value bundles the label with the underlying evidence
/// (sample count, interval) so reviewers can audit the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    /// Plenty of recent samples, low variance, fresh data.
    High,
    /// Adequate samples; some staleness or moderate variance.
    Medium,
    /// Sparse samples, large variance, or far-horizon estimate.
    Low,
    /// Almost no usable data; estimate should generally not drive a
    /// recommendation.
    VeryLow,
}

/// An (approximate) confidence interval around an estimated intensity.
///
/// Stored as bounds in gCO₂/kWh; the interpretation (1σ band, 95% CI,
/// min/max of samples) is whatever the producing provider documents in
/// its methodology label. The intent is that consumers can render a band
/// or width without knowing the producer's specific math.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceInterval {
    /// Lower bound, gCO₂/kWh.
    pub lower: f64,
    /// Upper bound, gCO₂/kWh.
    pub upper: f64,
}

/// Confidence in a single estimate.
///
/// Carries the level plus the underlying evidence: sample count, optional
/// numeric interval, and free-form notes (e.g., "fallback to static table",
/// "weekend samples only").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Confidence {
    /// Qualitative level.
    pub level: ConfidenceLevel,
    /// Number of historical samples that contributed to the estimate.
    pub sample_count: usize,
    /// Numeric interval, if computable.
    pub interval: Option<ConfidenceInterval>,
    /// Free-form notes describing why this confidence was assigned.
    pub notes: Vec<String>,
}

impl Confidence {
    /// Build a `VeryLow` confidence with a single explanatory note. Used
    /// for fallback paths where there's nothing to compute statistics from.
    pub fn very_low(reason: impl Into<String>) -> Self {
        Self {
            level: ConfidenceLevel::VeryLow,
            sample_count: 0,
            interval: None,
            notes: vec![reason.into()],
        }
    }
}

/// The provenance/freshness of the data a decision was made from.
///
/// Decision quality varies enormously with data state. Schedulers should
/// downgrade confidence (or refuse) when freshness is degraded, and reports
/// must record which state was in effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum DataFreshness {
    /// Live observed data within expected lag.
    FreshObserved,
    /// Observed data is older than expected; report the lag.
    StaleObserved {
        /// How far behind real time the freshest sample is.
        #[serde(with = "crate::duration_secs")]
        lag: Duration,
    },
    /// No live observations; only historical cache is available.
    HistoricalCacheOnly {
        /// UTC timestamp of the newest sample in the cache.
        #[serde(with = "time::serde::rfc3339")]
        newest_sample_at: OffsetDateTime,
    },
    /// Even the historical cache is unavailable; only the static fallback
    /// table is in play.
    StaticFallbackOnly,
    /// No usable data at all — scheduler must refuse or fall back to
    /// "run now" with explicit warnings.
    NoUsableData,
}

/// Temporal resolution of a provider's data.
///
/// `nami` Phase 0 operates on hourly resolution; finer granularity from
/// upstream sources (e.g., 5-minute interchange data) is downsampled before
/// it reaches the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum DataGranularity {
    /// Sub-hourly samples with the given period in seconds.
    SubHourly {
        /// Period between samples, in seconds.
        period_seconds: u32,
    },
    /// Hourly samples.
    Hourly,
    /// Daily samples.
    Daily,
    /// Annual averages (e.g., the static fallback table).
    Annual,
}
