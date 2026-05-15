//! The [`CarbonProvider`] trait: how `nami` talks to grid-carbon data sources.
//!
//! Implementations live in `nami-carbon-watttime` (primary) and
//! `nami-carbon-static` (offline fallback). A future `nami-carbon-electricitymaps`
//! is planned for Phase 1.
//!
//! The trait is async because real implementations make HTTP calls; the
//! static-table implementation simply ignores that and returns synchronously
//! inside an async fn.

use async_trait::async_trait;
use std::error::Error as StdError;
use time::{Duration, OffsetDateTime};

use crate::carbon::{CarbonIntensity, ForecastPoint};
use crate::region::Region;

/// A source of grid carbon-intensity data.
///
/// Implementations should be cheap to clone (e.g., wrap shared state in
/// `Arc`) so the CLI can pass them into both the scheduling and run phases
/// without recreating connections.
///
/// # Example
///
/// ```ignore
/// use nami_core::{CarbonProvider, Region};
/// use time::{Duration, OffsetDateTime};
///
/// async fn ask<P: CarbonProvider>(p: &P) {
///     let now = OffsetDateTime::now_utc();
///     let forecast = p.forecast(Region::Ercot, now, Duration::hours(24)).await;
///     // ... feed `forecast` into a Scheduler
/// }
/// ```
#[async_trait]
pub trait CarbonProvider: Send + Sync {
    /// Provider-specific error type, surfaced to the caller verbatim.
    type Error: StdError + Send + Sync + 'static;

    /// A short identifier for logs and reports (e.g., `"watttime"`).
    fn name(&self) -> &'static str;

    /// Whether this provider is known to support `region`.
    fn supports(&self, region: Region) -> bool;

    /// Current marginal (or average) intensity. Used during the run phase
    /// for sampling.
    async fn current(&self, region: Region) -> Result<CarbonIntensity, Self::Error>;

    /// Hourly forecast from `start` for at most `horizon`. Returned points
    /// should be sorted by `at` ascending. An empty result is valid and
    /// signals that the provider has no forecast data for this window — the
    /// scheduler will treat this as a forecast gap.
    async fn forecast(
        &self,
        region: Region,
        start: OffsetDateTime,
        horizon: Duration,
    ) -> Result<Vec<ForecastPoint>, Self::Error>;
}
