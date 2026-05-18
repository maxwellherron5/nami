//! Maintainer tool: download the pinned EPA eGRID release and convert its
//! balancing-authority sheet into the committed
//! `data/egrid-factors.toml`.
//!
//! This is **not** part of the shipped `nami` binary. It is gated behind
//! the `egrid-refresh` feature and pulls the `.xlsx` reader (`calamine`)
//! only when that feature is enabled:
//!
//! ```sh
//! cargo run -p nami-carbon-eia --features egrid-refresh --bin refresh-egrid
//! ```
//!
//! Run from the workspace root; it writes `data/egrid-factors.toml`.
//! The runtime ([`nami_carbon_eia::EgridFactors`]) only ever reads that
//! committed file — never the network or Excel.
//!
//! Fuel mapping (see `docs/methodology.md`):
//! `COL=BACCO2RT, NG=BAGCO2RT, OIL=BAOCO2RT`; `NUC/WAT/SUN/WND=0`
//! (non-combustion); `OTH/UNK=BANBCO2` (eGRID non-baseload composite).
//! A missing per-fuel rate falls back to `BANBCO2` with a recorded note.

use std::collections::BTreeMap;
use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};
use time::OffsetDateTime;

use nami_carbon_eia::{EGRID_SCHEMA_VERSION, EgridFile};
use nami_core::{FuelType, Region};

/// Pinned EPA eGRID release. Bumping this is a deliberate, reviewed
/// change (it moves every carbon number `nami` produces).
const PINNED_URL: &str =
    "https://www.epa.gov/system/files/documents/2025-06/egrid2023_data_rev2.xlsx";
const EGRID_RELEASE: &str = "eGRID2023";
const EGRID_DATA_YEAR: i32 = 2023;
const METHODOLOGY: &str = "egrid-2023-ba";
const SHEET: &str = "BA23";
const OUT_PATH: &str = "data/egrid-factors.toml";

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), BoxErr> {
    eprintln!("downloading pinned eGRID release:\n  {PINNED_URL}");
    // EPA's CDN rejects requests without a User-Agent (HTTP 403).
    let client = reqwest::Client::builder()
        .user_agent(concat!("nami-egrid-refresh/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let bytes = client
        .get(PINNED_URL)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    eprintln!("downloaded {} bytes; parsing sheet {SHEET}", bytes.len());

    let mut wb: Xlsx<_> = Xlsx::new(Cursor::new(bytes.to_vec()))?;
    let range = wb.worksheet_range(SHEET)?;

    let rows: Vec<&[Data]> = range.rows().collect();
    if rows.len() < 3 {
        return Err(format!("sheet {SHEET} has too few rows: {}", rows.len()).into());
    }
    // Row 0 = human descriptions, row 1 = field codes, data from row 2.
    let codes: Vec<String> = rows[1]
        .iter()
        .map(|c| c.to_string().trim().to_string())
        .collect();
    let col = |code: &str| -> Result<usize, BoxErr> {
        codes
            .iter()
            .position(|c| c == code)
            .ok_or_else(|| format!("column '{code}' not found in {SHEET}").into())
    };
    let i_bacode = col("BACODE")?;
    let i_coal = col("BACCO2RT")?;
    let i_oil = col("BAOCO2RT")?;
    let i_gas = col("BAGCO2RT")?;
    let i_nonbase = col("BANBCO2")?;

    let cell_f64 = |row: &[Data], idx: usize| -> Option<f64> {
        match row.get(idx) {
            Some(Data::Float(f)) => Some(*f),
            Some(Data::Int(i)) => Some(*i as f64),
            Some(Data::String(s)) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    };

    let mut regions: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    let mut notes: Vec<String> = vec![
        format!(
            "Per-fuel BA CO2 output emission rates (lb/MWh) from {EGRID_RELEASE} sheet {SHEET}."
        ),
        "COL=BACCO2RT, NG=BAGCO2RT, OIL=BAOCO2RT. NUC/WAT/SUN/WND=0 \
         (non-combustion). OTH/UNK=BANBCO2 (non-baseload composite)."
            .to_string(),
        "Missing per-fuel rate falls back to BANBCO2; such cases are \
         noted individually below."
            .to_string(),
    ];

    for row in &rows[2..] {
        let Some(Data::String(code)) = row.get(i_bacode) else {
            continue;
        };
        let Some(region) = nami_carbon_eia::region_from_respondent(code.trim()) else {
            continue; // not one of our seven Phase-0 BAs
        };

        let nonbase = cell_f64(row, i_nonbase)
            .ok_or_else(|| format!("{code}: missing BANBCO2 (non-baseload composite)"))?;
        let mut fuel_rate = |ft: FuelType, idx: usize| -> f64 {
            match cell_f64(row, idx) {
                Some(v) => v,
                None => {
                    notes.push(format!(
                        "{}/{}: no eGRID per-fuel rate; fell back to BANBCO2 ({nonbase})",
                        region.as_code(),
                        ft.as_code()
                    ));
                    nonbase
                }
            }
        };

        let mut m: BTreeMap<String, f64> = BTreeMap::new();
        m.insert(
            FuelType::Col.as_code().into(),
            fuel_rate(FuelType::Col, i_coal),
        );
        m.insert(
            FuelType::Ng.as_code().into(),
            fuel_rate(FuelType::Ng, i_gas),
        );
        m.insert(
            FuelType::Oil.as_code().into(),
            fuel_rate(FuelType::Oil, i_oil),
        );
        for z in [FuelType::Nuc, FuelType::Wat, FuelType::Sun, FuelType::Wnd] {
            m.insert(z.as_code().into(), 0.0);
        }
        m.insert(FuelType::Oth.as_code().into(), nonbase);
        m.insert(FuelType::Unk.as_code().into(), nonbase);
        regions.insert(region.as_code().to_string(), m);
    }

    // Every Phase-0 region must be present.
    let missing: Vec<&str> = Region::ALL
        .iter()
        .map(|r| r.as_code())
        .filter(|c| !regions.contains_key(*c))
        .collect();
    if !missing.is_empty() {
        return Err(format!("eGRID sheet missing regions: {missing:?}").into());
    }

    let file = EgridFile {
        schema_version: EGRID_SCHEMA_VERSION,
        egrid_release: EGRID_RELEASE.to_string(),
        egrid_data_year: EGRID_DATA_YEAR,
        source_url: PINNED_URL.to_string(),
        generated_at: OffsetDateTime::now_utc(),
        units: "lb_co2_per_mwh".to_string(),
        methodology: METHODOLOGY.to_string(),
        notes,
        regions,
    };

    let toml_text = toml::to_string_pretty(&file)?;
    std::fs::write(OUT_PATH, toml_text)?;
    eprintln!(
        "wrote {OUT_PATH} ({} regions, {EGRID_RELEASE})",
        file.regions.len()
    );
    Ok(())
}
