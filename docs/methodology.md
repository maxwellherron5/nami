# Methodology

> **Status:** living document. Every number `nami` produces should be
> traceable to a section here. Update this file in the same change that
> changes the math.

## Methodology versions

Every estimate carries a methodology label so future readers can audit
which version of the code produced it.

| Label | Status | Notes |
|---|---|---|
| `egrid-2023-ba` | implemented | eGRID emission-factor table (item 7) |
| `eia-930-v1+egrid-2023-ba` | implemented | carbon-intensity derivation (item 8) |
| `historical-pattern-mean-{N}w-hour-dow-month-v1` | implemented | forecast model (item 9), default N=8 |
| `static-fallback-annual-v1` | implemented | the static-table provider |

## Carbon intensity derivation

Given hourly EIA-930 generation by fuel type and eGRID emission factors:

```
intensity[region, t]
    = Σ_fuel (generation_mwh[fuel, region, t] × emission_factor[fuel, region])
    / Σ_fuel generation_mwh[fuel, region, t]
```

Units: MWh × gCO₂/kWh × (1000 kWh / MWh) / MWh = gCO₂/kWh. The kWh↔MWh
conversion lives in `CarbonIntensity::from_lbs_per_mwh` and
`EmissionFactor::from_lbs_per_mwh`.

For internal consistency, the denominator is the *sum of fuel-type
generation*, not EIA's reported "total net generation" field — these
don't always match, and using the sum makes the numerator and
denominator come from the same column.

Because `emission_factor` is already gCO₂/kWh and generation is MWh, the
×1000 (kWh/MWh) cancels between numerator and denominator: the result is
a generation-weighted mean of per-fuel factors, directly in gCO₂/kWh.
No explicit MWh↔kWh conversion is applied in the weighted mean itself
(the lb/MWh→gCO₂/kWh conversion already happened at the eGRID load
boundary).

### Negative generation and empty hours (item 8 — implemented)

Implemented in `nami_carbon_eia::derive_intensity`, producing a
`CarbonObservation` labelled `eia-930-v1+egrid-2023-ba`.

- **Negative per-fuel generation is clamped to 0**, with a note listing
  each clamped fuel and its raw value. Net-negative net generation is a
  small accounting artifact; counting it would yield negative
  "emissions" and could drive the denominator non-positive. Clamping a
  fuel to 0 is equivalent to excluding it from both sums.
- **An hour with no positive generation after clamping is refused**
  (`Error::DerivationFailed`), never zeroed — no defensible number
  exists, so the caller treats the hour as a gap (consistent with
  "refuse to estimate").
- Item-6 normalization provenance (`FuelMixHour::notes`, e.g.
  unknown-fuel→`UNK` mappings) is **carried forward** into the derived
  result's `warnings`, so no assumption is hidden downstream.

### Fuel-type normalization (implemented, item 6)

The live EIA-930 API returns more granular codes than the 9-category
schema. Normalization (see `docs/eia-api-notes.md` for the full table):

- `GEO` (geothermal) is folded into `OTH`; same-hour `GEO`+`OTH` MWh are
  summed.
- `BAT` (battery) and `PS` (pumped storage) are **excluded** from the
  generation mix: storage is not primary generation, has no intrinsic
  emission factor, and can be negative. It therefore never enters the
  `Σ generation` numerator or denominator.
- Unrecognized codes become `UNK` with a surfaced note.

### Emission factors (eGRID, item 7 — implemented)

Factors come from **EPA eGRID, balancing-authority level**, pinned to a
specific release and committed as `data/egrid-factors.toml`.

- **Pinned release:** eGRID2023 (rev2, 2025-06),
  `https://www.epa.gov/system/files/documents/2025-06/egrid2023_data_rev2.xlsx`,
  sheet `BA23`. Bumping the pin is a deliberate reviewed change.
- **BA-level, not subregion.** Our `Region` *is* a balancing authority,
  so eGRID's `BA` sheet maps 1:1 — no BA→subregion approximation.
- **Per-fuel column mapping:**
  `COL = BACCO2RT`, `NG = BAGCO2RT`, `OIL = BAOCO2RT`;
  `NUC, WAT, SUN, WND = 0` (non-combustion: no direct CO₂);
  `OTH, UNK = BANBCO2` (eGRID's non-baseload composite output emission
  rate — the documented stand-in for the heterogeneous other/unknown
  bucket, which after item-6 normalization also absorbs geothermal).
  A missing per-fuel cell falls back to `BANBCO2` with a recorded note.
- **Units & boundary.** The TOML stores raw eGRID **lb CO₂/MWh** exactly
  as published (directly checkable against the workbook). Conversion to
  internal gCO₂/kWh (`× 453.592 / 1000`) happens once, at the load
  boundary in `EgridFactors`.
- **Acquisition.** The committed TOML is produced by the `refresh-egrid`
  maintainer tool (gated behind the `egrid-refresh` feature; pulls the
  `.xlsx` reader only then). The shipped `nami` binary reads only the
  committed TOML — never the network or Excel — preserving the static,
  offline, reproducible, auditable design.

`OTH`/`UNK` use the non-baseload composite as described above; this is
the documented assumption that was previously marked "planned".

### Validation

Planned validation:

- Compare derived hourly intensity against EIA's published per-BA CO₂
  estimates (available in per-BA Excel exports starting July 2018).
  Target agreement within ±10%.
- Document any systematic differences and their probable cause.

These checks are sanity checks, not proof of exact correctness.

## Forecast model

Phase 0 forecast is a historical-pattern estimator:

```
forecast[region, target_t]
    = mean of historical observations matching
        (region, hour_of_day(target_t), day_of_week(target_t), month(target_t))
      over the most recent N weeks (default N=8)
```

For each forecast point, the model emits:

- Mean.
- Sample count.
- Interval: the 1σ band using the **sample** standard deviation (n−1
  denominator; `0` when n < 2, which `Confidence::assess` treats as
  non-computable).
- Confidence via `Confidence::assess` (sample count + relative interval
  width + freshness cap).
- Methodology label `historical-pattern-mean-{N}w-hour-dow-month-v1`
  with the actual `N` embedded for traceability (default N=8).

Implemented in `nami_carbon_eia::historical_pattern_forecast` (item 9).
Stances baked in (consequences of already-documented policy):

- **A pure-cache forecast is inherently `HistoricalCacheOnly`.** It
  never consults live observed data, so its confidence is capped at
  `Low` by the freshness rule — this is set by the model, not the
  caller, so the honesty cannot be bypassed.
- **Sample window is `(now − N weeks, now]`**, anchored on a
  caller-supplied `now` (deterministic/testable). Matching is by
  **exact** day-of-week and month (not weekday/weekend or season).
- **Hours with zero matching samples are omitted**, never invented; the
  result has fewer points than the horizon has hours and the scheduler
  treats the missing hours as gaps (consistent with "refuse to
  estimate"). A region with no cached history yields no forecast.
- Horizon start is floored to the hour; points are hour-aligned UTC and
  ascending.

The model is intentionally simple. More sophisticated approaches (e.g.,
seasonal-trend decomposition, state-space models) are deferred to
Phase 2+; they would not be useful until the Phase 0 plumbing is solid.

This is **not** a direct EIA forecast. EIA-930 publishes a day-ahead
*demand* forecast, but no carbon-intensity forecast and no future fuel-
mix forecast. Any forward-looking number `nami` produces is its own
model layered on observed history.

## Candidate window generation

Before any window can be scored, the scheduler enumerates the *candidate*
start times. Implemented in `nami-scheduler::candidate_windows`. The
Phase 0 rules:

- **Hour-aligned starts.** Candidate starts are snapped to UTC hour
  boundaries. EIA-930 is hourly; offering sub-hour start precision would
  imply resolution the data cannot support. The first candidate is the
  earliest whole hour `>= now` (i.e. `now` itself only when `now` is
  exactly on an hour boundary).
- **Deadline is inclusive.** A candidate is kept iff
  `start + D <= deadline`. `JobSpec` defines the deadline as the latest
  moment the job may *finish*, so finishing exactly at the deadline is
  permitted.
- **"Run now" is not in this set.** Running immediately (`start == now`,
  possibly mid-hour) is a separate baseline the scheduler always
  evaluates; it is intentionally excluded from the deferred candidate
  enumeration so the two are never conflated in a report.
- **Empty is not an error.** Zero candidates (job too long for the
  remaining time, non-positive duration, deadline already passed) is a
  normal outcome; the scheduler decides what it means (run now / refuse).

## Candidate window scoring

For a job of estimated duration `D` and candidate start time `s`:

```
window_intensity[s] = duration-weighted mean of forecast intensity
                      across hourly buckets overlapped by [s, s + D)
```

If `D < 1h`, the result is the intensity of the containing hour, labelled
as hourly-resolution.

## Materiality threshold

A lower-carbon recommendation is offered only if:

```
(run_now_intensity - selected_window_intensity) / run_now_intensity ≥ T
```

with default `T = 0.05` (5%). Below the threshold, the scheduler returns
`StartImmediately` or `Refuse(CandidateWindowsBelowMaterialityThreshold)`
depending on context, and reports surface the reason plainly.

The threshold is intentionally conservative because:

- Estimated average intensity is not marginal emissions; we should not
  imply precision the model cannot support.
- Forecast variance often exceeds 5% on a given hour, so smaller
  differences are within forecast noise.
- Users are better served by an honest "no meaningfully cleaner window"
  than by a confident-looking recommendation that's within noise.

## Data freshness states

| State | Meaning | Confidence implication |
|---|---|---|
| `FreshObserved` | Live EIA data within expected lag | Up to `High` |
| `StaleObserved { lag }` | EIA data, but older than expected | `Medium` cap |
| `HistoricalCacheOnly { newest_sample_at }` | No live data; only local cache | `Low` cap |
| `StaticFallbackOnly` | Only the annual-mean table | `VeryLow`, no recommendation |
| `NoUsableData` | Nothing | Refuse |

## Average vs marginal

Phase 0 `nami` produces **estimated average** carbon intensity. CLI output
and reports must never refer to these numbers as marginal emissions. If a
future Phase 2+ provider supplies marginal data, it will be modelled
explicitly at the provider boundary and surfaced with distinct labelling.
