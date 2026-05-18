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

## CLI

```sh
# Schedule + run a command in an estimated lower-carbon hour before the deadline
nami run --region MISO --deadline 2026-05-15T12:00:00Z --duration 3h -- cargo test

# Same decision, but only print it — do not run anything
nami preview --region MISO --deadline 2026-05-15T12:00:00Z --duration 3h -- cargo test

# Refresh one region's slice of the local historical cache from EIA-930
EIA_API_KEY=… nami refresh --region MISO          # --weeks N (default 8)

# Print the historical-pattern forecast points + confidence for a region
nami forecast --region MISO --horizon 24h

# Cache freshness, data sources, supported regions (+ optional report summary)
nami status [--report run-report.json]
```

- `run` / `preview` share flags: `--region`, `--deadline` (RFC 3339 UTC),
  `--duration` (`30s`/`45m`/`2h`/`1d`), `--report <path>` (JSON
  `RunReport`; printed to stdout if omitted). `run` additionally takes
  `--quiet` (silence the wrapped command) or `--log <file>` (redirect its
  output); it forwards SIGINT/SIGTERM/SIGHUP to the child, escalates to
  SIGKILL after a grace period, and propagates the child's exit code.
- `refresh` needs `EIA_API_KEY` (free registration; a missing key is a
  hard error, never a silent fallback). It updates only the requested
  region and preserves the rest of the cache.
- `forecast` is a read-only query over the local cache (`--cache`,
  `--weeks`); hours with no matching samples are omitted (never invented)
  and a cache-only basis caps confidence at `Low`.
- `status` is read-only and offline; it surfaces degraded states
  (missing/unusable cache, missing eGRID table, unset `EIA_API_KEY`)
  loudly rather than hiding them.
- Region detection is not implemented; `--region` is required (one of
  CAISO, ERCOT, MISO, PJM, NYISO, ISONE, SPP).

## Status

Phase 0 is complete: all five subcommands (`run`, `preview`, `refresh`,
`forecast`, `status`) are implemented and tested. EIA-930 fetch +
paginated cache refresh, the eGRID factor table, carbon-intensity
derivation, the historical-pattern forecast, scheduler decision logic,
and subprocess wrapping (signal forwarding + exit-code propagation) are
all in place, with live-API tests gated behind the `live-eia` feature.
Region auto-detection remains a Phase 1 item (`--region` is required).

See:

- `docs/project-brief.md` — scope, non-goals, roadmap (authoritative)
- `docs/methodology.md` — the math, with caveats
- `docs/public-data-sources.md` — what we use and what we don't
- `docs/confidence-and-materiality.md` — uncertainty model
- `docs/eia-api-notes.md` — EIA-930 API shape, fetch, and refresh notes
- `CLAUDE.md` — operational instructions for contributors

## License

Dual-licensed under MIT or Apache-2.0 at your option.
