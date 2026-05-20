# Use cases

> **Status:** living document. The point of this file is to make `nami`'s
> applicability *concrete* — what to actually use it for, what to expect
> in real output, and when it is the wrong tool. Methodology lives in
> `docs/methodology.md`; this document is about *fit*.

## When `nami` is useful

Two conditions both need to hold for `nami` to recommend a *deferred*
window over running now:

1. **The workload has real flexibility.** It has a finish-by deadline,
   not a start-now expectation. No human is waiting for it mid-run.
2. **The current hour is not already among the cleanest within that
   deadline.** Otherwise running now *is* the right answer — and `nami`
   will say so plainly rather than manufacture a deferral.

When (1) is true but (2) is not (you happen to be inside the daily solar
peak, say), `nami` honestly returns "run immediately — no materially
cleaner window before the deadline." That is the correct outcome, not
a failure mode. The audit `RunReport` still records the decision and
the comparison numbers; the decision is just "now."

## Workload patterns that fit

### 1. Nightly CI / test suite

A nightly cross-repo test run must be green by start of day. The
overnight window typically spans the dirtiest grid hours (low solar,
gas-heavy ramp), but the *cleanest* hour inside it often isn't midnight.

```sh
# Region resolved from NAMI_REGION or the nami config file.
nami preview --within 8h --duration 90m -- make integration-test
nami run     --within 8h --duration 90m -- make integration-test
```

> The `--within` form is on the roadmap (Phase A); today, use
> `--deadline <RFC 3339 UTC>` and pass `--region MISO` (or set
> `NAMI_REGION`). See `docs/product-roadmap.md`.

### 2. Embedding / vector-index rebuild

A reindex job runs once per day to refresh embeddings against new
corpus content; clients only need the new index by morning. The grid
delta between "midnight gas" and "predawn ramp" can be material.

```sh
nami run --region <BA> --duration 3h --deadline 2026-05-20T13:00:00Z \
    --report ~/.local/state/nami/reports/reindex.json \
    -- python rebuild_index.py
```

### 3. Batch ETL / data-warehouse transform

Overnight transforms feed the next morning's reports. Deadlines are
strict; *which* hour the work happens is not.

```sh
nami run --region <BA> --duration 2h --deadline 2026-05-20T11:00:00Z \
    --log /var/log/nami/etl.log --quiet \
    -- dbt run
```

`--log` redirects the wrapped child's stdout/stderr to a file; `--quiet`
silences it from the terminal (the `nami` summary itself still prints).

### 4. Documentation / static-site generation

Release docs, site rebuilds, large API reference generation — work that
must be ready by deploy time but can shift inside that window.

```sh
nami preview --region <BA> --duration 30m --deadline 2026-05-20T08:00:00Z \
    -- pnpm docs:build
```

### 5. Local model-evaluation suite

A larger sklearn / small-PyTorch evaluation run before checking work
in. End-to-end demo of this exact pattern (sklearn, ~3s of real work)
lives in `examples/demo.sh`.

```sh
nami run --region <BA> --duration 5m --deadline 2026-05-20T17:00:00Z \
    --report eval-report.json \
    -- python examples/sklearn_train.py
```

## What sample output actually looks like

A real run during a clean solar peak, with a 6 h deadline:

```text
nami preview — region CAISO — deadline 2026-05-19T02:37:11Z
No materially cleaner window found before the deadline.
Recommendation: run immediately.
Run-now estimate: 33 gCO2/kWh
Confidence: Low
Basis: historical-pattern forecast from hourly public data
       (historical-pattern-mean-8w-hour-dow-month-v1)
Warning: Estimate is average carbon intensity, not marginal emissions.
Warning: Not a guarantee of an actual emissions reduction.
Warning: window confidence = most conservative of 1 hourly forecast point(s)
```

This is the *expected* shape of an honest output. The next-morning
deferral you'd see at, say, 22:00 local on the same region looks the
same — different decision, same audit fields.

After execution the report carries: command, region, deadline,
decision + reason + confidence, provider info, freshness state,
methodology version, run-now estimate, selected-window estimate,
estimated improvement, materiality threshold, warnings, started/ended
timestamps, wall duration, and the child's exit code. The schema is
in `crates/nami-core/src/report.rs::RunReport`.

## Where `nami` is the wrong tool

Be honest with yourself about whether your workload fits. `nami` is
the wrong tool when:

- **A human is waiting for the result.** Interactive work, REPLs,
  ad-hoc scripts. The whole point is to shift *toward* cleaner hours;
  delay is the mechanism.
- **Your decision needs sub-hour resolution.** EIA-930 is hourly;
  `nami` is hourly. Anything that wants 5-minute precision should look
  elsewhere.
- **You need marginal emissions, not average intensity.** `nami` is
  explicit about producing the latter. If your accounting or research
  question is specifically marginal CO₂ (e.g., dispatchable-fossil
  response to load changes), a commercial provider like WattTime is the
  right primary source — `nami` may eventually add it as an opt-in
  capability but will not stop being a public-data tool by default.
- **The workload runs in a cloud queue you don't control.** If the
  cloud scheduler decides when your job runs (Lambda, SageMaker job
  queues, opaque CI runners), `nami`'s decision can't influence it.
  Use `nami preview` for the recommendation and surface it as
  metadata, but `nami run` won't help.
- **You're outside the Phase 0 supported BAs.** Today: CAISO, ERCOT,
  MISO, PJM, NYISO, ISONE, SPP. Other regions are planned (UK Carbon
  Intensity API, ENTSO-E) but not shipped.
- **Cross-region scheduling** ("schedule me into whichever of MISO/PJM/
  ERCOT is cleanest right now"). `nami` is single-region by design.
  Region *comparison* as a read-only informational surface may show up
  later (see `docs/product-roadmap.md`); scheduling across regions
  will not.

## "Will this even help me?" — a quick decision tree

```
Is the job something a human is waiting for?
  └── yes → don't use nami; just run it.
  └── no → does it have a hard finish-by deadline?
            └── no → set one. (`--within 8h` is fine.)
            └── yes → is your region supported and your cache fresh?
                       └── no → `nami status` will tell you what to fix.
                       └── yes → `nami preview` first. If it suggests a
                                 deferral with confidence ≥ Low and
                                 ≥ 5% improvement, `nami run` will earn
                                 its keep. If it says "run now," that
                                 *is* the right answer for this hour.
```
