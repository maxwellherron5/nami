//! eGRID emission-factor table: committed-TOML format and runtime loader.
//!
//! `nami` converts a fuel mix to carbon intensity by multiplying each
//! fuel's generation by a per-(fuel, region) emission factor. Those
//! factors come from EPA eGRID, **balancing-authority** level, pinned to
//! a specific release and committed as `data/egrid-factors.toml`.
//!
//! This module is the *runtime* side: parse + validate the committed
//! TOML and answer `(Region, FuelType) → EmissionFactor`. It performs no
//! network or Excel I/O — acquiring/refreshing the TOML from the pinned
//! eGRID release is the separate maintainer tool (`refresh-egrid`,
//! behind the `egrid-refresh` feature).
//!
//! ## Why BA-level, and the fuel mapping
//!
//! eGRID's `BA` sheet publishes, per balancing authority, CO₂ **output**
//! emission rates (lb/MWh) including per-fuel rates. Our `Region` *is* a
//! BA, so this maps 1:1 with no subregion approximation. The mapping
//! (see `docs/methodology.md`):
//!
//! - `COL → BACCO2RT`, `NG → BAGCO2RT`, `OIL → BAOCO2RT`
//! - `NUC, WAT, SUN, WND → 0` (non-combustion: no direct CO₂)
//! - `OTH, UNK → BANBCO2` (eGRID non-baseload composite — the documented
//!   stand-in for the heterogeneous "other"/"unknown" bucket, which after
//!   item-6 normalization also includes geothermal)
//!
//! ## Units
//!
//! The TOML stores raw eGRID values in **lb CO₂/MWh**, exactly as
//! published, so the committed file is directly checkable against the
//! eGRID workbook. Conversion to internal gCO₂/kWh happens here, at the
//! load boundary (`lb/MWh × 453.592 / 1000`).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use nami_core::{EmissionFactor, FuelType, Region};

use crate::error::{Error, Result};

/// Schema version of `data/egrid-factors.toml`. Bump on incompatible
/// format changes.
pub const EGRID_SCHEMA_VERSION: u32 = 1;

/// Default committed location of the factor table, relative to the
/// workspace root.
pub const DEFAULT_EGRID_PATH: &str = "data/egrid-factors.toml";

/// On-disk TOML shape. Shared by the runtime loader and the maintainer
/// tool so the written and read formats cannot drift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EgridFile {
    /// Format version; checked against [`EGRID_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// eGRID release label, e.g. `"eGRID2023"`.
    pub egrid_release: String,
    /// eGRID data year, e.g. `2023`.
    pub egrid_data_year: i32,
    /// Exact pinned EPA source URL the table was generated from.
    pub source_url: String,
    /// When the maintainer tool generated this file, UTC.
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
    /// Units of the values below. Must be `"lb_co2_per_mwh"`.
    pub units: String,
    /// Methodology label, e.g. `"egrid-2023-ba"`.
    pub methodology: String,
    /// Human-readable provenance / assumption notes.
    pub notes: Vec<String>,
    /// `region code → (fuel code → lb CO₂/MWh)`.
    pub regions: BTreeMap<String, BTreeMap<String, f64>>,
}

/// Validated, unit-converted emission factors, ready for derivation.
///
/// Dense `[region][fuel]` storage indexed by position in
/// [`Region::ALL`] / [`FuelType::ALL`], so [`EgridFactors::factor`] is
/// total and panic-free (validation guarantees every cell is present).
#[derive(Debug, Clone)]
pub struct EgridFactors {
    /// eGRID release label.
    pub release: String,
    /// eGRID data year.
    pub data_year: i32,
    /// Methodology label.
    pub methodology: String,
    factors: Vec<Vec<EmissionFactor>>, // [region_idx][fuel_idx], gCO₂/kWh
}

fn region_index(r: Region) -> usize {
    Region::ALL.iter().position(|x| *x == r).unwrap_or(0)
}

fn fuel_index(f: FuelType) -> usize {
    FuelType::ALL.iter().position(|x| *x == f).unwrap_or(0)
}

impl EgridFactors {
    /// Parse and validate a factor table from TOML text.
    pub fn from_toml_str(s: &str) -> Result<Self> {
        let file: EgridFile =
            toml::from_str(s).map_err(|e| Error::EgridTable(format!("parse: {e}")))?;
        Self::from_file(file)
    }

    /// Load and validate the committed factor table from `path`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::EgridTable(format!("not found: {}", path.display()))
            } else {
                Error::Io(e)
            }
        })?;
        Self::from_toml_str(&text)
    }

    fn from_file(file: EgridFile) -> Result<Self> {
        if file.schema_version != EGRID_SCHEMA_VERSION {
            return Err(Error::EgridTable(format!(
                "schema version mismatch: found v{}, expected v{EGRID_SCHEMA_VERSION}",
                file.schema_version
            )));
        }
        if file.units != "lb_co2_per_mwh" {
            return Err(Error::EgridTable(format!(
                "unexpected units '{}', expected 'lb_co2_per_mwh'",
                file.units
            )));
        }

        // Dense fill; validation = every (region, fuel) cell present,
        // finite, non-negative. The default (0.0) placeholder is
        // overwritten for every cell or we return an error.
        let mut factors =
            vec![vec![EmissionFactor::default(); FuelType::ALL.len()]; Region::ALL.len()];

        for region in Region::ALL {
            let rkey = region.as_code();
            let rmap = file
                .regions
                .get(rkey)
                .ok_or_else(|| Error::EgridTable(format!("missing region '{rkey}'")))?;
            for fuel in FuelType::ALL {
                let fkey = fuel.as_code();
                let lb = rmap.get(fkey).copied().ok_or_else(|| {
                    Error::EgridTable(format!("region '{rkey}' missing fuel '{fkey}'"))
                })?;
                let ef = EmissionFactor::from_lbs_per_mwh(lb).map_err(|e| {
                    Error::EgridTable(format!(
                        "region '{rkey}' fuel '{fkey}' invalid value {lb}: {e}"
                    ))
                })?;
                factors[region_index(*region)][fuel_index(*fuel)] = ef;
            }
        }

        Ok(Self {
            release: file.egrid_release,
            data_year: file.egrid_data_year,
            methodology: file.methodology,
            factors,
        })
    }

    /// The emission factor for `(region, fuel)` in gCO₂/kWh. Total and
    /// panic-free: every cell is guaranteed present by validation.
    pub fn factor(&self, region: Region, fuel: FuelType) -> EmissionFactor {
        self.factors[region_index(region)][fuel_index(fuel)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal but complete synthetic table (all 7 regions × 9 fuels).
    fn synthetic_toml() -> String {
        let mut s = String::from(
            "schema_version = 1\n\
             egrid_release = \"eGRIDtest\"\n\
             egrid_data_year = 2023\n\
             source_url = \"https://example.invalid/egrid.xlsx\"\n\
             generated_at = \"2026-05-17T00:00:00Z\"\n\
             units = \"lb_co2_per_mwh\"\n\
             methodology = \"egrid-test-ba\"\n\
             notes = [\"synthetic\"]\n\n",
        );
        for r in Region::ALL {
            s.push_str(&format!("[regions.{}]\n", r.as_code()));
            for (i, f) in FuelType::ALL.iter().enumerate() {
                // Distinct values so indexing bugs would show.
                s.push_str(&format!("{} = {}\n", f.as_code(), 100.0 + i as f64));
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn parses_and_converts_units() {
        let f = EgridFactors::from_toml_str(&synthetic_toml()).unwrap();
        assert_eq!(f.release, "eGRIDtest");
        assert_eq!(f.data_year, 2023);
        // COL is index 0 → 100 lb/MWh → 100*453.592/1000 gCO2/kWh.
        let col = f.factor(Region::Caiso, FuelType::Col).value();
        assert!((col - 100.0 * 453.592 / 1000.0).abs() < 1e-9);
        // UNK is index 8 → 108 lb/MWh.
        let unk = f.factor(Region::Spp, FuelType::Unk).value();
        assert!((unk - 108.0 * 453.592 / 1000.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_bad_schema_version() {
        let t = synthetic_toml().replace("schema_version = 1", "schema_version = 99");
        assert!(matches!(
            EgridFactors::from_toml_str(&t),
            Err(Error::EgridTable(_))
        ));
    }

    #[test]
    fn rejects_wrong_units() {
        let t = synthetic_toml().replace("lb_co2_per_mwh", "gco2_per_kwh");
        assert!(matches!(
            EgridFactors::from_toml_str(&t),
            Err(Error::EgridTable(_))
        ));
    }

    #[test]
    fn rejects_missing_region() {
        let t = synthetic_toml().replace("[regions.SPP]", "[regions.ZZZ]");
        let e = EgridFactors::from_toml_str(&t).unwrap_err();
        assert!(matches!(e, Error::EgridTable(m) if m.contains("SPP")));
    }

    #[test]
    fn rejects_missing_fuel() {
        // Drop CAISO's NG line.
        let t = synthetic_toml()
            .lines()
            .filter(|l| !(l.starts_with("NG = ")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(
            EgridFactors::from_toml_str(&t),
            Err(Error::EgridTable(_))
        ));
    }

    #[test]
    fn rejects_negative_factor() {
        let t = synthetic_toml().replacen("COL = 100", "COL = -5", 1);
        assert!(matches!(
            EgridFactors::from_toml_str(&t),
            Err(Error::EgridTable(_))
        ));
    }
}
