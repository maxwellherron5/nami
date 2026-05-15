//! Carbon-intensity values, fuel types, and emission factors.
//!
//! All carbon quantities are stored internally in **grams CO₂-equivalent per
//! kilowatt-hour** (gCO₂/kWh). API and file-format boundaries convert from
//! whatever upstream sources provide (EPA eGRID publishes lbs CO₂/MWh; see
//! [`CarbonIntensity::from_lbs_per_mwh`]).
//!
//! ## Average, not marginal
//!
//! Phase 0 produces *estimated average* carbon intensity by multiplying
//! observed fuel-mix generation against static emission factors. This is
//! **not** marginal emissions and must never be presented as such — see
//! `CLAUDE.md` and `docs/methodology.md` for the rationale.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// EIA-930 fuel-type categories.
///
/// The nine categories EIA-930 reports for hourly generation-by-fuel-type.
/// `Other` and `Unknown` require a documented composite emission factor;
/// see `docs/methodology.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FuelType {
    /// Coal.
    Col,
    /// Natural gas.
    Ng,
    /// Nuclear.
    Nuc,
    /// Oil / petroleum products.
    Oil,
    /// Hydro / water.
    Wat,
    /// Solar.
    Sun,
    /// Wind.
    Wnd,
    /// Other (biomass, geothermal, etc.).
    Oth,
    /// Unknown / confidential.
    Unk,
}

impl FuelType {
    /// The EIA-930 code for this fuel type.
    pub const fn as_code(self) -> &'static str {
        match self {
            FuelType::Col => "COL",
            FuelType::Ng => "NG",
            FuelType::Nuc => "NUC",
            FuelType::Oil => "OIL",
            FuelType::Wat => "WAT",
            FuelType::Sun => "SUN",
            FuelType::Wnd => "WND",
            FuelType::Oth => "OTH",
            FuelType::Unk => "UNK",
        }
    }

    /// All nine fuel types.
    pub const ALL: &'static [FuelType] = &[
        FuelType::Col,
        FuelType::Ng,
        FuelType::Nuc,
        FuelType::Oil,
        FuelType::Wat,
        FuelType::Sun,
        FuelType::Wnd,
        FuelType::Oth,
        FuelType::Unk,
    ];
}

impl std::fmt::Display for FuelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

/// A grid carbon-intensity value in gCO₂/kWh.
///
/// Newtype around `f64`. Phase 0 values are always *estimated average*
/// intensity derived from fuel-mix observations; this type does not
/// distinguish marginal vs. average because Phase 0 has no marginal source.
/// If a future provider supplies marginal data, model the distinction at
/// the provider boundary, not in this type.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CarbonIntensity(f64);

impl CarbonIntensity {
    /// Construct a carbon-intensity value. Returns an error for negative or
    /// non-finite inputs.
    ///
    /// # Examples
    ///
    /// ```
    /// use nami_core::CarbonIntensity;
    ///
    /// let ci = CarbonIntensity::new(245.0).unwrap();
    /// assert_eq!(ci.value(), 245.0);
    /// ```
    pub fn new(grams_per_kwh: f64) -> Result<Self> {
        if !grams_per_kwh.is_finite() || grams_per_kwh < 0.0 {
            return Err(Error::InvalidIntensity(format!(
                "expected non-negative finite gCO2/kWh, got {grams_per_kwh}"
            )));
        }
        Ok(Self(grams_per_kwh))
    }

    /// The intensity in gCO₂/kWh.
    pub fn value(&self) -> f64 {
        self.0
    }

    /// Convert from EPA eGRID's lbs CO₂/MWh into internal gCO₂/kWh.
    ///
    /// 1 lb = 453.592 g; 1 MWh = 1000 kWh; so `lbs/MWh × 453.592 / 1000 = g/kWh`.
    pub fn from_lbs_per_mwh(lbs_per_mwh: f64) -> Result<Self> {
        Self::new(lbs_per_mwh * 453.592 / 1000.0)
    }
}

/// An eGRID emission factor for a specific fuel type within a region.
///
/// In gCO₂/kWh, sharing units with [`CarbonIntensity`] so derivation math
/// composes without unit conversions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EmissionFactor {
    /// gCO₂-equivalent per kWh of generation from this fuel.
    grams_per_kwh: f64,
}

impl EmissionFactor {
    /// Construct an emission factor. Returns an error for negative or
    /// non-finite inputs.
    pub fn new(grams_per_kwh: f64) -> Result<Self> {
        if !grams_per_kwh.is_finite() || grams_per_kwh < 0.0 {
            return Err(Error::InvalidIntensity(format!(
                "expected non-negative finite emission factor, got {grams_per_kwh}"
            )));
        }
        Ok(Self { grams_per_kwh })
    }

    /// Convert from eGRID's published lbs CO₂/MWh.
    pub fn from_lbs_per_mwh(lbs_per_mwh: f64) -> Result<Self> {
        Self::new(lbs_per_mwh * 453.592 / 1000.0)
    }

    /// The factor in gCO₂/kWh.
    pub fn value(&self) -> f64 {
        self.grams_per_kwh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_intensity() {
        assert!(CarbonIntensity::new(-1.0).is_err());
    }

    #[test]
    fn rejects_non_finite_intensity() {
        assert!(CarbonIntensity::new(f64::NAN).is_err());
        assert!(CarbonIntensity::new(f64::INFINITY).is_err());
    }

    #[test]
    fn lbs_per_mwh_conversion() {
        // 1000 lbs/MWh ≈ 453.592 gCO2/kWh
        let ci = CarbonIntensity::from_lbs_per_mwh(1000.0).unwrap();
        assert!((ci.value() - 453.592).abs() < 1e-6);
        let ef = EmissionFactor::from_lbs_per_mwh(1000.0).unwrap();
        assert!((ef.value() - 453.592).abs() < 1e-6);
    }

    #[test]
    fn fuel_codes_match_eia() {
        assert_eq!(FuelType::Col.as_code(), "COL");
        assert_eq!(FuelType::Ng.as_code(), "NG");
        assert_eq!(FuelType::Wnd.as_code(), "WND");
        assert_eq!(FuelType::ALL.len(), 9);
    }
}
