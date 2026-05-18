//! Integration test: load and sanity-check the *committed*
//! `data/egrid-factors.toml` (produced by the `refresh-egrid` maintainer
//! tool from the pinned eGRID2023 release). No network/Excel here.

use nami_carbon_eia::EgridFactors;
use nami_core::{FuelType, Region};

// Compile-time include, robust to test CWD (workspace root is two levels
// up from this crate's manifest dir).
const COMMITTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/egrid-factors.toml"
));

const LB_TO_G_PER_KWH: f64 = 453.592 / 1000.0;

#[test]
fn committed_table_loads_and_converts() {
    let f = EgridFactors::from_toml_str(COMMITTED).expect("committed eGRID table must parse");

    assert_eq!(f.release, "eGRID2023");
    assert_eq!(f.data_year, 2023);
    assert_eq!(f.methodology, "egrid-2023-ba");

    // Every (region, fuel) is present and total/panic-free.
    for r in Region::ALL {
        for ft in FuelType::ALL {
            let v = f.factor(*r, *ft).value();
            assert!(v.is_finite() && v >= 0.0, "{r:?}/{ft:?} = {v}");
        }
    }

    // Concrete anchors from the pinned eGRID2023 BA23 sheet (lb/MWh),
    // converted at the load boundary to gCO2/kWh.
    let caiso_col = f.factor(Region::Caiso, FuelType::Col).value();
    assert!(
        (caiso_col - 1133.331 * LB_TO_G_PER_KWH).abs() < 1e-6,
        "CAISO coal: {caiso_col}"
    );

    // Non-combustion fuels are zero.
    assert_eq!(f.factor(Region::Caiso, FuelType::Nuc).value(), 0.0);
    assert_eq!(f.factor(Region::Caiso, FuelType::Wnd).value(), 0.0);

    // NYISO has no coal generation in eGRID2023 → factor is 0.
    assert_eq!(f.factor(Region::Nyiso, FuelType::Col).value(), 0.0);

    // OTH and UNK share the non-baseload composite, so they're equal.
    let oth = f.factor(Region::Ercot, FuelType::Oth).value();
    let unk = f.factor(Region::Ercot, FuelType::Unk).value();
    assert_eq!(oth, unk);
    assert!(
        (oth - 1246.406 * LB_TO_G_PER_KWH).abs() < 1e-6,
        "ERCOT oth: {oth}"
    );
}
