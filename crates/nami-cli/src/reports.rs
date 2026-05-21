//! Reports directory convention.
//!
//! `nami run` auto-archives every `RunReport` so a team can later ask
//! "how often did nami defer, and what was the average improvement when
//! it did?" via `nami report summary` (Phase B, the next item). The
//! existing `--report <path>` keeps pinning a single file (still useful
//! for CI artifacts); the new `--report-dir <path>` overrides only the
//! *directory* — filenames are auto-generated for sortability.
//!
//! Resolution order:
//! 1. `--report <path>` (one pinned file; existing behavior).
//! 2. `--report-dir <dir>` (auto filename inside `<dir>/<UTC-date>/`).
//! 3. otherwise: the default state directory (XDG_STATE_HOME-aware).
//!
//! If none of those can be determined (no `$HOME` and no
//! `$XDG_STATE_HOME`), the caller is told to fall back to printing
//! JSON on stdout — never silently dropping the artifact.

use std::path::PathBuf;

use time::OffsetDateTime;

use nami_core::Region;

use crate::RunArgs;

/// Where this resolved-to. Caller uses it to decide whether to emit a
/// stderr provenance line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// User pinned an exact file via `--report`. No date partitioning,
    /// no announcement (the user typed the path; they know where it is).
    Pinned(PathBuf),
    /// Auto-archived: `<dir>/<UTC-date>/<HH-MM-SS-nanos>-<BA>.json`. The
    /// caller announces the path on stderr.
    Archived(PathBuf),
    /// Neither flag was given AND no default state directory could be
    /// determined (no `$HOME`/`$XDG_STATE_HOME`). Caller falls back to
    /// printing the JSON on stdout with a stderr warning.
    Stdout,
}

/// Apply the precedence chain. Pure; takes a caller-supplied `now` and
/// `default_dir_override` so the tests don't depend on the real
/// `$HOME` / `$XDG_STATE_HOME`.
pub fn resolve_target(
    args: &RunArgs,
    region: Region,
    now: OffsetDateTime,
    default_state_dir: Option<&std::path::Path>,
) -> Target {
    if let Some(p) = &args.report {
        return Target::Pinned(p.clone());
    }
    let auto_name = auto_filename(now, region);
    let date_dir = utc_date_dir(now);
    if let Some(dir) = &args.report_dir {
        return Target::Archived(dir.join(date_dir).join(auto_name));
    }
    match default_state_dir {
        Some(base) => Target::Archived(base.join(date_dir).join(auto_name)),
        None => Target::Stdout,
    }
}

/// IO wrapper around [`resolve_target`] that consults the real
/// environment for the default state dir.
pub fn resolve_default(args: &RunArgs, region: Region, now: OffsetDateTime) -> Target {
    let default = default_state_dir();
    resolve_target(args, region, now, default.as_deref())
}

/// `$XDG_STATE_HOME/nami/reports`, else `$HOME/.local/state/nami/reports`,
/// else `None`.
pub fn default_state_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("XDG_STATE_HOME") {
        let p = PathBuf::from(p);
        if !p.as_os_str().is_empty() {
            return Some(p.join("nami").join("reports"));
        }
    }
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join(".local")
            .join("state")
            .join("nami")
            .join("reports")
    })
}

/// `YYYY-MM-DD` (UTC) — the date-partitioned subdirectory.
fn utc_date_dir(now: OffsetDateTime) -> String {
    let d = now.date();
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

/// `HH-MM-SS-nanos-<BA>.json`. Sortable by start time within a day;
/// nanoseconds make same-second-same-region collisions vanishingly
/// unlikely without any retry logic.
fn auto_filename(now: OffsetDateTime, region: Region) -> String {
    let t = now.time();
    format!(
        "{:02}-{:02}-{:02}-{:09}-{}.json",
        t.hour(),
        t.minute(),
        t.second(),
        t.nanosecond(),
        region.as_code()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use time::macros::datetime;

    fn now() -> OffsetDateTime {
        datetime!(2026-05-20 06:45:23.123456789 UTC)
    }

    fn args_no_report() -> RunArgs {
        RunArgs {
            profile: None,
            duration: None,
            deadline: None,
            within: None,
            by: None,
            region: None,
            report: None,
            report_dir: None,
            quiet: false,
            log: None,
            command: vec![],
        }
    }

    #[test]
    fn auto_filename_is_sortable_and_includes_region() {
        let name = auto_filename(now(), Region::Caiso);
        // Sortable lexicographically: HH-MM-SS prefix.
        assert!(name.starts_with("06-45-23-"));
        assert!(name.ends_with("-CAISO.json"));
        // Includes nanoseconds for collision resistance.
        assert!(name.contains("-123456789-"));
    }

    #[test]
    fn date_dir_is_iso_yyyy_mm_dd() {
        assert_eq!(utc_date_dir(now()), "2026-05-20");
    }

    #[test]
    fn pinned_report_wins_over_dir_and_default() {
        let mut args = args_no_report();
        args.report = Some(PathBuf::from("/tmp/foo.json"));
        args.report_dir = Some(PathBuf::from("/tmp/dir"));
        let t = resolve_target(
            &args,
            Region::Miso,
            now(),
            Some(Path::new("/should/not/be/used")),
        );
        assert_eq!(t, Target::Pinned(PathBuf::from("/tmp/foo.json")));
    }

    #[test]
    fn report_dir_overrides_default_and_uses_date_partition() {
        let mut args = args_no_report();
        args.report_dir = Some(PathBuf::from("/tmp/reports"));
        let t = resolve_target(&args, Region::Pjm, now(), Some(Path::new("/default/state")));
        match t {
            Target::Archived(p) => {
                assert!(p.starts_with("/tmp/reports/2026-05-20/"));
                assert!(p.to_string_lossy().ends_with("-PJM.json"));
            }
            other => panic!("expected Archived, got {other:?}"),
        }
    }

    #[test]
    fn default_state_dir_used_when_no_flags() {
        let args = args_no_report();
        let t = resolve_target(
            &args,
            Region::Ercot,
            now(),
            Some(Path::new("/default/state")),
        );
        match t {
            Target::Archived(p) => {
                assert!(p.starts_with("/default/state/2026-05-20/"));
                assert!(p.to_string_lossy().ends_with("-ERCOT.json"));
            }
            other => panic!("expected Archived, got {other:?}"),
        }
    }

    #[test]
    fn falls_back_to_stdout_when_no_default_and_no_flags() {
        let args = args_no_report();
        let t = resolve_target(&args, Region::Spp, now(), None);
        assert_eq!(t, Target::Stdout);
    }
}
