//! Live EIA-930 API tests (Phase 0 item 14).
//!
//! Gated behind the `live-eia` feature so the default test run never
//! touches the network:
//!
//! ```sh
//! EIA_API_KEY=… cargo test -p nami-carbon-eia --features live-eia
//! ```
//!
//! These exercise the real `fetch_region_json` HTTP/pagination path and
//! the end-to-end `refresh_region_cache` orchestration — the halves the
//! fixture-based unit tests deliberately cannot cover. Assertions are
//! shape/invariant checks, not value checks: the live grid changes hourly
//! and these are sanity checks, not a correctness proof (CLAUDE.md).
//!
//! If the feature is on but `EIA_API_KEY` is unset, each test logs and
//! returns rather than failing — opt-in network tests should not break a
//! CI that has no secret.
#![cfg(feature = "live-eia")]

use std::path::PathBuf;

use nami_carbon_eia::{
    HistoricalCache, fetch_region_json, parse_fuel_type_data, refresh_region_cache,
};
use nami_core::{CarbonIntensity, CarbonObservation, Region};
use time::{Duration, OffsetDateTime, Time};

/// Read the key, or skip the test (returns `None`) with a clear note.
fn api_key_or_skip(test: &str) -> Option<String> {
    match std::env::var("EIA_API_KEY") {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => {
            eprintln!("skipping {test}: EIA_API_KEY not set");
            None
        }
    }
}

fn truncate_to_hour(dt: OffsetDateTime) -> OffsetDateTime {
    dt.replace_time(Time::from_hms(dt.hour(), 0, 0).expect("valid hour"))
}

/// A recent, settled window: end 6h ago (well past EIA's ~1–2h reporting
/// lag), 24h long. Small on purpose — these hit the real API.
fn recent_window() -> (OffsetDateTime, OffsetDateTime) {
    let end = truncate_to_hour(OffsetDateTime::now_utc() - Duration::hours(6));
    (end - Duration::hours(24), end)
}

/// Path to the committed eGRID table (workspace `data/`), resolved from
/// the crate manifest dir since integration tests run with CWD = crate.
fn egrid_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/egrid-factors.toml")
}

fn unique_tmp(stem: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "nami-live-{stem}-{}-{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    ))
}

#[tokio::test]
async fn fetch_one_region_returns_in_window_data() {
    let Some(key) = api_key_or_skip("fetch_one_region_returns_in_window_data") else {
        return;
    };
    let (start, end) = recent_window();
    let client = reqwest::Client::new();

    let json = fetch_region_json(&client, Region::Caiso, start, end, &key)
        .await
        .expect("live fetch should succeed");
    let mix = parse_fuel_type_data(&json).expect("live response should parse");

    assert!(
        !mix.is_empty(),
        "expected some CAISO hours in a recent 24h window"
    );
    // The respondent facet must mean only CAISO comes back.
    assert!(
        mix.iter().all(|m| m.region == Region::Caiso),
        "facet should restrict to CAISO only"
    );
    // Every hour falls within the requested window and is strictly
    // ascending (the parser orders by (at, region); one region ⇒ unique).
    for m in &mix {
        assert!(
            m.at >= start && m.at <= end,
            "hour {} outside [{start}, {end}]",
            m.at
        );
    }
    for w in mix.windows(2) {
        assert!(w[1].at > w[0].at, "hours not strictly ascending");
    }
    // Each parsed hour has at least one fuel with generation.
    assert!(
        mix.iter().all(|m| !m.generation_mwh.is_empty()),
        "a parsed hour had no generation rows"
    );
}

#[tokio::test]
async fn refresh_writes_cache_and_preserves_other_regions() {
    if api_key_or_skip("refresh_writes_cache_and_preserves_other_regions").is_none() {
        return;
    }
    let egrid = egrid_path();
    assert!(
        egrid.exists(),
        "committed eGRID table missing at {}",
        egrid.display()
    );

    let dir = unique_tmp("refresh");
    std::fs::create_dir_all(&dir).unwrap();
    let cache_path = dir.join("historical-cache.json");

    // Pre-seed an unrelated region; the refresh must not clobber it.
    let seeded_at = time::macros::datetime!(2026-01-01 00:00 UTC);
    let mut seed = HistoricalCache::new(OffsetDateTime::now_utc(), "seed-v1");
    seed.set_region(
        Region::Ercot,
        vec![CarbonObservation {
            at: seeded_at,
            intensity: CarbonIntensity::new(400.0).unwrap(),
            methodology: "seed-v1".into(),
        }],
    );
    seed.save(&cache_path).expect("seed cache write");

    let now = OffsetDateTime::now_utc();
    let summary = refresh_region_cache(Region::Caiso, 1, &cache_path, &egrid, now)
        .await
        .expect("live refresh should succeed");

    assert_eq!(summary.region, Region::Caiso);
    assert!(
        summary.observations_written > 0,
        "expected CAISO observations from a 1-week refresh"
    );
    assert_eq!(
        summary.observations_written + summary.hours_skipped,
        summary.hours_parsed,
        "every parsed hour is either written or counted as a skipped gap"
    );

    // Reloading also re-validates (strict ascending, unique regions).
    let reloaded = HistoricalCache::load(&cache_path).expect("refreshed cache must reload");
    assert!(
        !reloaded.observations(Region::Caiso).is_empty(),
        "CAISO should be populated after refresh"
    );
    let ercot = reloaded.observations(Region::Ercot);
    assert_eq!(
        ercot.len(),
        1,
        "the pre-seeded ERCOT history must be preserved"
    );
    assert_eq!(ercot[0].at, seeded_at);

    let _ = std::fs::remove_dir_all(&dir);
}
