//! `nami forecast`: print the historical-pattern forecast points and
//! their confidence metadata for a region across a horizon.
//!
//! This is a read-only query over the local historical cache — it never
//! touches the network and never invents data. Hours with no matching
//! samples are omitted (refuse-to-estimate, not a fabricated zero), and
//! a pure-cache forecast is cache-only so confidence is capped at `Low`.
//! Language follows CLAUDE.md: "historical-pattern forecast", "estimated
//! average", never "marginal"/"optimal"/"precise".

use anyhow::Result;
use time::Duration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use nami_carbon_eia::{HistoricalCache, historical_pattern_forecast};
use nami_core::{ConfidenceLevel, ForecastHorizon, ForecastPoint, Region};

use crate::ForecastArgs;

pub fn run(args: ForecastArgs) -> Result<()> {
    let region = crate::resolve_region(args.region)?;
    let now = OffsetDateTime::now_utc();

    let cache = match HistoricalCache::load(&args.cache) {
        Ok(c) => c,
        Err(nami_carbon_eia::Error::CacheMissing(_)) => {
            println!(
                "No historical cache at {} — nothing to forecast.",
                args.cache.display()
            );
            println!("Populate it first: nami refresh --region {region} (requires EIA_API_KEY).");
            return Ok(());
        }
        Err(e) => {
            println!("Historical cache unusable: {e}");
            println!("No forecast produced (refusing to estimate from an unreadable cache).");
            return Ok(());
        }
    };

    let horizon = ForecastHorizon::new(now, args.horizon);
    let points = historical_pattern_forecast(&cache, region, horizon, now, args.weeks);
    let has_history = !cache.observations(region).is_empty();

    print!(
        "{}",
        render(region, args.weeks, args.horizon, has_history, &points)
    );
    Ok(())
}

/// Human-readable, language-rule-compliant rendering. Pure, so it is
/// unit-testable without a clock or filesystem.
fn render(
    region: Region,
    weeks: u32,
    horizon: Duration,
    has_history: bool,
    points: &[ForecastPoint],
) -> String {
    let mut s = String::new();

    if points.is_empty() {
        if has_history {
            s.push_str(&format!(
                "No hours in the next {} have matching samples in the last {} \
                 weeks for {region}.\n",
                fmt_dur(horizon),
                weeks
            ));
        } else {
            s.push_str(&format!(
                "No cached history for {region} — run: nami refresh --region \
                 {region}\n"
            ));
        }
        s.push_str("No forecast produced (refuse-to-estimate, not a fabricated zero).\n");
        return s;
    }

    s.push_str(&format!(
        "nami forecast — region {region} — {} point(s) over the next {}\n",
        points.len(),
        fmt_dur(horizon)
    ));
    s.push_str(&format!(
        "Basis: historical-pattern forecast (mean of matching \
         hour-of-day / day-of-week / month samples, last {weeks} weeks).\n"
    ));
    s.push_str(
        "Estimated AVERAGE carbon intensity (gCO2/kWh), not marginal \
         emissions. Cache-only basis caps confidence at Low.\n\n",
    );
    s.push_str(&format!(
        "{:<20}  {:>9}  {:>8}  {:>7}  {}\n",
        "hour (UTC)", "gCO2/kWh", "conf", "samples", "1σ interval gCO2/kWh"
    ));
    for p in points {
        let iv = match &p.confidence.interval {
            Some(i) => format!("[{:.0}, {:.0}]", i.lower, i.upper),
            None => "—".to_string(),
        };
        s.push_str(&format!(
            "{:<20}  {:>9.0}  {:>8}  {:>7}  {}\n",
            fmt_dt(p.at),
            p.intensity.value(),
            level_label(p.confidence.level),
            p.confidence.sample_count,
            iv
        ));
    }
    s.push('\n');
    s.push_str(&format!("Methodology: {}\n", points[0].methodology));
    s.push_str("Hours with no matching samples are omitted, never invented.\n");
    s
}

fn fmt_dt(dt: OffsetDateTime) -> String {
    dt.format(&Rfc3339).unwrap_or_else(|_| format!("{dt:?}"))
}

/// Compact duration label (`24h`, `2d`, `90m`, `45s`).
fn fmt_dur(d: Duration) -> String {
    let s = d.whole_seconds();
    if s != 0 && s % 86_400 == 0 {
        format!("{}d", s / 86_400)
    } else if s != 0 && s % 3_600 == 0 {
        format!("{}h", s / 3_600)
    } else if s != 0 && s % 60 == 0 {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}

fn level_label(l: ConfidenceLevel) -> &'static str {
    match l {
        ConfidenceLevel::High => "High",
        ConfidenceLevel::Medium => "Medium",
        ConfidenceLevel::Low => "Low",
        ConfidenceLevel::VeryLow => "VeryLow",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::{CarbonIntensity, Confidence, DataFreshness};
    use time::macros::datetime;

    fn point(at: OffsetDateTime, v: f64, n: usize, sd: f64) -> ForecastPoint {
        let fresh = DataFreshness::HistoricalCacheOnly {
            newest_sample_at: at,
        };
        ForecastPoint {
            at,
            intensity: CarbonIntensity::new(v).unwrap(),
            confidence: Confidence::assess(n, v, sd, &fresh),
            methodology: "historical-pattern-mean-8w-hour-dow-month-v1".into(),
        }
    }

    #[test]
    fn fmt_dur_units() {
        // Whole days collapse to `d` (24h == 1d, unambiguous-enough here).
        assert_eq!(fmt_dur(Duration::hours(24)), "1d");
        assert_eq!(fmt_dur(Duration::hours(36)), "36h");
        assert_eq!(fmt_dur(Duration::days(2)), "2d");
        assert_eq!(fmt_dur(Duration::minutes(90)), "90m");
        assert_eq!(fmt_dur(Duration::seconds(45)), "45s");
    }

    #[test]
    fn empty_with_history_is_refuse_not_zero() {
        let out = render(Region::Caiso, 8, Duration::hours(24), true, &[]);
        assert!(out.contains("matching samples"));
        assert!(out.contains("refuse-to-estimate"));
        assert!(!out.contains("0 gCO2/kWh"));
    }

    #[test]
    fn empty_without_history_points_to_refresh() {
        let out = render(Region::Pjm, 8, Duration::hours(24), false, &[]);
        assert!(out.contains("No cached history for PJM"));
        assert!(out.contains("nami refresh --region PJM"));
    }

    #[test]
    fn renders_points_with_confidence_and_compliant_language() {
        let pts = vec![
            point(datetime!(2026-05-20 10:00 UTC), 312.4, 6, 8.0),
            point(datetime!(2026-05-20 11:00 UTC), 280.0, 1, 0.0),
        ];
        let out = render(Region::Miso, 8, Duration::hours(2), true, &pts);
        let low = out.to_lowercase();
        assert!(out.contains("historical-pattern forecast"));
        assert!(out.contains("not marginal emissions"));
        assert!(out.contains("2026-05-20T10:00:00Z"));
        assert!(out.contains("historical-pattern-mean-8w-hour-dow-month-v1"));
        // Single-sample point has no computable interval.
        assert!(out.contains("—"));
        for banned in [
            "cleanest possible",
            "optimal carbon",
            "guaranteed",
            "real-time carbon",
            "precise grid",
        ] {
            assert!(!low.contains(banned), "banned phrase: {banned}");
        }
    }
}
