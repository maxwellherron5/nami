//! On-disk historical cache format.
//!
//! `nami` derives hourly average carbon intensity from EIA-930 fuel-mix
//! observations and EPA eGRID factors. Re-deriving the whole history on
//! every invocation would hammer the EIA API and be slow, so derived
//! [`CarbonObservation`]s are cached locally as JSON (default
//! `data/historical-cache.json`) and refreshed periodically (Phase 0
//! implementation item 13).
//!
//! This module owns *only the format and its load/save/validate
//! lifecycle*. It does not fetch from EIA (item 6+) or derive intensity
//! (item 8); it stores whatever observations it is given.
//!
//! Design choices:
//!
//! - **Versioned schema.** A `schema_version` is written and checked on
//!   load. An unknown version is refused, never silently misread.
//! - **`Vec`, not a map.** Regions are a small fixed set; a `Vec` of
//!   `RegionHistory` keeps the JSON unambiguous and human-auditable and
//!   sidesteps enum-as-map-key serialization quirks.
//! - **RFC 3339 timestamps.** Consistent with the rest of `nami`'s
//!   auditable serialization.
//! - **Atomic save.** Written to a sibling temp file then renamed, so a
//!   crash mid-write cannot corrupt an existing good cache.
//! - **Strict validation.** Per region, observations must be strictly
//!   ascending by timestamp and regions must be unique. Violations are
//!   surfaced loudly, not papered over.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use nami_core::{CarbonObservation, Region};

use crate::error::{Error, Result};

/// Schema version written into and required when reading the cache file.
/// Bump on any incompatible format change.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// Default maximum age before a cache is considered stale. CLAUDE.md
/// specifies the cache "refresh daily", so 24h is the default bound.
pub const DEFAULT_MAX_CACHE_AGE: Duration = Duration::hours(24);

/// One region's cached hourly observations, strictly ascending by `at`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionHistory {
    /// The region these observations belong to.
    pub region: Region,
    /// Hourly observations, strictly ascending by timestamp.
    pub observations: Vec<CarbonObservation>,
}

/// The full historical cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalCache {
    /// Format version; checked against [`CACHE_SCHEMA_VERSION`] on load.
    pub schema_version: u32,
    /// When this cache file was last (re)written, UTC.
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
    /// Methodology label of the derivation that produced these
    /// observations, for traceability (e.g.
    /// `"eia-930-v1+egrid-2024-subregion"`). Individual observations also
    /// carry their own label; this is the file-level summary.
    pub methodology_version: String,
    /// Per-region history.
    pub regions: Vec<RegionHistory>,
}

impl HistoricalCache {
    /// Create an empty cache stamped `generated_at` with the given
    /// methodology label and the current schema version.
    pub fn new(generated_at: OffsetDateTime, methodology_version: impl Into<String>) -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            generated_at,
            methodology_version: methodology_version.into(),
            regions: Vec::new(),
        }
    }

    /// Insert or replace a region's observations. The slice is sorted
    /// ascending by timestamp before storing; duplicate timestamps are
    /// left intact and will be caught by [`HistoricalCache::validate`]
    /// (a duplicate hour is a data bug we want surfaced, not hidden).
    pub fn set_region(&mut self, region: Region, mut observations: Vec<CarbonObservation>) {
        observations.sort_by_key(|o| o.at);
        let entry = RegionHistory {
            region,
            observations,
        };
        match self.regions.iter_mut().find(|r| r.region == region) {
            Some(existing) => *existing = entry,
            None => self.regions.push(entry),
        }
    }

    /// All observations for `region` (empty slice if the region is
    /// absent).
    pub fn observations(&self, region: Region) -> &[CarbonObservation] {
        self.regions
            .iter()
            .find(|r| r.region == region)
            .map_or(&[], |r| r.observations.as_slice())
    }

    /// Timestamp of the newest observation for `region`, if any.
    pub fn newest_sample(&self, region: Region) -> Option<OffsetDateTime> {
        self.observations(region).last().map(|o| o.at)
    }

    /// Number of regions with cached history.
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// Age of the cache relative to `now` (`now - generated_at`). May be
    /// negative if `generated_at` is in the future (clock skew).
    pub fn age(&self, now: OffsetDateTime) -> Duration {
        now - self.generated_at
    }

    /// Whether the cache is older than `max_age` relative to `now`.
    pub fn is_stale(&self, now: OffsetDateTime, max_age: Duration) -> bool {
        self.age(now) > max_age
    }

    /// Validate structural invariants: supported schema version, unique
    /// regions, and per-region observations strictly ascending by
    /// timestamp.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != CACHE_SCHEMA_VERSION {
            return Err(Error::CacheSchemaMismatch {
                found: self.schema_version,
                expected: CACHE_SCHEMA_VERSION,
            });
        }
        for (i, rh) in self.regions.iter().enumerate() {
            if self.regions[..i]
                .iter()
                .any(|prev| prev.region == rh.region)
            {
                return Err(Error::HistoricalCache(format!(
                    "duplicate region entry: {}",
                    rh.region
                )));
            }
            for pair in rh.observations.windows(2) {
                if pair[1].at <= pair[0].at {
                    return Err(Error::HistoricalCache(format!(
                        "{} observations not strictly ascending by timestamp \
                         (at {} then {})",
                        rh.region, pair[0].at, pair[1].at
                    )));
                }
            }
        }
        Ok(())
    }

    /// Load and validate the cache from `path`.
    ///
    /// Returns [`Error::CacheMissing`] when the file does not exist (a
    /// recoverable state the caller maps to a fallback), and
    /// [`Error::HistoricalCache`] / [`Error::CacheSchemaMismatch`] when it
    /// exists but is unusable.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::CacheMissing(path.display().to_string()));
            }
            Err(e) => return Err(Error::Io(e)),
        };
        let cache: HistoricalCache = serde_json::from_slice(&bytes)
            .map_err(|e| Error::HistoricalCache(format!("parse {}: {e}", path.display())))?;
        cache.validate()?;
        Ok(cache)
    }

    /// Atomically write the cache to `path` as pretty JSON.
    ///
    /// Validates first (a cache that would fail [`HistoricalCache::load`]
    /// is never written), creates the parent directory if needed, writes
    /// to a sibling temp file, then renames over `path`.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate()?;
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| Error::HistoricalCache(format!("serialize: {e}")))?;

        let tmp: PathBuf = {
            let mut name = path.file_name().unwrap_or_default().to_os_string();
            name.push(".tmp");
            path.with_file_name(name)
        };
        std::fs::write(&tmp, json.as_bytes())?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::CarbonIntensity;
    use time::macros::datetime;

    fn obs(at: OffsetDateTime, v: f64) -> CarbonObservation {
        CarbonObservation {
            at,
            intensity: CarbonIntensity::new(v).unwrap(),
            methodology: "test-v1".into(),
        }
    }

    fn sample_cache() -> HistoricalCache {
        let mut c = HistoricalCache::new(datetime!(2026-05-16 00:00 UTC), "test-v1");
        c.set_region(
            Region::Caiso,
            vec![
                obs(datetime!(2026-05-15 02:00 UTC), 240.0),
                obs(datetime!(2026-05-15 00:00 UTC), 250.0),
                obs(datetime!(2026-05-15 01:00 UTC), 245.0),
            ],
        );
        c.set_region(
            Region::Ercot,
            vec![obs(datetime!(2026-05-15 00:00 UTC), 400.0)],
        );
        c
    }

    #[test]
    fn set_region_sorts_ascending() {
        let c = sample_cache();
        let times: Vec<_> = c.observations(Region::Caiso).iter().map(|o| o.at).collect();
        assert_eq!(
            times,
            vec![
                datetime!(2026-05-15 00:00 UTC),
                datetime!(2026-05-15 01:00 UTC),
                datetime!(2026-05-15 02:00 UTC),
            ]
        );
        assert_eq!(
            c.newest_sample(Region::Caiso),
            Some(datetime!(2026-05-15 02:00 UTC))
        );
        assert_eq!(c.region_count(), 2);
        assert!(c.observations(Region::Pjm).is_empty());
        assert_eq!(c.newest_sample(Region::Pjm), None);
    }

    #[test]
    fn set_region_replaces_existing() {
        let mut c = sample_cache();
        c.set_region(
            Region::Ercot,
            vec![obs(datetime!(2026-05-15 05:00 UTC), 410.0)],
        );
        assert_eq!(c.region_count(), 2);
        assert_eq!(c.observations(Region::Ercot).len(), 1);
        assert_eq!(
            c.newest_sample(Region::Ercot),
            Some(datetime!(2026-05-15 05:00 UTC))
        );
    }

    #[test]
    fn validate_accepts_well_formed() {
        assert!(sample_cache().validate().is_ok());
    }

    #[test]
    fn validate_rejects_unsorted_or_duplicate_timestamps() {
        let mut c = HistoricalCache::new(datetime!(2026-05-16 00:00 UTC), "test-v1");
        // Bypass set_region's sort by constructing the Vec directly.
        c.regions.push(RegionHistory {
            region: Region::Caiso,
            observations: vec![
                obs(datetime!(2026-05-15 01:00 UTC), 1.0),
                obs(datetime!(2026-05-15 01:00 UTC), 2.0), // duplicate ts
            ],
        });
        assert!(matches!(c.validate(), Err(Error::HistoricalCache(_))));
    }

    #[test]
    fn validate_rejects_duplicate_region() {
        let mut c = HistoricalCache::new(datetime!(2026-05-16 00:00 UTC), "test-v1");
        c.regions.push(RegionHistory {
            region: Region::Caiso,
            observations: vec![],
        });
        c.regions.push(RegionHistory {
            region: Region::Caiso,
            observations: vec![],
        });
        assert!(matches!(c.validate(), Err(Error::HistoricalCache(_))));
    }

    #[test]
    fn validate_rejects_bad_schema_version() {
        let mut c = sample_cache();
        c.schema_version = 999;
        assert!(matches!(
            c.validate(),
            Err(Error::CacheSchemaMismatch {
                found: 999,
                expected: CACHE_SCHEMA_VERSION
            })
        ));
    }

    #[test]
    fn staleness_boundaries() {
        let c = HistoricalCache::new(datetime!(2026-05-15 00:00 UTC), "test-v1");
        let just_within = datetime!(2026-05-15 23:00 UTC);
        let just_over = datetime!(2026-05-16 01:00 UTC);
        assert!(!c.is_stale(just_within, DEFAULT_MAX_CACHE_AGE));
        assert!(c.is_stale(just_over, DEFAULT_MAX_CACHE_AGE));
        assert_eq!(c.age(datetime!(2026-05-15 06:00 UTC)), Duration::hours(6));
    }

    #[test]
    fn serializes_with_rfc3339_and_schema_version() {
        let json = serde_json::to_string(&sample_cache()).unwrap();
        assert!(json.contains("\"schema_version\":1"), "{json}");
        assert!(
            json.contains("\"generated_at\":\"2026-05-16T00:00:00Z\""),
            "{json}"
        );
    }

    #[test]
    fn round_trips_through_json() {
        let c = sample_cache();
        let back: HistoricalCache =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "nami-cache-test-{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        let path = dir.join("historical-cache.json");
        let c = sample_cache();
        c.save(&path).unwrap();
        let loaded = HistoricalCache::load(&path).unwrap();
        assert_eq!(c, loaded);
        // Temp (.tmp) sibling should have been renamed away, not left behind.
        assert!(!path.with_file_name("historical-cache.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_is_cache_missing() {
        let path = std::env::temp_dir().join(format!(
            "nami-cache-absent-{}-{}.json",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        assert!(matches!(
            HistoricalCache::load(&path),
            Err(Error::CacheMissing(_))
        ));
    }

    #[test]
    fn load_corrupt_json_is_historical_cache_error() {
        let dir = std::env::temp_dir().join(format!(
            "nami-cache-corrupt-{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.json");
        std::fs::write(&path, b"{ not valid json ").unwrap();
        assert!(matches!(
            HistoricalCache::load(&path),
            Err(Error::HistoricalCache(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
