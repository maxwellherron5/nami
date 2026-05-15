//! Provider traits and capability declarations.
//!
//! Per `CLAUDE.md`, `nami` does **not** treat all carbon providers as
//! equivalent. Each provider declares exactly which capabilities it
//! supports via [`ProviderMetadata::capabilities`], and the scheduler
//! makes decisions based on those capabilities — not just the numbers a
//! provider returns.
//!
//! The three data-fetching traits are intentionally narrow: a provider
//! that only has historical observations should implement only
//! [`HistoricalCarbonProvider`], so the type system prevents callers from
//! accidentally asking it for a forecast.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::error::Error as StdError;
use time::{Duration, OffsetDateTime};

use crate::carbon::{CarbonIntensity, FuelType};
use crate::confidence::{DataFreshness, DataGranularity};
use crate::observation::{CarbonObservation, ForecastHorizon, ForecastPoint};
use crate::region::Region;

/// What a provider can do. A provider must only list capabilities it
/// genuinely supports — overclaiming is worse than underclaiming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapability {
    /// Returns hourly historical observations.
    HistoricalHourly,
    /// Returns sub-hourly historical observations.
    HistoricalSubHourly,
    /// Returns real-time observed grid state with no meaningful lag.
    RealtimeObserved,
    /// Returns observed grid state with documented lag.
    RealtimeObservedWithLag,
    /// Returns day-ahead load (demand) forecasts from the BA.
    DayAheadLoadForecast,
    /// Returns renewable-generation forecasts (solar / wind).
    RenewableForecast,
    /// Returns *average* carbon-intensity forecasts. Phase 0 EIA provider
    /// must NOT advertise this — its forecast is `nami`'s historical-
    /// pattern model layered on EIA observations, not a forecast from
    /// EIA. Implementations that genuinely forecast (e.g. ISO public
    /// feeds in Phase 1) may declare it.
    AverageCarbonForecast,
    /// Returns marginal-emissions point estimates. Phase 0: never declared.
    MarginalEmissionsEstimate,
    /// Returns marginal-emissions forecasts. Phase 0: never declared.
    MarginalEmissionsForecast,
}

/// Static descriptive metadata about a provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderInfo {
    /// Short identifier (e.g., `"eia-egrid"`, `"static-fallback"`).
    pub name: String,
    /// Capabilities this provider declares.
    pub capabilities: Vec<ProviderCapability>,
    /// Temporal granularity of the provider's data.
    pub granularity: DataGranularity,
    /// Expected lag between real-world time and what the provider can
    /// answer for. `None` if the provider has no real-time dimension.
    #[serde(with = "crate::duration_secs::option")]
    pub expected_lag: Option<Duration>,
}

/// Metadata-only trait. Every provider should implement this.
pub trait ProviderMetadata: Send + Sync {
    /// Short identifier (e.g., `"eia-egrid"`).
    fn name(&self) -> &'static str;

    /// The set of capabilities this provider supports.
    fn capabilities(&self) -> Vec<ProviderCapability>;

    /// Temporal granularity of returned data.
    fn granularity(&self) -> DataGranularity;

    /// Typical lag for the provider's freshest data, if it has any.
    fn expected_lag(&self) -> Option<Duration>;

    /// Convenience: snapshot the metadata as a serializable record.
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: self.name().to_string(),
            capabilities: self.capabilities(),
            granularity: self.granularity(),
            expected_lag: self.expected_lag(),
        }
    }
}

/// Source of historical (past-hour) carbon-intensity observations.
///
/// Implementations should be cheap to clone (e.g., wrap shared state in
/// `Arc`) so the CLI can share a provider across phases.
#[async_trait]
pub trait HistoricalCarbonProvider: ProviderMetadata {
    /// Provider-specific error type.
    type Error: StdError + Send + Sync + 'static;

    /// Whether this provider has historical data for `region`.
    fn supports(&self, region: Region) -> bool;

    /// Hourly observations in `[start, end)`. Returned points should be
    /// sorted by `at` ascending. Gaps (missing hours) are permitted; the
    /// caller is responsible for downgrading confidence accordingly.
    async fn historical_intensity(
        &self,
        region: Region,
        start: OffsetDateTime,
        end: OffsetDateTime,
    ) -> Result<Vec<CarbonObservation>, Self::Error>;
}

/// Source of forecast carbon-intensity points.
///
/// "Forecast" here is whatever model the provider documents — for the
/// Phase 0 EIA provider, that is a historical-pattern mean over matching
/// (hour-of-day, day-of-week, month) samples from recent weeks. Callers
/// must surface the methodology in user-facing output.
#[async_trait]
pub trait ForecastProvider: ProviderMetadata {
    /// Provider-specific error type.
    type Error: StdError + Send + Sync + 'static;

    /// Whether this provider can produce a forecast for `region`.
    fn supports(&self, region: Region) -> bool;

    /// Hourly forecast points covering `horizon`. Returned points should
    /// be sorted by `at` ascending. Each point carries its own confidence
    /// and methodology label.
    async fn forecast_intensity(
        &self,
        region: Region,
        horizon: ForecastHorizon,
    ) -> Result<Vec<ForecastPoint>, Self::Error>;
}

/// Snapshot of the latest observed grid state for a region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridSnapshot {
    /// UTC instant the snapshot represents.
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// Fuel-mix shares (each entry's value is MWh generated in the hour).
    pub generation_mwh: Vec<(FuelType, f64)>,
    /// Estimated average intensity derived from the snapshot, if computable.
    pub intensity: Option<CarbonIntensity>,
    /// Freshness state of the snapshot.
    pub freshness: DataFreshness,
}

/// Source of the most recent observed grid state.
#[async_trait]
pub trait RealtimeGridProvider: ProviderMetadata {
    /// Provider-specific error type.
    type Error: StdError + Send + Sync + 'static;

    /// Whether this provider has live data for `region`.
    fn supports(&self, region: Region) -> bool;

    /// Latest observed snapshot. Should populate `freshness` honestly.
    async fn latest_observed_mix(&self, region: Region) -> Result<GridSnapshot, Self::Error>;
}
