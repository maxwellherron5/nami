Below is a replacement-style `CLAUDE.md` that keeps the structure and tone of your original file, but shifts the project toward the more defensible public-data, uncertainty-aware plan. I based the format and many operational conventions on your existing CLAUDE.md. 

````markdown
# CLAUDE.md

Operational instructions for Claude Code working on `nami`. Read this every session before touching code.

---

## What `nami` is

`nami` is a single-binary Rust CLI that wraps a user-supplied command and schedules it to run during an estimated lower-carbon grid window within a user-supplied deadline.

It is a **conservative, uncertainty-aware, public-data carbon-aware scheduler**.

It is **not** a precise carbon optimizer, not a measurement tool, not a training framework, not a cloud orchestrator, and not a guarantee of actual marginal emissions reductions.

`nami` converts temporal flexibility into a scheduling recommendation using only transparent, free, auditable public grid data. In Phase 0, that means EIA-930 hourly grid data plus EPA eGRID emission factors. The tool estimates cleaner candidate windows from historical hourly generation patterns and clearly labels uncertainty, confidence, data freshness, and methodology limitations.

The full design rationale, scope decisions, and roadmap live in `docs/project-brief.md`. Read it on the first session and re-read its non-goals section before adding anything new. **When the brief and this file disagree, the brief wins.** When in genuine doubt, ask the user.

---

## Critical context for every session

1. **Phase 0 is tightly scoped and free-data only.** Linux only. EIA-930 + EPA eGRID as the sole required data sources. Single static binary. No pause/resume. No GUI. No measurement. No queue. No commercial APIs. If a request seems to add scope beyond Phase 0, or to introduce a paid data dependency, flag it and ask before implementing.

2. **The free-data constraint is the project's identity, not a temporary limitation.** EIA-930 and eGRID are the Phase 0 foundation. Open Grid Emissions and ISO/RTO-specific public feeds are planned Phase 1 candidates. WattTime, Electricity Maps commercial API access, and similar paid or restricted APIs are explicitly Phase 2+ optional providers if added at all. Do not propose adding paid APIs to Phase 0 without explicit user direction.

3. **Do not overclaim what EIA-930 provides.** EIA-930 provides hourly observed grid data and demand forecasts. It does **not** provide future carbon intensity forecasts, future generation mix forecasts, marginal emissions, or sub-hourly clean-window certainty. Any Phase 0 forecast is `nami`'s model layered on historical hourly public data.

4. **Phase 0 scheduling resolution is hourly.** Do not build 5-minute, 15-minute, or minute-level scheduling claims on EIA-930. Jobs shorter than an hour may still be scheduled, but the carbon estimate is based on hourly windows and must be labeled accordingly.

5. **Forecast means estimated historical-pattern forecast, not direct grid operator carbon forecast.** The Phase 0 model estimates expected average carbon intensity from historical samples matching region, hour of day, day of week, month or season, and recent history. This must be reflected in code names, docs, CLI output, and reports.

6. **Refuse or downgrade confidence when data is missing, stale, sparse, or inconclusive.** If the historical cache is stale, the EIA API is down, a region is unsupported, samples are too sparse, or candidate windows differ by less than a configured materiality threshold, the tool surfaces that fact loudly. It does not invent numbers or paper over uncertainty with reasonable-looking defaults.

7. **Methodology must be transparent.** Every carbon intensity estimate `nami` produces should be traceable to its inputs: which EIA generation mix data, which eGRID emission factors, which historical samples, which forecast model, which confidence calculation, and which materiality threshold. Document the math in code comments and in `docs/methodology.md`. Future readers, reviewers, and users need to be able to audit any number the tool produces.

8. **Average emissions are not marginal emissions.** Phase 0 uses estimated average carbon intensity, not marginal emissions. Never imply that `nami` proves actual avoided marginal emissions. Reports should use language like "estimated lower average carbon intensity" unless a future provider explicitly supports marginal emissions.

9. **Wrap, don't replace.** `nami` never modifies the user's training code, environment, or framework. Every interaction with the user's job is through subprocess management. If a feature requires the user to change their training code, redesign it or drop it.

10. **Signal handling is a correctness concern, not a polish concern.** SIGINT during the wait phase cancels the schedule cleanly. SIGINT during the run forwards to the child. Exit codes propagate. Get this right early.

11. **Product positioning sits alongside the engineering thesis — they are not the same thing.** In product terms `nami` is a deadline-aware runner for deferrable developer workloads (nightly CI, batch ETL, embedding rebuilds, evals, docs gen, backups); carbon-aware scheduling is the differentiator. A feature can advance the *product wedge* (ergonomics, profiles, reports, integration recipes) without touching methodology, and vice versa. Don't collapse one framing into the other when designing or reviewing changes. See `docs/project-brief.md`'s "Product positioning" section and `docs/product-roadmap.md`. Identity-violating ergonomics shortcuts (e.g. defaulting to a commercial provider for broader appeal) remain off-limits — item 2 stands.

---

## Core project thesis

The defensible thesis is:

> Can a local CLI use only free, transparent public grid data to make conservative, uncertainty-aware scheduling decisions for flexible compute jobs?

The project does **not** claim:

> EIA/eGRID can accurately predict the cleanest future grid windows for arbitrary U.S. compute workloads.

The first thesis is viable. The second is too strong.

`nami` should always prefer methodological honesty over impressive-looking precision.

---

## Data philosophy

### Required properties for Phase 0 data sources

A Phase 0 data source must be:

- Free to access
- Publicly documented
- Auditable
- Usable without a commercial contract
- Suitable for reproducible methodology
- Stable enough for a CLI user to depend on
- Explicit about freshness, lag, and granularity limitations

### Phase 0 sources

- **EIA-930**: hourly balancing authority data, including fuel-type generation, demand, net generation, interchange, and day-ahead demand forecast.
- **EPA eGRID**: static emission factors used to convert generation mix into estimated average carbon intensity.

### Phase 1 candidate sources

These are promising but not Phase 0 requirements:

- Open Grid Emissions
- CAISO public renewable forecast and generation data
- ERCOT public generation and renewable forecast data
- PJM Data Miner feeds
- SPP public load and renewable forecast data
- NYISO, ISONE, and MISO public market/grid feeds where licensing and access are compatible

Phase 1 may introduce uneven regional capabilities. That is acceptable if capabilities are explicit and surfaced to the user.

### Phase 2+ candidate sources

These are deferred unless explicitly approved:

- Commercial Electricity Maps API access
- WattTime
- Paid marginal emissions providers
- Cloud-provider-native carbon APIs
- Any source requiring paid access, private credentials beyond free registration, or restrictive redistribution terms

---

## Provider capability model

Do not treat all carbon providers as equivalent.

Each provider must declare what it can and cannot do.

Suggested capability categories:

```rust
pub enum ProviderCapability {
    HistoricalHourly,
    HistoricalSubHourly,
    RealtimeObserved,
    RealtimeObservedWithLag,
    DayAheadLoadForecast,
    RenewableForecast,
    AverageCarbonForecast,
    MarginalEmissionsEstimate,
    MarginalEmissionsForecast,
}
````

The Phase 0 EIA/eGRID provider should likely advertise:

```text
HistoricalHourly
RealtimeObservedWithLag
DayAheadLoadForecast
```

It should **not** advertise:

```text
AverageCarbonForecast
MarginalEmissionsEstimate
MarginalEmissionsForecast
```

unless the implementation truly supports those semantics.

The scheduler should make decisions based not just on returned numbers, but also on provider capabilities, freshness, confidence, and materiality.

---

## Core abstractions

Prefer explicit traits over one vague all-powerful provider.

Suggested direction:

```rust
pub trait HistoricalCarbonProvider {
    fn historical_intensity(
        &self,
        region: Region,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CarbonObservation>>;
}

pub trait ForecastProvider {
    fn forecast_intensity(
        &self,
        region: Region,
        horizon: ForecastHorizon,
    ) -> Result<Vec<ForecastPoint>>;
}

pub trait RealtimeGridProvider {
    fn latest_observed_mix(
        &self,
        region: Region,
    ) -> Result<GridSnapshot>;
}

pub trait ProviderMetadata {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Vec<ProviderCapability>;
    fn granularity(&self) -> DataGranularity;
    fn expected_lag(&self) -> Option<Duration>;
}
```

Do not force providers to implement capabilities they do not actually have. It is better for a provider to be honest and limited than broad and misleading.

---

## Decision model

A scheduling decision is valid only if it includes:

* Candidate windows considered
* Estimated intensity per candidate window
* Run-now estimate
* Selected start time, if any
* Estimated improvement over run-now
* Confidence
* Data freshness
* Sample count
* Materiality threshold
* Provider name and capabilities
* Methodology label
* Warnings and caveats

The scheduler should support at least these decision outcomes:

```rust
pub enum SchedulingDecision {
    StartAt {
        start_time: DateTime<Utc>,
        reason: StartReason,
        confidence: Confidence,
    },
    StartImmediately {
        reason: StartReason,
        confidence: Confidence,
    },
    Refuse {
        reason: RefuseReason,
    },
}
```

Example refusal reasons:

```rust
pub enum RefuseReason {
    UnsupportedRegion,
    MissingHistoricalData,
    StaleHistoricalCache,
    InsufficientSamples,
    ProviderUnavailable,
    NoWindowBeforeDeadline,
    CandidateWindowsBelowMaterialityThreshold,
    ForecastTooUncertain,
}
```

Example start reasons:

```rust
pub enum StartReason {
    LowestEstimatedIntensity,
    RunNowAlreadyCleanest,
    DeadlineTooSoon,
    FallbackPolicyRunImmediately,
    UserForced,
}
```

---

## Confidence and materiality

Confidence is first-class. Do not treat it as a UI flourish.

A forecast point should include:

* Estimated intensity
* Sample count
* Standard deviation or interval width
* Confidence label
* Data source and methodology label

Suggested confidence dimensions:

```rust
pub struct Confidence {
    pub level: ConfidenceLevel,
    pub sample_count: usize,
    pub interval: Option<ConfidenceInterval>,
    pub notes: Vec<String>,
}

pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
    VeryLow,
}
```

A recommendation should only be made if the expected improvement is large enough to matter.

Default materiality threshold:

```text
5% estimated improvement over running now
```

If the best candidate window improves estimated average intensity by less than the threshold, prefer:

```text
No recommendation made: candidate windows differ by less than the configured materiality threshold.
```

or:

```text
Start immediately: no materially cleaner window found before deadline.
```

Do not present tiny differences as meaningful.

---

## User-facing language rules

Use careful language.

Prefer:

* "estimated lower-carbon window"
* "expected average carbon intensity"
* "historical-pattern forecast"
* "confidence: medium"
* "based on hourly public data"
* "not marginal emissions"
* "not a guarantee of actual emissions reduction"

Avoid:

* "cleanest possible time"
* "optimal carbon time"
* "guaranteed emissions reduction"
* "real-time carbon forecast"
* "precise grid carbon"
* "marginal emissions" unless the provider actually supports it

Example good output:

```text
Recommended start: 2026-05-15 02:00 UTC
Expected intensity: 318 gCO₂/kWh
Run-now estimate: 391 gCO₂/kWh
Estimated improvement: 18.7%
Confidence: Medium
Basis: 8 weeks of historical EIA-930 hourly fuel mix matched by region/hour/day/month.
Warning: This is an average-intensity estimate, not marginal emissions.
```

Example good non-recommendation:

```text
No materially cleaner window found before the deadline.
Best candidate was only 2.1% lower than running now, below the 5.0% threshold.
Recommendation: run immediately.
Confidence: Low
```

---

## Coding conventions

### Rust style

* Edition 2024, MSRV 1.80.
* `rustfmt` enforced; run `cargo fmt` before any commit.
* `clippy` clean; run:

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

* No `#[allow(clippy::...)]` annotations without an accompanying comment explaining why.
* Avoid `unsafe`. If you genuinely need it, surface that to the user; don't sneak it in.
* No `unwrap()` or `expect()` in library code. Use `?` and propagate errors.
* `unwrap()` is acceptable in tests and in `main.rs` where panicking is the desired failure mode.
* Public items get rustdoc with at least one example for non-trivial APIs.

### Error handling

* Libraries use `thiserror` for typed errors.
* Each crate exports its own `Error` enum and `Result<T> = std::result::Result<T, Error>` alias.
* Binary crate may use `anyhow` for top-level command handling.
* Never swallow errors.
* If an error is genuinely safe to ignore, log it at `tracing::warn!` and explain why in a comment.

### Async vs sync

Use `tokio` for:

* HTTP calls
* subprocess management
* timers
* signal handling

Do **not** use async for:

* carbon intensity math
* forecast computation
* scheduler decision logic
* small local file reads/writes unless there is a concrete reason

The runtime is multi-threaded `tokio`. Do not assume single-threaded execution.

### Logging

* Use `tracing` for everything.
* Do not use `println!` or `eprintln!` except for intentional CLI user-facing output and tests.
* Use structured fields, not string interpolation.

Good:

```rust
tracing::info!(
    region = %region,
    confidence = ?confidence.level,
    improvement_pct = improvement_pct,
    "scheduling decision computed"
);
```

Bad:

```rust
info!("scheduling for {} with improvement {}", region, improvement_pct);
```

### Serialization

* `serde` derives on every type that crosses a boundary.
* JSON for runtime reports and caches.
* TOML for static config and factor tables.
* No YAML.

### Naming

* Crates:

  * `nami-core`
  * `nami-carbon-eia`
  * `nami-carbon-static`
  * `nami-scheduler`
  * `nami-region`
  * `nami-cli`

* Modules:

  * short, lowercase, no prefixes
  * `forecast`, not `nami_forecast`

* Types:

  * PascalCase and descriptive
  * `CarbonIntensity`, not `CI`
  * `ForecastPoint`, not `Point`

* Errors:

  * `Error` enum and `Result<T>` alias per crate

---

## Project structure

```text
nami/
  Cargo.toml
  rust-toolchain.toml
  CLAUDE.md
  README.md
  docs/
    project-brief.md
    methodology.md
    public-data-sources.md
    eia-api-notes.md
    confidence-and-materiality.md
  data/
    egrid-factors.toml
    historical-cache.json
  crates/
    nami-core/
    nami-carbon-eia/
    nami-carbon-static/
    nami-scheduler/
    nami-region/
    nami-cli/
  examples/
  tests/
```

Within a crate:

```text
nami-foo/
  Cargo.toml
  src/
    lib.rs
    error.rs
    <module>.rs
  tests/
```

`lib.rs` is the table of contents, not the implementation. Implementations go in named modules.

---

## Build, test, and verify

Commands to run during development and before any commit:

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo doc --workspace --no-deps
```

When working on a feature, run:

```sh
cargo test -p <crate>
```

for fast iteration. Run the full workspace suite before ending a session.

---

## Testing strategy

### Unit tests

Unit tests live inline in `#[cfg(test)] mod tests`.

Use them for:

* carbon intensity math
* confidence interval computation
* materiality threshold logic
* scheduler decisions
* time-window generation
* sample matching
* refusal behavior
* stale-cache handling

### Integration tests

Integration tests live in `crates/<crate>/tests/`.

Use them for:

* provider behavior
* scheduler-provider interaction
* CLI parsing
* report generation
* cache read/write paths

### Fixture-based tests

Fixture-based tests for EIA parsing should use captured responses under:

```text
tests/fixtures/
```

Do not hit the live EIA API in standard tests.

### Live API tests

Live API tests are gated behind a feature flag:

```sh
cargo test --features live-eia
```

They require:

```text
EIA_API_KEY
```

Never commit credentials.

### Methodology validation tests

Validation tests should check that:

* fuel mix sums are handled consistently
* unit conversions are correct
* missing fuel categories are surfaced
* confidence degrades when samples are sparse
* stale data triggers fallback/refusal behavior
* small estimated differences do not produce overstated recommendations

Where possible, compare derived estimates against an external reference dataset. These tests should be documented as sanity checks, not proof of exact correctness.

---

## Don't add a dependency without considering alternatives

Before `cargo add <thing>`, check whether:

* It is actually needed
* It pulls a large transitive tree
* A 20-line internal implementation would be simpler
* The license is compatible
* It makes the static binary goal harder
* It affects reproducibility

Preferred licenses:

* MIT
* Apache-2.0
* BSD

Avoid GPL dependencies unless explicitly approved.

Pin dependencies loosely:

```toml
tokio = "1"
```

not:

```toml
tokio = "1.42.3"
```

unless there is a specific reason to pin tightly.

---

## Domain knowledge you need

### EIA-930

EIA-930 is the Phase 0 grid data foundation.

It provides hourly data from U.S. balancing authorities, including:

* demand
* day-ahead demand forecast
* net generation
* generation by fuel type
* interchange

Important limitations:

* Hourly granularity
* Reporting lag
* No direct carbon intensity forecast
* No future fuel mix forecast
* No marginal emissions
* Fuel-type sums may not perfectly match reported total net generation
* Smaller balancing authorities can have noisier or less complete data
* Always use UTC timestamps internally

Phase 0 supported balancing areas should start conservative:

* CAISO
* ERCOT
* MISO
* PJM
* NYISO
* ISONE
* SPP

Do not assume all regions have equal data quality.

### EPA eGRID

EPA eGRID provides emission factors.

In Phase 0, eGRID is used to map generation mix to estimated average carbon intensity.

Important limitations:

* Static factors
* Updated periodically, not in real time
* Average emissions, not marginal emissions
* Geographic mapping may be imperfect
* Some fuel categories require documented assumptions

Units:

```text
lbs CO₂/MWh × 453.592 / 1000 = gCO₂/kWh
```

Use `gCO₂/kWh` internally.

### Carbon intensity derivation

Phase 0 estimated average carbon intensity:

```text
intensity[region, t] =
    Σ_fuel generation_mwh[fuel, region, t] × emission_factor[fuel, region]
    ----------------------------------------------------------------------
              Σ_fuel generation_mwh[fuel, region, t]
```

Be careful with MWh ↔ kWh conversion.

Use the sum of fuel-type generation for internal consistency unless methodology docs explicitly say otherwise.

Document how `Other` and `Unknown` are handled.

### Forecast modeling

Phase 0 forecast is a historical-pattern estimate.

A simple defensible baseline:

```text
forecast[region, target_t] =
    mean of historical observations matching:
      - region
      - hour of day
      - day of week
      - month or season
    over the last N weeks
```

Default `N` may be 8, but this should be configurable later.

The forecast should include:

* mean
* sample count
* variance or interval
* confidence label
* methodology label

Do not call this a direct EIA forecast.

### Candidate window scoring

For a job with estimated duration `D`, candidate start time `s`, and hourly forecast points:

```text
window_intensity[s] =
    duration-weighted mean intensity over [s, s + D]
```

If duration is shorter than one hour, use the containing hourly estimate and label the result as hourly-resolution.

If the job crosses multiple hourly buckets, weight by overlap duration.

### Materiality

A lower-carbon recommendation requires a meaningful difference.

Default:

```text
best_window_improvement >= 5%
```

If not met, do not overstate the result.

### Data freshness

Every provider response should include freshness metadata.

For EIA-930:

* observed data can lag
* cache can be stale
* missing hours are possible

The scheduler should know whether it is operating with:

* fresh observed data
* stale observed data
* historical cache only
* static fallback only
* no usable data

---

## Subprocess wrapping

* Use `tokio::process::Command`.
* Validate the command exists before scheduling.
* Spawn child process only at the selected start time.
* By default, inherit stdout/stderr.
* Allow `--quiet` to silence output.
* Allow `--log <file>` to redirect output.

Signal behavior:

* During wait:

  * SIGINT cancels schedule
  * exit cleanly
* During run:

  * forward SIGINT/SIGTERM/SIGHUP to child
  * allow grace period
  * then SIGKILL if needed

Exit behavior:

* Propagate the child exit code.
* If the wrapped command exits with code 42, `nami` exits with code 42.

---

## CLI behavior

Core subcommands:

```text
nami run
nami preview
nami forecast
nami status
```

### `nami run`

Schedules and runs a command.

Example:

```sh
nami run --region MISO --deadline "2026-05-15T12:00:00Z" --duration 3h -- cargo test
```

### `nami preview`

Computes a recommendation but does not run the command.

### `nami forecast`

Prints candidate forecast points and confidence metadata.

### `nami status`

Reports cache freshness, supported regions, provider availability, and configured data sources.

---

## Reports

Every run or preview should be able to emit a `RunReport`.

A report must include:

* command
* args
* region
* deadline
* estimated duration
* selected start time
* actual start time, if run
* actual end time, if run
* scheduling decision
* reason
* confidence
* provider
* provider capabilities
* data freshness
* methodology version
* run-now estimate
* selected-window estimate
* estimated improvement
* materiality threshold
* warnings
* child exit code, if run

Reports should be JSON-serializable.

---

## Anti-patterns / what not to do

* **Do not claim precise carbon optimization.** `nami` estimates lower-carbon windows from public data. It does not know the true future grid state.

* **Do not claim marginal emissions reductions in Phase 0.** Phase 0 uses estimated average intensity.

* **Do not pretend EIA provides carbon forecasts.** It does not.

* **Do not add sub-hour scheduling claims on top of hourly data.** Hourly data can support hourly decisions, not minute-level certainty.

* **Do not add commercial API support in Phase 0.** Paid APIs are out of scope.

* **Do not add per-stage energy measurement.** That belongs to measurement tools. `nami` schedules.

* **Do not add cloud-native features.** Kubernetes operators, AWS integrations, and batch queue systems are out of scope.

* **Do not add framework-specific features.** No PyTorch hooks, JAX integrations, or training-code introspection.

* **Do not silently estimate.** If confidence is low, say so. If data is missing, stale, or sparse, surface it.

* **Do not present tiny differences as meaningful.** Respect the materiality threshold.

* **Do not hide fallback behavior.** Static fallback and degraded modes must be obvious in CLI output and reports.

* **Do not optimize prematurely.** The hot path is data fetch, cache read/write, forecast computation, and subprocess management. Keep the architecture clear.

---

## Per-session workflow

### Starting a session

1. Read this file.
2. Read `docs/project-brief.md`, especially scope and non-goals.
3. Read `docs/methodology.md` if touching carbon math, forecasting, confidence, or provider behavior.
4. Run:

```sh
cargo build --workspace
```

5. Restate the session goal.
6. Plan in chat before opening files.
7. Identify affected crates, modules, tests, and methodology docs.

### During a session

* Make one logical change at a time.
* Write tests alongside implementation.
* Avoid unrelated refactors.
* If a methodology decision appears, stop and surface it.
* If a data-source assumption appears, document and test it.
* If confidence, materiality, or fallback behavior changes, update docs.
* If adding a dependency, ask first.
* If the implementation would cause `nami` to overclaim, redesign it.

### Before ending a session

Run:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Update relevant docs if behavior changed.

Summarize:

* what was done
* what remains
* what surprised you
* what assumptions were introduced
* what tests validate the change
* suggested commit message

Do not auto-commit. Let the user review and commit.

---

## First-session goals

The first session establishes the skeleton. Do not implement business logic.

1. Set up the Cargo workspace:

   * root `Cargo.toml`
   * `rust-toolchain.toml`
   * `.gitignore`
   * include `data/historical-cache.json` in `.gitignore`

2. Create the crates:

   * `nami-core`
   * `nami-carbon-eia`
   * `nami-carbon-static`
   * `nami-scheduler`
   * `nami-region`
   * `nami-cli`

3. In each crate:

   * create `Cargo.toml`
   * create `src/lib.rs` or `src/main.rs`
   * create `src/error.rs` where appropriate
   * add stub `Error` enum and `Result<T>` alias

4. In `nami-core`, define core data types:

   * `JobSpec`
   * `Region`
   * `FuelType`
   * `CarbonIntensity`
   * `EmissionFactor`
   * `CarbonObservation`
   * `ForecastPoint`
   * `ForecastHorizon`
   * `Confidence`
   * `ConfidenceLevel`
   * `ConfidenceInterval`
   * `DataFreshness`
   * `DataGranularity`
   * `ProviderCapability`
   * `ProviderInfo`
   * `SchedulingDecision`
   * `StartReason`
   * `RefuseReason`
   * `RunReport`

5. In `nami-core`, sketch traits:

   * `HistoricalCarbonProvider`
   * `ForecastProvider`
   * `RealtimeGridProvider`
   * `ProviderMetadata`
   * `Scheduler`
   * `Sink`

6. In `nami-carbon-static`, implement a placeholder `StaticTableProvider`.

   * Hardcoded flat annual averages are acceptable for the skeleton.
   * It must clearly identify itself as static fallback data.
   * It must return low confidence.
   * It must not masquerade as a real forecast provider.

7. In `nami-cli`, set up `clap` derive for:

   * `run`
   * `preview`
   * `forecast`
   * `status`

   Handlers can be `unimplemented!()` in the first session.

8. Add initial docs:

   * `docs/project-brief.md`
   * `docs/methodology.md`
   * `docs/public-data-sources.md`
   * `docs/confidence-and-materiality.md`

9. Verify:

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Do not implement:

* EIA API client
* eGRID factor lookup
* carbon intensity derivation
* forecast model
* scheduler logic
* subprocess wrapping
* cache refresh
* actual command execution

Types, traits, CLI skeleton, docs, and methodology scaffolding first.

---

## Phase 0 implementation goals

After the skeleton, implement Phase 0 in this order:

1. Static provider and report plumbing
2. Time-window generation
3. Materiality threshold logic
4. Confidence model
5. Historical cache format
6. EIA fixture parsing
7. eGRID factor table loading
8. Carbon intensity derivation
9. Historical-pattern forecast
10. Scheduler decision logic
11. CLI preview output
12. CLI run subprocess wrapping
13. Cache refresh
14. Live EIA tests behind feature flag
15. Documentation hardening

At every step, preserve the public-data, uncertainty-aware framing.

---

## Phase 1 roadmap

Phase 1 may add better free providers, but only after Phase 0 is stable.

Candidate additions:

* Open Grid Emissions provider
* CAISO public data provider
* ERCOT public data provider
* PJM Data Miner provider
* SPP public data provider
* Better region detection
* Provider capability comparison
* Provider selection strategy
* Region-specific confidence models

Phase 1 should expect uneven provider capabilities. Do not hide that. Surface it.

---

## Phase 2+ roadmap

Potential future directions:

* Marginal emissions modeling
* EPA CAMPD/CEMS integration
* Plant-to-balancing-authority mapping
* Validation against external emissions datasets
* Commercial provider adapters, if explicitly approved
* More sophisticated forecasting models
* Multi-region scheduling
* Batch queue integration

Do not build Phase 2 features during Phase 0.

---

## When you're unsure

Stop. Ask the user.

Especially ask before deciding:

* Whether a new provider is in scope
* Whether a data source is free enough
* Whether a licensing condition is acceptable
* Whether to add a dependency
* Whether a forecast method is methodologically defensible
* Whether to use average or marginal emissions terminology
* Whether confidence should refuse or merely warn
* Whether to broaden region support
* Whether to refactor existing architecture
* Whether to change report semantics

This is a serious project held to research-grade standards, not a toy. Contributors value correctness, clarity, conservative scope, and transparent methodology over speed.

If a number cannot be defended, do not produce it.

If a recommendation cannot be trusted, downgrade it or refuse.

If the data cannot support the claim, change the claim.

```
```

