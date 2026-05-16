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

    /// Derive a confidence from forecast-sample evidence and the data
    /// freshness state.
    ///
    /// Three axes are graded independently and the **most conservative**
    /// (lowest) level wins:
    ///
    /// 1. **Sample count** — `>=6 → High`, `3..=5 → Medium`,
    ///    `1..=2 → Low`, `0 → VeryLow`.
    /// 2. **Relative interval width** `r = std_dev / mean` (the 1σ
    ///    half-width as a fraction of the mean) — `r < 0.10 → High`,
    ///    `r < 0.25 → Medium`, `r <= 0.40 → Low`, else `VeryLow`. Not
    ///    computable (fewer than 2 samples, non-positive mean, or a
    ///    non-finite input) grades `VeryLow`.
    /// 3. **Freshness cap** — `FreshObserved → High`,
    ///    `StaleObserved → Medium`, `HistoricalCacheOnly → Low`,
    ///    `StaticFallbackOnly`/`NoUsableData → VeryLow`.
    ///
    /// The interval (when computable) is the 1σ band
    /// `[max(0, mean - std_dev), mean + std_dev]` in gCO₂/kWh.
    ///
    /// Every axis appends a note explaining its contribution so a report
    /// reviewer can audit the assigned level. See
    /// `docs/confidence-and-materiality.md`.
    ///
    /// # Examples
    ///
    /// ```
    /// use nami_core::{Confidence, ConfidenceLevel, DataFreshness};
    ///
    /// let c = Confidence::assess(8, 300.0, 15.0, &DataFreshness::FreshObserved);
    /// assert_eq!(c.level, ConfidenceLevel::High);
    /// assert_eq!(c.sample_count, 8);
    /// let iv = c.interval.unwrap();
    /// assert_eq!((iv.lower, iv.upper), (285.0, 315.0));
    /// ```
    pub fn assess(sample_count: usize, mean: f64, std_dev: f64, freshness: &DataFreshness) -> Self {
        let mut notes = Vec::new();

        let sample_level = grade_sample_count(sample_count);
        notes.push(format!("sample count {sample_count} → {sample_level:?}"));

        let (width_level, interval) = grade_width(sample_count, mean, std_dev, &mut notes);
        let (cap_level, cap_label) = freshness_cap(freshness);
        notes.push(format!("freshness cap: {cap_label} → {cap_level:?}"));

        // Most conservative wins. `ConfidenceLevel` is declared
        // High < Medium < Low < VeryLow, so the *maximum* is the most
        // conservative.
        let level = sample_level.max(width_level).max(cap_level);
        notes.push(format!("=> {level:?} (most conservative of the three)"));

        Self {
            level,
            sample_count,
            interval,
            notes,
        }
    }
}

/// Grade the sample-count axis.
fn grade_sample_count(n: usize) -> ConfidenceLevel {
    match n {
        0 => ConfidenceLevel::VeryLow,
        1..=2 => ConfidenceLevel::Low,
        3..=5 => ConfidenceLevel::Medium,
        _ => ConfidenceLevel::High,
    }
}

/// Grade the relative-interval-width axis and compute the 1σ interval.
///
/// Pushes an explanatory note. The interval is `None` (and the level is
/// `VeryLow`) when variance cannot be defensibly estimated: fewer than 2
/// samples, non-positive mean, or non-finite inputs.
fn grade_width(
    sample_count: usize,
    mean: f64,
    std_dev: f64,
    notes: &mut Vec<String>,
) -> (ConfidenceLevel, Option<ConfidenceInterval>) {
    if sample_count < 2 || !mean.is_finite() || mean <= 0.0 || !std_dev.is_finite() || std_dev < 0.0
    {
        notes.push(
            "interval not computable (need ≥2 samples, positive finite mean, \
             non-negative finite std) → VeryLow"
                .to_string(),
        );
        return (ConfidenceLevel::VeryLow, None);
    }

    let r = std_dev / mean;
    let level = if r < 0.10 {
        ConfidenceLevel::High
    } else if r < 0.25 {
        ConfidenceLevel::Medium
    } else if r <= 0.40 {
        ConfidenceLevel::Low
    } else {
        ConfidenceLevel::VeryLow
    };
    notes.push(format!("relative interval ±{:.1}% → {level:?}", r * 100.0));
    let interval = ConfidenceInterval {
        lower: (mean - std_dev).max(0.0),
        upper: mean + std_dev,
    };
    (level, Some(interval))
}

/// The confidence cap implied by the data freshness state.
fn freshness_cap(freshness: &DataFreshness) -> (ConfidenceLevel, &'static str) {
    match freshness {
        DataFreshness::FreshObserved => (ConfidenceLevel::High, "fresh-observed"),
        DataFreshness::StaleObserved { .. } => (ConfidenceLevel::Medium, "stale-observed"),
        DataFreshness::HistoricalCacheOnly { .. } => {
            (ConfidenceLevel::Low, "historical-cache-only")
        }
        DataFreshness::StaticFallbackOnly => (ConfidenceLevel::VeryLow, "static-fallback-only"),
        DataFreshness::NoUsableData => (ConfidenceLevel::VeryLow, "no-usable-data"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_axes_good_is_high_with_interval() {
        let c = Confidence::assess(8, 300.0, 15.0, &DataFreshness::FreshObserved);
        assert_eq!(c.level, ConfidenceLevel::High);
        assert_eq!(c.sample_count, 8);
        let iv = c.interval.unwrap();
        assert_eq!((iv.lower, iv.upper), (285.0, 315.0));
        assert!(c.notes.len() >= 4);
    }

    #[test]
    fn freshness_cap_dominates_good_stats() {
        // Great stats, but only the historical cache → capped at Low.
        let fresh = DataFreshness::HistoricalCacheOnly {
            newest_sample_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        let c = Confidence::assess(12, 300.0, 6.0, &fresh);
        assert_eq!(c.level, ConfidenceLevel::Low);
    }

    #[test]
    fn width_axis_dominates() {
        // r = 120/300 = 0.40 → Low (inclusive upper bound).
        let c = Confidence::assess(10, 300.0, 120.0, &DataFreshness::FreshObserved);
        assert_eq!(c.level, ConfidenceLevel::Low);
        // r just over 0.40 → VeryLow.
        let c2 = Confidence::assess(10, 300.0, 120.1, &DataFreshness::FreshObserved);
        assert_eq!(c2.level, ConfidenceLevel::VeryLow);
    }

    #[test]
    fn width_boundaries() {
        // r = 0.10 exactly → not < 0.10, so Medium (with ≥6 samples, fresh).
        let c = Confidence::assess(6, 100.0, 10.0, &DataFreshness::FreshObserved);
        assert_eq!(c.level, ConfidenceLevel::Medium);
        // r = 0.25 exactly → not < 0.25, so Low.
        let c2 = Confidence::assess(6, 100.0, 25.0, &DataFreshness::FreshObserved);
        assert_eq!(c2.level, ConfidenceLevel::Low);
    }

    #[test]
    fn sample_axis_dominates() {
        // Only 2 samples → Low even with tight spread and fresh data.
        let c = Confidence::assess(2, 300.0, 1.0, &DataFreshness::FreshObserved);
        assert_eq!(c.level, ConfidenceLevel::Low);
    }

    #[test]
    fn zero_samples_is_very_low_no_interval() {
        let c = Confidence::assess(0, 300.0, 0.0, &DataFreshness::FreshObserved);
        assert_eq!(c.level, ConfidenceLevel::VeryLow);
        assert!(c.interval.is_none());
    }

    #[test]
    fn one_sample_has_no_interval_and_is_very_low() {
        // Single sample: variance not estimable → width axis VeryLow.
        let c = Confidence::assess(1, 300.0, 0.0, &DataFreshness::FreshObserved);
        assert!(c.interval.is_none());
        assert_eq!(c.level, ConfidenceLevel::VeryLow);
    }

    #[test]
    fn static_fallback_is_always_very_low() {
        let c = Confidence::assess(100, 300.0, 1.0, &DataFreshness::StaticFallbackOnly);
        assert_eq!(c.level, ConfidenceLevel::VeryLow);
    }

    #[test]
    fn no_usable_data_is_very_low() {
        let c = Confidence::assess(100, 300.0, 1.0, &DataFreshness::NoUsableData);
        assert_eq!(c.level, ConfidenceLevel::VeryLow);
    }

    #[test]
    fn non_positive_mean_blocks_interval() {
        let c = Confidence::assess(8, 0.0, 1.0, &DataFreshness::FreshObserved);
        assert!(c.interval.is_none());
        assert_eq!(c.level, ConfidenceLevel::VeryLow);
    }

    #[test]
    fn interval_lower_bound_clamps_at_zero() {
        // mean - std would be negative; clamp lower to 0. r huge → VeryLow,
        // but the interval itself must still be well-formed.
        let c = Confidence::assess(8, 10.0, 50.0, &DataFreshness::FreshObserved);
        let iv = c.interval.unwrap();
        assert_eq!(iv.lower, 0.0);
        assert_eq!(iv.upper, 60.0);
        assert_eq!(c.level, ConfidenceLevel::VeryLow);
    }
}
