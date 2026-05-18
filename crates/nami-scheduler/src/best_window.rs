//! `BestWindowScheduler`: the Phase 0 scheduling policy.
//!
//! Combines the pieces built earlier into a single decision:
//!
//! 1. Reject jobs that cannot even *run now* before the deadline
//!    ([`RefuseReason::NoWindowBeforeDeadline`]).
//! 2. Require a non-empty forecast; score the **run-now** window
//!    `[now, now + D)` as the baseline. If the forecast does not cover
//!    that window there is no baseline to measure improvement against, so
//!    refuse ([`RefuseReason::ForecastTooUncertain`]) — materiality is
//!    defined relative to run-now and we will not assert an unverifiable
//!    improvement.
//! 3. Enumerate hour-aligned [`candidate_windows`] before the deadline
//!    and score each by the **duration-weighted mean** of the forecast
//!    intensity over the hourly buckets it overlaps. A window is
//!    *unscorable* (skipped) if any overlapped hour is missing from the
//!    forecast — we never score on partial data.
//! 4. Feed run-now + scorable candidates to [`assess_materiality`]:
//!    - materially cleaner ⇒ [`SchedulingDecision::StartAt`]
//!      ([`StartReason::LowestEstimatedIntensity`]);
//!    - otherwise ⇒ [`SchedulingDecision::StartImmediately`]
//!      ([`StartReason::RunNowAlreadyCleanest`]). A sub-threshold
//!      difference is within forecast noise; the job still must run.
//!
//! Special cases: no hour-aligned deferred window fits before the
//! deadline (but run-now does) ⇒ `StartImmediately`
//! ([`StartReason::DeadlineTooSoon`]); candidates existed but none were
//! scorable (forecast gaps) ⇒ `StartImmediately`
//! ([`StartReason::RunNowAlreadyCleanest`]) with an explanatory note.
//!
//! Decision [`Confidence`] for a window is the **most conservative**
//! level across the hourly forecast points it overlaps, with their
//! sample counts summed — consistent with the worst-of-three confidence
//! philosophy elsewhere in `nami`.

use time::{Duration, OffsetDateTime};

use nami_core::{
    CarbonIntensity, Confidence, ConfidenceLevel, ForecastPoint, JobSpec, RefuseReason, Scheduler,
    SchedulingDecision, StartReason,
};

use crate::materiality::{
    DEFAULT_MATERIALITY_THRESHOLD_PCT, MaterialityVerdict, assess_materiality,
};
use crate::window::{candidate_windows, floor_to_hour};

/// The Phase 0 scheduler: pick the cleanest contiguous window before the
/// deadline if it materially beats running now.
#[derive(Debug, Clone, Copy)]
pub struct BestWindowScheduler {
    materiality_pct: f64,
}

impl Default for BestWindowScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl BestWindowScheduler {
    /// Construct with the default materiality threshold
    /// ([`DEFAULT_MATERIALITY_THRESHOLD_PCT`]).
    pub fn new() -> Self {
        Self {
            materiality_pct: DEFAULT_MATERIALITY_THRESHOLD_PCT,
        }
    }

    /// Construct with a custom materiality threshold (percent improvement
    /// over run-now required before a deferral is recommended).
    pub fn with_materiality(materiality_pct: f64) -> Self {
        Self { materiality_pct }
    }
}

/// A window's score: its duration-weighted mean intensity plus the
/// aggregated confidence of the forecast points that backed it.
struct Scored {
    intensity: CarbonIntensity,
    confidence: Confidence,
}

/// Score `[start, start + duration)` against the hourly forecast.
///
/// Returns `None` if any overlapped hour is absent from the forecast
/// (unscorable — we do not estimate on partial data) or if the result is
/// not a valid intensity.
fn score_window(
    start: OffsetDateTime,
    duration: Duration,
    forecast: &[ForecastPoint],
) -> Option<Scored> {
    let end = start.checked_add(duration)?;
    let mut bucket = floor_to_hour(start).ok()?;

    let mut weighted = 0.0_f64;
    let mut total = 0.0_f64;
    let mut worst = ConfidenceLevel::High;
    let mut samples = 0usize;
    let mut levels_seen = 0usize;

    while bucket < end {
        let bucket_end = bucket.checked_add(Duration::hours(1))?;
        let lo = if start > bucket { start } else { bucket };
        let hi = if end < bucket_end { end } else { bucket_end };
        let overlap = (hi - lo).as_seconds_f64();
        if overlap > 0.0 {
            // Every overlapped hour must be present in the forecast.
            let point = forecast.iter().find(|p| p.at == bucket)?;
            weighted += overlap * point.intensity.value();
            total += overlap;
            worst = worst.max(point.confidence.level);
            samples += point.confidence.sample_count;
            levels_seen += 1;
        }
        bucket = bucket_end;
    }

    if total <= 0.0 {
        return None;
    }
    let intensity = CarbonIntensity::new(weighted / total).ok()?;
    let confidence = Confidence {
        level: worst,
        sample_count: samples,
        interval: None,
        notes: vec![format!(
            "window confidence = most conservative of {levels_seen} hourly forecast point(s)"
        )],
    };
    Some(Scored {
        intensity,
        confidence,
    })
}

impl Scheduler for BestWindowScheduler {
    fn decide(
        &self,
        job: &JobSpec,
        forecast: &[ForecastPoint],
        now: OffsetDateTime,
    ) -> SchedulingDecision {
        let duration = job.estimated_duration;

        // 1. Can the job even run now and finish before the deadline?
        let run_now_end = match now.checked_add(duration) {
            Some(e) => e,
            None => {
                return SchedulingDecision::Refuse {
                    reason: RefuseReason::NoWindowBeforeDeadline,
                };
            }
        };
        if run_now_end > job.deadline {
            return SchedulingDecision::Refuse {
                reason: RefuseReason::NoWindowBeforeDeadline,
            };
        }

        // 2. No forecast → nothing to schedule on.
        if forecast.is_empty() {
            return SchedulingDecision::Refuse {
                reason: RefuseReason::ForecastTooUncertain,
            };
        }

        // Run-now baseline. No baseline ⇒ cannot assess materiality.
        let Some(run_now) = score_window(now, duration, forecast) else {
            return SchedulingDecision::Refuse {
                reason: RefuseReason::ForecastTooUncertain,
            };
        };

        // 3. Enumerate + score deferred candidates.
        let candidates = match candidate_windows(now, job.deadline, duration) {
            Ok(c) => c,
            Err(_) => {
                return SchedulingDecision::Refuse {
                    reason: RefuseReason::NoWindowBeforeDeadline,
                };
            }
        };

        if candidates.is_empty() {
            // Run-now fits (checked above) but no hour-aligned deferral
            // does: there is simply no room to shift.
            return SchedulingDecision::StartImmediately {
                reason: StartReason::DeadlineTooSoon,
                confidence: run_now.confidence,
            };
        }

        let mut scorable: Vec<(OffsetDateTime, Scored)> = Vec::new();
        for c in &candidates {
            if let Some(s) = score_window(c.start, duration, forecast) {
                scorable.push((c.start, s));
            }
        }

        if scorable.is_empty() {
            // Candidates existed but the forecast couldn't back any of
            // them. We have a run-now estimate; running now is the
            // honest, safe answer.
            let mut confidence = run_now.confidence;
            confidence.notes.push(format!(
                "{} candidate window(s) unscorable (forecast gaps); no \
                 trustworthy cleaner window",
                candidates.len()
            ));
            return SchedulingDecision::StartImmediately {
                reason: StartReason::RunNowAlreadyCleanest,
                confidence,
            };
        }

        // 4. Materiality.
        let candidate_intensities: Vec<CarbonIntensity> =
            scorable.iter().map(|(_, s)| s.intensity).collect();
        match assess_materiality(
            run_now.intensity,
            &candidate_intensities,
            self.materiality_pct,
        ) {
            MaterialityVerdict::MateriallyCleaner { index, .. } => {
                let (start_time, scored) = &scorable[index];
                SchedulingDecision::StartAt {
                    start_time: *start_time,
                    reason: StartReason::LowestEstimatedIntensity,
                    confidence: scored.confidence.clone(),
                }
            }
            MaterialityVerdict::NotMaterial { .. } => SchedulingDecision::StartImmediately {
                reason: StartReason::RunNowAlreadyCleanest,
                confidence: run_now.confidence,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::Region;
    use time::macros::datetime;

    fn conf(level: ConfidenceLevel, n: usize) -> Confidence {
        Confidence {
            level,
            sample_count: n,
            interval: None,
            notes: vec![],
        }
    }

    fn fp(at: OffsetDateTime, v: f64, level: ConfidenceLevel) -> ForecastPoint {
        ForecastPoint {
            at,
            intensity: CarbonIntensity::new(v).unwrap(),
            confidence: conf(level, 5),
            methodology: "test".into(),
        }
    }

    fn job(now_plus_hours_deadline: i64, dur_h: i64) -> JobSpec {
        JobSpec {
            command: vec!["x".into()],
            estimated_duration: Duration::hours(dur_h),
            deadline: datetime!(2026-05-20 10:00 UTC) + Duration::hours(now_plus_hours_deadline),
            region: Region::Caiso,
        }
    }

    const NOW: OffsetDateTime = datetime!(2026-05-20 10:00 UTC);

    #[test]
    fn picks_materially_cleaner_window() {
        // Hours 10..=19 all 400 except 14:00 = 280 (30% cleaner).
        let mut fc: Vec<ForecastPoint> = (0..10)
            .map(|h| fp(NOW + Duration::hours(h), 400.0, ConfidenceLevel::Low))
            .collect();
        fc[4] = fp(NOW + Duration::hours(4), 280.0, ConfidenceLevel::Low); // 14:00
        let s = BestWindowScheduler::new();
        let d = s.decide(&job(10, 1), &fc, NOW);
        match d {
            SchedulingDecision::StartAt {
                start_time, reason, ..
            } => {
                assert_eq!(start_time, datetime!(2026-05-20 14:00 UTC));
                assert_eq!(reason, StartReason::LowestEstimatedIntensity);
            }
            other => panic!("expected StartAt, got {other:?}"),
        }
    }

    #[test]
    fn sub_threshold_runs_now() {
        // Best candidate only ~1% cleaner than run-now.
        let mut fc: Vec<ForecastPoint> = (0..10)
            .map(|h| fp(NOW + Duration::hours(h), 400.0, ConfidenceLevel::Low))
            .collect();
        fc[4] = fp(NOW + Duration::hours(4), 396.0, ConfidenceLevel::Low);
        let d = BestWindowScheduler::new().decide(&job(10, 1), &fc, NOW);
        assert!(matches!(
            d,
            SchedulingDecision::StartImmediately {
                reason: StartReason::RunNowAlreadyCleanest,
                ..
            }
        ));
    }

    #[test]
    fn empty_forecast_refuses() {
        let d = BestWindowScheduler::new().decide(&job(10, 1), &[], NOW);
        assert!(matches!(
            d,
            SchedulingDecision::Refuse {
                reason: RefuseReason::ForecastTooUncertain
            }
        ));
    }

    #[test]
    fn missing_run_now_baseline_refuses() {
        // Forecast covers 13:00.. but NOT the run-now hour 10:00.
        let fc: Vec<ForecastPoint> = (3..10)
            .map(|h| fp(NOW + Duration::hours(h), 300.0, ConfidenceLevel::Low))
            .collect();
        let d = BestWindowScheduler::new().decide(&job(10, 1), &fc, NOW);
        assert!(matches!(
            d,
            SchedulingDecision::Refuse {
                reason: RefuseReason::ForecastTooUncertain
            }
        ));
    }

    #[test]
    fn cannot_finish_before_deadline_refuses() {
        // D = 2h but deadline is now + 1h.
        let fc = vec![fp(NOW, 400.0, ConfidenceLevel::Low)];
        let d = BestWindowScheduler::new().decide(&job(1, 2), &fc, NOW);
        assert!(matches!(
            d,
            SchedulingDecision::Refuse {
                reason: RefuseReason::NoWindowBeforeDeadline
            }
        ));
    }

    #[test]
    fn no_room_to_defer_starts_immediately_deadline_too_soon() {
        // Unaligned now so no hour-aligned deferred window fits, but
        // run-now does.
        let now = datetime!(2026-05-20 10:30 UTC);
        let jb = JobSpec {
            command: vec!["x".into()],
            estimated_duration: Duration::hours(1),
            deadline: datetime!(2026-05-20 11:45 UTC),
            region: Region::Caiso,
        };
        let fc = vec![
            fp(datetime!(2026-05-20 10:00 UTC), 400.0, ConfidenceLevel::Low),
            fp(datetime!(2026-05-20 11:00 UTC), 400.0, ConfidenceLevel::Low),
        ];
        let d = BestWindowScheduler::new().decide(&jb, &fc, now);
        assert!(matches!(
            d,
            SchedulingDecision::StartImmediately {
                reason: StartReason::DeadlineTooSoon,
                ..
            }
        ));
    }

    #[test]
    fn candidates_unscorable_runs_now() {
        // Unaligned now + sub-hour job: run-now [10:30,10:50) needs only
        // bucket 10:00 (present). Deferred candidates start at 11:00,
        // 12:00, … whose buckets are all absent from the forecast.
        let now = datetime!(2026-05-20 10:30 UTC);
        let jb = JobSpec {
            command: vec!["x".into()],
            estimated_duration: Duration::minutes(20),
            deadline: datetime!(2026-05-20 20:00 UTC),
            region: Region::Caiso,
        };
        let fc = vec![fp(
            datetime!(2026-05-20 10:00 UTC),
            400.0,
            ConfidenceLevel::Low,
        )];
        let d = BestWindowScheduler::new().decide(&jb, &fc, now);
        match d {
            SchedulingDecision::StartImmediately {
                reason: StartReason::RunNowAlreadyCleanest,
                confidence,
            } => assert!(confidence.notes.iter().any(|n| n.contains("unscorable"))),
            other => panic!("expected StartImmediately/RunNowAlreadyCleanest, got {other:?}"),
        }
    }

    #[test]
    fn window_confidence_is_most_conservative() {
        // 2h job spanning 10:00 (Medium) and 11:00 (Low) → Low.
        let fc = vec![
            fp(NOW, 400.0, ConfidenceLevel::Medium),
            fp(NOW + Duration::hours(1), 400.0, ConfidenceLevel::Low),
            fp(NOW + Duration::hours(2), 400.0, ConfidenceLevel::Medium),
        ];
        // Force a deferral: make 12:00 much cleaner is impossible (only 3
        // pts); instead check run-now confidence aggregation via the
        // sub-threshold path (StartImmediately carries run_now.confidence).
        let d = BestWindowScheduler::new().decide(&job(3, 2), &fc, NOW);
        let c = match d {
            SchedulingDecision::StartImmediately { confidence, .. } => confidence,
            SchedulingDecision::StartAt { confidence, .. } => confidence,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(c.level, ConfidenceLevel::Low); // worst of Medium+Low
        assert_eq!(c.sample_count, 10); // 5 + 5
    }
}
