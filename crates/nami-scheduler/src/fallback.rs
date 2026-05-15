//! Fallback scheduling policy.
//!
//! When no forecast-capable provider is available — only the static
//! annual-mean table — there is no time-varying signal, and therefore no
//! materially cleaner window to find. The honest outcome is to run
//! immediately and say so loudly. This is a *complete* policy, not a
//! placeholder: the windowed scheduler (item 10) handles the case where a
//! real forecast exists.

use nami_core::{Confidence, SchedulingDecision, StartReason};

/// Build the scheduling decision used when only a static baseline is
/// available.
///
/// Returns [`SchedulingDecision::StartImmediately`] with
/// [`StartReason::FallbackPolicyRunImmediately`]. The caller supplies the
/// `confidence`, which should already reflect the degraded data state
/// (typically `VeryLow`, e.g. from the static fallback provider's
/// `baseline_confidence`).
///
/// # Examples
///
/// ```
/// use nami_core::{Confidence, ConfidenceLevel, SchedulingDecision, StartReason};
/// use nami_scheduler::static_fallback_decision;
///
/// let decision = static_fallback_decision(Confidence::very_low("no provider"));
/// match decision {
///     SchedulingDecision::StartImmediately { reason, confidence } => {
///         assert_eq!(reason, StartReason::FallbackPolicyRunImmediately);
///         assert_eq!(confidence.level, ConfidenceLevel::VeryLow);
///     }
///     _ => panic!("expected StartImmediately"),
/// }
/// ```
pub fn static_fallback_decision(confidence: Confidence) -> SchedulingDecision {
    SchedulingDecision::StartImmediately {
        reason: StartReason::FallbackPolicyRunImmediately,
        confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::ConfidenceLevel;

    #[test]
    fn fallback_is_start_immediately_with_given_confidence() {
        let decision = static_fallback_decision(Confidence::very_low("static only"));
        match decision {
            SchedulingDecision::StartImmediately { reason, confidence } => {
                assert_eq!(reason, StartReason::FallbackPolicyRunImmediately);
                assert_eq!(confidence.level, ConfidenceLevel::VeryLow);
                assert_eq!(confidence.sample_count, 0);
                assert!(!confidence.notes.is_empty());
            }
            other => panic!("expected StartImmediately, got {other:?}"),
        }
    }
}
