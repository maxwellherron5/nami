# `nami` project brief

> **Status:** living document. Authoritative for scope decisions. When this
> file and `CLAUDE.md` disagree, this file wins.

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

These add capability *if* they preserve the free-and-auditable property:

- Open Grid Emissions provider.
- CAISO / ERCOT / PJM / SPP / NYISO / MISO / ISONE public market feeds
  where licensing and access are compatible.
- Better region detection. Deterministic resolution (flag / `NAMI_REGION`
  env / config file) is implemented in the `nami-region` crate;
  IP-based geolocation remains deferred (third-party network call,
  spatial dependency, and is in tension with the refuse-rather-than-
  guess stance).
- Provider capability comparison and selection.
- UK Carbon Intensity API.
- ENTSO-E transparency platform (Europe).

Phase 1 will introduce *uneven* regional capabilities. That is acceptable
provided each region's capabilities are explicit in CLI output and
reports.

## Phase 2+ candidates

Deferred unless explicitly approved:

- Commercial Electricity Maps API or WattTime adapter.
- Marginal emissions modelling (CAMPD/CEMS-based plant-level work).
- Multi-region scheduling.
- Batch queue integration.
- Sophisticated forecasting (state-space, ensemble, ML).

## Non-goals

These will not be added even if requested:

- Anything that requires modifying the user's training code.
- Anything that overclaims precision or pretends EIA provides direct
  carbon forecasts.
- Anything that obscures fallback behaviour or data degradation from
  the user.
- Anything that produces a number `nami` cannot defend in a methodology
  doc.

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
