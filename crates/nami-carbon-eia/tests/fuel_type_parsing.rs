//! Integration test: parse the captured (sanitized real) EIA-930
//! `fuel-type-data` fixture. No live API access — see
//! `tests/fixtures/README.md` for provenance.

use nami_carbon_eia::parse_fuel_type_data;
use nami_core::{FuelType, Region};

const FIXTURE: &str = include_str!("fixtures/eia-fuel-type-sample.json");

#[test]
fn parses_captured_fixture() {
    let mix = parse_fuel_type_data(FIXTURE).expect("fixture should parse");

    // 2 respondents × 7 hourly periods (T00..=T06).
    assert_eq!(mix.len(), 14, "expected 14 region-hours, got {}", mix.len());

    // Output is ordered by (at, region).
    for w in mix.windows(2) {
        assert!(
            (w[0].at, w[0].region.as_code()) <= (w[1].at, w[1].region.as_code()),
            "output not ordered"
        );
    }

    // Only CAISO and ERCOT appear.
    assert!(
        mix.iter()
            .all(|m| matches!(m.region, Region::Caiso | Region::Ercot))
    );

    // Concrete anchor: CISO 2026-05-12T00 — GEO(675) + OTH(-557) folded
    // into Oth = 118.0.
    let ciso0 = mix
        .iter()
        .find(|m| {
            m.region == Region::Caiso && m.at == time::macros::datetime!(2026-05-12 00:00 UTC)
        })
        .expect("CISO T00 present");
    let oth = ciso0
        .generation_mwh
        .iter()
        .find(|(f, _)| *f == FuelType::Oth)
        .expect("CISO T00 has Oth");
    assert!((oth.1 - 118.0).abs() < 1e-6, "GEO+OTH aggregation: {oth:?}");

    // Concrete anchor: ERCO 2026-05-12T00 — BAT(-55) is storage and must
    // be excluded; the mix must contain no battery contribution.
    let erco0 = mix
        .iter()
        .find(|m| {
            m.region == Region::Ercot && m.at == time::macros::datetime!(2026-05-12 00:00 UTC)
        })
        .expect("ERCO T00 present");
    // Every fuel present is one of our canonical generation categories,
    // and battery (-55) did not leak in as a negative blob.
    assert!(
        erco0
            .generation_mwh
            .iter()
            .all(|(f, _)| FuelType::ALL.contains(f))
    );
    let erco0_total: f64 = erco0.generation_mwh.iter().map(|(_, v)| v).sum();
    // Raw ERCO T00 generation rows (excluding BAT -55):
    // COL 5409 + NG 26481 + NUC 3861 + OTH 52 + SUN 23788 + WAT 19 + WND 3593
    assert!(
        (erco0_total - 63203.0).abs() < 1e-6,
        "ERCO T00 total excl. storage: {erco0_total}"
    );

    // The fixture has no genuinely unknown fuel codes, so no notes.
    assert!(mix.iter().all(|m| m.notes.is_empty()));
}
