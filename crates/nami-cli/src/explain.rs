//! `nami report explain <path>`: render a friendly prose explanation
//! of a persisted `RunReport`.
//!
//! `nami status --report` summarizes a report compactly; this command
//! is the *narrative* counterpart, intended for "why did nami choose
//! this?" — pulling the materiality threshold, the improvement
//! percentage, the run-now baseline, the confidence level + sample
//! count, the freshness state, and the methodology label into a
//! coherent story, with the decision-specific framing the user is
//! actually asking about.
//!
//! Pure `explain(&RunReport) -> String` so the prose is unit-testable
//! against synthetic reports without touching the filesystem.

use anyhow::{Context, Result};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use nami_core::{DataFreshness, RefuseReason, RunReport, SchedulingDecision, StartReason};

use crate::ExplainArgs;

pub fn run(args: ExplainArgs) -> Result<()> {
    let text = std::fs::read_to_string(&args.report)
        .with_context(|| format!("reading {}", args.report.display()))?;
    let report: RunReport = serde_json::from_str(&text)
        .with_context(|| format!("parsing {}", args.report.display()))?;
    print!("{}", explain(&report));
    Ok(())
}

/// Pure prose rendering. Branches on `SchedulingDecision` so each
/// outcome gets its own framing ("why did nami defer / run now /
/// refuse?"), then appends shared provenance and job context.
pub fn explain(r: &RunReport) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "nami explain — report submitted {}\n\n",
        fmt_dt(r.submitted_at)
    ));

    match &r.decision {
        SchedulingDecision::StartAt {
            start_time,
            reason,
            confidence,
        } => {
            s.push_str(&format!(
                "Decision: defer to {} (reason: {:?}).\n",
                fmt_dt(*start_time),
                reason
            ));
            if let (Some(rn), Some(sel), Some(imp)) = (
                r.run_now_estimate.as_ref(),
                r.selected_window_estimate.as_ref(),
                r.estimated_improvement_pct,
            ) {
                s.push_str(&format!(
                    "  Estimated average intensity in the selected window is \
                     {:.0} gCO2/kWh, versus {:.0} gCO2/kWh if you ran now — a \
                     {:.1}% improvement, against the materiality threshold of \
                     {:.1}%.\n",
                    sel.mean_intensity.value(),
                    rn.mean_intensity.value(),
                    imp,
                    r.materiality_threshold_pct
                ));
            }
            s.push_str(&format!(
                "\nConfidence: {:?} ({} sample(s) backed the selected window).\n",
                confidence.level, confidence.sample_count
            ));
            push_confidence_notes(&mut s, confidence);
        }
        SchedulingDecision::StartImmediately { reason, confidence } => {
            s.push_str(&format!(
                "Decision: run immediately (reason: {:?}).\n",
                reason
            ));
            s.push_str(&format!("  {}\n", start_now_sentence(reason)));
            if let Some(rn) = r.run_now_estimate.as_ref() {
                match r.estimated_improvement_pct {
                    Some(imp) => s.push_str(&format!(
                        "  Run-now estimate: {:.0} gCO2/kWh. The best deferred \
                         candidate beat that by only {:.1}%, against the \
                         {:.1}% materiality threshold.\n",
                        rn.mean_intensity.value(),
                        imp,
                        r.materiality_threshold_pct
                    )),
                    None => s.push_str(&format!(
                        "  Run-now estimate: {:.0} gCO2/kWh. No deferred \
                         candidate was scorable, or none beat the {:.1}% \
                         materiality threshold.\n",
                        rn.mean_intensity.value(),
                        r.materiality_threshold_pct
                    )),
                }
            }
            s.push_str(&format!(
                "\nConfidence: {:?} ({} sample(s)).\n",
                confidence.level, confidence.sample_count
            ));
            push_confidence_notes(&mut s, confidence);
        }
        SchedulingDecision::Refuse { reason } => {
            s.push_str(&format!("Decision: refused (reason: {:?}).\n", reason));
            s.push_str(&format!(
                "  {}\n",
                refuse_sentence(reason, r.materiality_threshold_pct)
            ));
            s.push_str(
                "\nNo run-now estimate, no selected window — nami declined to \
                 produce a number it could not defend.\n",
            );
        }
    }

    // Shared provenance.
    s.push_str(&format!(
        "\nProvider: {} ({:?} granularity).\n",
        r.provider.name, r.provider.granularity
    ));
    s.push_str(&format!("Methodology: {}.\n", r.methodology_version));
    s.push_str(&format!(
        "Data freshness: {}.\n",
        freshness_label(&r.data_freshness)
    ));

    // Job context.
    s.push_str(&format!("\nJob: {} {}\n", r.command, r.args.join(" ")));
    s.push_str(&format!(
        "Region: {} — deadline {} — estimated duration {}\n",
        r.region,
        fmt_dt(r.deadline),
        fmt_dur(r.estimated_duration)
    ));
    if let (Some(start), Some(end)) = (r.started_at, r.finished_at) {
        let code = r
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "—".into());
        s.push_str(&format!(
            "Started: {} — finished: {} — exit code: {code}\n",
            fmt_dt(start),
            fmt_dt(end)
        ));
    }

    if !r.warnings.is_empty() {
        s.push_str("\nWarnings:\n");
        for w in &r.warnings {
            s.push_str(&format!("  - {w}\n"));
        }
    }

    s.push_str("\nReminder: estimates are average carbon intensity, not marginal emissions.\n");
    s
}

fn start_now_sentence(reason: &StartReason) -> &'static str {
    match reason {
        StartReason::RunNowAlreadyCleanest => {
            "no deferred window beat run-now by the materiality threshold — \
             running now is at least as clean as anything between now and the deadline."
        }
        StartReason::DeadlineTooSoon => {
            "the deadline left no room to defer to a later hour-aligned start."
        }
        StartReason::FallbackPolicyRunImmediately => {
            "the model could not produce a real forecast (static fallback or \
             missing data) and defaulted to running now rather than guessing."
        }
        StartReason::LowestEstimatedIntensity => {
            "running now is the lowest-intensity option among the considered windows."
        }
        StartReason::UserForced => "you explicitly forced an immediate start.",
    }
}

fn refuse_sentence(reason: &RefuseReason, threshold_pct: f64) -> String {
    match reason {
        RefuseReason::NoWindowBeforeDeadline => {
            "no hour-aligned window of the requested duration fits before the \
             deadline — the job simply cannot finish in time."
                .to_string()
        }
        RefuseReason::ForecastTooUncertain => {
            "the forecast did not cover the run-now hour, so there is no \
             baseline to measure a candidate window against (refuse-to-estimate)."
                .to_string()
        }
        RefuseReason::CandidateWindowsBelowMaterialityThreshold => format!(
            "no candidate window beat run-now by the {threshold_pct:.1}% \
                 materiality threshold — the available improvements are inside \
                 the forecast's noise floor."
        ),
        RefuseReason::UnsupportedRegion => {
            "the requested region is not supported by any configured provider.".to_string()
        }
        RefuseReason::MissingHistoricalData => {
            "required historical data was missing from the cache.".to_string()
        }
        RefuseReason::StaleHistoricalCache => {
            "the historical cache was older than the staleness bound — refusing \
             rather than basing a decision on stale data."
                .to_string()
        }
        RefuseReason::InsufficientSamples => {
            "too few historical samples backed the forecast for a defensible \
             estimate."
                .to_string()
        }
        RefuseReason::ProviderUnavailable => {
            "the data provider was unavailable; no defensible estimate exists \
             without it."
                .to_string()
        }
    }
}

fn push_confidence_notes(s: &mut String, c: &nami_core::Confidence) {
    if c.notes.is_empty() {
        return;
    }
    s.push_str("  Why this confidence level:\n");
    for n in &c.notes {
        s.push_str(&format!("    - {n}\n"));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::{
        CarbonIntensity, Confidence, ConfidenceLevel, DataGranularity, ProviderInfo, Region,
        WindowEstimate,
    };
    use time::macros::datetime;

    fn base() -> RunReport {
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
                name: "eia-egrid-historical-pattern".into(),
                capabilities: vec![],
                granularity: DataGranularity::Hourly,
                expected_lag: None,
            },
            data_freshness: DataFreshness::HistoricalCacheOnly {
                newest_sample_at: datetime!(2026-05-20 04:00 UTC),
            },
            methodology_version: "historical-pattern-mean-8w-hour-dow-month-v1".into(),
            warnings: vec!["not marginal emissions".into()],
            submitted_at: datetime!(2026-05-20 06:00 UTC),
            started_at: None,
            finished_at: None,
            wall_duration: None,
            exit_code: None,
        }
    }

    fn conf(level: ConfidenceLevel, n: usize, note: &str) -> Confidence {
        Confidence {
            level,
            sample_count: n,
            interval: None,
            notes: vec![note.to_string()],
        }
    }

    #[test]
    fn explain_start_at_includes_improvement_and_threshold() {
        let mut r = base();
        r.decision = SchedulingDecision::StartAt {
            start_time: datetime!(2026-05-20 14:00 UTC),
            reason: StartReason::LowestEstimatedIntensity,
            confidence: conf(
                ConfidenceLevel::Medium,
                4,
                "cache-only basis caps to Low → Medium",
            ),
        };
        r.run_now_estimate = Some(WindowEstimate {
            start: datetime!(2026-05-20 06:00 UTC),
            duration: Duration::hours(2),
            mean_intensity: CarbonIntensity::new(412.0).unwrap(),
            confidence: conf(ConfidenceLevel::Medium, 4, ""),
        });
        r.selected_window_estimate = Some(WindowEstimate {
            start: datetime!(2026-05-20 14:00 UTC),
            duration: Duration::hours(2),
            mean_intensity: CarbonIntensity::new(298.0).unwrap(),
            confidence: conf(ConfidenceLevel::Medium, 4, ""),
        });
        r.estimated_improvement_pct = Some(27.7);

        let out = explain(&r);
        assert!(out.contains("Decision: defer to 2026-05-20T14:00:00Z"));
        assert!(out.contains("298 gCO2/kWh"));
        assert!(out.contains("412 gCO2/kWh"));
        assert!(out.contains("27.7% improvement"));
        assert!(out.contains("5.0%"));
        assert!(out.contains("Confidence: Medium (4 sample(s)"));
        assert!(out.contains("Why this confidence level:"));
        assert!(out.contains("historical-cache-only"));
    }

    #[test]
    fn explain_start_immediately_run_now_already_cleanest() {
        let mut r = base();
        r.decision = SchedulingDecision::StartImmediately {
            reason: StartReason::RunNowAlreadyCleanest,
            confidence: conf(ConfidenceLevel::Low, 2, "cache-only basis caps at Low"),
        };
        r.run_now_estimate = Some(WindowEstimate {
            start: datetime!(2026-05-20 06:00 UTC),
            duration: Duration::hours(2),
            mean_intensity: CarbonIntensity::new(312.0).unwrap(),
            confidence: conf(ConfidenceLevel::Low, 2, ""),
        });
        r.estimated_improvement_pct = Some(2.4);

        let out = explain(&r);
        assert!(out.contains("Decision: run immediately"));
        assert!(out.contains("RunNowAlreadyCleanest"));
        assert!(out.contains("running now is at least as clean"));
        assert!(out.contains("312 gCO2/kWh"));
        assert!(out.contains("beat that by only 2.4%"));
        assert!(out.contains("5.0% materiality"));
    }

    #[test]
    fn explain_refuse_no_window_before_deadline() {
        let r = base(); // default decision is Refuse(NoWindowBeforeDeadline).
        let out = explain(&r);
        assert!(out.contains("Decision: refused"));
        assert!(out.contains("NoWindowBeforeDeadline"));
        assert!(out.contains("the job simply cannot finish in time"));
        assert!(out.contains("declined to produce a number"));
    }

    #[test]
    fn explain_refuse_below_materiality_threshold_quotes_threshold() {
        let mut r = base();
        r.decision = SchedulingDecision::Refuse {
            reason: RefuseReason::CandidateWindowsBelowMaterialityThreshold,
        };
        r.materiality_threshold_pct = 7.5;
        let out = explain(&r);
        assert!(out.contains("7.5% materiality threshold"));
        assert!(out.contains("noise floor"));
    }

    #[test]
    fn explain_includes_provenance_and_warnings_and_marginal_disclaimer() {
        let r = base();
        let out = explain(&r);
        assert!(out.contains("Provider: eia-egrid-historical-pattern"));
        assert!(out.contains("Methodology: historical-pattern-mean-8w-hour-dow-month-v1"));
        assert!(out.contains("Data freshness: historical-cache-only"));
        assert!(out.contains("Job: python train.py"));
        assert!(out.contains("Region: MISO"));
        // The warning we seeded is rendered.
        assert!(out.contains("- not marginal emissions"));
        // The standing disclaimer the project insists on.
        assert!(out.contains("not marginal emissions"));
    }
}
