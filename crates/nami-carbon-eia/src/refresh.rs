//! Historical-cache refresh (Phase 0 item 13).
//!
//! Fetches EIA-930 `electricity/rto/fuel-type-data` for **one** region
//! over a recent window, derives estimated average carbon intensity per
//! region-hour through the committed eGRID factor table, and merges the
//! result into the local [`HistoricalCache`] — other regions already in
//! the cache are left untouched.
//!
//! Split into a networked half and a pure half so the data-shaping logic
//! is unit-testable without the network (the live fetch is exercised by
//! the `live-eia` tests, item 14):
//!
//! - [`fetch_region_json`] — paginated HTTP, returns a combined
//!   `{"response":{"data":[…]}}` document;
//! - [`process_response`] — pure: combined JSON + factors → observations
//!   plus a [`RefreshSummary`]'s counters;
//! - [`refresh_region_cache`] — orchestrates load-factors → fetch →
//!   derive → merge → atomic save.
//!
//! Honesty notes (CLAUDE.md): an hour with no positive generation is a
//! *gap*, counted and skipped, never a fabricated zero; a missing
//! `EIA_API_KEY` is a hard error, not a silent fallback; an existing but
//! unusable cache is refused rather than clobbered.

use std::path::Path;

use serde::Deserialize;
use time::{Duration, OffsetDateTime};

use nami_core::{CarbonObservation, Region};

use crate::api::{parse_fuel_type_data, respondent_code};
use crate::cache::HistoricalCache;
use crate::derive::{DERIVATION_METHODOLOGY, derive_intensity};
use crate::egrid::EgridFactors;
use crate::error::{Error, Result};

/// EIA v2 fuel-type-data endpoint (hourly generation by fuel type).
const EIA_ENDPOINT: &str = "https://api.eia.gov/v2/electricity/rto/fuel-type-data/data/";

/// Rows per page. EIA v2 caps a single response at 5000 rows.
const PAGE_LEN: usize = 5000;

/// Hard cap on pagination iterations, so a misbehaving `total` cannot
/// spin forever. 8 weeks × 7 regions × 24 h × ~12 fuels ≪ this.
const MAX_PAGES: usize = 64;

/// How many characters of an error body to keep (avoid logging the API
/// key, which the `request` echo can contain — we only ever read
/// `response`, but truncate defensively).
const ERR_BODY_CAP: usize = 512;

/// Outcome of a refresh, for the CLI summary and tests.
#[derive(Debug, Clone, PartialEq)]
pub struct RefreshSummary {
    /// Region refreshed.
    pub region: Region,
    /// Inclusive UTC start of the fetched window (top of hour).
    pub start: OffsetDateTime,
    /// Inclusive UTC end of the fetched window (top of hour).
    pub end: OffsetDateTime,
    /// Region-hours parsed from EIA after normalization.
    pub hours_parsed: usize,
    /// Observations actually written to the cache.
    pub observations_written: usize,
    /// Hours skipped because no positive generation could be derived
    /// (treated as gaps, not zeros).
    pub hours_skipped: usize,
    /// De-duplicated, capped provenance/derivation warnings.
    pub warnings: Vec<String>,
}

/// Round `dt` down to the top of its UTC hour.
fn truncate_to_hour(dt: OffsetDateTime) -> Result<OffsetDateTime> {
    dt.replace_minute(0)
        .and_then(|d| d.replace_second(0))
        .and_then(|d| d.replace_nanosecond(0))
        .map_err(|e| Error::Malformed(format!("could not truncate {dt} to the hour: {e}")))
}

/// Format an instant as EIA's `YYYY-MM-DDTHH` (UTC) period string.
fn eia_period(dt: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}",
        dt.year(),
        u8::from(dt.month()),
        dt.day(),
        dt.hour()
    )
}

/// Build the EIA query parameters for one page. Pure and order-stable so
/// it can be asserted in tests. `api_key` is included last.
fn build_query(
    respondent: &str,
    start: OffsetDateTime,
    end: OffsetDateTime,
    offset: usize,
    length: usize,
    api_key: &str,
) -> Vec<(String, String)> {
    vec![
        ("frequency".into(), "hourly".into()),
        ("data[0]".into(), "value".into()),
        ("facets[respondent][]".into(), respondent.into()),
        ("start".into(), eia_period(start)),
        ("end".into(), eia_period(end)),
        ("sort[0][column]".into(), "period".into()),
        ("sort[0][direction]".into(), "asc".into()),
        ("offset".into(), offset.to_string()),
        ("length".into(), length.to_string()),
        ("api_key".into(), api_key.into()),
    ]
}

#[derive(Deserialize)]
struct PageEnvelope {
    response: PageBody,
}

#[derive(Deserialize)]
struct PageBody {
    #[serde(default)]
    data: Vec<serde_json::Value>,
    /// EIA reports `total` as a JSON string in this API version; accept a
    /// number too, defensively.
    #[serde(default)]
    total: Option<serde_json::Value>,
}

/// Best-effort parse of EIA's `total` (string or number) into a count.
fn parse_total(v: &Option<serde_json::Value>) -> Option<usize> {
    match v {
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        Some(serde_json::Value::Number(n)) => n.as_u64().map(|x| x as usize),
        _ => None,
    }
}

/// Fetch all pages for `region` over `[start, end]` and return a single
/// combined `{"response":{"data":[…]}}` document.
///
/// Pages are concatenated *before* parsing so a page boundary that falls
/// mid-hour cannot split one region-hour into two partial mixes.
pub async fn fetch_region_json(
    client: &reqwest::Client,
    region: Region,
    start: OffsetDateTime,
    end: OffsetDateTime,
    api_key: &str,
) -> Result<String> {
    let respondent = respondent_code(region);
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut offset = 0usize;
    let mut completed = false;

    for _ in 0..MAX_PAGES {
        let query = build_query(respondent, start, end, offset, PAGE_LEN, api_key);
        let resp = client.get(EIA_ENDPOINT).query(&query).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let mut body = resp.text().await.unwrap_or_default();
            body.truncate(ERR_BODY_CAP);
            return Err(Error::Api {
                status: status.as_u16(),
                body,
            });
        }
        let text = resp.text().await?;
        let page: PageEnvelope = serde_json::from_str(&text)
            .map_err(|e| Error::Malformed(format!("EIA page parse: {e}")))?;

        let got = page.response.data.len();
        rows.extend(page.response.data);
        offset += got;

        // Terminate only on a definitive signal: an empty page, or a
        // known `total` we have reached. A short page is NOT treated as
        // terminal on its own — EIA can return fewer than `length` rows
        // on a non-final page, and when `total` is absent that would
        // silently truncate history. When `total` is unknown we page
        // until an empty response (MAX_PAGES still bounds the loop).
        if got == 0 || parse_total(&page.response.total).is_some_and(|t| offset >= t) {
            completed = true;
            break;
        }
    }

    if !completed {
        // Hit MAX_PAGES without a definitive end: refuse rather than
        // proceed on possibly-truncated history (CLAUDE.md: do not
        // silently estimate on incomplete data).
        return Err(Error::Malformed(format!(
            "EIA pagination exceeded {MAX_PAGES} pages without reaching the \
             reported total; refusing to proceed on possibly-truncated data"
        )));
    }

    let combined = serde_json::json!({ "response": { "data": rows } });
    serde_json::to_string(&combined).map_err(|e| Error::Malformed(format!("combine pages: {e}")))
}

/// Pure core: combined EIA JSON + factor table → derived observations
/// (ascending, unique by hour) plus the counters/warnings for a
/// [`RefreshSummary`]. No I/O; CLAUDE.md keeps derivation synchronous.
///
/// Returns `(observations, hours_parsed, hours_skipped, warnings)`.
pub fn process_response(
    json: &str,
    region: Region,
    factors: &EgridFactors,
) -> Result<(Vec<CarbonObservation>, usize, usize, Vec<String>)> {
    let mix = parse_fuel_type_data(json)?;
    let hours_parsed = mix.len();

    let mut observations = Vec::with_capacity(hours_parsed);
    let mut warnings: Vec<String> = Vec::new();
    let mut skipped = 0usize;

    for hour in &mix {
        if hour.region != region {
            // The respondent facet should make this impossible; if EIA
            // ever ignores it, refuse rather than silently mix regions.
            return Err(Error::Malformed(format!(
                "EIA returned {} data while refreshing {}",
                hour.region.as_code(),
                region.as_code()
            )));
        }
        match derive_intensity(hour, factors) {
            Ok(d) => {
                for w in d.warnings {
                    if !warnings.contains(&w) {
                        warnings.push(w);
                    }
                }
                observations.push(d.observation);
            }
            Err(Error::DerivationFailed(_)) => {
                skipped += 1;
            }
            Err(e) => return Err(e),
        }
    }

    if observations.is_empty() {
        warnings.insert(
            0,
            format!(
                "no usable observations derived for {} in the requested window \
                 (parsed {hours_parsed} hours, {skipped} had no positive generation)",
                region.as_code()
            ),
        );
    }
    cap_warnings(&mut warnings);
    Ok((observations, hours_parsed, skipped, warnings))
}

/// Keep warning output bounded and auditable.
fn cap_warnings(warnings: &mut Vec<String>) {
    const MAX: usize = 50;
    if warnings.len() > MAX {
        let extra = warnings.len() - MAX;
        warnings.truncate(MAX);
        warnings.push(format!("(+{extra} more warnings suppressed)"));
    }
}

/// Refresh one region's slice of the historical cache end-to-end.
///
/// `weeks` of hourly history ending at `now` (truncated to the hour) are
/// fetched. An existing cache is loaded so other regions are preserved; a
/// *missing* cache is created fresh, but an existing-but-**unusable**
/// cache is refused (we do not overwrite a file we cannot understand).
pub async fn refresh_region_cache(
    region: Region,
    weeks: u32,
    cache_path: &Path,
    egrid_path: &Path,
    now: OffsetDateTime,
) -> Result<RefreshSummary> {
    let factors = EgridFactors::load(egrid_path)?;
    let api_key = std::env::var("EIA_API_KEY").map_err(|_| Error::MissingApiKey)?;

    let end = truncate_to_hour(now)?;
    let start = end - Duration::weeks(i64::from(weeks));

    let client = reqwest::Client::builder()
        .user_agent(concat!("nami/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let json = fetch_region_json(&client, region, start, end, &api_key).await?;
    let (observations, hours_parsed, hours_skipped, warnings) =
        process_response(&json, region, &factors)?;

    let mut cache = match HistoricalCache::load(cache_path) {
        Ok(c) => c,
        Err(Error::CacheMissing(_)) => HistoricalCache::new(now, DERIVATION_METHODOLOGY),
        Err(e) => return Err(e), // refuse to clobber an unreadable cache
    };
    cache.generated_at = now;
    cache.methodology_version = DERIVATION_METHODOLOGY.to_string();
    let observations_written = observations.len();
    cache.set_region(region, observations);
    cache.save(cache_path)?;

    Ok(RefreshSummary {
        region,
        start,
        end,
        hours_parsed,
        observations_written,
        hours_skipped,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nami_core::FuelType;
    use time::macros::datetime;

    const FIXTURE: &str = include_str!("../tests/fixtures/eia-fuel-type-sample.json");

    /// Synthetic full factor table (all regions × fuels), distinct rates.
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

    #[test]
    fn eia_period_formats_utc_hour() {
        assert_eq!(eia_period(datetime!(2026-05-12 00:00 UTC)), "2026-05-12T00");
        assert_eq!(eia_period(datetime!(2026-01-09 23:00 UTC)), "2026-01-09T23");
    }

    #[test]
    fn truncate_drops_sub_hour() {
        let t = truncate_to_hour(datetime!(2026-05-12 03:47:12.9 UTC)).unwrap();
        assert_eq!(t, datetime!(2026-05-12 03:00 UTC));
    }

    #[test]
    fn build_query_has_key_facet_and_window() {
        let q = build_query(
            "CISO",
            datetime!(2026-05-12 00:00 UTC),
            datetime!(2026-05-19 00:00 UTC),
            5000,
            5000,
            "SECRET",
        );
        let get = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
        assert_eq!(get("facets[respondent][]").as_deref(), Some("CISO"));
        assert_eq!(get("frequency").as_deref(), Some("hourly"));
        assert_eq!(get("start").as_deref(), Some("2026-05-12T00"));
        assert_eq!(get("end").as_deref(), Some("2026-05-19T00"));
        assert_eq!(get("offset").as_deref(), Some("5000"));
        assert_eq!(get("api_key").as_deref(), Some("SECRET"));
    }

    #[test]
    fn parse_total_accepts_string_or_number() {
        assert_eq!(parse_total(&Some(serde_json::json!("1344"))), Some(1344));
        assert_eq!(parse_total(&Some(serde_json::json!(42))), Some(42));
        assert_eq!(parse_total(&None), None);
        assert_eq!(parse_total(&Some(serde_json::json!("x"))), None);
    }

    #[test]
    fn page_envelope_reads_data_and_total() {
        let j = r#"{"response":{"data":[{"a":1}],"total":"7"}}"#;
        let p: PageEnvelope = serde_json::from_str(j).unwrap();
        assert_eq!(p.response.data.len(), 1);
        assert_eq!(parse_total(&p.response.total), Some(7));
    }

    /// The committed fixture is a multi-region capture; a real refresh
    /// uses the respondent facet, so production only ever sees one
    /// region. Re-filter the fixture to CISO to mirror that.
    fn fixture_single_region(respondent: &str) -> String {
        let mut v: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let data = v["response"]["data"].as_array().unwrap();
        let kept: Vec<_> = data
            .iter()
            .filter(|r| r["respondent"] == respondent)
            .cloned()
            .collect();
        v["response"]["data"] = serde_json::Value::Array(kept);
        v.to_string()
    }

    #[test]
    fn process_fixture_yields_ascending_unique_observations() {
        let f = factors();
        let ciso = fixture_single_region("CISO");
        let (obs, parsed, skipped, _warn) = process_response(&ciso, Region::Caiso, &f).unwrap();

        assert!(parsed > 0, "fixture should parse some hours");
        assert_eq!(obs.len() + skipped, parsed);
        for o in &obs {
            assert_eq!(o.methodology, DERIVATION_METHODOLOGY);
        }
        for pair in obs.windows(2) {
            assert!(pair[1].at > pair[0].at, "observations must be ascending");
        }
        // Must round-trip through the cache's strict validation.
        let mut c = HistoricalCache::new(datetime!(2026-05-18 00:00 UTC), DERIVATION_METHODOLOGY);
        c.set_region(Region::Caiso, obs);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn process_rejects_region_mismatch() {
        let f = factors();
        // Fixture is CISO/CAISO data; asking for ERCOT must refuse.
        let err = process_response(FIXTURE, Region::Ercot, &f);
        assert!(matches!(err, Err(Error::Malformed(_))));
    }

    #[test]
    fn cap_warnings_truncates() {
        let mut w: Vec<String> = (0..60).map(|i| i.to_string()).collect();
        cap_warnings(&mut w);
        assert_eq!(w.len(), 51);
        assert!(w.last().unwrap().contains("more warnings suppressed"));
    }
}
