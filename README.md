# nami

**Run deferrable developer jobs when the grid is likely cleaner — and
know when nami is guessing.**

A single-binary Rust CLI for jobs that need to *finish* by a deadline,
not start immediately: nightly CI, batch ETL, embedding/index rebuilds,
model evals, docs generation, backups. nami estimates which upcoming
hour has the lowest average carbon intensity using only free public
data, executes your command in that window, and writes an auditable
report — refusing to recommend when the difference isn't meaningful or
the data isn't strong enough.

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

## Honest by design

The honesty is the value proposition, not a limitation to apologize for.
Concretely, every nami number is:

- **Estimated average, not marginal.** nami never claims marginal
  emissions; the report labels which signal produced each figure.
- **Bounded by a materiality threshold.** Below 5% improvement over
  running now (the default), nami does not recommend deferring —
  forecast noise often exceeds that.
- **Refused, not guessed, when the data won't support a claim.** Missing
  cache, sparse samples, foreign data, or sub-threshold differences all
  yield an explicit refusal with reason and confidence.
- **Auditable.** Each `RunReport` ties its numbers back to the EIA-930
  observations, the pinned eGRID release, the forecast methodology
  version, the materiality threshold, and the data freshness state.

This stance is the project's identity. Commercial signals (marginal
emissions APIs, paid forecasters) could in principle slot into the
provider-capability abstraction as opt-in modes later, but they will
never become the default — see `docs/product-roadmap.md`.

## Use it for

Workloads with a natural finish-by deadline and no human waiting on
them — the kinds of jobs an engineer is already willing to defer
overnight or to the next morning:

- **Nightly CI / test suites** that must be green by start of day.
- **Embedding or vector-index rebuilds** with a daily cadence.
- **Batch ETL / data-warehouse transforms** before a morning report.
- **Documentation / static-site generation** for a release deadline.
- **Local model-evaluation suites** before a check-in.

Concrete commands, sample output, and the cases where nami is the
*wrong* tool live in [`docs/use-cases.md`](docs/use-cases.md).

## CLI

Start with `preview` — it shows nami's actual recommendation instantly,
without taking control of your job:

```sh
# Try it first: real scheduling decision over real grid data, nothing runs.
nami preview --region MISO --deadline 2026-05-15T12:00:00Z --duration 3h -- cargo test

# When ready, let nami wrap the command end-to-end: wait for the chosen
# window, spawn the process, forward signals, propagate the exit code.
nami run --region MISO --deadline 2026-05-15T12:00:00Z --duration 3h -- cargo test

# Refresh one region's slice of the local historical cache from EIA-930.
EIA_API_KEY=… nami refresh --region MISO          # --weeks N (default 8)

# Inspect the historical-pattern forecast curve over a horizon.
nami forecast --region MISO --horizon 24h

# Cache freshness, data sources, supported regions (+ optional report summary).
nami status [--report run-report.json]
```

> **Coming next** (see [`docs/product-roadmap.md`](docs/product-roadmap.md)):
> named profiles in `nami.toml` so `nami run nightly` replaces the long
> flag list, and relative deadlines (`--within 8h`, `--by 7am`) so you
> don't hand-type an RFC 3339 timestamp. The current flag surface still
> works and is the engine those ergonomics will sit on.

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
- **Region resolution** (when `--region` is omitted): `NAMI_REGION` env
  var, then `region = "<BA>"` in the nami config file
  (`$NAMI_CONFIG`, else `$XDG_CONFIG_HOME/nami/config.toml`, else
  `$HOME/.config/nami/config.toml`); otherwise refuse. No IP
  geolocation, no timezone guessing — BA boundaries do not follow either,
  and a heuristic would be confidently wrong too often. When a value
  comes from anywhere other than the flag, `nami` announces the source
  on stderr. Supported regions: CAISO, ERCOT, MISO, PJM, NYISO, ISONE,
  SPP.

## Status

Phase 0 is complete: all five subcommands (`run`, `preview`, `refresh`,
`forecast`, `status`) are implemented and tested. EIA-930 fetch +
paginated cache refresh, the eGRID factor table, carbon-intensity
derivation, the historical-pattern forecast, scheduler decision logic,
and subprocess wrapping (signal forwarding + exit-code propagation) are
all in place, with live-API tests gated behind the `live-eia` feature.
Deterministic region resolution (flag / `NAMI_REGION` env / config file)
is in; IP-based auto-detection remains deferred (would add a third-party
network call and a spatial dependency, and isn't aligned with the
project's refuse-rather-than-guess stance). The next work — ergonomics
(`nami.toml` profiles, relative deadlines, `nami init`, `nami doctor`)
and longitudinal reports — is laid out in
[`docs/product-roadmap.md`](docs/product-roadmap.md).

See:

- `docs/project-brief.md` — scope, non-goals, roadmap (authoritative)
- `docs/use-cases.md` — what to actually use nami for (and what not to)
- `docs/product-roadmap.md` — ergonomics-first roadmap (Phases A–D)
- `docs/methodology.md` — the math, with caveats
- `docs/public-data-sources.md` — what we use and what we don't
- `docs/confidence-and-materiality.md` — uncertainty model
- `docs/eia-api-notes.md` — EIA-930 API shape, fetch, and refresh notes
- `CLAUDE.md` — operational instructions for contributors

## License

Dual-licensed under MIT or Apache-2.0 at your option.
