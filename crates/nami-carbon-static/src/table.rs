//! Annual-average carbon-intensity values for supported WattTime regions.
//!
//! Values are coarse: they exist to keep `nami` functional offline and to
//! exercise the [`CarbonProvider`](nami_core::CarbonProvider) trait surface,
//! **not** to make real scheduling decisions. The static provider always
//! reports [`Confidence::Low`] and is expected to be overridden by a live
//! provider whenever one is reachable.
//!
//! Numbers below are approximate 2023–2024 annual averages from public EIA
//! and ISO sources, in gCO₂-equivalent per kWh. Treat them as order-of-
//! magnitude only.

use nami_core::Region;

/// Annual mean intensity (gCO₂/kWh) for one supported region.
pub(crate) const TABLE: &[(Region, f64)] = &[
    (Region::CaisoNorth, 230.0),
    (Region::Ercot, 380.0),
    (Region::Miso, 430.0),
    (Region::Pjm, 360.0),
    (Region::Nyiso, 220.0),
    (Region::IsoNe, 240.0),
    (Region::Spp, 410.0),
];

/// Look up the annual mean for `region`.
pub(crate) fn mean_for(region: Region) -> Option<f64> {
    TABLE.iter().find_map(|(r, v)| (*r == region).then_some(*v))
}
