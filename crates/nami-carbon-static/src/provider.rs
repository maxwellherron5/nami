//! The [`StaticTableProvider`] implementation.

use async_trait::async_trait;
use time::{Duration, OffsetDateTime};

use nami_core::{
    CarbonIntensity, CarbonProvider, Confidence, ForecastPoint, IntensityKind, Region,
};

use crate::error::{Error, Result};
use crate::table;

/// An offline [`CarbonProvider`] backed by a small static table of annual
/// regional averages.
///
/// This exists for two reasons:
/// 1. To keep `nami` minimally functional when WattTime is unreachable.
/// 2. To exercise the [`CarbonProvider`] trait surface in tests without
///    network access.
///
/// It always emits [`IntensityKind::Average`] (because that's what an annual
/// mean is) and [`Confidence::Low`] (because annual means say nothing useful
/// about a specific hour).
///
/// # Example
///
/// ```no_run
/// use nami_carbon_static::StaticTableProvider;
/// use nami_core::{CarbonProvider, Region};
///
/// # async fn demo() {
/// let p = StaticTableProvider::new();
/// let now = p.current(Region::Ercot).await.unwrap();
/// assert!(now.value() > 0.0);
/// # }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct StaticTableProvider;

impl StaticTableProvider {
    /// Construct a new static-table provider. Cheap; no I/O.
    pub fn new() -> Self {
        Self
    }

    fn intensity_for(region: Region) -> Result<CarbonIntensity> {
        let g = table::mean_for(region).ok_or(Error::UnsupportedRegion(region))?;
        Ok(CarbonIntensity::new(g, IntensityKind::Average)?)
    }
}

#[async_trait]
impl CarbonProvider for StaticTableProvider {
    type Error = Error;

    fn name(&self) -> &'static str {
        "static-table"
    }

    fn supports(&self, region: Region) -> bool {
        table::mean_for(region).is_some()
    }

    async fn current(&self, region: Region) -> Result<CarbonIntensity> {
        Self::intensity_for(region)
    }

    async fn forecast(
        &self,
        region: Region,
        start: OffsetDateTime,
        horizon: Duration,
    ) -> Result<Vec<ForecastPoint>> {
        let intensity = Self::intensity_for(region)?;
        // Emit one point per hour over the horizon. The value is constant —
        // that's the whole point of "we have no real forecast." Confidence
        // is Low so any reasonable scheduler will treat this as a flat
        // signal and likely fall back to "run now."
        let hours = horizon.whole_hours().max(0);
        let points = (0..hours)
            .map(|h| ForecastPoint {
                at: start + Duration::hours(h),
                intensity,
                confidence: Confidence::Low,
            })
            .collect();
        Ok(points)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[tokio::test]
    async fn supports_all_known_regions() {
        let p = StaticTableProvider::new();
        for r in Region::ALL {
            assert!(p.supports(*r), "should support {r}");
            let ci = p.current(*r).await.unwrap();
            assert_eq!(ci.kind, IntensityKind::Average);
            assert!(ci.value() > 0.0);
        }
    }

    #[tokio::test]
    async fn forecast_emits_one_point_per_hour() {
        let p = StaticTableProvider::new();
        let start = datetime!(2030-01-01 00:00 UTC);
        let pts = p
            .forecast(Region::Ercot, start, Duration::hours(12))
            .await
            .unwrap();
        assert_eq!(pts.len(), 12);
        assert_eq!(pts[0].at, start);
        assert_eq!(pts[11].at, start + Duration::hours(11));
        assert!(pts.iter().all(|p| p.confidence == Confidence::Low));
    }
}
