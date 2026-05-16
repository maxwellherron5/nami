//! Materiality threshold logic.
//!
//! A lower-carbon recommendation is only offered if the estimated
//! improvement over running now is large enough to matter. This module
//! owns both the default threshold and the pure comparison that decides
//! whether the best candidate clears it.
//!
//! The verdict here is intentionally *not* a [`SchedulingDecision`]: it
//! carries no confidence, freshness, or reason mapping. The scheduler
//! (Phase 0 implementation item 10) maps a [`MaterialityVerdict`] onto a
//! decision, attaching the appropriate
//! [`StartReason`](nami_core::StartReason) /
//! [`RefuseReason`](nami_core::RefuseReason) for its context.
//!
//! See `docs/confidence-and-materiality.md` for the rationale behind the
//! default value and `docs/methodology.md` for the formula.
//!
//! [`SchedulingDecision`]: nami_core::SchedulingDecision

use nami_core::CarbonIntensity;

/// Default materiality threshold, as a percentage improvement of the
/// selected window's estimated average intensity over running now.
///
/// A candidate window must beat run-now by at least this percentage
/// before the scheduler will recommend deferring to it. Conservative by
/// design: forecast variance frequently exceeds this, and average
/// intensity is not marginal emissions.
pub const DEFAULT_MATERIALITY_THRESHOLD_PCT: f64 = 5.0;

/// The outcome of a materiality assessment.
///
/// `index` (when present) is the position in the `candidates` slice
/// passed to [`assess_materiality`], so the caller can recover the
/// corresponding window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaterialityVerdict {
    /// The best candidate beats run-now by at least the threshold.
    MateriallyCleaner {
        /// Index of the winning candidate in the input slice.
        index: usize,
        /// Its improvement over run-now, in percent (always `>= threshold`).
        improvement_pct: f64,
    },
    /// No candidate clears the threshold — run now. `best_improvement_pct`
    /// is the best improvement we *did* find (which may be `<= 0` when
    /// run-now was already the cleanest option), or `None` when there
    /// were no candidates at all.
    NotMaterial {
        /// Best improvement found, even though it didn't clear the bar.
        best_improvement_pct: Option<f64>,
    },
}

/// Percentage improvement of `candidate` over `run_now`:
/// `(run_now - candidate) / run_now * 100`.
///
/// Positive means cleaner than running now. Returns `None` when
/// `run_now` is zero (no defensible baseline to compute a ratio against —
/// realistically unreachable, since grid intensity is never exactly
/// zero, but handled rather than dividing by zero).
fn improvement_pct(run_now: f64, candidate: f64) -> Option<f64> {
    if run_now <= 0.0 {
        return None;
    }
    Some((run_now - candidate) / run_now * 100.0)
}

/// Decide whether the cleanest candidate window is materially cleaner
/// than running now.
///
/// The best candidate is the one with the lowest estimated intensity;
/// ties resolve to the lowest index (the earliest window, since
/// candidates are ordered by start time). The comparison is inclusive:
/// an improvement exactly equal to `threshold_pct` counts as material,
/// matching `docs/methodology.md`.
///
/// # Examples
///
/// ```
/// use nami_core::CarbonIntensity;
/// use nami_scheduler::{assess_materiality, MaterialityVerdict};
///
/// let run_now = CarbonIntensity::new(391.0).unwrap();
/// let candidates = [
///     CarbonIntensity::new(380.0).unwrap(), // ~2.8% — not enough
///     CarbonIntensity::new(318.0).unwrap(), // ~18.7% — material
/// ];
/// match assess_materiality(run_now, &candidates, 5.0) {
///     MaterialityVerdict::MateriallyCleaner { index, .. } => assert_eq!(index, 1),
///     v => panic!("expected MateriallyCleaner, got {v:?}"),
/// }
/// ```
pub fn assess_materiality(
    run_now: CarbonIntensity,
    candidates: &[CarbonIntensity],
    threshold_pct: f64,
) -> MaterialityVerdict {
    let run_now = run_now.value();

    // Lowest intensity wins; ties keep the earliest index. CarbonIntensity
    // guarantees finite values, so partial_cmp never yields None here.
    let best = candidates
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.value()
                .partial_cmp(&b.value())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, ci)| (i, ci.value()));

    let Some((index, best_intensity)) = best else {
        return MaterialityVerdict::NotMaterial {
            best_improvement_pct: None,
        };
    };

    match improvement_pct(run_now, best_intensity) {
        Some(pct) if pct >= threshold_pct => MaterialityVerdict::MateriallyCleaner {
            index,
            improvement_pct: pct,
        },
        Some(pct) => MaterialityVerdict::NotMaterial {
            best_improvement_pct: Some(pct),
        },
        None => MaterialityVerdict::NotMaterial {
            best_improvement_pct: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ci(v: f64) -> CarbonIntensity {
        CarbonIntensity::new(v).unwrap()
    }

    #[test]
    fn clears_threshold_picks_cleanest() {
        let v = assess_materiality(ci(391.0), &[ci(380.0), ci(318.0)], 5.0);
        match v {
            MaterialityVerdict::MateriallyCleaner {
                index,
                improvement_pct,
            } => {
                assert_eq!(index, 1);
                assert!((improvement_pct - 18.67).abs() < 0.05);
            }
            other => panic!("expected MateriallyCleaner, got {other:?}"),
        }
    }

    #[test]
    fn below_threshold_is_not_material_but_reports_best() {
        let v = assess_materiality(ci(100.0), &[ci(98.0)], 5.0);
        match v {
            MaterialityVerdict::NotMaterial {
                best_improvement_pct: Some(p),
            } => assert!((p - 2.0).abs() < 1e-9),
            other => panic!("expected NotMaterial(Some), got {other:?}"),
        }
    }

    #[test]
    fn exactly_at_threshold_is_material() {
        // 100 -> 95 is exactly 5.0%.
        let v = assess_materiality(ci(100.0), &[ci(95.0)], 5.0);
        assert!(matches!(
            v,
            MaterialityVerdict::MateriallyCleaner { index: 0, .. }
        ));
    }

    #[test]
    fn run_now_already_cleanest_yields_negative_best() {
        let v = assess_materiality(ci(200.0), &[ci(250.0), ci(300.0)], 5.0);
        match v {
            MaterialityVerdict::NotMaterial {
                best_improvement_pct: Some(p),
            } => assert!(p < 0.0, "expected negative improvement, got {p}"),
            other => panic!("expected NotMaterial(Some negative), got {other:?}"),
        }
    }

    #[test]
    fn no_candidates_is_not_material_with_none() {
        let v = assess_materiality(ci(391.0), &[], 5.0);
        assert_eq!(
            v,
            MaterialityVerdict::NotMaterial {
                best_improvement_pct: None
            }
        );
    }

    #[test]
    fn ties_resolve_to_earliest_index() {
        let v = assess_materiality(ci(400.0), &[ci(300.0), ci(300.0)], 5.0);
        match v {
            MaterialityVerdict::MateriallyCleaner { index, .. } => assert_eq!(index, 0),
            other => panic!("expected MateriallyCleaner index 0, got {other:?}"),
        }
    }

    #[test]
    fn zero_baseline_is_not_material() {
        let v = assess_materiality(ci(0.0), &[ci(0.0)], 5.0);
        assert_eq!(
            v,
            MaterialityVerdict::NotMaterial {
                best_improvement_pct: None
            }
        );
    }
}
