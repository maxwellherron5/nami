//! Historical-pattern forecast model.
//!
//! `nami` does **not** fetch a carbon forecast from anyone — EIA-930
//! publishes no such thing. This module *models* an expected average
//! intensity for a future hour from the cached history, and labels it as
//! exactly that. Pure, synchronous math (no I/O); CLAUDE.md forbids
//! async for forecast computation.
//!
//! ## Model
//!
//! For a target hour `t`, the estimate is the mean of cached
//! observations that match **region + hour-of-day + day-of-week +
//! month**, drawn from the **last `N` weeks** (default
//! [`DEFAULT_FORECAST_WEEKS`] = 8) ending at the caller-supplied `now`.
//! The 1σ band uses the *sample* standard deviation (n−1).
//!
//! ## Methodology stances (documented; see `docs/methodology.md`)
//!
//! - **A pure-cache forecast is inherently
//!   [`DataFreshness::HistoricalCacheOnly`].** It never uses live
//!   observed data, so — per the confidence freshness caps — its
//!   confidence is capped at `Low`. This honesty is baked into the
//!   model, not left to the caller.
//! - **Hours with zero matching samples are omitted**, never invented.
//!   The result simply has fewer points than the horizon has hours; the
//!   scheduler treats missing hours as gaps (consistent with
//!   "refuse to estimate").
//! - **Exact day-of-week** (not weekday/weekend buckets), **month**
//!   (not season), per `methodology.md`'s specification.
//! - The methodology label embeds the actual `N`
//!   (`historical-pattern-mean-{N}w-hour-dow-month-v1`) so a report can
//!   be traced to the exact window used.

use time::{Duration, OffsetDateTime};

use nami_core::{
    CarbonIntensity, Confidence, DataFreshness, ForecastHorizon, ForecastPoint, Region,
};

use crate::cache::HistoricalCache;

/// Default look-back window, in weeks, for the historical-pattern model.
pub const DEFAULT_FORECAST_WEEKS: u32 = 8;

/// The methodology label for a given look-back `weeks`.
fn methodology_label(weeks: u32) -> String {
    format!("historical-pattern-mean-{weeks}w-hour-dow-month-v1")
}

/// Floor a UTC instant to the start of its hour, panic-free.
fn floor_to_hour(t: OffsetDateTime) -> Option<OffsetDateTime> {
    let secs_into_hour = t.unix_timestamp().rem_euclid(3600);
    t.checked_sub(
        Duration::seconds(secs_into_hour) + Duration::nanoseconds(i64::from(t.nanosecond())),
    )
}

/// Produce hourly [`ForecastPoint`]s for `region` across `horizon`.
///
/// Returns one point per *estimable* whole UTC hour in
/// `[floor_hour(horizon.start), horizon.end())` — hours with no matching
/// samples are omitted. Points are ascending by time. An empty result
/// means nothing in the horizon could be estimated (e.g. the region has
/// no cached history, or `weeks` is 0).
///
/// `now` is passed explicitly (not read from the clock) so forecasting
/// is deterministic and testable; it anchors the "last `weeks` weeks"
/// sample window: `(now − weeks, now]`.
pub fn historical_pattern_forecast(
    cache: &HistoricalCache,
    region: Region,
    horizon: ForecastHorizon,
    now: OffsetDateTime,
    weeks: u32,
) -> Vec<ForecastPoint> {
    let observations = cache.observations(region);
    let Some(newest_sample_at) = cache.newest_sample(region) else {
        return Vec::new(); // no history for this region — cannot forecast
    };
    let Some(window_start) = now.checked_sub(Duration::weeks(i64::from(weeks))) else {
        return Vec::new();
    };
    let Some(mut at) = floor_to_hour(horizon.start) else {
        return Vec::new();
    };
    let end = horizon.end();

    // A pure-cache forecast is, by construction, cache-only: confidence
    // is capped at Low regardless of how many samples back it.
    let freshness = DataFreshness::HistoricalCacheOnly { newest_sample_at };
    let label = methodology_label(weeks);

    let mut points = Vec::new();
    while at < end {
        let want_hour = at.hour();
        let want_dow = at.weekday();
        let want_month = at.month();

        let samples: Vec<f64> = observations
            .iter()
            .filter(|o| o.at > window_start && o.at <= now)
            .filter(|o| {
                o.at.hour() == want_hour && o.at.weekday() == want_dow && o.at.month() == want_month
            })
            .map(|o| o.intensity.value())
            .collect();

        if !samples.is_empty() {
            let n = samples.len();
            let mean = samples.iter().sum::<f64>() / n as f64;
            let std_dev = if n >= 2 {
                let var =
                    samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
                var.sqrt()
            } else {
                0.0
            };
            if let Ok(intensity) = CarbonIntensity::new(mean) {
                points.push(ForecastPoint {
                    at,
                    intensity,
                    confidence: Confidence::assess(n, mean, std_dev, &freshness),
                    methodology: label.clone(),
                });
            }
        }

        match at.checked_add(Duration::hours(1)) {
            Some(next) => at = next,
            None => break,
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::{CarbonObservation, ConfidenceLevel};
    use time::macros::datetime;

    fn obs(at: OffsetDateTime, v: f64) -> CarbonObservation {
        CarbonObservation {
            at,
            intensity: CarbonIntensity::new(v).unwrap(),
            methodology: "eia-930-v1+egrid-2023-ba".into(),
        }
    }

    // 2026-05-20 14:00 UTC is a Wednesday in May. Build same-hour/
    // same-weekday/same-month history one, two, three weeks prior.
    fn cache_with_history() -> HistoricalCache {
        let mut c = HistoricalCache::new(datetime!(2026-05-19 00:00 UTC), "test");
        c.set_region(
            Region::Caiso,
            vec![
                obs(datetime!(2026-05-13 14:00 UTC), 100.0), // same wd/hr/mo, matches
                obs(datetime!(2026-05-06 14:00 UTC), 200.0), // same wd/hr/mo, matches
                obs(datetime!(2026-04-29 14:00 UTC), 300.0), // same wd/hr but APRIL — excluded
                obs(datetime!(2026-05-13 03:00 UTC), 999.0), // wrong hour (03:00) — excluded
                obs(datetime!(2026-05-12 14:00 UTC), 888.0), // +8d ≠ same weekday — excluded
                obs(datetime!(2026-01-07 14:00 UTC), 777.0), // out of 8-week window
            ],
        );
        c
    }

    fn horizon_one_hour() -> ForecastHorizon {
        ForecastHorizon::new(datetime!(2026-05-20 14:00 UTC), Duration::hours(1))
    }

    #[test]
    fn means_only_matching_samples() {
        let c = cache_with_history();
        let now = datetime!(2026-05-19 00:00 UTC);
        let f = historical_pattern_forecast(&c, Region::Caiso, horizon_one_hour(), now, 8);
        assert_eq!(f.len(), 1);
        let p = &f[0];
        assert_eq!(p.at, datetime!(2026-05-20 14:00 UTC));
        // Only the two May Wednesday 14:00 samples (100, 200) → mean 150.
        assert!((p.intensity.value() - 150.0).abs() < 1e-9);
        assert_eq!(p.confidence.sample_count, 2);
        assert_eq!(
            p.methodology,
            "historical-pattern-mean-8w-hour-dow-month-v1"
        );
        // Cache-only forecast can never exceed Low confidence.
        assert!(p.confidence.level >= ConfidenceLevel::Low);
    }

    #[test]
    fn omits_hours_with_no_samples() {
        let c = cache_with_history();
        let now = datetime!(2026-05-19 00:00 UTC);
        // 6-hour horizon starting 14:00; only the 14:00 bucket has data.
        let h = ForecastHorizon::new(datetime!(2026-05-20 14:00 UTC), Duration::hours(6));
        let f = historical_pattern_forecast(&c, Region::Caiso, h, now, 8);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].at, datetime!(2026-05-20 14:00 UTC));
    }

    #[test]
    fn unknown_region_yields_empty() {
        let c = cache_with_history();
        let now = datetime!(2026-05-19 00:00 UTC);
        let f = historical_pattern_forecast(&c, Region::Pjm, horizon_one_hour(), now, 8);
        assert!(f.is_empty());
    }

    #[test]
    fn zero_weeks_window_yields_empty() {
        let c = cache_with_history();
        let now = datetime!(2026-05-19 00:00 UTC);
        let f = historical_pattern_forecast(&c, Region::Caiso, horizon_one_hour(), now, 0);
        assert!(f.is_empty());
    }

    #[test]
    fn label_reflects_actual_weeks() {
        let c = cache_with_history();
        let now = datetime!(2026-05-19 00:00 UTC);
        let f = historical_pattern_forecast(&c, Region::Caiso, horizon_one_hour(), now, 4);
        assert_eq!(f.len(), 1);
        assert_eq!(
            f[0].methodology,
            "historical-pattern-mean-4w-hour-dow-month-v1"
        );
    }

    #[test]
    fn unaligned_horizon_start_is_floored() {
        let c = cache_with_history();
        let now = datetime!(2026-05-19 00:00 UTC);
        let h = ForecastHorizon::new(datetime!(2026-05-20 14:37:12 UTC), Duration::hours(1));
        let f = historical_pattern_forecast(&c, Region::Caiso, h, now, 8);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].at, datetime!(2026-05-20 14:00 UTC));
    }
}
