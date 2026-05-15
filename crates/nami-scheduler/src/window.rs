//! Candidate execution-window generation.
//!
//! Given the current time, a deadline, and a job's estimated duration,
//! enumerate the set of *candidate* start windows the scheduler may later
//! score against a forecast (Phase 0 implementation item 10).
//!
//! Phase 0 decisions baked in here (see `docs/methodology.md`):
//!
//! - **Hourly resolution.** Candidate starts are aligned to UTC hour
//!   boundaries. EIA-930 is hourly; finer alignment would imply a
//!   precision the data cannot support.
//! - **Deadline is inclusive.** A window whose end lands exactly on the
//!   deadline is allowed — [`nami_core::JobSpec`] defines the deadline as
//!   the latest moment the job may *finish*.
//! - **"Run now" is not a candidate here.** The scheduler treats running
//!   immediately (start == `now`, possibly mid-hour) as a separate
//!   baseline. This function only enumerates *deferred*, hour-aligned
//!   options.

use time::{Duration, OffsetDateTime};

use crate::error::{Error, Result};

/// One candidate execution window: the job would occupy
/// `[start, start + duration)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateWindow {
    /// UTC start instant, aligned to an hour boundary.
    pub start: OffsetDateTime,
    /// The job's estimated duration (echoed for convenience).
    pub duration: Duration,
}

impl CandidateWindow {
    /// The exclusive end of the window (`start + duration`), or `None` if
    /// that computation would overflow the representable range.
    pub fn end(&self) -> Option<OffsetDateTime> {
        self.start.checked_add(self.duration)
    }
}

/// The UTC hour boundary at or before `t` (minutes, seconds, and
/// sub-seconds zeroed).
fn floor_to_hour(t: OffsetDateTime) -> Result<OffsetDateTime> {
    let secs_into_hour = t.unix_timestamp().rem_euclid(3600);
    let drop = Duration::seconds(secs_into_hour) + Duration::nanoseconds(i64::from(t.nanosecond()));
    t.checked_sub(drop)
        .ok_or_else(|| Error::TimeOutOfRange(format!("flooring {t} to the hour underflowed")))
}

/// Generate hour-aligned candidate start windows in `[now, deadline]` such
/// that the entire job fits before `deadline`.
///
/// - The first candidate is the earliest whole UTC hour `>= now` (which
///   is `now` itself when `now` is already on an hour boundary).
/// - The last candidate is the latest hour-aligned start with
///   `start + duration <= deadline`.
/// - Returns an empty vector when the job cannot fit before the deadline,
///   when `duration` is non-positive, or when `deadline <= now`. These are
///   not errors — the scheduler decides what an empty candidate set means
///   (run now, refuse, etc.).
///
/// The result is ordered ascending by `start`.
///
/// # Examples
///
/// ```
/// use nami_scheduler::candidate_windows;
/// use time::Duration;
/// use time::macros::datetime;
///
/// let now = datetime!(2030-01-01 10:00 UTC);
/// let deadline = datetime!(2030-01-01 14:00 UTC);
/// let windows = candidate_windows(now, deadline, Duration::hours(1)).unwrap();
/// // 10:00, 11:00, 12:00, 13:00 (13:00 + 1h == 14:00, inclusive).
/// assert_eq!(windows.len(), 4);
/// assert_eq!(windows[0].start, now);
/// ```
pub fn candidate_windows(
    now: OffsetDateTime,
    deadline: OffsetDateTime,
    duration: Duration,
) -> Result<Vec<CandidateWindow>> {
    if duration <= Duration::ZERO || deadline <= now {
        return Ok(Vec::new());
    }

    let floored = floor_to_hour(now)?;
    let mut start = if floored == now {
        now
    } else {
        floored
            .checked_add(Duration::hours(1))
            .ok_or_else(|| Error::TimeOutOfRange("advancing to next hour overflowed".into()))?
    };

    let mut windows = Vec::new();
    loop {
        let Some(end) = start.checked_add(duration) else {
            break;
        };
        if end > deadline {
            break;
        }
        windows.push(CandidateWindow { start, duration });
        let Some(next) = start.checked_add(Duration::hours(1)) else {
            break;
        };
        start = next;
    }
    Ok(windows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn aligned_now_includes_now_as_first_candidate() {
        let now = datetime!(2030-01-01 10:00 UTC);
        let deadline = datetime!(2030-01-01 14:00 UTC);
        let w = candidate_windows(now, deadline, Duration::hours(1)).unwrap();
        assert_eq!(w.len(), 4);
        assert_eq!(w[0].start, now);
        assert_eq!(w[3].start, datetime!(2030-01-01 13:00 UTC));
        assert_eq!(w[3].end().unwrap(), deadline); // inclusive
    }

    #[test]
    fn unaligned_now_starts_at_next_whole_hour() {
        let now = datetime!(2030-01-01 10:15:30 UTC);
        let deadline = datetime!(2030-01-01 14:00 UTC);
        let w = candidate_windows(now, deadline, Duration::hours(1)).unwrap();
        assert_eq!(
            w.iter().map(|c| c.start).collect::<Vec<_>>(),
            vec![
                datetime!(2030-01-01 11:00 UTC),
                datetime!(2030-01-01 12:00 UTC),
                datetime!(2030-01-01 13:00 UTC),
            ]
        );
    }

    #[test]
    fn sub_second_now_still_aligns_forward() {
        let now = datetime!(2030-01-01 10:00:00.5 UTC);
        let deadline = datetime!(2030-01-01 12:00 UTC);
        let w = candidate_windows(now, deadline, Duration::hours(1)).unwrap();
        // 10:00:00.5 is past the 10:00 boundary, so first candidate is 11:00.
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].start, datetime!(2030-01-01 11:00 UTC));
    }

    #[test]
    fn exact_fit_yields_single_window() {
        let now = datetime!(2030-01-01 10:00 UTC);
        let deadline = datetime!(2030-01-01 12:00 UTC);
        let w = candidate_windows(now, deadline, Duration::hours(2)).unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].start, now);
        assert_eq!(w[0].end().unwrap(), deadline);
    }

    #[test]
    fn job_too_long_for_deadline_yields_nothing() {
        let now = datetime!(2030-01-01 10:00 UTC);
        let deadline = datetime!(2030-01-01 11:00 UTC);
        let w = candidate_windows(now, deadline, Duration::hours(2)).unwrap();
        assert!(w.is_empty());
    }

    #[test]
    fn sub_hour_duration_still_hour_aligned_starts() {
        let now = datetime!(2030-01-01 10:00 UTC);
        let deadline = datetime!(2030-01-01 12:00 UTC);
        let w = candidate_windows(now, deadline, Duration::minutes(30)).unwrap();
        // Hour-aligned starts whose 30-min window finishes by 12:00:
        // 10:00 -> 10:30 ok, 11:00 -> 11:30 ok, 12:00 -> 12:30 too late.
        assert_eq!(
            w.iter().map(|c| c.start).collect::<Vec<_>>(),
            vec![
                datetime!(2030-01-01 10:00 UTC),
                datetime!(2030-01-01 11:00 UTC),
            ]
        );
    }

    #[test]
    fn zero_or_negative_duration_yields_nothing() {
        let now = datetime!(2030-01-01 10:00 UTC);
        let deadline = datetime!(2030-01-01 14:00 UTC);
        assert!(
            candidate_windows(now, deadline, Duration::ZERO)
                .unwrap()
                .is_empty()
        );
        assert!(
            candidate_windows(now, deadline, Duration::hours(-1))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn deadline_before_now_yields_nothing() {
        let now = datetime!(2030-01-01 10:00 UTC);
        let deadline = datetime!(2030-01-01 09:00 UTC);
        assert!(
            candidate_windows(now, deadline, Duration::hours(1))
                .unwrap()
                .is_empty()
        );
    }
}
