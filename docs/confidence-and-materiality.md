# Confidence and materiality

> **Status:** living document. Update in the same change that changes
> confidence logic or the materiality threshold.

## Why this document exists

`nami`'s defensibility rests on labelling uncertainty plainly. Carbon
intensity derived from public data has real, irreducible uncertainty —
EIA-930's reporting lag, smaller BAs' noisier data, the gap between
average and marginal emissions, the limits of a historical-pattern
forecast. The tool's job is not to make that uncertainty go away. Its job
is to make it legible.

## Confidence

[`Confidence`](../crates/nami-core/src/confidence.rs) is a struct, not an
enum, because we want to carry both the qualitative label and the
underlying evidence:

```rust
pub struct Confidence {
    pub level: ConfidenceLevel,           // High | Medium | Low | VeryLow
    pub sample_count: usize,              // historical samples used
    pub interval: Option<ConfidenceInterval>, // optional gCO₂/kWh band
    pub notes: Vec<String>,               // explanatory free text
}
```

A reviewer reading a `RunReport` should be able to see *why* a particular
confidence level was assigned, not just that it was.

### Level assignment

Implemented in `nami_core::Confidence::assess`. Three axes are graded
**independently**, and the **most conservative** (lowest) of the three is
the assigned level. This replaces an earlier draft table whose mixed
AND/OR wording was ambiguous; the worst-of-three rule is precise and
fails safe (any one weak axis pulls the result down).

**Axis 1 — sample count** (number of matching historical observations;
the forecast matches region/hour/day/month, so roughly one per week):

| Samples | Level |
|---|---|
| ≥ 6 | `High` |
| 3–5 | `Medium` |
| 1–2 | `Low` |
| 0 | `VeryLow` |

**Axis 2 — relative interval width** `r = std_dev / mean` (the 1σ
half-width as a fraction of the mean):

| `r` | Level |
|---|---|
| `r < 0.10` | `High` |
| `0.10 ≤ r < 0.25` | `Medium` |
| `0.25 ≤ r ≤ 0.40` | `Low` |
| `r > 0.40` | `VeryLow` |

Not computable — fewer than 2 samples, non-positive mean, or any
non-finite input — grades `VeryLow` and yields no interval. (Consequence:
a single sample is always `VeryLow`, because one sample gives no
defensible variance estimate.)

**Axis 3 — freshness cap** (see
[`methodology.md`](methodology.md#data-freshness-states)):

| Freshness | Cap |
|---|---|
| `FreshObserved` | `High` |
| `StaleObserved` | `Medium` |
| `HistoricalCacheOnly` | `Low` |
| `StaticFallbackOnly` | `VeryLow` |
| `NoUsableData` | `VeryLow` |

The interval (when computable) is the 1σ band
`[max(0, mean − std_dev), mean + std_dev]` in gCO₂/kWh. Every axis
appends a note to the `Confidence` so a report reviewer can see exactly
why the level was assigned.

The numeric bands are still subject to empirical tuning once real data
is in the system, but the **worst-of-three combination rule is fixed**.

## Materiality

Even with `High` confidence, a tiny estimated improvement is not a useful
recommendation. The scheduler honours a materiality threshold:

```
default: 5% estimated improvement of selected window over run-now
```

Below the threshold, the scheduler does not produce a `StartAt`
recommendation. Instead it returns:

- `StartImmediately { reason: RunNowAlreadyCleanest, .. }` if run-now is
  competitive, or
- `Refuse { reason: CandidateWindowsBelowMaterialityThreshold }` if the
  user asked for a recommendation and no window beats the threshold.

### Why 5%?

Two reasons:

1. **Forecast variance often exceeds 5% within an hour.** Recommending a
   window because its mean was 2% lower than run-now would be inside the
   noise floor of the forecast itself.
2. **Average ≠ marginal.** Phase 0 numbers are estimated average
   intensity. Sub-5% differences in average intensity rarely correspond
   to real-world marginal-emissions improvements, even when they're
   accurately measured.

The threshold is configurable (planned) but defaults conservatively. The
guidance is: when in doubt, do not recommend.

## Data freshness

The freshness state of the underlying data is recorded on every
`RunReport`:

| State | What it means |
|---|---|
| `FreshObserved` | Live EIA data within expected lag |
| `StaleObserved { lag }` | EIA data older than expected |
| `HistoricalCacheOnly { newest_sample_at }` | No live data |
| `StaticFallbackOnly` | Only annual-mean table available |
| `NoUsableData` | Nothing usable |

Confidence caps follow freshness — see
[`methodology.md`](methodology.md#data-freshness-states).

## What this means in practice

`nami` will frequently say "no materially cleaner window" or "refuse" —
and that is the right behaviour. The tool's contract with the user is
not "I will always find you a recommendation." It is "the recommendations
I do make are honest about what I know."
