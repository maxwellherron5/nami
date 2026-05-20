//! Named profiles from the nami config file.
//!
//! Profiles let `nami run nightly` replace a long `--region` /
//! `--duration` / `--deadline` / command flag list. They live in the
//! same config file as the existing region-resolution `region = "..."`
//! key (see [`nami_region::config_path`]) under `[profiles.<name>]`
//! sections, so existing config files keep working unchanged.
//!
//! The pure half of the loader ([`resolve_profile`]) takes already-read
//! config text + a caller-supplied `now`, so the parsing / validation
//! logic is unit-testable without touching the filesystem. The I/O
//! wrapper ([`load_profile`]) reads the file at the path
//! `nami_region::config_path()` resolves to.
//!
//! Precedence (CLI wins, profile fills the gaps, then the existing
//! region-resolution chain finishes the job for `region`):
//!
//! ```text
//! region:   --region flag > profile.region > NAMI_REGION env > config `region` key > refuse
//! duration: --duration flag > profile.duration                                     > error
//! deadline: --deadline flag > profile.deadline / profile.within (= now + within)  > error
//! command:  CLI command (after `--`) > profile.command                            > error
//! ```
//!
//! Provenance is announced on stderr whenever a field comes from a
//! profile rather than the flag, per CLAUDE.md's "do not hide how a
//! value was chosen."

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use nami_core::Region;

use crate::{RunArgs, parse_datetime, parse_duration};

/// On-disk profile file shape. Ignores unknown top-level keys (the
/// `region` key for nami-region's resolver coexists here), and unknown
/// per-profile keys, so a forward-compatible config doesn't break.
#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    profiles: HashMap<String, RawProfile>,
}

#[derive(Debug, Default, Deserialize)]
struct RawProfile {
    region: Option<String>,
    duration: Option<String>,
    within: Option<String>,
    deadline: Option<String>,
    command: Option<Vec<String>>,
}

/// A profile parsed and resolved against a caller-supplied `now`.
#[derive(Debug, Clone)]
pub struct ProfileFields {
    /// Profile name, for provenance messages.
    pub name: String,
    pub region: Option<Region>,
    pub duration: Option<Duration>,
    pub deadline: Option<OffsetDateTime>,
    pub command: Option<Vec<String>>,
}

/// Pure: parse `config_text` and resolve the named profile against
/// `now`. `config_path` is only used in error messages.
pub fn resolve_profile(
    name: &str,
    config_text: &str,
    config_path: &Path,
    now: OffsetDateTime,
) -> Result<ProfileFields> {
    let cfg: ConfigFile = toml::from_str(config_text)
        .with_context(|| format!("parsing nami config {}", config_path.display()))?;

    let raw = cfg.profiles.get(name).ok_or_else(|| {
        let mut available: Vec<&String> = cfg.profiles.keys().collect();
        available.sort();
        let list = if available.is_empty() {
            "none".to_string()
        } else {
            available
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        anyhow!(
            "no profile `{name}` in {} (available: {list})",
            config_path.display()
        )
    })?;

    if raw.within.is_some() && raw.deadline.is_some() {
        return Err(anyhow!(
            "profile `{name}`: `within` and `deadline` are mutually exclusive"
        ));
    }

    let region = match raw.region.as_deref() {
        Some(s) => Some(
            s.parse::<Region>()
                .map_err(|e| anyhow!("profile `{name}`: region {s:?}: {e}"))?,
        ),
        None => None,
    };

    let duration = match raw.duration.as_deref() {
        Some(s) => {
            Some(parse_duration(s).map_err(|e| anyhow!("profile `{name}`: duration {s:?}: {e}"))?)
        }
        None => None,
    };
    if let Some(d) = duration {
        if d <= Duration::ZERO {
            return Err(anyhow!("profile `{name}`: duration must be positive"));
        }
    }

    let deadline = if let Some(s) = raw.deadline.as_deref() {
        Some(parse_datetime(s).map_err(|e| anyhow!("profile `{name}`: deadline {s:?}: {e}"))?)
    } else if let Some(s) = raw.within.as_deref() {
        let w = parse_duration(s).map_err(|e| anyhow!("profile `{name}`: within {s:?}: {e}"))?;
        if w <= Duration::ZERO {
            return Err(anyhow!("profile `{name}`: within must be positive"));
        }
        Some(now + w)
    } else {
        None
    };

    let command = match raw.command.as_ref() {
        Some(v) if v.is_empty() => {
            return Err(anyhow!("profile `{name}`: command must not be empty"));
        }
        Some(v) => Some(v.clone()),
        None => None,
    };

    Ok(ProfileFields {
        name: name.to_string(),
        region,
        duration,
        deadline,
        command,
    })
}

/// I/O wrapper: read the nami config file and resolve `name` against `now`.
pub fn load_profile(name: &str, now: OffsetDateTime) -> Result<ProfileFields> {
    let path = nami_region::config_path().ok_or_else(|| {
        anyhow!(
            "could not determine the nami config file path; set $NAMI_CONFIG or \
             ensure $HOME / $XDG_CONFIG_HOME is set"
        )
    })?;
    let text = read_required_config(&path)?;
    resolve_profile(name, &text, &path, now)
}

fn read_required_config(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => anyhow!(
            "nami config file not found at {} — create it with a `[profiles.<name>]` \
             section to use --profile",
            path.display()
        ),
        _ => anyhow::Error::new(e).context(format!("reading nami config {}", path.display())),
    })
}

/// Apply a resolved profile to `args`: CLI-supplied fields win; profile
/// fills the gaps. Each field actually sourced from the profile is
/// announced on stderr for honest provenance.
pub fn merge_into(args: &mut RunArgs, profile: ProfileFields) {
    if args.region.is_none() {
        if let Some(r) = profile.region {
            eprintln!(
                "nami: region {r} applied from profile `{}` (no --region given)",
                profile.name
            );
            args.region = Some(r);
        }
    }
    if args.duration.is_none() {
        if let Some(d) = profile.duration {
            eprintln!(
                "nami: duration {} applied from profile `{}` (no --duration given)",
                fmt_dur(d),
                profile.name
            );
            args.duration = Some(d);
        }
    }
    if args.deadline.is_none() {
        if let Some(dl) = profile.deadline {
            eprintln!(
                "nami: deadline {} applied from profile `{}`",
                fmt_dt(dl),
                profile.name
            );
            args.deadline = Some(dl);
        }
    }
    if args.command.is_empty() {
        if let Some(c) = profile.command {
            eprintln!(
                "nami: command applied from profile `{}` ({})",
                profile.name,
                c.join(" ")
            );
            args.command = c;
        }
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
    use std::path::PathBuf;
    use time::macros::datetime;

    fn cfg_path() -> PathBuf {
        PathBuf::from("/tmp/nami/config.toml")
    }

    fn now() -> OffsetDateTime {
        datetime!(2026-05-20 06:00 UTC)
    }

    #[test]
    fn resolves_a_minimal_profile_with_within() {
        let text = r#"
            [profiles.nightly]
            region   = "MISO"
            duration = "2h"
            within   = "8h"
            command  = ["cargo", "test"]
        "#;
        let p = resolve_profile("nightly", text, &cfg_path(), now()).unwrap();
        assert_eq!(p.name, "nightly");
        assert_eq!(p.region, Some(Region::Miso));
        assert_eq!(p.duration, Some(Duration::hours(2)));
        assert_eq!(p.deadline, Some(now() + Duration::hours(8)));
        assert_eq!(
            p.command.as_deref(),
            Some(&["cargo".into(), "test".into()][..])
        );
    }

    #[test]
    fn deadline_field_used_when_set() {
        let text = r#"
            [profiles.fixed]
            duration = "30m"
            deadline = "2026-05-21T07:00:00Z"
        "#;
        let p = resolve_profile("fixed", text, &cfg_path(), now()).unwrap();
        assert_eq!(p.deadline, Some(datetime!(2026-05-21 07:00 UTC)));
        assert_eq!(p.region, None);
        assert_eq!(p.command, None);
    }

    #[test]
    fn within_and_deadline_are_mutually_exclusive() {
        let text = r#"
            [profiles.both]
            duration = "1h"
            within   = "8h"
            deadline = "2026-05-21T07:00:00Z"
        "#;
        let err = resolve_profile("both", text, &cfg_path(), now()).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn missing_profile_is_an_error_with_listing() {
        let text = r#"
            [profiles.nightly]
            duration = "1h"
            within   = "2h"
            [profiles.reindex]
            duration = "30m"
            within   = "4h"
        "#;
        let err = resolve_profile("eod", text, &cfg_path(), now()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no profile `eod`"));
        // Both available profiles are listed for the user.
        assert!(msg.contains("nightly") && msg.contains("reindex"));
    }

    #[test]
    fn bad_region_string_is_rejected() {
        let text = r#"
            [profiles.bad]
            region   = "ATLANTIS"
            duration = "1h"
            within   = "2h"
        "#;
        assert!(resolve_profile("bad", text, &cfg_path(), now()).is_err());
    }

    #[test]
    fn bad_duration_string_is_rejected() {
        let text = r#"
            [profiles.bad]
            duration = "2x"
            within   = "8h"
        "#;
        assert!(resolve_profile("bad", text, &cfg_path(), now()).is_err());
    }

    #[test]
    fn zero_within_is_rejected() {
        let text = r#"
            [profiles.bad]
            duration = "1h"
            within   = "0h"
        "#;
        let err = resolve_profile("bad", text, &cfg_path(), now()).unwrap_err();
        assert!(err.to_string().contains("within must be positive"));
    }

    #[test]
    fn empty_command_array_is_rejected() {
        let text = r#"
            [profiles.bad]
            duration = "1h"
            within   = "2h"
            command  = []
        "#;
        assert!(resolve_profile("bad", text, &cfg_path(), now()).is_err());
    }

    #[test]
    fn coexists_with_top_level_region_key() {
        // The file-level `region` key used by nami-region's resolver
        // must coexist peacefully; unknown top-level keys are ignored.
        let text = r#"
            region = "CAISO"
            [profiles.foo]
            duration = "1h"
            within   = "8h"
        "#;
        let p = resolve_profile("foo", text, &cfg_path(), now()).unwrap();
        assert_eq!(p.region, None); // profile-level region wins ONLY if set
        assert!(p.duration.is_some());
    }
}
