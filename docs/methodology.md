# Methodology

> **Status:** living document. Every number `nami` produces should be
> traceable to a section here. Update this file in the same change that
> changes the math.

## Methodology versions

Every estimate carries a methodology label so future readers can audit
which version of the code produced it.

| Label | Status | Notes |
|---|---|---|
| `eia-930-v1+egrid-2024-subregion` | not yet implemented | Phase 0 target |
| `historical-pattern-mean-8w-hour-dow-month-v1` | not yet implemented | Phase 0 forecast model |
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

### Handling `Other` and `Unknown` fuel categories

After normalization, `OTH` (now including geothermal) and `UNK` still
need an emission factor.

**Planned approach (not yet implemented):** assign `OTH` and `UNK` the
eGRID non-baseload composite factor for the region, with a documented
note in every report that this assumption was made. Sensitivity analysis
will accompany the implementation.

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
- Variance / interval (1σ band over the matching samples).
- Confidence label, derived from sample count and variance.
- Methodology label (`historical-pattern-mean-8w-hour-dow-month-v1`).

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
