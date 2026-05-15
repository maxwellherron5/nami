//! Observed and forecast data points.
//!
//! [`CarbonObservation`] is the historical record. [`ForecastPoint`] is the
//! model output — explicitly *modelled*, not fetched from a carbon
//! forecaster API. [`ForecastHorizon`] describes the window over which a
//! forecast is requested.

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::carbon::CarbonIntensity;
use crate::confidence::Confidence;

/// One observed historical hour of (estimated) grid carbon intensity.
///
/// Produced by `nami-carbon-eia` from EIA-930 generation-by-fuel-type
/// observations multiplied through eGRID factors. The intensity is
/// always an *estimated* average — EIA does not publish carbon directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CarbonObservation {
    /// UTC instant marking the start of the hour this observation covers.
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// Estimated average intensity for this hour in gCO₂/kWh.
    pub intensity: CarbonIntensity,
    /// Methodology label this observation was derived under (e.g.,
    /// "eia-930-v1+egrid-2024-subregion"). Lets consumers track which
    /// version of the math produced the number.
    pub methodology: String,
}

/// One point on a forecast curve.
///
/// Unlike [`CarbonObservation`], a `ForecastPoint` is the output of
/// `nami`'s historical-pattern model. The `confidence` field must
/// reflect how many historical samples backed the estimate and how much
/// they varied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastPoint {
    /// UTC instant marking the start of the hour this point covers.
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// Modelled mean intensity for this hour.
    pub intensity: CarbonIntensity,
    /// Confidence + evidence backing the estimate.
    pub confidence: Confidence,
    /// Methodology label (e.g.,
    /// "historical-pattern-mean-8w-hour-dow-month-v1").
    pub methodology: String,
}

/// A window over which a forecast is requested.
///
/// A `[start, start + duration)` half-open interval in UTC. Forecasts are
/// hourly-aligned in Phase 0; sub-hourly horizons are not meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ForecastHorizon {
    /// UTC instant the forecast window opens.
    #[serde(with = "time::serde::rfc3339")]
    pub start: OffsetDateTime,
    /// Length of the forecast window.
    #[serde(with = "crate::duration_secs")]
    pub duration: Duration,
}

impl ForecastHorizon {
    /// Construct a horizon of `duration` starting at `start`.
    pub fn new(start: OffsetDateTime, duration: Duration) -> Self {
        Self { start, duration }
    }

    /// The exclusive end of the forecast window.
    pub fn end(&self) -> OffsetDateTime {
        self.start + self.duration
    }
}
