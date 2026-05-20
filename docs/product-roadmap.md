# Product roadmap

> **Status:** living document. Authoritative for *what's next*; the
> *engineering identity* lives in `docs/project-brief.md` and supersedes
> anything here that drifts.

This roadmap orders work by *adoption leverage*, not by methodological
sophistication. The Phase 0 implementation (`docs/project-brief.md`) is
complete and the core engineering is sound. The largest remaining risk
is product-side: the default user experience requires too much setup
for what is often an "advice" interaction. The phases below address
that first; better forecasting and providers come *after*, not before.

Each phase below carries an explicit **Non-goals** list. They exist to
make scope creep accountable: identity-violating shortcuts are off the
table even when they would broaden adoption.

---

## Phase A — Lovable CLI

**Why:** the engine is solid; the on-ramp is steep. Today a new user
must pass `--region`, `--duration`, and an RFC 3339 `--deadline` on every
invocation, and run `nami refresh` once they have an `EIA_API_KEY`. The
Phase 1 region resolution removed one of those barriers; the rest are
ergonomics work. Nothing in this phase changes methodology, output
semantics, or the `RunReport` schema — it is pure surface polish.

### Scope

- **`nami.toml` profiles.** A user-level (`~/.config/nami/config.toml`)
  or repo-local config file holding named profiles. The existing
  `region = "<BA>"` resolution key stays; profiles live under
  `[profiles.<name>]` with fields `duration`, `deadline` or `within`,
  optional `materiality_threshold_pct`, optional default `region`
  override. Usage: `nami run nightly`, `nami preview reindex`.
- **Relative deadlines.** Accept `--within 8h`, `--by 7am`, `--by
  tomorrow-9am` alongside the RFC 3339 form. Parsing happens at the CLI
  boundary; internally everything stays UTC `OffsetDateTime` and the
  `RunReport`'s `deadline` is still RFC 3339. `7am` resolves against the
  host's local timezone, normalized to UTC immediately — the resolved
  value is echoed back on stderr so the user can verify it.
- **`nami init`.** A guided first-run: writes a minimal `nami.toml`
  with a configured `region`, checks `EIA_API_KEY` presence (without
  ever printing the key), checks the `data/egrid-factors.toml` file,
  and offers to run `nami refresh` once.
- **`nami doctor`.** A diagnostic that reports each precondition's
  state and an actionable next step: eGRID table present, `EIA_API_KEY`
  set, cache present and fresh per region, supported region resolved.
  Read-only.
- **Friendlier output.** Shell completions (bash/zsh/fish). Cleaner
  refusal messages that lead with the user's next action ("set
  `NAMI_REGION` to one of …"). The unimplemented-subcommand panic path
  is already gone, but the same care applies to any future stub.

### Non-goals for Phase A

- New data sources, new methodology, new confidence semantics. Every
  Phase A change should be defensible as "the engine is unchanged; the
  surface is friendlier."
- A configuration system that pretends to be cross-region scheduling.
  Profiles select among already-shipped behaviors, not new ones.

---

## Phase B — Reports as a product

**Why:** `RunReport` is already rich (decision, provider, freshness,
methodology, materiality threshold, run-now/selected estimates, exit
code, warnings). It's a one-shot artifact today; turned into a
longitudinal record it becomes the most defensible value `nami`
delivers — a team can answer "how often is `nami` recommending we
defer, and by how much?" honestly, with audit data behind it.

### Scope

- **Reports directory convention.** Default `~/.local/state/nami/
  reports/<UTC-date>/<run-id>.json` (or `$XDG_STATE_HOME` equivalent),
  overridable via `--report-dir`. The existing `--report <path>` flag
  still works and pins exactly one file.
- **`nami report summary --since 30d`.** Aggregates over the reports
  directory: count of jobs scheduled, deferred, run-immediately,
  refused; average estimated improvement when deferred; distribution
  of confidence levels; most common refusal reasons. Output is
  human-readable and JSON-emittable (`--json`).
- **`nami explain <report.json>`.** Renders the decision in prose: why
  the scheduler chose what it did, in terms of the materiality
  threshold, sample counts, freshness state, and which forecast hours
  were considered. The existing `status --report` summary is the seed
  for this.

### Non-goals for Phase B

- A daemon, a database, or a web UI. Reports are flat files; summaries
  are computed on demand.
- Phoning home / aggregating across users. Everything stays local.

---

## Phase C — Integration recipes

**Why:** the analysis is right that `nami` becomes meaningfully more
useful when it's wired into existing workflows (CI runners, cron,
systemd timers). The wrong way to ship that is to take on a Python
package, a Kubernetes operator, or a JS-side action repo we'd have to
maintain. The *right* way is **documented recipes** that compose the
already-released binary with tools the user already runs.

### Scope (all documentation, no new packages)

- **GitHub Actions example.** A workflow snippet that installs the
  released `nami` binary, runs `nami preview` against the workflow's
  region (from a repo secret or matrix var), and posts the
  recommendation as a step summary / PR comment. Optionally uses
  `nami run` for jobs the team is willing to defer on hosted runners.
- **`cron` and `systemd.timer` recipes.** How to wrap a nightly job in
  `nami run` from a cron entry; the equivalent in a `systemd` unit.
- **Shell-script orchestration.** The `examples/demo.sh` pattern,
  generalized: refresh → preview → run → summarize.

### Non-goals for Phase C

- A `nami` GitHub Action repo we publish and version.
- A Python package wrapper.
- A Kubernetes operator or CronJob CRD.
- An MCP server (interesting, but speculative until profiles and
  reports are in).

Each of those is *removed* from scope deliberately. If a community
member ships one as a third-party project on top of the binary, that's
healthy; what we do not do is fragment maintenance attention.

---

## Phase D — Provider-capability hardening and better forecasting

**Why:** the historical-pattern forecast is methodologically defensible
but practically thin — exact `(hour-of-day, day-of-week, month)`
matching over an 8-week lookback gives 2–4 samples per hour. The
right next step is a *better signal on the same data class*, not a
jump to commercial providers. EIA-930 publishes a day-ahead **demand
forecast**; combined with the historical fuel-mix-vs-demand pattern,
or with Open Grid Emissions' pre-derived hourly emissions, this gives
a more responsive estimate while preserving the free-public-data
identity.

### Scope

- **EIA demand-forecast-aware model.** A second forecast methodology
  that consumes EIA's day-ahead demand forecast for the requested BA
  and projects expected fuel mix by binning historical observations by
  *demand level* rather than only by clock pattern. New methodology
  label (e.g. `eia930-demand-binned-v1`). Both models continue to
  publish confidence and sample counts honestly.
- **Open Grid Emissions provider.** A second free public-data
  provider, slotted behind the existing `ProviderMetadata`/
  `ProviderCapability` model. Per-region availability is uneven; that
  must be surfaced (`nami status` already shows per-region freshness;
  it gains per-region provider availability).
- **Documented path for opt-in commercial providers.** The capability
  abstraction already supports `AverageCarbonForecast`,
  `MarginalEmissionsEstimate`, `MarginalEmissionsForecast`. A future
  contributor *could* implement an Electricity Maps or WattTime
  adapter behind a feature flag. The roadmap-level decision is that
  any such adapter is **opt-in only, never the default**, and the
  `RunReport` must label which provider/signal produced each number.
  This is documentation now; no code commitment.

### Non-goals for Phase D

- Sub-hourly scheduling claims. EIA-930 is hourly; we are hourly.
- Multi-region scheduling. Region *comparison* might appear as a
  read-only informational surface (`nami compare --regions
  CAISO,PJM`) — selecting *across* regions does not.
- Demoting free public data from the default. A commercial provider
  added in this phase or later runs alongside, not in place of, the
  EIA/OGE path.
- ML-flavored forecasting (state-space, ensemble) before the
  simpler demand-binned model has had real validation runs.

---

## Sequencing and exit criteria

Roughly: A → B → C → D, with the caveat that the data-quality work in
D can happen in parallel with C once A and B are in users' hands.

A reasonable exit criterion for "done with each phase":

| Phase | Done when |
|---|---|
| A | A first-time user can go from clone to first `nami run nightly` in under five minutes, without reading any docs beyond `nami init`. |
| B | `nami report summary --since 30d` answers "how often did `nami` defer, and what was the average improvement when it did?" from real reports on disk. |
| C | The `examples/` directory has a CI recipe, a cron recipe, and a `systemd.timer` recipe that each run unchanged against the released binary. |
| D | A `nami forecast` call shows both the historical-pattern *and* the demand-binned model side-by-side, with confidence and methodology labels distinct, and a documented `nami status` provider-availability matrix per region. |
