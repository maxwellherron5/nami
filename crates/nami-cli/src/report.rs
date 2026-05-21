//! `nami report summary`: aggregate over the auto-archived reports
//! directory to answer "how often did nami defer, and by how much?"
//!
//! Walks `<reports-dir>/<UTC-date>/*.json` (the layout `nami run`
//! writes), filters by `--since` and optionally `--region`, then
//! returns counts of deferred / run-immediately / refused decisions,
//! improvement statistics when deferred, confidence distribution, top
//! refusal reasons, and per-region counts. Corrupt files are skipped
//! and counted, not fatal — a single bad JSON shouldn't poison the
//! whole aggregation.
//!
//! Pure `summarize(&[RunReport], window_start, ...) -> Summary` and
//! `render(&Summary, ...)` are separated from the IO walk so the
//! aggregation logic is unit-testable without a real reports dir.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use time::{Duration, OffsetDateTime};

use nami_core::{ConfidenceLevel, RefuseReason, Region, RunReport, SchedulingDecision};

use crate::reports::default_state_dir;
use crate::{ReportArgs, ReportSubcommand, SummaryArgs};

pub fn run(args: ReportArgs) -> Result<()> {
    match args.sub {
        ReportSubcommand::Summary(s) => summary(s),
    }
}

fn summary(args: SummaryArgs) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    let window_start = now - args.since;

    let dir = args
        .reports_dir
        .clone()
        .or_else(default_state_dir)
        .ok_or_else(|| {
            anyhow!(
                "no reports directory; set $XDG_STATE_HOME / $HOME or \
                 pass --reports-dir"
            )
        })?;

    let (reports, skipped) = walk(&dir, window_start)?;
    let s = summarize(&reports, window_start, now, args.region, &dir, skipped);
    if args.json {
        let json = serde_json::to_string_pretty(&JsonSummary::from(&s, args.since))
            .map_err(|e| anyhow!("serialising summary: {e}"))?;
        println!("{json}");
    } else {
        print!("{}", render(&s, args.since));
    }
    Ok(())
}

/// What `summarize` returns. Kept pure-data so both the human renderer
/// and the JSON serializer build from the same numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub reports_dir: PathBuf,
    pub window_start: OffsetDateTime,
    pub window_end: OffsetDateTime,
    pub region_filter: Option<Region>,
    pub total: usize,
    pub deferred: usize,
    pub run_immediately: usize,
    pub refused: usize,
    pub improvement_pcts: Vec<f64>,
    pub confidence: BTreeMap<&'static str, usize>,
    pub regions: BTreeMap<String, usize>,
    pub refusal_reasons: BTreeMap<&'static str, usize>,
    pub skipped_files: usize,
}

pub fn summarize(
    reports: &[RunReport],
    window_start: OffsetDateTime,
    window_end: OffsetDateTime,
    region_filter: Option<Region>,
    reports_dir: &Path,
    skipped_files: usize,
) -> Summary {
    let mut s = Summary {
        reports_dir: reports_dir.to_path_buf(),
        window_start,
        window_end,
        region_filter,
        total: 0,
        deferred: 0,
        run_immediately: 0,
        refused: 0,
        improvement_pcts: Vec::new(),
        confidence: empty_confidence_buckets(),
        regions: BTreeMap::new(),
        refusal_reasons: BTreeMap::new(),
        skipped_files,
    };

    for r in reports {
        if r.submitted_at < window_start || r.submitted_at > window_end {
            continue;
        }
        if let Some(want) = region_filter {
            if r.region != want {
                continue;
            }
        }
        s.total += 1;
        *s.regions.entry(r.region.as_code().to_string()).or_default() += 1;

        match &r.decision {
            SchedulingDecision::StartAt { confidence, .. } => {
                s.deferred += 1;
                bump_confidence(&mut s.confidence, confidence.level);
                if let Some(p) = r.estimated_improvement_pct {
                    if p.is_finite() {
                        s.improvement_pcts.push(p);
                    }
                }
            }
            SchedulingDecision::StartImmediately { confidence, .. } => {
                s.run_immediately += 1;
                bump_confidence(&mut s.confidence, confidence.level);
            }
            SchedulingDecision::Refuse { reason } => {
                s.refused += 1;
                *s.refusal_reasons.entry(refuse_label(reason)).or_default() += 1;
            }
        }
    }

    s
}

fn empty_confidence_buckets() -> BTreeMap<&'static str, usize> {
    BTreeMap::from([("High", 0), ("Medium", 0), ("Low", 0), ("VeryLow", 0)])
}

fn bump_confidence(buckets: &mut BTreeMap<&'static str, usize>, level: ConfidenceLevel) {
    let key = match level {
        ConfidenceLevel::High => "High",
        ConfidenceLevel::Medium => "Medium",
        ConfidenceLevel::Low => "Low",
        ConfidenceLevel::VeryLow => "VeryLow",
    };
    *buckets.entry(key).or_default() += 1;
}

fn refuse_label(reason: &RefuseReason) -> &'static str {
    match reason {
        RefuseReason::UnsupportedRegion => "UnsupportedRegion",
        RefuseReason::MissingHistoricalData => "MissingHistoricalData",
        RefuseReason::StaleHistoricalCache => "StaleHistoricalCache",
        RefuseReason::InsufficientSamples => "InsufficientSamples",
        RefuseReason::ProviderUnavailable => "ProviderUnavailable",
        RefuseReason::NoWindowBeforeDeadline => "NoWindowBeforeDeadline",
        RefuseReason::CandidateWindowsBelowMaterialityThreshold => {
            "CandidateWindowsBelowMaterialityThreshold"
        }
        RefuseReason::ForecastTooUncertain => "ForecastTooUncertain",
    }
}

pub fn render(s: &Summary, since: Duration) -> String {
    let mut out = String::new();
    let region_clause = match s.region_filter {
        Some(r) => format!(" — region {r}"),
        None => String::new(),
    };
    out.push_str(&format!(
        "nami report summary — {} run(s) in the last {}{region_clause}\n",
        s.total,
        fmt_dur(since),
    ));
    out.push_str(&format!(
        "                     (from {})\n\n",
        s.reports_dir.display()
    ));

    out.push_str("Decisions:\n");
    out.push_str(&format!("  start-at (deferred)    : {}\n", s.deferred));
    out.push_str(&format!(
        "  start-immediately      : {}\n",
        s.run_immediately
    ));
    out.push_str(&format!("  refused                : {}\n\n", s.refused));

    if !s.improvement_pcts.is_empty() {
        let stats = improvement_stats(&s.improvement_pcts);
        out.push_str("When deferred, estimated improvement:\n");
        out.push_str(&format!("  mean   : {:.1}%\n", stats.mean));
        out.push_str(&format!("  median : {:.1}%\n", stats.median));
        out.push_str(&format!(
            "  range  : {:.1}% — {:.1}%\n\n",
            stats.min, stats.max
        ));
    }

    out.push_str("Confidence distribution:\n");
    // Render in conventional order, not alphabetical.
    for level in ["High", "Medium", "Low", "VeryLow"] {
        out.push_str(&format!(
            "  {level:<8} : {}\n",
            s.confidence.get(level).copied().unwrap_or(0)
        ));
    }
    out.push('\n');

    if s.region_filter.is_none() && !s.regions.is_empty() {
        out.push_str("Regions: ");
        let parts: Vec<String> = s
            .regions
            .iter()
            .map(|(r, n)| format!("{r} ({n})"))
            .collect();
        out.push_str(&parts.join(", "));
        out.push('\n');
    }

    if !s.refusal_reasons.is_empty() {
        out.push_str(&format!("\nRefusal reasons ({}):\n", s.refused));
        for (reason, n) in &s.refusal_reasons {
            out.push_str(&format!("  {reason}: {n}\n"));
        }
    }

    if s.skipped_files > 0 {
        out.push_str(&format!(
            "\nSkipped {} unparseable file(s).\n",
            s.skipped_files
        ));
    }
    out
}

struct ImprovementStats {
    mean: f64,
    median: f64,
    min: f64,
    max: f64,
}

fn improvement_stats(xs: &[f64]) -> ImprovementStats {
    debug_assert!(!xs.is_empty());
    let mean = xs.iter().copied().sum::<f64>() / xs.len() as f64;
    let mut sorted: Vec<f64> = xs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let median = if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    };
    let min = sorted[0];
    let max = sorted[n - 1];
    ImprovementStats {
        mean,
        median,
        min,
        max,
    }
}

/// JSON-friendly view: mirrors `Summary` but with explicit field names
/// for the public surface (the in-memory `Summary` is internal).
#[derive(Debug, Serialize)]
struct JsonSummary<'a> {
    since: String,
    #[serde(with = "time::serde::rfc3339")]
    window_start: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    window_end: OffsetDateTime,
    reports_dir: String,
    region_filter: Option<String>,
    total_runs: usize,
    decisions: JsonDecisions,
    improvement_when_deferred: Option<JsonImprovement>,
    confidence_distribution: &'a BTreeMap<&'static str, usize>,
    regions: &'a BTreeMap<String, usize>,
    refusal_reasons: &'a BTreeMap<&'static str, usize>,
    skipped_files: usize,
}

#[derive(Debug, Serialize)]
struct JsonDecisions {
    deferred: usize,
    run_immediately: usize,
    refused: usize,
}

#[derive(Debug, Serialize)]
struct JsonImprovement {
    mean_pct: f64,
    median_pct: f64,
    min_pct: f64,
    max_pct: f64,
}

impl<'a> JsonSummary<'a> {
    fn from(s: &'a Summary, since: Duration) -> Self {
        let imp = if s.improvement_pcts.is_empty() {
            None
        } else {
            let st = improvement_stats(&s.improvement_pcts);
            Some(JsonImprovement {
                mean_pct: st.mean,
                median_pct: st.median,
                min_pct: st.min,
                max_pct: st.max,
            })
        };
        JsonSummary {
            since: fmt_dur(since),
            window_start: s.window_start,
            window_end: s.window_end,
            reports_dir: s.reports_dir.display().to_string(),
            region_filter: s.region_filter.map(|r| r.as_code().to_string()),
            total_runs: s.total,
            decisions: JsonDecisions {
                deferred: s.deferred,
                run_immediately: s.run_immediately,
                refused: s.refused,
            },
            improvement_when_deferred: imp,
            confidence_distribution: &s.confidence,
            regions: &s.regions,
            refusal_reasons: &s.refusal_reasons,
            skipped_files: s.skipped_files,
        }
    }
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

/// Walk `<dir>/<YYYY-MM-DD>/*.json`. Returns `(reports, skipped_count)`.
/// Filters at the date-directory level by `window_start` so a huge
/// reports tree doesn't deserialize months we'll discard; the precise
/// `submitted_at` check happens later in `summarize`.
fn walk(dir: &Path, window_start: OffsetDateTime) -> Result<(Vec<RunReport>, usize)> {
    if !dir.exists() {
        return Err(anyhow!(
            "no reports directory at {} — run `nami run …` to populate it, or pass --reports-dir",
            dir.display()
        ));
    }

    let cutoff = window_start.date();
    let mut reports = Vec::new();
    let mut skipped = 0usize;

    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading reports directory {}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !is_in_window(name, cutoff) {
            continue;
        }
        // Walk JSONs inside the date dir.
        let inner = match std::fs::read_dir(&path) {
            Ok(it) => it,
            Err(e) => {
                eprintln!("nami: skipping {} ({e})", path.display());
                continue;
            }
        };
        for f in inner.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match std::fs::read_to_string(&p) {
                Ok(text) => match serde_json::from_str::<RunReport>(&text) {
                    Ok(r) => reports.push(r),
                    Err(_) => skipped += 1,
                },
                Err(_) => skipped += 1,
            }
        }
    }
    Ok((reports, skipped))
}

/// True iff `name` parses as `YYYY-MM-DD` and the date is `>= cutoff`.
/// Anything that doesn't match the expected date layout is skipped,
/// not errored — a stray file in the reports dir shouldn't break the
/// whole walk.
fn is_in_window(name: &str, cutoff: time::Date) -> bool {
    let format = time::macros::format_description!("[year]-[month]-[day]");
    match time::Date::parse(name, format) {
        Ok(d) => d >= cutoff,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::{
        CarbonIntensity, Confidence, DataFreshness, DataGranularity, ProviderInfo, StartReason,
        WindowEstimate,
    };
    use time::macros::datetime;

    fn now() -> OffsetDateTime {
        datetime!(2026-05-20 12:00 UTC)
    }

    fn base_report(region: Region) -> RunReport {
        RunReport {
            command: "python".into(),
            args: vec!["train.py".into()],
            region,
            deadline: datetime!(2026-05-20 18:00 UTC),
            estimated_duration: Duration::hours(1),
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
            methodology_version: "test-v1".into(),
            warnings: vec![],
            submitted_at: datetime!(2026-05-20 11:00 UTC),
            started_at: None,
            finished_at: None,
            wall_duration: None,
            exit_code: None,
        }
    }

    fn deferred(region: Region, improvement: f64, level: ConfidenceLevel) -> RunReport {
        let mut r = base_report(region);
        r.decision = SchedulingDecision::StartAt {
            start_time: datetime!(2026-05-20 15:00 UTC),
            reason: StartReason::LowestEstimatedIntensity,
            confidence: Confidence {
                level,
                sample_count: 6,
                interval: None,
                notes: vec![],
            },
        };
        r.estimated_improvement_pct = Some(improvement);
        r.selected_window_estimate = Some(WindowEstimate {
            start: datetime!(2026-05-20 15:00 UTC),
            duration: Duration::hours(1),
            mean_intensity: CarbonIntensity::new(280.0).unwrap(),
            confidence: Confidence {
                level,
                sample_count: 6,
                interval: None,
                notes: vec![],
            },
        });
        r
    }

    fn ran_now(region: Region, level: ConfidenceLevel) -> RunReport {
        let mut r = base_report(region);
        r.decision = SchedulingDecision::StartImmediately {
            reason: StartReason::RunNowAlreadyCleanest,
            confidence: Confidence {
                level,
                sample_count: 3,
                interval: None,
                notes: vec![],
            },
        };
        r
    }

    fn refused(region: Region, reason: RefuseReason) -> RunReport {
        let mut r = base_report(region);
        r.decision = SchedulingDecision::Refuse { reason };
        r
    }

    #[test]
    fn empty_input_yields_zero_counts() {
        let s = summarize(
            &[],
            datetime!(2026-04-20 00:00 UTC),
            now(),
            None,
            Path::new("/tmp/reports"),
            0,
        );
        assert_eq!(
            (s.total, s.deferred, s.run_immediately, s.refused),
            (0, 0, 0, 0)
        );
        assert!(s.regions.is_empty());
        assert!(s.improvement_pcts.is_empty());
    }

    #[test]
    fn aggregates_a_mixed_set() {
        let reports = vec![
            deferred(Region::Caiso, 18.0, ConfidenceLevel::Medium),
            deferred(Region::Caiso, 12.0, ConfidenceLevel::Low),
            ran_now(Region::Miso, ConfidenceLevel::Low),
            ran_now(Region::Caiso, ConfidenceLevel::Medium),
            refused(Region::Miso, RefuseReason::NoWindowBeforeDeadline),
        ];
        let s = summarize(
            &reports,
            datetime!(2026-04-20 00:00 UTC),
            now(),
            None,
            Path::new("/tmp/reports"),
            0,
        );
        assert_eq!(s.total, 5);
        assert_eq!((s.deferred, s.run_immediately, s.refused), (2, 2, 1));
        assert_eq!(s.regions.get("CAISO"), Some(&3));
        assert_eq!(s.regions.get("MISO"), Some(&2));
        assert_eq!(s.confidence.get("Medium"), Some(&2));
        assert_eq!(s.confidence.get("Low"), Some(&2));
        // Improvement stats only over deferrals.
        assert_eq!(s.improvement_pcts.len(), 2);
        let stats = improvement_stats(&s.improvement_pcts);
        assert!((stats.mean - 15.0).abs() < 1e-9);
        assert_eq!(s.refusal_reasons.get("NoWindowBeforeDeadline"), Some(&1));
    }

    #[test]
    fn region_filter_excludes_other_regions() {
        let reports = vec![
            deferred(Region::Caiso, 10.0, ConfidenceLevel::Low),
            deferred(Region::Miso, 20.0, ConfidenceLevel::Low),
        ];
        let s = summarize(
            &reports,
            datetime!(2026-04-20 00:00 UTC),
            now(),
            Some(Region::Miso),
            Path::new("/tmp/reports"),
            0,
        );
        assert_eq!(s.total, 1);
        assert_eq!(s.regions.get("MISO"), Some(&1));
        assert!(!s.regions.contains_key("CAISO"));
        assert!((s.improvement_pcts[0] - 20.0).abs() < 1e-9);
    }

    #[test]
    fn window_filter_excludes_old_reports() {
        let mut old = deferred(Region::Caiso, 10.0, ConfidenceLevel::Low);
        old.submitted_at = datetime!(2026-01-01 00:00 UTC);
        let mut fresh = deferred(Region::Caiso, 10.0, ConfidenceLevel::Low);
        fresh.submitted_at = datetime!(2026-05-19 00:00 UTC);
        let reports = vec![old, fresh];
        let s = summarize(
            &reports,
            datetime!(2026-04-20 00:00 UTC),
            now(),
            None,
            Path::new("/tmp/reports"),
            0,
        );
        assert_eq!(s.total, 1);
        assert_eq!(s.deferred, 1);
    }

    #[test]
    fn render_human_summary_has_expected_sections() {
        let reports = vec![
            deferred(Region::Caiso, 18.0, ConfidenceLevel::Medium),
            ran_now(Region::Miso, ConfidenceLevel::Low),
            refused(Region::Miso, RefuseReason::ForecastTooUncertain),
        ];
        let s = summarize(
            &reports,
            datetime!(2026-04-20 00:00 UTC),
            now(),
            None,
            Path::new("/tmp/reports"),
            2,
        );
        let out = render(&s, Duration::days(30));
        assert!(out.contains("3 run(s) in the last 30d"));
        assert!(out.contains("start-at (deferred)    : 1"));
        assert!(out.contains("start-immediately      : 1"));
        assert!(out.contains("refused                : 1"));
        assert!(out.contains("mean   : 18.0%"));
        assert!(out.contains("ForecastTooUncertain: 1"));
        assert!(out.contains("Regions:"));
        assert!(out.contains("Skipped 2 unparseable file(s)."));
    }

    #[test]
    fn render_omits_improvement_block_when_no_deferrals() {
        let reports = vec![ran_now(Region::Miso, ConfidenceLevel::Low)];
        let s = summarize(
            &reports,
            datetime!(2026-04-20 00:00 UTC),
            now(),
            None,
            Path::new("/tmp/reports"),
            0,
        );
        let out = render(&s, Duration::days(30));
        assert!(!out.contains("When deferred"));
    }

    #[test]
    fn json_summary_round_trips_with_expected_keys() {
        let reports = vec![deferred(Region::Caiso, 14.2, ConfidenceLevel::Medium)];
        let s = summarize(
            &reports,
            datetime!(2026-04-20 00:00 UTC),
            now(),
            None,
            Path::new("/tmp/reports"),
            0,
        );
        let json = serde_json::to_string(&JsonSummary::from(&s, Duration::days(30))).unwrap();
        // Keys we promise to the user / scripts:
        for k in [
            "total_runs",
            "decisions",
            "improvement_when_deferred",
            "confidence_distribution",
            "regions",
            "refusal_reasons",
            "skipped_files",
            "window_start",
            "window_end",
        ] {
            assert!(json.contains(k), "missing JSON key {k}");
        }
        // The mean is the single improvement value.
        assert!(json.contains("\"mean_pct\":14.2"));
    }
}
