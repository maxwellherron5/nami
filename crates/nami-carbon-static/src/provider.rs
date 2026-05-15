//! The [`StaticTableProvider`] implementation.

use time::Duration;

use nami_core::{
    CarbonIntensity, Confidence, DataGranularity, ProviderCapability, ProviderMetadata, Region,
};

use crate::error::{Error, Result};
use crate::table;

/// Offline fallback "provider" that returns flat annual regional means.
///
/// `StaticTableProvider` exists for one purpose: to give `nami` *some*
/// baseline carbon-intensity number when the EIA API is unreachable and
/// the local historical cache is missing or stale. It is **not** a
/// forecast provider and intentionally does not implement
/// [`ForecastProvider`](nami_core::ForecastProvider),
/// [`HistoricalCarbonProvider`](nami_core::HistoricalCarbonProvider), or
/// [`RealtimeGridProvider`](nami_core::RealtimeGridProvider) — declaring
/// any of those would overclaim what an annual mean can support.
///
/// Instead, the CLI calls [`StaticTableProvider::baseline`] directly when
/// it is forced into the static-fallback path, attaches a
/// [`Confidence::very_low`] note to the resulting estimate, and records
/// [`DataFreshness::StaticFallbackOnly`](nami_core::DataFreshness::StaticFallbackOnly)
/// in the run report so the user sees exactly which path was taken.
///
/// # Example
///
/// ```no_run
/// use nami_carbon_static::StaticTableProvider;
/// use nami_core::Region;
///
/// let p = StaticTableProvider::new();
/// let baseline = p.baseline(Region::Caiso).unwrap();
/// assert!(baseline.value() > 0.0);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct StaticTableProvider;

impl StaticTableProvider {
    /// Construct a new static-table provider. Cheap; no I/O.
    pub fn new() -> Self {
        Self
    }

    /// Whether the table contains an entry for `region`.
    pub fn supports(&self, region: Region) -> bool {
        table::mean_for(region).is_some()
    }

    /// The annual-mean baseline intensity for `region`. Always
    /// `VeryLow`-confidence in any context that uses it.
    pub fn baseline(&self, region: Region) -> Result<CarbonIntensity> {
        let g = table::mean_for(region).ok_or(Error::UnsupportedRegion(region))?;
        Ok(CarbonIntensity::new(g)?)
    }

    /// A pre-built [`Confidence`] suitable for any number sourced from
    /// this provider. Always `VeryLow` with an explanatory note.
    pub fn baseline_confidence() -> Confidence {
        Confidence::very_low(
            "static fallback: flat annual regional mean; not derived from \
             any time-varying data",
        )
    }
}

impl ProviderMetadata for StaticTableProvider {
    fn name(&self) -> &'static str {
        "static-fallback"
    }

    fn capabilities(&self) -> Vec<ProviderCapability> {
        // Intentionally empty: an annual mean cannot honestly claim any
        // of the data-fetching capabilities. The provider exists only as
        // a labelled baseline source.
        Vec::new()
    }

    fn granularity(&self) -> DataGranularity {
        DataGranularity::Annual
    }

    fn expected_lag(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::ConfidenceLevel;

    #[test]
    fn supports_all_known_regions() {
        let p = StaticTableProvider::new();
        for r in Region::ALL {
            assert!(p.supports(*r), "should support {r}");
            let ci = p.baseline(*r).unwrap();
            assert!(ci.value() > 0.0);
        }
    }

    #[test]
    fn declares_zero_capabilities() {
        let p = StaticTableProvider::new();
        assert!(p.capabilities().is_empty());
        assert_eq!(p.granularity(), DataGranularity::Annual);
        assert_eq!(p.expected_lag(), None);
    }

    #[test]
    fn baseline_confidence_is_very_low() {
        let c = StaticTableProvider::baseline_confidence();
        assert_eq!(c.level, ConfidenceLevel::VeryLow);
        assert_eq!(c.sample_count, 0);
        assert!(!c.notes.is_empty());
    }
}
