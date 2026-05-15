//! Carbon-intensity values and forecast points.
//!
//! All carbon intensities are stored internally in grams CO₂-equivalent per
//! kilowatt-hour (gCO₂/kWh). API boundaries convert from whatever the
//! upstream provider returns (WattTime reports lbs CO₂/MWh; ElectricityMaps
//! reports gCO₂/kWh directly).

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::{Error, Result};

/// Whether an intensity is marginal or average.
///
/// Marginal is the right signal for load shifting: it answers "what does
/// running one more kWh emit *right now*?" Average is what most public
/// dashboards display and is included for compatibility with providers that
/// expose only average values. Schedulers should prefer marginal when both
/// are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntensityKind {
    /// Emissions per additional kWh of demand met by the marginal generator.
    Marginal,
    /// Mean emissions per kWh across all generators currently on the bus.
    Average,
}

/// A grid carbon-intensity reading in gCO₂/kWh.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CarbonIntensity {
    /// gCO₂-equivalent per kWh.
    grams_per_kwh: f64,
    /// Whether this is a marginal or average value.
    pub kind: IntensityKind,
}

impl CarbonIntensity {
    /// Construct a carbon-intensity value. Returns an error for negative or
    /// non-finite inputs.
    ///
    /// # Examples
    ///
    /// ```
    /// use nami_core::{CarbonIntensity, IntensityKind};
    ///
    /// let ci = CarbonIntensity::new(245.0, IntensityKind::Marginal).unwrap();
    /// assert_eq!(ci.value(), 245.0);
    /// ```
    pub fn new(grams_per_kwh: f64, kind: IntensityKind) -> Result<Self> {
        if !grams_per_kwh.is_finite() || grams_per_kwh < 0.0 {
            return Err(Error::InvalidIntensity(format!(
                "expected non-negative finite gCO2/kWh, got {grams_per_kwh}"
            )));
        }
        Ok(Self {
            grams_per_kwh,
            kind,
        })
    }

    /// The intensity in gCO₂/kWh.
    pub fn value(&self) -> f64 {
        self.grams_per_kwh
    }

    /// Convert from WattTime's lbs CO₂/MWh into internal gCO₂/kWh.
    ///
    /// 1 lb = 453.592 g; 1 MWh = 1000 kWh; so `lbs/MWh × 453.592 / 1000 = g/kWh`.
    pub fn from_lbs_per_mwh(lbs_per_mwh: f64, kind: IntensityKind) -> Result<Self> {
        Self::new(lbs_per_mwh * 453.592 / 1000.0, kind)
    }
}

/// Provider confidence in a single forecast point.
///
/// WattTime's forecast horizon extends to ~72 hours with degrading certainty.
/// Schedulers use this to decide whether a far-future window is trustworthy
/// enough to bet on, or whether running immediately is safer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Forecast horizon close enough that we trust the value.
    High,
    /// Mid-horizon; usable but factor in uncertainty.
    Medium,
    /// Far-horizon or noisy upstream signal; treat with skepticism.
    Low,
}

/// One point on a forecast curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ForecastPoint {
    /// The UTC instant this forecast applies to.
    pub at: OffsetDateTime,
    /// The expected intensity at `at`.
    pub intensity: CarbonIntensity,
    /// How much weight the scheduler should place on this point.
    pub confidence: Confidence,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_intensity() {
        assert!(CarbonIntensity::new(-1.0, IntensityKind::Marginal).is_err());
    }

    #[test]
    fn rejects_non_finite_intensity() {
        assert!(CarbonIntensity::new(f64::NAN, IntensityKind::Average).is_err());
        assert!(CarbonIntensity::new(f64::INFINITY, IntensityKind::Average).is_err());
    }

    #[test]
    fn lbs_per_mwh_conversion() {
        // 1000 lbs/MWh ≈ 453.592 gCO2/kWh
        let ci = CarbonIntensity::from_lbs_per_mwh(1000.0, IntensityKind::Marginal).unwrap();
        assert!((ci.value() - 453.592).abs() < 1e-6);
    }
}
