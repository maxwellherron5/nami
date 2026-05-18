//! `nami preview`: compute a recommendation and report it, without
//! executing the wrapped command.
//!
//! Two data paths:
//!
//! - **EIA / historical-pattern** (preferred): a usable historical cache
//!   exists → run [`historical_pattern_forecast`] and
//!   [`BestWindowScheduler`]; emit a full [`RunReport`] plus a
//!   careful human-readable summary.
//! - **Static fallback** (degraded): the cache is missing, unusable, or
//!   has no matching samples → the item-1 static annual-mean path, with
//!   the degradation surfaced loudly.
//!
//! User-facing text follows CLAUDE.md's language rules: estimated
//! average intensity (never "marginal", "optimal", "cleanest possible",
//! "guaranteed", "real-time", or "precise").

use anyhow::{Result, anyhow};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use nami_carbon_eia::{
    DEFAULT_CACHE_PATH, DEFAULT_FORECAST_WEEKS, DEFAULT_MAX_CACHE_AGE, HistoricalCache,
    historical_pattern_forecast,
};
use nami_carbon_static::StaticTableProvider;
use nami_core::{
    DataFreshness, DataGranularity, ForecastHorizon, JobSpec, ProviderInfo, ProviderMetadata,
    RefuseReason, Region, RunReport, Scheduler, SchedulingDecision, Sink, StartReason,
    WindowEstimate,
};
use nami_scheduler::{
    BestWindowScheduler, DEFAULT_MATERIALITY_THRESHOLD_PCT, score_window, static_fallback_decision,
};

use crate::RunArgs;
use crate::sink::JsonFileSink;

const STATIC_METHODOLOGY: &str = "static-fallback-annual-v1";

/// Outcome of attempting to load the historical cache.
pub(crate) enum CacheState {
    /// Loaded. Staleness is judged per-region by the caller from the
    /// queried region's newest sample (not a file-level flag): a
    /// per-region `refresh` rewrites the whole file's `generated_at`, so
    /// a file-level age would under-report a stale untouched region.
    Present(Box<HistoricalCache>),
    /// No cache file present (a normal state, not an error).
    Missing,
    /// Cache file present but unusable; carries the reason.
    Unusable(String),
}

/// Run `nami preview`: assemble the report, print a human summary, and
/// (if `--report`) write the JSON report.
pub fn run(args: RunArgs) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    let cache = load_cache(DEFAULT_CACHE_PATH, now);
    let report = assemble(&args, now, cache)?;

    print!("{}", human_summary(&report, "preview"));

    if let Some(path) = &args.report {
        JsonFileSink(path.clone())
            .record(&report)
            .map_err(|e| anyhow!("failed to write run report: {e}"))?;
    }
    Ok(())
}

pub(crate) fn load_cache(path: &str, now: OffsetDateTime) -> CacheState {
    let _ = now; // staleness is now decided per-region in `assemble`
    match HistoricalCache::load(path) {
        Ok(c) => CacheState::Present(Box::new(c)),
        Err(nami_carbon_eia::Error::CacheMissing(_)) => CacheState::Missing,
        Err(e) => CacheState::Unusable(e.to_string()),
    }
}

/// Assemble the [`RunReport`]. Pure given `now` and `cache` (forecast and
/// scheduler are pure), so it is unit-testable without a clock or files.
pub(crate) fn assemble(
    args: &RunArgs,
    now: OffsetDateTime,
    cache: CacheState,
) -> Result<RunReport> {
    let region = args.region.ok_or_else(|| {
        anyhow!(
            "no --region given and automatic region detection is not implemented \
             yet; pass one of: CAISO, ERCOT, MISO, PJM, NYISO, ISONE, SPP"
        )
    })?;
    let job = JobSpec {
        command: args.command.clone(),
        estimated_duration: args.duration,
        deadline: args.deadline,
        region,
    };
    job.validate(now).map_err(|e| anyhow!("invalid job: {e}"))?;
    let (command, cmd_args) = job
        .command
        .split_first()
        .ok_or_else(|| anyhow!("empty command"))?;

    // Disclaimers that apply to every preview.
    let base_warnings = || {
        vec![
            "Estimate is average carbon intensity, not marginal emissions.".to_string(),
            "Not a guarantee of an actual emissions reduction.".to_string(),
        ]
    };

    // Decide which path to take.
    let (fallback_reason, cache_for_forecast): (Option<String>, Option<&HistoricalCache>) =
        match &cache {
            CacheState::Missing => (
                Some(format!(
                    "No historical cache at {DEFAULT_CACHE_PATH}; using the \
                     static annual-mean fallback (not a forecast)."
                )),
                None,
            ),
            CacheState::Unusable(msg) => (
                Some(format!(
                    "Historical cache unusable ({msg}); using the static \
                     annual-mean fallback (not a forecast)."
                )),
                None,
            ),
            CacheState::Present(c) => (None, Some(c.as_ref())),
        };

    if let Some(c) = cache_for_forecast {
        // Per-region freshness: the age of the newest sample we actually
        // have for this region — not the file-level `generated_at`, which
        // a single-region `refresh` rewrites for the whole file.
        let newest = c.newest_sample(region).unwrap_or(now);
        let stale = now - newest > DEFAULT_MAX_CACHE_AGE;
        let horizon = ForecastHorizon::new(now, job.deadline - now);
        let forecast = historical_pattern_forecast(c, region, horizon, now, DEFAULT_FORECAST_WEEKS);

        if forecast.is_empty() {
            let reason = format!(
                "Historical cache has no matching samples for {region} in the \
                 last {DEFAULT_FORECAST_WEEKS} weeks; using the static \
                 annual-mean fallback (not a forecast)."
            );
            return Ok(static_report(
                command,
                cmd_args,
                region,
                args,
                now,
                base_warnings(),
                Some(reason),
            ));
        }

        let decision = BestWindowScheduler::new().decide(&job, &forecast, now);
        let methodology = forecast
            .first()
            .map(|p| p.methodology.clone())
            .unwrap_or_default();

        let run_now = score_window(now, job.estimated_duration, &forecast);
        let selected = match &decision {
            SchedulingDecision::StartAt { start_time, .. } => {
                score_window(*start_time, job.estimated_duration, &forecast)
            }
            _ => None,
        };

        let run_now_estimate = run_now.as_ref().map(|s| WindowEstimate {
            start: now,
            duration: job.estimated_duration,
            mean_intensity: s.intensity,
            confidence: s.confidence.clone(),
        });
        let selected_window_estimate = match (&decision, &selected) {
            (SchedulingDecision::StartAt { start_time, .. }, Some(s)) => Some(WindowEstimate {
                start: *start_time,
                duration: job.estimated_duration,
                mean_intensity: s.intensity,
                confidence: s.confidence.clone(),
            }),
            _ => None,
        };
        let estimated_improvement_pct = match (&run_now, &selected) {
            (Some(rn), Some(sel)) if rn.intensity.value() > 0.0 => {
                Some((rn.intensity.value() - sel.intensity.value()) / rn.intensity.value() * 100.0)
            }
            _ => None,
        };

        let mut warnings = base_warnings();
        if stale {
            warnings.push(format!(
                "STALE DATA: the newest cached sample for {region} is older \
                 than {} hours; this recommendation is based on out-of-date \
                 patterns.",
                DEFAULT_MAX_CACHE_AGE.whole_hours()
            ));
        }
        if let SchedulingDecision::StartAt { confidence, .. }
        | SchedulingDecision::StartImmediately { confidence, .. } = &decision
        {
            warnings.extend(confidence.notes.iter().cloned());
        }

        return Ok(RunReport {
            command: command.clone(),
            args: cmd_args.to_vec(),
            region,
            deadline: args.deadline,
            estimated_duration: args.duration,
            decision,
            run_now_estimate,
            selected_window_estimate,
            estimated_improvement_pct,
            materiality_threshold_pct: DEFAULT_MATERIALITY_THRESHOLD_PCT,
            provider: eia_provider_info(),
            data_freshness: DataFreshness::HistoricalCacheOnly {
                newest_sample_at: newest,
            },
            methodology_version: methodology,
            warnings,
            submitted_at: now,
            started_at: None,
            finished_at: None,
            wall_duration: None,
            exit_code: None,
        });
    }

    Ok(static_report(
        command,
        cmd_args,
        region,
        args,
        now,
        base_warnings(),
        fallback_reason,
    ))
}

/// The historical-pattern provider's declared metadata. It deliberately
/// does **not** advertise `AverageCarbonForecast` — the forecast is
/// `nami`'s own historical-pattern model layered on cached EIA-derived
/// observations, not a forecast from EIA (CLAUDE.md).
fn eia_provider_info() -> ProviderInfo {
    ProviderInfo {
        name: "eia-egrid-historical-pattern".to_string(),
        capabilities: vec![nami_core::ProviderCapability::HistoricalHourly],
        granularity: DataGranularity::Hourly,
        expected_lag: None,
    }
}

#[allow(clippy::too_many_arguments)] // small private helper; explicit is clearer than a params struct here
fn static_report(
    command: &str,
    cmd_args: &[String],
    region: Region,
    args: &RunArgs,
    now: OffsetDateTime,
    mut warnings: Vec<String>,
    fallback_reason: Option<String>,
) -> RunReport {
    let provider = StaticTableProvider::new();
    let confidence = StaticTableProvider::baseline_confidence();
    let run_now_estimate = provider
        .baseline(region)
        .ok()
        .map(|baseline| WindowEstimate {
            start: now,
            duration: args.duration,
            mean_intensity: baseline,
            confidence: confidence.clone(),
        });
    if let Some(r) = fallback_reason {
        warnings.insert(0, r);
    }
    warnings.push(
        "Static fallback: a flat annual regional mean, not a time-varying \
         forecast. Confidence is VeryLow."
            .to_string(),
    );

    RunReport {
        command: command.to_string(),
        args: cmd_args.to_vec(),
        region,
        deadline: args.deadline,
        estimated_duration: args.duration,
        decision: static_fallback_decision(confidence),
        run_now_estimate,
        selected_window_estimate: None,
        estimated_improvement_pct: None,
        materiality_threshold_pct: DEFAULT_MATERIALITY_THRESHOLD_PCT,
        provider: provider.info(),
        data_freshness: DataFreshness::StaticFallbackOnly,
        methodology_version: STATIC_METHODOLOGY.to_string(),
        warnings,
        submitted_at: now,
        started_at: None,
        finished_at: None,
        wall_duration: None,
        exit_code: None,
    }
}

fn fmt_dt(dt: OffsetDateTime) -> String {
    dt.format(&Rfc3339).unwrap_or_else(|_| format!("{dt:?}"))
}

fn level_label(c: &nami_core::Confidence) -> &'static str {
    use nami_core::ConfidenceLevel::*;
    match c.level {
        High => "High",
        Medium => "Medium",
        Low => "Low",
        VeryLow => "Very low",
    }
}

fn refuse_text(r: &RefuseReason) -> String {
    match r {
        RefuseReason::NoWindowBeforeDeadline => {
            "the job cannot finish before the deadline".to_string()
        }
        RefuseReason::ForecastTooUncertain => {
            "the historical-pattern forecast does not cover the run-now \
             window, so there is no baseline to compare against"
                .to_string()
        }
        RefuseReason::UnsupportedRegion => "the region is not supported".to_string(),
        RefuseReason::MissingHistoricalData => "required historical data is missing".to_string(),
        RefuseReason::StaleHistoricalCache => "the historical cache is too stale".to_string(),
        RefuseReason::InsufficientSamples => {
            "there are too few samples for a defensible estimate".to_string()
        }
        RefuseReason::ProviderUnavailable => "the data provider is unavailable".to_string(),
        RefuseReason::CandidateWindowsBelowMaterialityThreshold => {
            "no candidate window is materially cleaner than running now".to_string()
        }
    }
}

/// Build the careful, language-rule-compliant terminal summary.
///
/// `subcommand` is the invoking command name (`"preview"` / `"run"`) so
/// the header reflects how it was actually invoked — `nami run` reuses
/// this exact summary and must not mislabel itself as `preview`.
pub(crate) fn human_summary(r: &RunReport, subcommand: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "nami {subcommand} — region {} — deadline {}\n",
        r.region,
        fmt_dt(r.deadline)
    ));

    match &r.decision {
        SchedulingDecision::StartAt {
            start_time,
            confidence,
            ..
        } => {
            s.push_str(&format!("Recommended start: {}\n", fmt_dt(*start_time)));
            if let Some(sel) = &r.selected_window_estimate {
                s.push_str(&format!(
                    "Expected average intensity: {:.0} gCO2/kWh\n",
                    sel.mean_intensity.value()
                ));
            }
            if let Some(rn) = &r.run_now_estimate {
                s.push_str(&format!(
                    "Run-now estimate: {:.0} gCO2/kWh\n",
                    rn.mean_intensity.value()
                ));
            }
            if let Some(p) = r.estimated_improvement_pct {
                s.push_str(&format!(
                    "Estimated improvement: {:.1}% (materiality threshold: {:.1}%)\n",
                    p, r.materiality_threshold_pct
                ));
            }
            s.push_str(&format!("Confidence: {}\n", level_label(confidence)));
        }
        SchedulingDecision::StartImmediately { reason, confidence } => {
            match reason {
                StartReason::RunNowAlreadyCleanest => s.push_str(
                    "No materially cleaner window found before the deadline.\n\
                     Recommendation: run immediately.\n",
                ),
                StartReason::DeadlineTooSoon => s.push_str(
                    "Deadline too soon to defer.\n\
                     Recommendation: run immediately.\n",
                ),
                StartReason::FallbackPolicyRunImmediately => s.push_str(
                    "No live grid data available.\n\
                     Recommendation: run immediately.\n",
                ),
                _ => s.push_str("Recommendation: run immediately.\n"),
            }
            if let Some(rn) = &r.run_now_estimate {
                s.push_str(&format!(
                    "Run-now estimate: {:.0} gCO2/kWh\n",
                    rn.mean_intensity.value()
                ));
            }
            s.push_str(&format!("Confidence: {}\n", level_label(confidence)));
        }
        SchedulingDecision::Refuse { reason } => {
            s.push_str(&format!(
                "No recommendation: {}.\n\
                 The job is not scheduled; decide how to proceed yourself.\n",
                refuse_text(reason)
            ));
        }
    }

    let basis = match r.data_freshness {
        DataFreshness::StaticFallbackOnly => format!(
            "Basis: static annual regional mean — a degraded fallback, \
             not a forecast ({})\n",
            r.methodology_version
        ),
        _ => format!(
            "Basis: historical-pattern forecast from hourly public data \
             ({})\n",
            r.methodology_version
        ),
    };
    s.push_str(&basis);
    for w in &r.warnings {
        s.push_str(&format!("Warning: {w}\n"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::{CarbonIntensity, CarbonObservation};
    use time::Duration;
    use time::macros::datetime;

    fn args_for(region: Option<Region>, deadline: OffsetDateTime, dur_h: i64) -> RunArgs {
        RunArgs {
            duration: Duration::hours(dur_h),
            deadline,
            region,
            report: None,
            quiet: false,
            log: None,
            command: vec!["python".into(), "train.py".into()],
        }
    }

    #[test]
    fn missing_cache_uses_static_fallback() {
        let now = datetime!(2026-05-20 06:00 UTC);
        let a = args_for(Some(Region::Caiso), datetime!(2026-05-20 18:00 UTC), 2);
        let r = assemble(&a, now, CacheState::Missing).unwrap();
        assert_eq!(r.data_freshness, DataFreshness::StaticFallbackOnly);
        assert_eq!(r.provider.name, "static-fallback");
        assert!(r.warnings.iter().any(|w| w.contains("No historical cache")));
        assert!(matches!(
            r.decision,
            SchedulingDecision::StartImmediately {
                reason: StartReason::FallbackPolicyRunImmediately,
                ..
            }
        ));
    }

    #[test]
    fn unusable_cache_surfaces_cause() {
        let now = datetime!(2026-05-20 06:00 UTC);
        let a = args_for(Some(Region::Caiso), datetime!(2026-05-20 18:00 UTC), 2);
        let r = assemble(&a, now, CacheState::Unusable("schema mismatch".into())).unwrap();
        assert_eq!(r.data_freshness, DataFreshness::StaticFallbackOnly);
        assert!(r.warnings.iter().any(|w| w.contains("schema mismatch")));
    }

    #[test]
    fn cache_present_but_no_samples_falls_back() {
        let now = datetime!(2026-05-20 06:00 UTC);
        let c = HistoricalCache::new(now, "test"); // empty: no region history
        let a = args_for(Some(Region::Caiso), datetime!(2026-05-20 18:00 UTC), 1);
        let r = assemble(&a, now, CacheState::Present(Box::new(c))).unwrap();
        assert_eq!(r.data_freshness, DataFreshness::StaticFallbackOnly);
        assert!(r.warnings.iter().any(|w| w.contains("no matching samples")));
    }

    #[test]
    fn eia_path_with_history_produces_cache_only_report() {
        let now = datetime!(2026-05-20 10:00 UTC);
        // One prior-week sample matching 10:00/Wed/May.
        let mut c = HistoricalCache::new(now, "test");
        c.set_region(
            Region::Caiso,
            vec![CarbonObservation {
                at: datetime!(2026-05-13 10:00 UTC),
                intensity: CarbonIntensity::new(350.0).unwrap(),
                methodology: "eia-930-v1+egrid-2023-ba".into(),
            }],
        );
        let a = args_for(Some(Region::Caiso), datetime!(2026-05-20 15:00 UTC), 1);
        let r = assemble(&a, now, CacheState::Present(Box::new(c))).unwrap();

        assert!(matches!(
            r.data_freshness,
            DataFreshness::HistoricalCacheOnly { .. }
        ));
        assert_eq!(r.provider.name, "eia-egrid-historical-pattern");
        assert!(
            !r.provider
                .capabilities
                .contains(&nami_core::ProviderCapability::AverageCarbonForecast)
        );
        assert!(
            r.methodology_version
                .starts_with("historical-pattern-mean-8w")
        );
        // Only the run-now hour has a sample → no materially cleaner
        // window → run immediately.
        assert!(matches!(
            r.decision,
            SchedulingDecision::StartImmediately {
                reason: StartReason::RunNowAlreadyCleanest,
                ..
            }
        ));
        let rn = r.run_now_estimate.expect("run-now estimate present");
        assert!((rn.mean_intensity.value() - 350.0).abs() < 1e-9);
    }

    /// Staleness is judged from the queried region's newest sample, not
    /// the file-level `generated_at` (which a per-region `refresh`
    /// rewrites for the whole file). A region whose newest sample is old
    /// must still be flagged STALE even when the cache file is "fresh".
    #[test]
    fn per_region_staleness_uses_newest_sample_not_file_timestamp() {
        let now = datetime!(2026-05-20 10:00 UTC);
        let obs = |at| CarbonObservation {
            at,
            intensity: CarbonIntensity::new(350.0).unwrap(),
            methodology: "eia-930-v1+egrid-2023-ba".into(),
        };
        let a = args_for(Some(Region::Caiso), datetime!(2026-05-20 15:00 UTC), 1);

        // Stale: newest CAISO sample is 7 days old, but the file itself
        // was just written (generated_at = now).
        let mut stale = HistoricalCache::new(now, "test");
        stale.set_region(Region::Caiso, vec![obs(datetime!(2026-05-13 10:00 UTC))]);
        let r = assemble(&a, now, CacheState::Present(Box::new(stale))).unwrap();
        assert!(
            r.warnings.iter().any(|w| w.contains("STALE DATA")),
            "old newest sample must be flagged STALE despite a fresh file"
        );

        // Fresh: a recent sample (1h old) ⇒ no STALE warning, plus a
        // prior-week same-hour sample so the run-now hour is forecast.
        let mut fresh = HistoricalCache::new(now, "test");
        fresh.set_region(
            Region::Caiso,
            vec![
                obs(datetime!(2026-05-13 10:00 UTC)),
                obs(datetime!(2026-05-20 09:00 UTC)),
            ],
        );
        let r = assemble(&a, now, CacheState::Present(Box::new(fresh))).unwrap();
        assert!(
            !r.warnings.iter().any(|w| w.contains("STALE DATA")),
            "a recent newest sample must not be flagged STALE"
        );
    }

    #[test]
    fn summary_language_is_compliant() {
        let now = datetime!(2026-05-20 06:00 UTC);
        let a = args_for(Some(Region::Caiso), datetime!(2026-05-20 18:00 UTC), 2);
        let r = assemble(&a, now, CacheState::Missing).unwrap();
        // The header reflects the invoking subcommand (run must not
        // mislabel itself as preview).
        assert!(human_summary(&r, "preview").starts_with("nami preview — region CAISO"));
        assert!(human_summary(&r, "run").starts_with("nami run — region CAISO"));
        let text = human_summary(&r, "preview").to_lowercase();
        assert!(text.contains("run immediately"));
        assert!(text.contains("not marginal emissions"));
        // On the static fallback path the basis line must not claim a
        // historical-pattern forecast (CLAUDE.md: don't overclaim).
        assert!(text.contains("static annual regional mean"));
        assert!(!text.contains("historical-pattern forecast"));
        for banned in [
            "cleanest possible",
            "optimal carbon",
            "guaranteed",
            "real-time carbon",
            "precise grid",
        ] {
            assert!(!text.contains(banned), "banned phrase present: {banned}");
        }
    }

    #[test]
    fn missing_region_is_an_error() {
        let now = datetime!(2026-05-20 06:00 UTC);
        let a = args_for(None, datetime!(2026-05-20 18:00 UTC), 2);
        assert!(assemble(&a, now, CacheState::Missing).is_err());
    }

    #[test]
    fn past_deadline_is_an_error() {
        let now = datetime!(2026-05-20 19:00 UTC);
        let a = args_for(Some(Region::Caiso), datetime!(2026-05-20 18:00 UTC), 2);
        assert!(assemble(&a, now, CacheState::Missing).is_err());
    }
}
