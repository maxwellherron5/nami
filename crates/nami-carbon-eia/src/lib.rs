//! EIA-930 + EPA eGRID public-data carbon provider for `nami`.
//!
//! Phase 0 skeleton. The HTTP client, fixture parsing, eGRID factor table,
//! carbon-intensity derivation, and historical-pattern forecast model all
//! land in subsequent sessions per `CLAUDE.md`'s phased implementation plan.
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

mod error;

pub use error::{Error, Result};
