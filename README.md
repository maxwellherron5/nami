# nami

A conservative, uncertainty-aware, public-data carbon-aware scheduler for
flexible compute jobs.

## What

`nami` is a single-binary Rust CLI that wraps a user-supplied command and
schedules it to run during an *estimated* lower-carbon window on the U.S.
grid, within a deadline you supply. You write:

```sh
nami run --region MISO --deadline 2026-05-15T12:00:00Z --duration 3h -- cargo test
```

`nami` consults only **free, transparent, publicly auditable** grid data —
specifically EIA-930 hourly fuel-mix observations and EPA eGRID emission
factors — to estimate which hourly windows before your deadline have lower
average carbon intensity than running immediately. If a materially cleaner
window exists (default threshold: 5% lower than run-now), `nami` waits until
that hour, spawns your command, forwards signals, propagates the exit code,
and writes a JSON report describing exactly which data, methodology, and
confidence level produced the decision.

If no materially cleaner window exists, `nami` says so plainly and runs
immediately. If the data is missing, stale, or too sparse to support a
confident recommendation, `nami` refuses to make one and explains why.

It does **not** modify your training code, your environment, or your
framework. It is a scheduler, not a measurement tool, not a training
framework, not a cloud orchestrator.

## Why

Grid carbon intensity varies by 3–10× across hours on most U.S. ISOs as
wind, solar, and demand fluctuate. Most flexible compute jobs — overnight
batch training, periodic re-indexing, scheduled data processing — care
about *whether* they finish by their deadline, not *which* hours they
occupy. That gap between "when compute happens" and "when results are
needed" is real optimization surface.

The hard parts are honest ones. The grid is not actually predictable to
the precision most carbon-aware tooling implies. EIA-930 publishes
observed hourly fuel mix with reporting lag; eGRID emission factors are
annual averages, not real-time signals; "forecast" in this context means
"average of historical samples that match this hour, day, and month," not
a direct grid-operator carbon forecast. **`nami`'s thesis is that a CLI
can still produce useful, conservative scheduling recommendations from
this data alone — provided it labels its uncertainty plainly and refuses
to make claims it cannot defend.**

Concretely, `nami`:

- Uses only free, openly documented data sources (EIA-930 + eGRID in
  Phase 0; ISO public feeds and Open Grid Emissions in Phase 1). No
  commercial APIs.
- Reports every estimate with sample count, confidence level, freshness,
  and a methodology label tying the number to a specific version of the
  derivation.
- Honors a configurable materiality threshold (default 5%): tiny
  estimated improvements do not produce recommendations.
- Distinguishes estimated *average* intensity from marginal emissions —
  and never claims the latter.
- Falls back loudly, never silently. A static-fallback decision is
  marked as such in the report; a stale-cache decision is too.

## Status

Phase 0, skeleton. Workspace, core types, capability-declaring provider
traits, and a static-fallback baseline-only provider compile and have
tests. The EIA-930 client, eGRID factor table, carbon-intensity derivation,
historical-pattern forecast model, scheduler logic, and subprocess wrapper
all land in subsequent sessions per the order in `CLAUDE.md`'s
"Phase 0 implementation goals".

See:

- `docs/project-brief.md` — scope, non-goals, roadmap (authoritative)
- `docs/methodology.md` — the math, with caveats
- `docs/public-data-sources.md` — what we use and what we don't
- `docs/confidence-and-materiality.md` — uncertainty model
- `CLAUDE.md` — operational instructions for contributors

## License

Dual-licensed under MIT or Apache-2.0 at your option.
