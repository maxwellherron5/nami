//! EIA-930 v2 API response parsing.
//!
//! Turns an `electricity/rto/fuel-type-data` JSON response into normalized
//! per-region-hour fuel-mix data ([`FuelMixHour`]). This is the parsing
//! layer only — fetching over HTTP is Phase 0 implementation item 13, and
//! carbon-intensity derivation from the mix is item 8.
//!
//! See `docs/eia-api-notes.md` for the response shape, the
//! respondent-code mapping, the UTC period assumption, the
//! `value`-as-string quirk, and the storage/unknown-fuel handling.
//!
//! Key normalization decisions (all documented and tested):
//!
//! - **Respondent → [`Region`].** EIA balancing-authority codes differ
//!   from ours (`CISO`→CAISO, `ERCO`→ERCOT, `NYIS`→NYISO, `ISNE`→ISONE,
//!   `SWPP`→SPP, `MISO`/`PJM` unchanged). An unrecognized respondent is a
//!   hard error: we only ever query our seven regions, so an unexpected
//!   one signals a query/scope bug, not benign extra data.
//! - **Storage excluded.** `BAT` (battery) and `PS` (pumped storage) are
//!   not primary generation and carry no intrinsic emission factor; their
//!   rows are dropped from the generation mix entirely.
//! - **`GEO` → `OTH`.** Geothermal folds into `OTH` per CLAUDE.md. When
//!   both `GEO` and `OTH` appear in the same region-hour their values are
//!   summed.
//! - **Unknown fuel → `UNK` + note.** A genuinely unrecognized code maps
//!   to [`FuelType::Unk`] and records a surfaced note on the affected
//!   [`FuelMixHour`], so `nami` keeps working if EIA adds a code while
//!   the assumption stays visible.
//! - **Missing values.** A `null`/absent `value` means the fuel was not
//!   reported that hour and is skipped (absence is not a fabricated
//!   zero). An unparseable/non-finite value is skipped *and* noted.

use std::collections::BTreeMap;

use serde::Deserialize;
use time::{Date, OffsetDateTime, PrimitiveDateTime, Time};

use nami_core::{FuelType, Region};

use crate::error::{Error, Result};

/// Normalized fuel-mix generation for one region-hour.
///
/// `generation_mwh` contains only *primary generation* fuels (storage
/// excluded), aggregated per [`FuelType`] and ordered by
/// [`FuelType::ALL`]. Values are raw MWh as reported (which can be
/// negative for some fuels); interpretation is the derivation layer's job.
#[derive(Debug, Clone, PartialEq)]
pub struct FuelMixHour {
    /// UTC hour this mix covers.
    pub at: OffsetDateTime,
    /// Region (mapped from the EIA respondent code).
    pub region: Region,
    /// Generation by fuel type, MWh, aggregated and ordered.
    pub generation_mwh: Vec<(FuelType, f64)>,
    /// Surfaced notes (e.g. an unrecognized EIA fuel code mapped to UNK,
    /// or an unparseable value that was skipped).
    pub notes: Vec<String>,
}

/// EIA balancing-authority respondent codes ↔ our [`Region`].
const RESPONDENTS: &[(&str, Region)] = &[
    ("CISO", Region::Caiso),
    ("ERCO", Region::Ercot),
    ("MISO", Region::Miso),
    ("PJM", Region::Pjm),
    ("NYIS", Region::Nyiso),
    ("ISNE", Region::IsoNe),
    ("SWPP", Region::Spp),
];

/// Map an EIA respondent code to a [`Region`], if recognized.
pub fn region_from_respondent(code: &str) -> Option<Region> {
    RESPONDENTS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, r)| *r)
}

/// The EIA respondent code for a [`Region`] (used to build queries in a
/// later session).
///
/// An exhaustive match (not a table lookup) so the compiler guarantees
/// totality — no panic path. Must stay in sync with the `RESPONDENTS`
/// table; the `respondent_round_trip` test enforces that.
pub fn respondent_code(region: Region) -> &'static str {
    match region {
        Region::Caiso => "CISO",
        Region::Ercot => "ERCO",
        Region::Miso => "MISO",
        Region::Pjm => "PJM",
        Region::Nyiso => "NYIS",
        Region::IsoNe => "ISNE",
        Region::Spp => "SWPP",
    }
}

/// Classification of a raw EIA fuel-type code.
enum FuelClass {
    /// A primary generation fuel counting toward the mix.
    Generation(FuelType),
    /// Storage (`BAT`, `PS`) — excluded from the generation mix.
    Storage,
    /// Unrecognized — mapped to `UNK` with a surfaced note.
    Unknown,
}

fn classify_fuel(code: &str) -> FuelClass {
    match code.to_ascii_uppercase().as_str() {
        "BAT" | "PS" => FuelClass::Storage,
        _ => match FuelType::from_eia_code(code) {
            Some(ft) => FuelClass::Generation(ft),
            None => FuelClass::Unknown,
        },
    }
}

/// Parse an EIA hourly period string (`YYYY-MM-DDTHH`) as a UTC instant.
///
/// EIA-930 RTO hourly series are reported in UTC; CLAUDE.md requires we
/// always treat them as such.
fn parse_eia_period(period: &str) -> Result<OffsetDateTime> {
    let (date_str, hour_str) = period.split_once('T').ok_or_else(|| {
        Error::Malformed(format!("EIA period '{period}' missing 'T' hour separator"))
    })?;
    let date_fmt = time::macros::format_description!("[year]-[month]-[day]");
    let date = Date::parse(date_str, date_fmt)
        .map_err(|e| Error::Malformed(format!("EIA period '{period}' bad date: {e}")))?;
    let hour: u8 = hour_str
        .parse()
        .map_err(|e| Error::Malformed(format!("EIA period '{period}' bad hour: {e}")))?;
    let time = Time::from_hms(hour, 0, 0)
        .map_err(|e| Error::Malformed(format!("EIA period '{period}' hour out of range: {e}")))?;
    Ok(PrimitiveDateTime::new(date, time).assume_utc())
}

/// Parse EIA's `value` field, which may be a JSON string, number, or null.
///
/// `Ok(None)` = missing (null / absent / empty). `Ok(Some(f))` = a finite
/// number. `Err(reason)` = present but unparseable or non-finite (caller
/// skips the row and records the reason as a note).
fn parse_value(v: &serde_json::Value) -> std::result::Result<Option<f64>, String> {
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(n) => match n.as_f64() {
            Some(f) if f.is_finite() => Ok(Some(f)),
            _ => Err(format!("non-finite number {n}")),
        },
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(None);
            }
            match t.parse::<f64>() {
                Ok(f) if f.is_finite() => Ok(Some(f)),
                Ok(_) => Err(format!("non-finite value \"{s}\"")),
                Err(e) => Err(format!("unparseable value \"{s}\": {e}")),
            }
        }
        other => Err(format!("unexpected value JSON type: {other}")),
    }
}

#[derive(Deserialize)]
struct Envelope {
    response: ResponseBody,
}

#[derive(Deserialize)]
struct ResponseBody {
    #[serde(default)]
    data: Vec<RawRow>,
}

#[derive(Deserialize)]
struct RawRow {
    period: String,
    respondent: String,
    fueltype: String,
    #[serde(default)]
    value: serde_json::Value,
}

/// Accumulator keyed by region-hour. `BTreeMap` keeps output deterministic
/// (ordered by timestamp then respondent code) for auditable, diffable
/// results and stable tests.
type Key = (OffsetDateTime, &'static str); // (at, respondent code, for ordering)

struct Acc {
    region: Region,
    fuels: BTreeMap<usize, f64>, // FuelType::ALL index -> summed MWh
    notes: Vec<String>,
}

/// Parse an EIA `electricity/rto/fuel-type-data` JSON response into
/// normalized per-region-hour fuel mixes.
///
/// Output is ordered by `(at, region)` and, within an hour, by
/// [`FuelType::ALL`]. See the module docs for the normalization rules.
pub fn parse_fuel_type_data(json: &str) -> Result<Vec<FuelMixHour>> {
    let env: Envelope = serde_json::from_str(json)
        .map_err(|e| Error::Malformed(format!("EIA response parse: {e}")))?;

    let mut acc: BTreeMap<Key, Acc> = BTreeMap::new();

    for row in env.response.data {
        let region = region_from_respondent(&row.respondent).ok_or_else(|| {
            Error::Malformed(format!(
                "unexpected EIA respondent code '{}' (only the seven \
                 Phase-0 regions are queried)",
                row.respondent
            ))
        })?;
        let at = parse_eia_period(&row.period)?;

        let (fuel, unknown) = match classify_fuel(&row.fueltype) {
            FuelClass::Storage => continue, // excluded from generation mix
            FuelClass::Generation(ft) => (ft, false),
            FuelClass::Unknown => (FuelType::Unk, true),
        };

        let key: Key = (at, respondent_code(region));
        let entry = acc.entry(key).or_insert_with(|| Acc {
            region,
            fuels: BTreeMap::new(),
            notes: Vec::new(),
        });

        match parse_value(&row.value) {
            Ok(None) => continue, // not reported this hour; not a fabricated 0
            Ok(Some(v)) => {
                let idx = FuelType::ALL
                    .iter()
                    .position(|f| *f == fuel)
                    .unwrap_or(FuelType::ALL.len() - 1);
                *entry.fuels.entry(idx).or_insert(0.0) += v;
                if unknown {
                    let note = format!(
                        "unrecognized EIA fuel code '{}' mapped to UNK",
                        row.fueltype
                    );
                    if !entry.notes.contains(&note) {
                        entry.notes.push(note);
                    }
                }
            }
            Err(reason) => {
                let note = format!("skipped {} value at {}: {reason}", row.fueltype, row.period);
                if !entry.notes.contains(&note) {
                    entry.notes.push(note);
                }
            }
        }
    }

    let mut out: Vec<FuelMixHour> = acc
        .into_iter()
        .map(|((at, _), a)| {
            let mut generation_mwh: Vec<(FuelType, f64)> = a
                .fuels
                .into_iter()
                .map(|(idx, v)| (FuelType::ALL[idx], v))
                .collect();
            generation_mwh.sort_by_key(|(ft, _)| {
                FuelType::ALL
                    .iter()
                    .position(|f| f == ft)
                    .unwrap_or(usize::MAX)
            });
            FuelMixHour {
                at,
                region: a.region,
                generation_mwh,
                notes: a.notes,
            }
        })
        .collect();
    out.sort_by_key(|m| (m.at, m.region.as_code()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn respondent_round_trip() {
        for (code, region) in RESPONDENTS {
            assert_eq!(region_from_respondent(code), Some(*region));
            assert_eq!(respondent_code(*region), *code);
        }
        assert_eq!(region_from_respondent("ZZZZ"), None);
    }

    #[test]
    fn period_parsing() {
        assert_eq!(
            parse_eia_period("2026-05-12T00").unwrap(),
            datetime!(2026-05-12 00:00 UTC)
        );
        assert_eq!(
            parse_eia_period("2026-05-12T23").unwrap(),
            datetime!(2026-05-12 23:00 UTC)
        );
        assert!(parse_eia_period("2026-05-12").is_err()); // no hour
        assert!(parse_eia_period("2026-05-12T24").is_err()); // hour OOR
        assert!(parse_eia_period("notadate T00").is_err());
    }

    #[test]
    fn value_parsing_flavors() {
        use serde_json::json;
        assert_eq!(parse_value(&json!("1234")).unwrap(), Some(1234.0));
        assert_eq!(parse_value(&json!("-55")).unwrap(), Some(-55.0));
        assert_eq!(parse_value(&json!(" 12.5 ")).unwrap(), Some(12.5));
        assert_eq!(parse_value(&json!(987)).unwrap(), Some(987.0));
        assert_eq!(parse_value(&json!(null)).unwrap(), None);
        assert_eq!(parse_value(&json!("")).unwrap(), None);
        assert!(parse_value(&json!("abc")).is_err());
    }

    #[test]
    fn classify_storage_and_unknown() {
        assert!(matches!(classify_fuel("BAT"), FuelClass::Storage));
        assert!(matches!(classify_fuel("ps"), FuelClass::Storage));
        assert!(matches!(
            classify_fuel("NG"),
            FuelClass::Generation(FuelType::Ng)
        ));
        assert!(matches!(
            classify_fuel("GEO"),
            FuelClass::Generation(FuelType::Oth)
        ));
        assert!(matches!(classify_fuel("XYZ"), FuelClass::Unknown));
    }

    #[test]
    fn aggregates_geo_into_oth_and_excludes_storage() {
        let json = r#"{"response":{"data":[
          {"period":"2026-05-12T00","respondent":"CISO","fueltype":"OTH","value":"-557"},
          {"period":"2026-05-12T00","respondent":"CISO","fueltype":"GEO","value":"675"},
          {"period":"2026-05-12T00","respondent":"ERCO","fueltype":"BAT","value":"-55"},
          {"period":"2026-05-12T00","respondent":"ERCO","fueltype":"NG","value":"26481"}
        ]}}"#;
        let mix = parse_fuel_type_data(json).unwrap();
        assert_eq!(mix.len(), 2);

        let ciso = mix.iter().find(|m| m.region == Region::Caiso).unwrap();
        // GEO(675) + OTH(-557) = 118 under FuelType::Oth.
        assert_eq!(ciso.generation_mwh, vec![(FuelType::Oth, 118.0)]);

        let erco = mix.iter().find(|m| m.region == Region::Ercot).unwrap();
        // BAT excluded entirely; only NG remains.
        assert_eq!(erco.generation_mwh, vec![(FuelType::Ng, 26481.0)]);
        assert!(erco.notes.is_empty());
    }

    #[test]
    fn unknown_fuel_maps_to_unk_with_note() {
        let json = r#"{"response":{"data":[
          {"period":"2026-05-12T00","respondent":"PJM","fueltype":"FUSION","value":"42"}
        ]}}"#;
        let mix = parse_fuel_type_data(json).unwrap();
        assert_eq!(mix.len(), 1);
        assert_eq!(mix[0].generation_mwh, vec![(FuelType::Unk, 42.0)]);
        assert_eq!(mix[0].notes.len(), 1);
        assert!(mix[0].notes[0].contains("FUSION"));
    }

    #[test]
    fn unexpected_respondent_is_hard_error() {
        let json = r#"{"response":{"data":[
          {"period":"2026-05-12T00","respondent":"FPL","fueltype":"NG","value":"1"}
        ]}}"#;
        assert!(matches!(
            parse_fuel_type_data(json),
            Err(Error::Malformed(_))
        ));
    }

    #[test]
    fn null_value_skipped_unparseable_noted() {
        let json = r#"{"response":{"data":[
          {"period":"2026-05-12T00","respondent":"MISO","fueltype":"NG","value":null},
          {"period":"2026-05-12T00","respondent":"MISO","fueltype":"COL","value":"oops"}
        ]}}"#;
        let mix = parse_fuel_type_data(json).unwrap();
        assert_eq!(mix.len(), 1);
        assert!(mix[0].generation_mwh.is_empty()); // NG null skipped, COL unparseable skipped
        assert_eq!(mix[0].notes.len(), 1);
        assert!(mix[0].notes[0].contains("COL"));
    }
}
