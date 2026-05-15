//! Annual-average carbon-intensity values for supported balancing
//! authorities.
//!
//! These exist solely so that `nami` can produce *some* baseline number
//! when EIA-930 and the historical cache are both unavailable. They are
//! coarse annual means and must always be reported with `VeryLow`
//! confidence and an explicit "static fallback only" data-freshness
//! marker. They are explicitly **not** a forecast.
//!
//! Numbers below are approximate 2023–2024 annual averages drawn from
//! public EIA and ISO sources, in gCO₂-equivalent per kWh. Treat them as
//! order-of-magnitude only. See `docs/methodology.md` for sourcing and
//! refresh expectations.

use nami_core::Region;

/// Annual mean intensity (gCO₂/kWh) per supported region.
pub(crate) const TABLE: &[(Region, f64)] = &[
    (Region::Caiso, 250.0),
    (Region::Ercot, 400.0),
    (Region::Miso, 430.0),
    (Region::Pjm, 380.0),
    (Region::Nyiso, 250.0),
    (Region::IsoNe, 250.0),
    (Region::Spp, 480.0),
];

pub(crate) fn mean_for(region: Region) -> Option<f64> {
    TABLE.iter().find_map(|(r, v)| (*r == region).then_some(*v))
}
