# `nami` project brief

> **Status:** living document. Authoritative for scope decisions. When this
> file and `CLAUDE.md` disagree, this file wins.

## Product positioning

In product terms, `nami` is a **deadline-aware runner for deferrable
developer workloads**: nightly CI, batch ETL, embedding/index rebuilds,
model evals, docs generation, backups, scheduled data processing. These
jobs share two properties: they have a natural finish-by deadline, and
no human is waiting on them mid-run. Carbon-aware scheduling is `nami`'s
*differentiator* — it picks the window inside that flexibility — but the
addressable use case is broader than "carbon optimization."

The *engineering thesis* below (free public data, honest uncertainty)
sits alongside this positioning. They are not the same thing and should
not be collapsed into one: a feature can advance the product wedge
(ergonomics, profiles, reports) without touching methodology, and vice
versa. Concrete use cases — and the cases where `nami` is the wrong
tool — live in `docs/use-cases.md`. The ergonomics-first work plan is in
`docs/product-roadmap.md`.

## Thesis

> Can a local CLI use only free, transparent public grid data to make
> conservative, uncertainty-aware scheduling decisions for flexible
> compute jobs?

The project explicitly does **not** claim the stronger thesis:

> EIA/eGRID can accurately predict the cleanest future grid windows for
> arbitrary U.S. compute workloads.

The first thesis is viable. The second is too strong. `nami` always
prefers methodological honesty over impressive-looking precision.

## Scope (Phase 0)

In:

- Linux-only single static binary.
- **Free, public, auditable data only — as a brand, not a constraint.**
  EIA-930 and eGRID are the Phase 0 sources. Any future commercial
  provider (Electricity Maps, WattTime) would be opt-in via the
  provider-capability abstraction, never the default, and the report
  would label which signal produced each number.
- EIA-930 hourly fuel-mix observations + EPA eGRID emission factors as
  the sole live data sources.
- Estimated *average* carbon intensity per region per hour.
- Historical-pattern forecast: mean of matching (region, hour-of-day,
  day-of-week, month) samples from the last N weeks (default N=8).
- Hourly scheduling resolution; sub-hourly claims are not made.
- A configurable materiality threshold (default 5%) governing whether a
  recommendation is offered.
- Confidence as a first-class property of every estimate, including
  sample count, optional interval, and explanatory notes.
- Subprocess wrapping with correct signal forwarding and exit-code
  propagation.
- A `RunReport` JSON artifact tying every produced number to its data
  source, methodology version, and confidence.

Out:

- Marginal emissions (no Phase 0 source provides them honestly).
- Commercial APIs: WattTime, Electricity Maps' paid tier, cloud-vendor
  carbon APIs. All deferred to Phase 2+ at the earliest.
- Sub-hourly scheduling claims.
- Pause / resume of running jobs.
- GUI.
- Per-stage energy measurement (`nami` schedules; measurement tools
  measure).
- Framework-specific integration (PyTorch, JAX, etc.).
- Cloud-native orchestration (Kubernetes, AWS Batch, etc.).
- Multi-region scheduling.
- Anything that would make a number `nami` produces undefensible.

## Phase 1 candidates

Two parallel tracks. The **product-ergonomics track** (see
`docs/product-roadmap.md` Phases A–B) is the next priority because it
unblocks adoption; the **data-quality track** below adds capability
*if* it preserves the free-and-auditable property.

Data-quality track:

- Open Grid Emissions provider.
- CAISO / ERCOT / PJM / SPP / NYISO / MISO / ISONE public market feeds
  where licensing and access are compatible.
- **Region detection — deterministic resolution shipped.**
  Flag / `NAMI_REGION` env / config file via the `nami-region` crate.
  Cloud instance-metadata fallback (AWS/GCP/Azure region → BA) is a
  next candidate: still free, no third-party network call, no IP leak,
  and accurate for the cloud-job use case. IP geolocation remains
  deferred (third-party leak, spatial dependency, refuse-rather-than-
  guess tension).
- Provider capability comparison and selection.
- UK Carbon Intensity API.
- ENTSO-E transparency platform (Europe).

Phase 1 will introduce *uneven* regional capabilities. That is acceptable
provided each region's capabilities are explicit in CLI output and
reports.

## Phase 2+ candidates

Deferred unless explicitly approved. Each is judged against the
identity invariant (free public data is the default; commercial signals
are opt-in, never displacing the default):

- **Better forecasting on the existing public data class.** EIA-930
  publishes a day-ahead *demand* forecast; combining it with the
  historical fuel-mix-vs-demand pattern (or with Open Grid Emissions'
  pre-derived hourly emissions) would give a more responsive signal
  without leaving public data. This is the highest-leverage methodology
  work after Phase 1.
- Commercial Electricity Maps API or WattTime adapter — **opt-in only,
  never the default**, and surfaced in reports as a distinct
  provider/signal label.
- Marginal emissions modelling (CAMPD/CEMS-based plant-level work) —
  again, opt-in and clearly labelled.
- Sophisticated forecasting (state-space, ensemble, ML) on top of the
  public data class.
- Integration recipes (GitHub Action, cron/systemd) as
  *documentation*, not as packages we maintain.

Explicit non-candidates even at Phase 2+: multi-region scheduling,
Kubernetes operators, framework-specific integrations (PyTorch, JAX),
a Python package wrapper, per-stage energy measurement. See Non-goals.

## Non-goals

These will not be added even if requested:

- Anything that requires modifying the user's training code.
- Anything that overclaims precision or pretends EIA provides direct
  carbon forecasts.
- Anything that obscures fallback behaviour or data degradation from
  the user.
- Anything that produces a number `nami` cannot defend in a methodology
  doc.
- **Anything that demotes free public data from the default mode.** A
  commercial provider can be added behind the capability seam, but the
  free-public-data mode must remain the brand and the default.
- Multi-region scheduling. Region *comparison* as a read-only
  informational surface might be acceptable later; scheduling across
  regions is not.
- Kubernetes operators, framework-specific hooks, a Python package
  wrapper, GUI. We can ship *example recipes* in docs without taking on
  packages or operators to maintain.

## Roadmap order (Phase 0)

After the skeleton:

1. Static provider and report plumbing.
2. Time-window generation.
3. Materiality threshold logic.
4. Confidence model.
5. Historical cache format.
6. EIA fixture parsing.
7. eGRID factor table loading.
8. Carbon intensity derivation.
9. Historical-pattern forecast.
10. Scheduler decision logic.
11. CLI preview output.
12. CLI run subprocess wrapping.
13. Cache refresh.
14. Live EIA tests behind feature flag.
15. Documentation hardening.

At every step, preserve the public-data, uncertainty-aware framing.
