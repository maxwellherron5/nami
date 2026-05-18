//! EIA-930 + EPA eGRID public-data carbon provider for `nami`.
//!
//! Phase 0 status: the historical cache format ([`HistoricalCache`]),
//! EIA-930 `fuel-type-data` parsing ([`parse_fuel_type_data`]), the
//! eGRID factor-table loader ([`EgridFactors`]), carbon-intensity
//! derivation ([`derive_intensity`]), the historical-pattern forecast
//! model ([`historical_pattern_forecast`]), and the paginated EIA fetch +
//! cache refresh ([`refresh_region_cache`]) are implemented. Live API
//! tests are gated behind the `live-eia` feature (off by default;
//! `tests/live_eia.rs`, requires `EIA_API_KEY`).
//!
//! The committed `data/egrid-factors.toml` is produced by the
//! `refresh-egrid` maintainer tool (behind the `egrid-refresh` feature),
//! which downloads a pinned EPA eGRID release and converts its
//! balancing-authority sheet. The shipped `nami` binary never includes
//! that tool or its `.xlsx`/HTTP dependencies.
//!
//! When implemented, this crate will provide:
//!
//! - A [`HistoricalCarbonProvider`](nami_core::HistoricalCarbonProvider)
//!   backed by EIA-930 hourly fuel-mix observations multiplied through
//!   eGRID emission factors.
//! - A [`ForecastProvider`](nami_core::ForecastProvider) implementing a
//!   historical-pattern model (mean over matching hour-of-day,
//!   day-of-week, month from recent weeks).
//! - A [`RealtimeGridProvider`](nami_core::RealtimeGridProvider) returning
//!   the most recent observed snapshot with explicit lag metadata.
//!
//! All three will advertise their capabilities via
//! [`ProviderMetadata`](nami_core::ProviderMetadata) and surface
//! [`DataFreshness`](nami_core::DataFreshness) honestly — including
//! "stale", "historical-cache-only", and "no usable data" states.

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]

mod api;
mod cache;
mod derive;
mod egrid;
mod error;
mod forecast;
mod refresh;

pub use api::{FuelMixHour, parse_fuel_type_data, region_from_respondent, respondent_code};
pub use cache::{
    CACHE_SCHEMA_VERSION, DEFAULT_CACHE_PATH, DEFAULT_MAX_CACHE_AGE, HistoricalCache, RegionHistory,
};
pub use derive::{DERIVATION_METHODOLOGY, DerivedObservation, derive_intensity};
pub use egrid::{DEFAULT_EGRID_PATH, EGRID_SCHEMA_VERSION, EgridFactors, EgridFile};
pub use error::{Error, Result};
pub use forecast::{DEFAULT_FORECAST_WEEKS, historical_pattern_forecast};
pub use refresh::{RefreshSummary, fetch_region_json, process_response, refresh_region_cache};
