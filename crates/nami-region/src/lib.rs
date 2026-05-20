//! Region resolution for `nami` (config + environment precedence).
//!
//! When the user does not pass `--region`, the balancing authority is
//! resolved **deterministically** — no network call, no IP geolocation,
//! no timezone guessing. BA boundaries do not follow timezones or state
//! lines (e.g. Texas is Central time but ERCOT, not MISO/SPP), so a
//! heuristic would be confidently wrong too often for a tool whose whole
//! premise is refusing to overclaim.
//!
//! Precedence (first hit wins):
//!
//! 1. an explicit value — the `--region` flag;
//! 2. the `NAMI_REGION` environment variable;
//! 3. `region = "<BA>"` in the config file
//!    (`$NAMI_CONFIG`, else `$XDG_CONFIG_HOME/nami/config.toml`, else
//!    `$HOME/.config/nami/config.toml`);
//! 4. otherwise [`Error::Unresolved`] — refuse, do not guess.
//!
//! IP-based geolocation is intentionally **out of scope here**: it adds a
//! third-party network call (leaking the host's location), a spatial
//! dependency, and a data file, and is deferred to a later phase.

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]

mod error;

use std::path::PathBuf;

use serde::Deserialize;

use nami_core::Region;

pub use error::{Error, Result};

/// Which source supplied the resolved region (honest provenance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionSource {
    /// The explicit `--region` flag.
    Flag,
    /// The `NAMI_REGION` environment variable.
    Env,
    /// The config file at this path.
    Config(PathBuf),
}

/// A resolved region together with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The resolved balancing authority.
    pub region: Region,
    /// The source the value came from.
    pub source: RegionSource,
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    region: Option<String>,
}

fn parse(value: &str, from: &'static str) -> Result<Region> {
    let trimmed = value.trim();
    trimmed.parse::<Region>().map_err(|_| Error::InvalidRegion {
        value: trimmed.to_string(),
        from,
    })
}

/// Pure resolver: apply the precedence chain to already-gathered inputs.
///
/// Kept free of I/O so the precedence logic is unit-testable without
/// touching the real environment or filesystem. `config` is the config
/// file's TOML text paired with its path (for error/provenance
/// messages); pass `None` when there is no readable config file.
pub fn resolve(
    flag: Option<Region>,
    env_region: Option<&str>,
    config: Option<(&str, PathBuf)>,
) -> Result<Resolved> {
    if let Some(region) = flag {
        return Ok(Resolved {
            region,
            source: RegionSource::Flag,
        });
    }

    if let Some(raw) = env_region {
        if !raw.trim().is_empty() {
            return Ok(Resolved {
                region: parse(raw, "NAMI_REGION environment variable")?,
                source: RegionSource::Env,
            });
        }
    }

    if let Some((text, path)) = config {
        let cfg: ConfigFile = toml::from_str(text).map_err(|e| Error::Config {
            path: path.display().to_string(),
            msg: e.to_string(),
        })?;
        if let Some(raw) = cfg.region.as_deref() {
            if !raw.trim().is_empty() {
                return Ok(Resolved {
                    region: parse(raw, "config file `region`")?,
                    source: RegionSource::Config(path),
                });
            }
        }
    }

    Err(Error::Unresolved)
}

/// The config file path: `$NAMI_CONFIG`, else
/// `$XDG_CONFIG_HOME/nami/config.toml`, else
/// `$HOME/.config/nami/config.toml`. `None` if no base can be determined.
pub fn config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NAMI_CONFIG") {
        let p = PathBuf::from(p);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("nami").join("config.toml"))
}

/// Resolve using the real environment and filesystem.
///
/// A missing config file is not an error (it just doesn't contribute);
/// any other read failure is surfaced rather than silently ignored.
pub fn resolve_default(flag: Option<Region>) -> Result<Resolved> {
    let env_region = std::env::var("NAMI_REGION").ok();

    let config = match config_path() {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(text) => Some((text, path)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(Error::Io(e)),
        },
        None => None,
    };
    let config_ref = config.as_ref().map(|(t, p)| (t.as_str(), p.clone()));

    resolve(flag, env_region.as_deref(), config_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> Option<(&str, PathBuf)> {
        Some((text, PathBuf::from("/tmp/nami/config.toml")))
    }

    #[test]
    fn flag_wins_over_everything() {
        let r = resolve(Some(Region::Caiso), Some("ERCOT"), cfg("region = \"MISO\"")).unwrap();
        assert_eq!(r.region, Region::Caiso);
        assert_eq!(r.source, RegionSource::Flag);
    }

    #[test]
    fn env_used_when_no_flag_and_beats_config() {
        let r = resolve(None, Some("ercot"), cfg("region = \"MISO\"")).unwrap();
        assert_eq!(r.region, Region::Ercot);
        assert_eq!(r.source, RegionSource::Env);
    }

    #[test]
    fn blank_env_falls_through_to_config() {
        let r = resolve(None, Some("   "), cfg("region = \"PJM\"")).unwrap();
        assert_eq!(r.region, Region::Pjm);
        assert!(matches!(r.source, RegionSource::Config(_)));
    }

    #[test]
    fn config_used_when_no_flag_or_env() {
        let r = resolve(None, None, cfg("region = \"ISONE\"")).unwrap();
        assert_eq!(r.region, Region::IsoNe);
    }

    #[test]
    fn config_without_region_key_is_unresolved() {
        let e = resolve(None, None, cfg("other = 1")).unwrap_err();
        assert!(matches!(e, Error::Unresolved));
    }

    #[test]
    fn nothing_anywhere_is_unresolved() {
        assert!(matches!(
            resolve(None, None, None).unwrap_err(),
            Error::Unresolved
        ));
    }

    #[test]
    fn invalid_env_value_is_rejected_not_ignored() {
        let e = resolve(None, Some("ATLANTIS"), None).unwrap_err();
        match e {
            Error::InvalidRegion { value, from } => {
                assert_eq!(value, "ATLANTIS");
                assert!(from.contains("NAMI_REGION"));
            }
            other => panic!("expected InvalidRegion, got {other:?}"),
        }
    }

    #[test]
    fn invalid_config_value_is_rejected() {
        let e = resolve(None, None, cfg("region = \"nope\"")).unwrap_err();
        assert!(matches!(e, Error::InvalidRegion { .. }));
    }

    #[test]
    fn malformed_config_is_a_config_error() {
        let e = resolve(None, None, cfg("region = ")).unwrap_err();
        assert!(matches!(e, Error::Config { .. }));
    }

    #[test]
    fn region_codes_are_case_insensitive() {
        assert_eq!(
            resolve(None, Some("CaIsO"), None).unwrap().region,
            Region::Caiso
        );
    }
}
