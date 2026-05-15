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

### Level assignment (planned)

The forecast layer will assign levels roughly as follows (subject to
empirical tuning as we collect data):

| Level | Trigger |
|---|---|
| `High` | Fresh observed data + ≥6 weeks of matching historical samples + interval width < ±10% of mean |
| `Medium` | Stale-observed or 3–6 weeks of samples or interval width < ±25% of mean |
| `Low` | Historical-cache-only or 1–3 weeks of samples or interval width up to ±40% |
| `VeryLow` | Static fallback only, sparse samples, or interval width >±40% |

These bands are placeholders and will be revisited once real data is in
the system.

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
