//! `nami preview`: compute a recommendation and emit a [`RunReport`]
//! without executing the wrapped command.
//!
//! Phase 0 item 1 scope: the only data path wired here is the static
//! fallback. With no forecast-capable provider available there is no
//! time-varying signal, so the decision is deterministically
//! "run immediately" at `VeryLow` confidence, with every degradation
//! surfaced in the report's warnings and freshness fields. The EIA
//! provider and the windowed scheduler will slot into this same flow in
//! later sessions.

use anyhow::{Result, anyhow, bail};
use time::OffsetDateTime;

use nami_carbon_static::StaticTableProvider;
use nami_core::{DataFreshness, JobSpec, ProviderMetadata, RunReport, Sink, WindowEstimate};
use nami_scheduler::{DEFAULT_MATERIALITY_THRESHOLD_PCT, static_fallback_decision};

use crate::RunArgs;
use crate::sink::ReportSink;

/// Methodology label for the static-fallback path. Mirrors the row in
/// `docs/methodology.md`.
const METHODOLOGY_VERSION: &str = "static-fallback-annual-v1";

/// Run `nami preview`: build the report and write it to the chosen sink.
pub fn run(args: RunArgs) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    let report = build_report(&args, now)?;

    let sink = match &args.report {
        Some(path) => ReportSink::File(path.clone()),
        None => ReportSink::Stdout,
    };
    sink.record(&report)
        .map_err(|e| anyhow!("failed to write run report: {e}"))?;
    Ok(())
}

/// Assemble the [`RunReport`] for the static-fallback preview path.
///
/// Pure given `now`, so it is unit-testable without a clock or process.
fn build_report(args: &RunArgs, now: OffsetDateTime) -> Result<RunReport> {
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

    let provider = StaticTableProvider::new();
    if !provider.supports(region) {
        bail!("static fallback table has no entry for region {region}");
    }
    let baseline = provider
        .baseline(region)
        .map_err(|e| anyhow!("static baseline lookup failed: {e}"))?;
    let confidence = StaticTableProvider::baseline_confidence();

    // `clap` requires at least one command token, and `JobSpec::validate`
    // re-checks; this split is therefore infallible here.
    let (command, cmd_args) = job
        .command
        .split_first()
        .ok_or_else(|| anyhow!("empty command"))?;

    let run_now = WindowEstimate {
        start: now,
        duration: args.duration,
        mean_intensity: baseline,
        confidence: confidence.clone(),
    };

    Ok(RunReport {
        command: command.clone(),
        args: cmd_args.to_vec(),
        region,
        deadline: args.deadline,
        estimated_duration: args.duration,
        decision: static_fallback_decision(confidence),
        run_now_estimate: Some(run_now),
        selected_window_estimate: None,
        estimated_improvement_pct: None,
        materiality_threshold_pct: DEFAULT_MATERIALITY_THRESHOLD_PCT,
        provider: provider.info(),
        data_freshness: DataFreshness::StaticFallbackOnly,
        methodology_version: METHODOLOGY_VERSION.to_string(),
        warnings: vec![
            "No EIA-930 provider available in this build; used the static \
             annual-mean fallback table. This is a coarse baseline, not a \
             forecast."
                .to_string(),
            "Estimate is average carbon intensity, not marginal emissions.".to_string(),
            "Confidence is VeryLow: no time-varying signal was used. \
             Recommendation is to run immediately."
                .to_string(),
        ],
        submitted_at: now,
        started_at: None,
        finished_at: None,
        wall_duration: None,
        exit_code: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::{Region, SchedulingDecision, StartReason};
    use time::Duration;
    use time::macros::datetime;

    fn args() -> RunArgs {
        RunArgs {
            duration: Duration::hours(2),
            deadline: datetime!(2030-01-01 12:00 UTC),
            region: Some(Region::Caiso),
            report: None,
            command: vec!["python".into(), "train.py".into()],
        }
    }

    #[test]
    fn builds_static_fallback_report() {
        let now = datetime!(2030-01-01 06:00 UTC);
        let r = build_report(&args(), now).unwrap();

        assert_eq!(r.command, "python");
        assert_eq!(r.args, vec!["train.py".to_string()]);
        assert_eq!(r.region, Region::Caiso);
        assert_eq!(r.materiality_threshold_pct, 5.0);
        assert_eq!(r.data_freshness, DataFreshness::StaticFallbackOnly);
        assert_eq!(r.provider.name, "static-fallback");
        assert!(r.provider.capabilities.is_empty());
        assert!(r.selected_window_estimate.is_none());
        assert!(r.estimated_improvement_pct.is_none());
        assert!(r.run_now_estimate.is_some());
        assert!(r.exit_code.is_none());
        assert!(!r.warnings.is_empty());

        match r.decision {
            SchedulingDecision::StartImmediately { reason, .. } => {
                assert_eq!(reason, StartReason::FallbackPolicyRunImmediately);
            }
            other => panic!("expected StartImmediately, got {other:?}"),
        }
    }

    #[test]
    fn report_round_trips_through_json() {
        let now = datetime!(2030-01-01 06:00 UTC);
        let r = build_report(&args(), now).unwrap();
        let json = serde_json::to_string(&r).unwrap();
        let back: RunReport = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn missing_region_is_an_error() {
        let mut a = args();
        a.region = None;
        assert!(build_report(&a, datetime!(2030-01-01 06:00 UTC)).is_err());
    }

    #[test]
    fn past_deadline_is_an_error() {
        let now = datetime!(2030-01-01 13:00 UTC); // after the 12:00 deadline
        assert!(build_report(&args(), now).is_err());
    }
}
