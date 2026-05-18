//! `nami status`: report supported regions, configured data sources,
//! provider availability, and per-region historical-cache freshness;
//! optionally summarize a previously written run report.
//!
//! Read-only and offline. Degraded states (missing/unusable cache,
//! missing eGRID table, unset `EIA_API_KEY`) are surfaced loudly rather
//! than hidden, per CLAUDE.md.

use anyhow::{Context, Result};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use nami_carbon_eia::{DEFAULT_MAX_CACHE_AGE, EgridFactors, HistoricalCache};
use nami_core::{DataFreshness, Region, RunReport, SchedulingDecision};

use crate::StatusArgs;

pub fn run(args: StatusArgs) -> Result<()> {
    let now = OffsetDateTime::now_utc();

    println!("nami status — {}", fmt_dt(now));
    println!();

    let codes: Vec<&str> = Region::ALL.iter().map(|r| r.as_code()).collect();
    println!("Supported regions: {}", codes.join(", "));
    println!();

    println!("Data sources:");
    match EgridFactors::load(&args.egrid) {
        Ok(f) => println!(
            "  eGRID factors: OK — {} (data year {}), methodology {} [{}]",
            f.release,
            f.data_year,
            f.methodology,
            args.egrid.display()
        ),
        Err(e) => println!(
            "  eGRID factors: UNAVAILABLE — {e} [{}]",
            args.egrid.display()
        ),
    }
    let key_set = std::env::var("EIA_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);
    println!(
        "  EIA-930 API: EIA_API_KEY {} (required for `nami refresh`)",
        if key_set { "is set" } else { "is NOT set" }
    );
    println!(
        "  Static fallback: always available (flat annual mean, VeryLow \
         confidence — not a forecast)"
    );
    println!();

    println!("Historical cache [{}]:", args.cache.display());
    match HistoricalCache::load(&args.cache) {
        Err(nami_carbon_eia::Error::CacheMissing(_)) => {
            println!("  none — run `nami refresh --region <R>` to populate it");
        }
        Err(e) => {
            println!("  UNUSABLE — {e}");
        }
        Ok(c) => {
            println!(
                "  schema v{}, file written {}, {} region(s) with history",
                c.schema_version,
                fmt_dt(c.generated_at),
                c.region_count()
            );
            let max_h = DEFAULT_MAX_CACHE_AGE.whole_hours();
            // Per-region freshness (a single-region refresh rewrites the
            // file timestamp, so staleness is judged per region).
            for r in Region::ALL {
                match c.newest_sample(*r) {
                    None => println!("  {:<6}     no data", r.as_code()),
                    Some(ns) => {
                        let n = c.observations(*r).len();
                        let age_h = (now - ns).whole_hours();
                        let stale = now - ns > DEFAULT_MAX_CACHE_AGE;
                        println!(
                            "  {:<6} {:>5} obs, newest {} (age {}h){}",
                            r.as_code(),
                            n,
                            fmt_dt(ns),
                            age_h,
                            if stale {
                                format!("  STALE (> {max_h}h)")
                            } else {
                                String::new()
                            }
                        );
                    }
                }
            }
        }
    }

    if let Some(path) = &args.report {
        println!();
        println!("Run report [{}]:", path.display());
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading run report {}", path.display()))?;
        let report: RunReport = serde_json::from_str(&text)
            .with_context(|| format!("parsing run report {}", path.display()))?;
        print!("{}", summarize_report(&report));
    }

    Ok(())
}

/// Pure, testable one-screen summary of a persisted [`RunReport`].
fn summarize_report(r: &RunReport) -> String {
    let mut s = String::new();
    s.push_str(&format!("  command: {} {}\n", r.command, r.args.join(" ")));
    s.push_str(&format!(
        "  region {} — deadline {}\n",
        r.region,
        fmt_dt(r.deadline)
    ));
    match &r.decision {
        SchedulingDecision::StartAt {
            start_time,
            reason,
            confidence,
        } => s.push_str(&format!(
            "  decision: start at {} ({reason:?}), confidence {:?}\n",
            fmt_dt(*start_time),
            confidence.level
        )),
        SchedulingDecision::StartImmediately { reason, confidence } => s.push_str(&format!(
            "  decision: start immediately ({reason:?}), confidence {:?}\n",
            confidence.level
        )),
        SchedulingDecision::Refuse { reason } => {
            s.push_str(&format!("  decision: refused ({reason:?})\n"));
        }
    }
    if let Some(p) = r.estimated_improvement_pct {
        s.push_str(&format!(
            "  estimated improvement: {p:.1}% (materiality threshold {:.1}%)\n",
            r.materiality_threshold_pct
        ));
    }
    s.push_str(&format!(
        "  provider: {} — freshness {}\n",
        r.provider.name,
        freshness_label(&r.data_freshness)
    ));
    s.push_str(&format!("  methodology: {}\n", r.methodology_version));
    if let Some(code) = r.exit_code {
        s.push_str(&format!("  child exit code: {code}\n"));
    }
    if !r.warnings.is_empty() {
        s.push_str(&format!("  warnings: {}\n", r.warnings.len()));
    }
    s
}

fn freshness_label(f: &DataFreshness) -> &'static str {
    match f {
        DataFreshness::FreshObserved => "fresh-observed",
        DataFreshness::StaleObserved { .. } => "stale-observed",
        DataFreshness::HistoricalCacheOnly { .. } => "historical-cache-only",
        DataFreshness::StaticFallbackOnly => "static-fallback-only",
        DataFreshness::NoUsableData => "no-usable-data",
    }
}

fn fmt_dt(dt: OffsetDateTime) -> String {
    dt.format(&Rfc3339).unwrap_or_else(|_| format!("{dt:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::{CarbonIntensity, DataGranularity};
    use nami_core::{Confidence, ProviderInfo, RefuseReason, StartReason, WindowEstimate};
    use time::Duration;
    use time::macros::datetime;

    fn base_report() -> RunReport {
        RunReport {
            command: "python".into(),
            args: vec!["train.py".into()],
            region: Region::Miso,
            deadline: datetime!(2026-05-20 18:00 UTC),
            estimated_duration: Duration::hours(2),
            decision: SchedulingDecision::Refuse {
                reason: RefuseReason::NoWindowBeforeDeadline,
            },
            run_now_estimate: None,
            selected_window_estimate: None,
            estimated_improvement_pct: None,
            materiality_threshold_pct: 5.0,
            provider: ProviderInfo {
                name: "static-fallback".into(),
                capabilities: vec![],
                granularity: DataGranularity::Annual,
                expected_lag: None,
            },
            data_freshness: DataFreshness::StaticFallbackOnly,
            methodology_version: "static-fallback-annual-v1".into(),
            warnings: vec!["not marginal emissions".into()],
            submitted_at: datetime!(2026-05-20 06:00 UTC),
            started_at: None,
            finished_at: None,
            wall_duration: None,
            exit_code: None,
        }
    }

    #[test]
    fn summarizes_refuse_report() {
        let out = summarize_report(&base_report());
        assert!(out.contains("region MISO"));
        assert!(out.contains("decision: refused"));
        assert!(out.contains("NoWindowBeforeDeadline"));
        assert!(out.contains("freshness static-fallback-only"));
        assert!(out.contains("warnings: 1"));
    }

    #[test]
    fn summarizes_startat_with_improvement_and_exit() {
        let mut r = base_report();
        r.decision = SchedulingDecision::StartAt {
            start_time: datetime!(2026-05-20 14:00 UTC),
            reason: StartReason::LowestEstimatedIntensity,
            confidence: Confidence::very_low("t"),
        };
        r.estimated_improvement_pct = Some(18.7);
        r.run_now_estimate = Some(WindowEstimate {
            start: datetime!(2026-05-20 06:00 UTC),
            duration: Duration::hours(2),
            mean_intensity: CarbonIntensity::new(390.0).unwrap(),
            confidence: Confidence::very_low("t"),
        });
        r.exit_code = Some(0);
        let out = summarize_report(&r);
        assert!(out.contains("start at 2026-05-20T14:00:00Z"));
        assert!(out.contains("LowestEstimatedIntensity"));
        assert!(out.contains("estimated improvement: 18.7% (materiality threshold 5.0%)"));
        assert!(out.contains("child exit code: 0"));
    }
}
