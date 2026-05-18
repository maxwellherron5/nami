//! Carbon-intensity derivation.
//!
//! Turns one normalized [`FuelMixHour`] plus the [`EgridFactors`] table
//! into a [`CarbonObservation`] — an *estimated average* gCO₂/kWh for
//! that region-hour. Pure, synchronous math (no I/O); CLAUDE.md forbids
//! async for derivation.
//!
//! ## Formula
//!
//! ```text
//! intensity = Σ_fuel (gen_mwh[fuel] × ef[fuel, region])
//!           / Σ_fuel gen_mwh[fuel]
//! ```
//!
//! `ef` is already gCO₂/kWh and generation is MWh; the MWh↔kWh factor
//! (×1000) cancels between numerator and denominator, so the result is a
//! generation-weighted mean of per-fuel factors, directly in gCO₂/kWh.
//! The denominator is the **sum of fuel-type generation** (internal
//! consistency), never EIA's separately-reported total.
//!
//! ## Methodology decisions (documented, see `docs/methodology.md`)
//!
//! - **Negative generation is clamped to 0** (with a recorded note
//!   listing each clamped fuel and the raw value). Net-negative net
//!   generation is a small accounting artifact; counting it would yield
//!   negative "emissions" and could drive the denominator non-positive.
//!   Clamping a fuel to 0 is equivalent to excluding it.
//! - **An hour with no positive generation is refused**, not zeroed:
//!   [`Error::DerivationFailed`]. No defensible number exists, so the
//!   caller treats it as a gap (consistent with "refuse to estimate").
//! - Provenance from item-6 normalization (`FuelMixHour::notes`, e.g.
//!   unknown-fuel→UNK mappings) is **carried forward** into
//!   [`DerivedObservation::warnings`] so nothing is hidden downstream.

use nami_core::CarbonObservation;

use crate::api::FuelMixHour;
use crate::egrid::EgridFactors;
use crate::error::{Error, Result};

/// Methodology label stamped on every derived observation. Combines the
/// EIA-930 parsing version with the pinned eGRID factor table.
pub const DERIVATION_METHODOLOGY: &str = "eia-930-v1+egrid-2023-ba";

/// A derived observation plus the provenance/warnings that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedObservation {
    /// The estimated average-intensity observation for the hour.
    pub observation: CarbonObservation,
    /// Carried-forward `FuelMixHour` notes plus any derivation-specific
    /// notes (e.g. clamped negative generation). Surfaced, never hidden.
    pub warnings: Vec<String>,
}

/// Derive the estimated average carbon intensity for one region-hour.
///
/// Returns [`Error::DerivationFailed`] when, after clamping negatives,
/// there is no positive generation to weight by.
///
/// # Examples
///
/// ```no_run
/// use nami_carbon_eia::{derive_intensity, EgridFactors};
/// # fn demo(mix: &nami_carbon_eia::FuelMixHour, f: &EgridFactors) {
/// let derived = derive_intensity(mix, f).expect("hour has generation");
/// println!("{} gCO2/kWh", derived.observation.intensity.value());
/// # }
/// ```
pub fn derive_intensity(mix: &FuelMixHour, factors: &EgridFactors) -> Result<DerivedObservation> {
    let mut warnings = mix.notes.clone();

    let mut numerator = 0.0_f64;
    let mut denominator = 0.0_f64;

    for &(fuel, raw_gen) in &mix.generation_mwh {
        let mwh = if raw_gen < 0.0 {
            warnings.push(format!(
                "clamped negative {} generation ({raw_gen:.3} MWh) to 0",
                fuel.as_code()
            ));
            0.0
        } else {
            raw_gen
        };
        if mwh == 0.0 {
            continue; // contributes nothing to either sum
        }
        let ef = factors.factor(mix.region, fuel).value();
        numerator += mwh * ef;
        denominator += mwh;
    }

    if denominator <= 0.0 {
        return Err(Error::DerivationFailed(format!(
            "{} at {}: no positive generation after clamping",
            mix.region.as_code(),
            mix.at
        )));
    }

    let intensity = nami_core::CarbonIntensity::new(numerator / denominator).map_err(|e| {
        Error::DerivationFailed(format!(
            "{} at {}: invalid derived intensity: {e}",
            mix.region.as_code(),
            mix.at
        ))
    })?;

    Ok(DerivedObservation {
        observation: CarbonObservation {
            at: mix.at,
            intensity,
            methodology: DERIVATION_METHODOLOGY.to_string(),
        },
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::{FuelType, Region};
    use time::macros::datetime;

    /// Full synthetic factor table (all 7 regions × 9 fuels), per-fuel
    /// lb/MWh = (fuel_index + 1) * 1000 so values are distinct.
    fn factors() -> EgridFactors {
        let mut s = String::from(
            "schema_version = 1\n\
             egrid_release = \"t\"\n\
             egrid_data_year = 2023\n\
             source_url = \"https://example.invalid\"\n\
             generated_at = \"2026-05-18T00:00:00Z\"\n\
             units = \"lb_co2_per_mwh\"\n\
             methodology = \"t\"\n\
             notes = []\n\n",
        );
        for r in Region::ALL {
            s.push_str(&format!("[regions.{}]\n", r.as_code()));
            for (i, f) in FuelType::ALL.iter().enumerate() {
                s.push_str(&format!(
                    "{} = {}\n",
                    f.as_code(),
                    (i as f64 + 1.0) * 1000.0
                ));
            }
            s.push('\n');
        }
        EgridFactors::from_toml_str(&s).unwrap()
    }

    fn mix(generation: Vec<(FuelType, f64)>, notes: Vec<String>) -> FuelMixHour {
        FuelMixHour {
            at: datetime!(2026-05-12 00:00 UTC),
            region: Region::Caiso,
            generation_mwh: generation,
            notes,
        }
    }

    #[test]
    fn generation_weighted_mean() {
        let f = factors();
        let m = mix(vec![(FuelType::Ng, 600.0), (FuelType::Wnd, 400.0)], vec![]);
        let d = derive_intensity(&m, &f).unwrap();

        let ef_ng = f.factor(Region::Caiso, FuelType::Ng).value();
        let ef_wnd = f.factor(Region::Caiso, FuelType::Wnd).value();
        let expected = (600.0 * ef_ng + 400.0 * ef_wnd) / 1000.0;
        assert!((d.observation.intensity.value() - expected).abs() < 1e-9);
        assert_eq!(d.observation.methodology, DERIVATION_METHODOLOGY);
        assert_eq!(d.observation.at, datetime!(2026-05-12 00:00 UTC));
        assert!(d.warnings.is_empty());
    }

    #[test]
    fn negative_generation_clamped_with_note() {
        let f = factors();
        let m = mix(vec![(FuelType::Col, -50.0), (FuelType::Ng, 1000.0)], vec![]);
        let d = derive_intensity(&m, &f).unwrap();

        // COL clamped to 0 → result is purely NG's factor.
        let ef_ng = f.factor(Region::Caiso, FuelType::Ng).value();
        assert!((d.observation.intensity.value() - ef_ng).abs() < 1e-9);
        assert_eq!(d.warnings.len(), 1);
        assert!(d.warnings[0].contains("clamped negative COL"));
    }

    #[test]
    fn carries_mix_notes_forward() {
        let f = factors();
        let m = mix(
            vec![(FuelType::Ng, 100.0)],
            vec!["unrecognized EIA fuel code 'X' mapped to UNK".to_string()],
        );
        let d = derive_intensity(&m, &f).unwrap();
        assert!(d.warnings.iter().any(|w| w.contains("mapped to UNK")));
    }

    #[test]
    fn refuses_hour_with_no_positive_generation() {
        let f = factors();
        let m = mix(vec![(FuelType::Col, -10.0), (FuelType::Ng, 0.0)], vec![]);
        assert!(matches!(
            derive_intensity(&m, &f),
            Err(Error::DerivationFailed(_))
        ));
    }

    #[test]
    fn refuses_empty_mix() {
        let f = factors();
        let m = mix(vec![], vec![]);
        assert!(matches!(
            derive_intensity(&m, &f),
            Err(Error::DerivationFailed(_))
        ));
    }

    #[test]
    fn single_fuel_mix_equals_that_fuels_factor() {
        let f = factors();
        let m = mix(vec![(FuelType::Nuc, 500.0)], vec![]);
        let d = derive_intensity(&m, &f).unwrap();
        let ef_nuc = f.factor(Region::Caiso, FuelType::Nuc).value();
        assert!((d.observation.intensity.value() - ef_nuc).abs() < 1e-9);
    }
}
