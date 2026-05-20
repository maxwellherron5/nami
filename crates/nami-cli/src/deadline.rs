//! Relative-deadline parsing for `--within` and `--by`.
//!
//! `--within <duration>` (e.g. `8h`) is a thin wrapper around the
//! existing duration parser: the deadline is `now + duration`.
//!
//! `--by <time-of-day>` accepts compact forms like `7am`, `7:30am`,
//! `19:30`, `tomorrow-9am`, `tomorrow 14:00`, `today 23:00`. The
//! resolution is the **next occurrence** of that time-of-day, in UTC —
//! the host's local timezone is intentionally not consulted, because
//! `time::UtcOffset::current_local_offset` is unsound under tokio's
//! multi-threaded runtime, and silently guessing a timezone is exactly
//! the kind of guess this tool refuses to make. The resolved instant
//! is always echoed back on stderr ("nami: deadline … (from --by 7am)
//! [UTC]") so the user can verify and switch to `--deadline` with an
//! explicit RFC 3339 offset if they want a different timezone.

use anyhow::Result;
use time::format_description::well_known::Rfc3339;
use time::{Date, Duration, OffsetDateTime, PrimitiveDateTime, Time};

use crate::RunArgs;

/// A parsed `--by` value, resolved against a runtime `now`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByTime {
    hour: u8,
    minute: u8,
    /// 0 = today's next occurrence; ≥1 = N days from today.
    day_offset: i64,
    /// The raw input string, kept for the stderr provenance line.
    pub raw: String,
}

impl ByTime {
    /// Resolve to a UTC instant against `now`. If `day_offset == 0`
    /// and the candidate time-of-day is already past, bumps to
    /// tomorrow — so `--by 7am` at 09:00 picks tomorrow's 7am, never
    /// a stale deadline in the past.
    pub fn resolve(&self, now: OffsetDateTime) -> Result<OffsetDateTime, String> {
        let time = Time::from_hms(self.hour, self.minute, 0)
            .map_err(|e| format!("invalid time {}:{}: {e}", self.hour, self.minute))?;
        let base_date = add_days(now.date(), self.day_offset)?;
        let candidate = PrimitiveDateTime::new(base_date, time).assume_utc();
        if self.day_offset == 0 && candidate <= now {
            let next = add_days(base_date, 1)?;
            Ok(PrimitiveDateTime::new(next, time).assume_utc())
        } else {
            Ok(candidate)
        }
    }
}

fn add_days(date: Date, n: i64) -> Result<Date, String> {
    let secs = n
        .checked_mul(86_400)
        .ok_or_else(|| format!("day offset {n} overflows"))?;
    date.checked_add(Duration::seconds(secs))
        .ok_or_else(|| format!("date arithmetic overflowed: {date} + {n}d"))
}

/// Parse a `--by` value. Used as clap's `value_parser`.
pub fn parse_by(s: &str) -> Result<ByTime, String> {
    let raw = s.to_string();
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("--by value is empty".to_string());
    }
    let lower = trimmed.to_ascii_lowercase();

    let (day_offset, rest) = if let Some(r) = strip_prefix_ci(&lower, "tomorrow-") {
        (1i64, r)
    } else if let Some(r) = strip_prefix_ci(&lower, "tomorrow ") {
        (1i64, r)
    } else if let Some(r) = strip_prefix_ci(&lower, "today-") {
        (0i64, r)
    } else if let Some(r) = strip_prefix_ci(&lower, "today ") {
        (0i64, r)
    } else {
        (0i64, lower.clone())
    };

    let (hour, minute) = parse_time_of_day(rest.trim())?;
    Ok(ByTime {
        hour,
        minute,
        day_offset,
        raw,
    })
}

fn strip_prefix_ci(haystack: &str, prefix: &str) -> Option<String> {
    haystack
        .strip_prefix(prefix)
        .map(|r| r.trim_start().to_string())
}

fn parse_time_of_day(s: &str) -> Result<(u8, u8), String> {
    let (rest, ampm) = if let Some(r) = s.strip_suffix("am") {
        (r.trim_end(), Some(false))
    } else if let Some(r) = s.strip_suffix("pm") {
        (r.trim_end(), Some(true))
    } else {
        (s, None)
    };

    let (h_str, m_str) = match rest.split_once(':') {
        Some((h, m)) => (h, m),
        None => (rest, "0"),
    };
    let h: u32 = h_str
        .trim()
        .parse()
        .map_err(|_| format!("could not parse hour in {s:?}"))?;
    let m: u32 = m_str
        .trim()
        .parse()
        .map_err(|_| format!("could not parse minute in {s:?}"))?;

    if m >= 60 {
        return Err(format!("minute {m} out of range in {s:?}"));
    }

    let hour: u32 = match ampm {
        Some(false) => {
            if h == 0 || h > 12 {
                return Err(format!(
                    "12-hour clock hour {h} invalid in {s:?} (use 1-12 with am/pm)"
                ));
            }
            if h == 12 { 0 } else { h }
        }
        Some(true) => {
            if h == 0 || h > 12 {
                return Err(format!(
                    "12-hour clock hour {h} invalid in {s:?} (use 1-12 with am/pm)"
                ));
            }
            if h == 12 { 12 } else { h + 12 }
        }
        None => {
            if h >= 24 {
                return Err(format!(
                    "24-hour clock hour {h} out of range in {s:?} (use 0-23)"
                ));
            }
            h
        }
    };
    Ok((hour as u8, m as u8))
}

/// Reduce CLI `--within` / `--by` to a concrete `args.deadline` *before*
/// profile merge runs, so CLI deadlines win over profile-supplied ones.
/// Echoes the resolved instant on stderr when it came from --within /
/// --by (the user typed a relative form; surfacing the absolute UTC
/// answer is part of being honest).
pub fn normalize(args: &mut RunArgs, now: OffsetDateTime) -> Result<()> {
    if args.deadline.is_some() {
        return Ok(());
    }
    if let Some(d) = args.within {
        let dl = now + d;
        eprintln!(
            "nami: deadline {} (from --within {})",
            fmt_dt(dl),
            fmt_dur(d)
        );
        args.deadline = Some(dl);
        return Ok(());
    }
    if let Some(by) = args.by.clone() {
        let dl = by
            .resolve(now)
            .map_err(|e| anyhow::anyhow!("--by {:?}: {e}", by.raw))?;
        eprintln!("nami: deadline {} (from --by {}) [UTC]", fmt_dt(dl), by.raw);
        args.deadline = Some(dl);
    }
    Ok(())
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
    use time::macros::datetime;

    fn now() -> OffsetDateTime {
        datetime!(2026-05-20 06:00 UTC)
    }

    #[test]
    fn parse_simple_12h() {
        let b = parse_by("7am").unwrap();
        assert_eq!((b.hour, b.minute, b.day_offset), (7, 0, 0));
        let b = parse_by("7:30am").unwrap();
        assert_eq!((b.hour, b.minute, b.day_offset), (7, 30, 0));
        let b = parse_by("7:30pm").unwrap();
        assert_eq!((b.hour, b.minute, b.day_offset), (19, 30, 0));
        let b = parse_by("12am").unwrap();
        assert_eq!(b.hour, 0);
        let b = parse_by("12pm").unwrap();
        assert_eq!(b.hour, 12);
    }

    #[test]
    fn parse_24h() {
        let b = parse_by("19:30").unwrap();
        assert_eq!((b.hour, b.minute), (19, 30));
        let b = parse_by("00:00").unwrap();
        assert_eq!((b.hour, b.minute), (0, 0));
        let b = parse_by("23:59").unwrap();
        assert_eq!((b.hour, b.minute), (23, 59));
    }

    #[test]
    fn parse_tomorrow_prefix() {
        let b = parse_by("tomorrow-9am").unwrap();
        assert_eq!((b.hour, b.day_offset), (9, 1));
        let b = parse_by("tomorrow 14:00").unwrap();
        assert_eq!((b.hour, b.day_offset), (14, 1));
        let b = parse_by("today 5am").unwrap();
        assert_eq!((b.hour, b.day_offset), (5, 0));
    }

    #[test]
    fn parse_rejects_invalid() {
        for bad in ["xyz", "25:00", "7zm", "13pm", "0pm", "7:60", "", "  "] {
            assert!(parse_by(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn resolve_picks_today_when_future() {
        let b = parse_by("7am").unwrap();
        // now is 06:00 → 07:00 today
        assert_eq!(b.resolve(now()).unwrap(), datetime!(2026-05-20 07:00 UTC));
    }

    #[test]
    fn resolve_bumps_to_tomorrow_when_past() {
        let b = parse_by("5am").unwrap();
        // now is 06:00 → 05:00 today is past → tomorrow 05:00
        assert_eq!(b.resolve(now()).unwrap(), datetime!(2026-05-21 05:00 UTC));
    }

    #[test]
    fn resolve_respects_explicit_tomorrow() {
        let b = parse_by("tomorrow 11pm").unwrap();
        // 23:00 tomorrow regardless of current time-of-day.
        assert_eq!(b.resolve(now()).unwrap(), datetime!(2026-05-21 23:00 UTC));
    }

    #[test]
    fn resolve_today_equals_now_bumps_to_tomorrow() {
        // A `--by` exactly at `now` is in the past for the scheduler's
        // strict deadline check; bump to tomorrow rather than yielding
        // an unschedulable deadline.
        let b = parse_by("6am").unwrap();
        assert_eq!(b.resolve(now()).unwrap(), datetime!(2026-05-21 06:00 UTC));
    }
}
