//! `nami doctor`: precondition checks with explicit pass/warn/fail
//! semantics and a concrete suggested fix per failing check.
//!
//! `nami status` is informational and always exits 0. `nami doctor`
//! is *actionable*: each check is one of `ok`/`warn`/`fail`; any
//! `fail` makes the command exit nonzero (and `--strict` also exits
//! nonzero on `warn`), so it composes cleanly with CI gates or a
//! pre-flight check before scheduling.
//!
//! Checks, in order:
//! 1. Region resolves via the regular `flag / NAMI_REGION / config`
//!    chain (a `--region` flag here only acts as an override).
//! 2. eGRID factor table loads from `--egrid` (default
//!    `data/egrid-factors.toml`).
//! 3. `EIA_API_KEY` set (a `warn`, since only `nami refresh` needs it).
//! 4. Historical cache loads, and the resolved region has non-stale
//!    observations in it.

use anyhow::Result;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use nami_carbon_eia::{DEFAULT_MAX_CACHE_AGE, EgridFactors, HistoricalCache};

use crate::DoctorArgs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub(crate) struct Check {
    pub status: Status,
    pub title: String,
    pub detail: String,
    pub fix: Option<String>,
}

pub fn run(args: DoctorArgs) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    let checks = gather_checks(&args, now);
    let (out, ok, warn, fail) = render(&checks);
    print!("{out}");
    if fail > 0 || (args.strict && warn > 0) {
        // Keep `ok` for the summary line (already in `out`); avoid the
        // unused-binding warning by binding it for grep.
        let _ = ok;
        std::process::exit(1);
    }
    Ok(())
}

/// Run the actual preconditions against the filesystem / env / time
/// source. Returns the checks in a stable order suitable for `render`.
fn gather_checks(args: &DoctorArgs, now: OffsetDateTime) -> Vec<Check> {
    let mut checks = Vec::new();

    // 1. Region resolves.
    let region = match nami_region::resolve_default(args.region) {
        Ok(r) => {
            let src = match &r.source {
                nami_region::RegionSource::Flag => "from --region".to_string(),
                nami_region::RegionSource::Env => "from NAMI_REGION".to_string(),
                nami_region::RegionSource::Config(p) => {
                    format!("from config {}", p.display())
                }
            };
            checks.push(Check {
                status: Status::Ok,
                title: "region".into(),
                detail: format!("{} ({src})", r.region),
                fix: None,
            });
            Some(r.region)
        }
        Err(e) => {
            checks.push(Check {
                status: Status::Fail,
                title: "region".into(),
                detail: format!("{e}"),
                fix: Some("nami init --region <BA>".into()),
            });
            None
        }
    };

    // 2. eGRID factor table.
    match EgridFactors::load(&args.egrid) {
        Ok(f) => checks.push(Check {
            status: Status::Ok,
            title: "eGRID factor table".into(),
            detail: format!(
                "{} (data year {}) [{}]",
                f.release,
                f.data_year,
                args.egrid.display()
            ),
            fix: None,
        }),
        Err(e) => checks.push(Check {
            status: Status::Fail,
            title: "eGRID factor table".into(),
            detail: format!("{e}"),
            fix: Some(
                "cargo run -p nami-carbon-eia --features egrid-refresh \
                 --bin refresh-egrid"
                    .into(),
            ),
        }),
    }

    // 3. EIA_API_KEY (warn, not fail — only `nami refresh` needs it).
    let key_set = std::env::var("EIA_API_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    if key_set {
        checks.push(Check {
            status: Status::Ok,
            title: "EIA_API_KEY".into(),
            detail: "set".into(),
            fix: None,
        });
    } else {
        checks.push(Check {
            status: Status::Warn,
            title: "EIA_API_KEY".into(),
            detail: "not set (only needed for `nami refresh`)".into(),
            fix: Some("free key: https://www.eia.gov/opendata/register.php".into()),
        });
    }

    // 4. Historical cache, region-aware.
    match HistoricalCache::load(&args.cache) {
        Err(nami_carbon_eia::Error::CacheMissing(_)) => {
            let fix = match region {
                Some(r) => format!("nami refresh --region {r}"),
                None => "set a region (above), then: nami refresh --region <BA>".into(),
            };
            checks.push(Check {
                status: Status::Fail,
                title: "historical cache".into(),
                detail: format!("missing at {}", args.cache.display()),
                fix: Some(fix),
            });
        }
        Err(e) => checks.push(Check {
            status: Status::Fail,
            title: "historical cache".into(),
            detail: format!("unusable: {e}"),
            fix: Some(format!(
                "inspect {} and refresh or remove",
                args.cache.display()
            )),
        }),
        Ok(c) => match region {
            None => checks.push(Check {
                status: Status::Warn,
                title: "historical cache".into(),
                detail: format!(
                    "{} region(s) cached; no configured region to evaluate freshness against",
                    c.region_count()
                ),
                fix: Some("nami init --region <BA>".into()),
            }),
            Some(r) => match c.newest_sample(r) {
                None => checks.push(Check {
                    status: Status::Fail,
                    title: format!("{r} cache"),
                    detail: "no observations for this region".into(),
                    fix: Some(format!("nami refresh --region {r}")),
                }),
                Some(ns) => {
                    let age = now - ns;
                    let n = c.observations(r).len();
                    let max_h = DEFAULT_MAX_CACHE_AGE.whole_hours();
                    let age_h = age.whole_hours();
                    if age > DEFAULT_MAX_CACHE_AGE {
                        checks.push(Check {
                            status: Status::Warn,
                            title: format!("{r} cache"),
                            detail: format!(
                                "{n} obs, newest {} (age {age_h}h, > {max_h}h)",
                                fmt_dt(ns)
                            ),
                            fix: Some(format!("nami refresh --region {r}")),
                        });
                    } else {
                        checks.push(Check {
                            status: Status::Ok,
                            title: format!("{r} cache"),
                            detail: format!("{n} obs, newest {} (age {age_h}h)", fmt_dt(ns)),
                            fix: None,
                        });
                    }
                }
            },
        },
    }

    checks
}

/// Pure rendering: format the check list and return `(text, ok_count,
/// warn_count, fail_count)`. Kept separate from `gather_checks` so the
/// output shape is unit-testable without touching the filesystem.
pub(crate) fn render(checks: &[Check]) -> (String, usize, usize, usize) {
    let mut out = String::from("nami doctor\n");
    let mut ok = 0usize;
    let mut warn = 0usize;
    let mut fail = 0usize;
    for c in checks {
        let tag = match c.status {
            Status::Ok => "ok  ",
            Status::Warn => "warn",
            Status::Fail => "fail",
        };
        out.push_str(&format!("  {tag}  {} — {}\n", c.title, c.detail));
        if let Some(fix) = &c.fix {
            out.push_str(&format!("        → {fix}\n"));
        }
        match c.status {
            Status::Ok => ok += 1,
            Status::Warn => warn += 1,
            Status::Fail => fail += 1,
        }
    }
    out.push_str(&format!("Summary: {ok} ok, {warn} warn, {fail} fail\n"));
    (out, ok, warn, fail)
}

fn fmt_dt(dt: OffsetDateTime) -> String {
    dt.format(&Rfc3339).unwrap_or_else(|_| format!("{dt:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::Region;
    use std::path::PathBuf;

    fn check(status: Status, title: &str, detail: &str, fix: Option<&str>) -> Check {
        Check {
            status,
            title: title.into(),
            detail: detail.into(),
            fix: fix.map(str::to_string),
        }
    }

    #[test]
    fn renders_tags_and_summary() {
        let checks = vec![
            check(Status::Ok, "region", "MISO (from NAMI_REGION)", None),
            check(
                Status::Warn,
                "EIA_API_KEY",
                "not set (only needed for `nami refresh`)",
                Some("free key: https://www.eia.gov/opendata/register.php"),
            ),
            check(
                Status::Fail,
                "MISO cache",
                "no observations for this region",
                Some("nami refresh --region MISO"),
            ),
        ];
        let (out, ok, warn, fail) = render(&checks);
        assert_eq!((ok, warn, fail), (1, 1, 1));
        assert!(out.contains("ok    region"));
        assert!(out.contains("warn  EIA_API_KEY"));
        assert!(out.contains("fail  MISO cache"));
        // Fix lines render with the arrow prefix.
        assert!(out.contains("        → nami refresh --region MISO"));
        // Summary line is present and accurate.
        assert!(out.contains("Summary: 1 ok, 1 warn, 1 fail"));
    }

    #[test]
    fn render_handles_all_ok_with_no_fix_lines() {
        let checks = vec![
            check(Status::Ok, "region", "PJM (from --region)", None),
            check(Status::Ok, "EIA_API_KEY", "set", None),
        ];
        let (out, ok, warn, fail) = render(&checks);
        assert_eq!((ok, warn, fail), (2, 0, 0));
        assert!(!out.contains("→"));
        assert!(out.contains("Summary: 2 ok, 0 warn, 0 fail"));
    }

    #[test]
    fn empty_checks_still_render_summary() {
        let (out, ok, warn, fail) = render(&[]);
        assert_eq!((ok, warn, fail), (0, 0, 0));
        assert!(out.starts_with("nami doctor\n"));
        assert!(out.contains("Summary: 0 ok, 0 warn, 0 fail"));
    }

    /// `gather_checks` against a known-bad environment: bogus paths
    /// plus an explicit region override means the region check passes
    /// while eGRID + cache fail. EIA_API_KEY is environment-dependent
    /// so we don't assert its specific status.
    #[test]
    fn gather_against_bogus_paths_yields_expected_failures() {
        let args = DoctorArgs {
            region: Some(Region::Miso),
            cache: PathBuf::from("/no/such/cache.json"),
            egrid: PathBuf::from("/no/such/egrid.toml"),
            strict: false,
        };
        let now = time::macros::datetime!(2026-05-20 06:00 UTC);
        let checks = gather_checks(&args, now);

        // Region resolves from the explicit flag.
        let region_check = &checks[0];
        assert_eq!(region_check.title, "region");
        assert_eq!(region_check.status, Status::Ok);
        assert!(region_check.detail.contains("MISO"));

        // eGRID load fails because the path doesn't exist.
        let egrid_check = checks
            .iter()
            .find(|c| c.title == "eGRID factor table")
            .expect("egrid check present");
        assert_eq!(egrid_check.status, Status::Fail);

        // Historical cache is missing → fail with a region-specific fix.
        let cache_check = checks
            .iter()
            .find(|c| c.title == "historical cache")
            .expect("cache check present");
        assert_eq!(cache_check.status, Status::Fail);
        assert_eq!(
            cache_check.fix.as_deref(),
            Some("nami refresh --region MISO")
        );
    }
}
