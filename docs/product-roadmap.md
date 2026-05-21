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

- **`nami.toml` profiles — shipped.** Named profiles live under
  `[profiles.<name>]` in the existing nami config file
  (`$NAMI_CONFIG` / `$XDG_CONFIG_HOME/nami/config.toml` / `~/.config/
  nami/config.toml`), alongside the existing `region = "<BA>"` key.
  Fields: `region`, `duration`, `within` (= `now + within`) or
  `deadline` (RFC 3339), `command`. Usage:
  `nami preview --profile nightly`, `nami run --profile nightly`.
  CLI flags override profile fields; each profile-sourced field is
  announced on stderr. `materiality_threshold_pct` and repo-local
  `./nami.toml` discovery are deliberate future additions, not yet
  shipped.
- **Relative deadlines — shipped.** `--within 8h`, `--by 7am`,
  `--by tomorrow-9am`, `--by 19:30` alongside the absolute
  `--deadline <RFC 3339>`. The three flags are mutually exclusive
  (enforced at the clap layer). Parsing happens at the CLI boundary;
  internally everything stays UTC `OffsetDateTime` and the
  `RunReport`'s `deadline` is still RFC 3339. The resolved instant is
  echoed back on stderr ("`nami: deadline … (from --by 7am) [UTC]`")
  so the user can verify. `--by` is **interpreted as UTC** —
  `time::UtcOffset::current_local_offset` is unsound under tokio's
  multi-threaded runtime, and silently guessing a timezone is
  off-character. Non-UTC interpretations: use `--deadline` with an
  explicit RFC 3339 offset.
- **`nami init` — shipped.** Writes a minimal config file with the
  chosen `region` and a commented-out example profile at the same path
  the region resolver reads from. Refuses to clobber existing files
  without `--force`; `--dry-run` prints what would be written; atomic
  write via temp file + rename. After writing, prints a brief checklist
  (eGRID table, `EIA_API_KEY` presence, per-region cache) with concrete
  next-step commands. Deliberately does **not** run `nami refresh`
  itself — surprise network calls on first contact are out of character
  for this tool. An interactive prompt mode for the region pick is a
  future addition.
- **`nami doctor` — shipped.** Walks preconditions (region resolves
  via the regular chain, eGRID factor table loads, `EIA_API_KEY` set,
  historical cache present and not stale for the resolved region) with
  explicit `ok` / `warn` / `fail` tagging and a concrete fix line per
  failing check. Exits nonzero on any `fail`; `--strict` also exits
  nonzero on `warn` — useful as a CI preflight gate. Composes the same
  region resolver and cache/freshness logic the scheduling path uses,
  so a green `nami doctor` means `nami preview` / `nami run` will not
  fall back due to a missing precondition.
- **Friendlier output — shipped.** Shell completions via `nami
  completions <shell>` (bash / zsh / fish / powershell / elvish),
  derived from the live clap tree so they stay in sync with new
  subcommands automatically. Refusal messages were rewritten along
  the way to lead with the user's next action (the region-resolver
  "no region: pass --region, set NAMI_REGION, or …" message is the
  archetype; `nami doctor` extends the same pattern to every
  precondition with an explicit `→ <fix command>` line).

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

- **Reports directory convention — shipped.** `nami run` auto-archives
  every `RunReport` to `$XDG_STATE_HOME/nami/reports/<UTC-date>/<auto>.json`
  (else `$HOME/.local/state/nami/reports/...`); the path is announced on
  stderr. `--report <path>` keeps pinning a single file (preferred for
  CI artifacts); `--report-dir <dir>` overrides only the directory and
  is mutually exclusive with `--report`. Filenames are
  `<HH-MM-SS-nanos>-<BA>.json` — sortable, region-tagged, collision-
  free without retry logic. Writes are atomic (temp-file + rename) so a
  crashed run can't poison the directory `nami report summary` will
  later aggregate over. `nami preview` does not auto-archive (Phase B
  aggregations are about *actual* runs, not informational previews).
- **`nami report summary --since 30d` — shipped.** Walks the auto-
  archived reports directory (date-partitioned, so a `--since 30d` walk
  doesn't deserialize months we discard), filters by `--region` if
  given, then aggregates: decisions (deferred / run-immediately /
  refused), improvement statistics when deferred (mean / median /
  range), confidence distribution, per-region counts, top refusal
  reasons. Corrupt JSONs are counted and skipped rather than failing
  the whole walk — a single bad file can't poison the aggregation.
  Human-readable output by default; `--json` emits a stable schema
  for scripts.
- **`nami report explain <report.json>` — shipped.** Renders the
  decision in prose: branches on `StartAt` / `StartImmediately` /
  `Refuse` so each outcome gets its own framing ("why did nami defer
  / run now / refuse?"), then pulls the materiality threshold,
  estimated improvement, run-now baseline, confidence level + sample
  count, freshness, provider, methodology, and warnings into the
  story. Placed under `nami report explain` (not top-level) for
  cohesion with `nami report summary` — both are operations on
  reports.

### Non-goals for Phase B

- A daemon, a database, or a web UI. Reports are flat files; summaries
  are computed on demand.
- Phoning home / aggregating across users. Everything stays local.

---

## Phase C — Integration recipes — shipped

**Why:** `nami` becomes meaningfully more useful when wired into
existing workflows (CI runners, cron, systemd timers). The wrong way
to ship that is to take on a Python package, a Kubernetes operator,
or a JS-side action repo we'd have to maintain. The *right* way is
**documented recipes** that compose the already-released binary with
tools the user already runs. All three live under
[`examples/integrations/`](../examples/integrations/).

### Scope (all documentation, no new packages)

- **GitHub Actions example — shipped.** `examples/integrations/github-
  action.yml` — nightly workflow with `EIA_API_KEY` as a repo secret,
  `nami doctor --strict` as a preflight, and two mutually-exclusive
  modes (advisory `nami preview` for PR visibility, or scheduled `nami
  run --within 1h` for actual deferral with the GitHub-runner cost
  caveat called out).
- **`cron` recipe — shipped.** `examples/integrations/cron/
  nami-nightly.sh` + `crontab.example` — a wrapper script that sources
  an env file, runs `nami doctor --strict` + `refresh` + `run
  --within 6h`, and logs to a state file. Cron is where long
  `--within` windows actually make sense (no per-minute billing).
- **`systemd.timer` recipe — shipped.** `examples/integrations/
  systemd/` — paired `nami-refresh.{service,timer}` and `nami-nightly.
  {service,timer}` units, with `LoadCredential=` for the `EIA_API_KEY`
  (modern systemd secret pattern — the key never lives in the unit
  file) and `Persistent=true` timers that catch up after sleep.

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
| B | ~~`nami report summary --since 30d` answers "how often did `nami` defer, and what was the average improvement when it did?" from real reports on disk.~~ **Shipped.** Reports auto-archive; `nami report summary` aggregates; `nami report explain` narrates a single decision. |
| C | ~~The `examples/` directory has a CI recipe, a cron recipe, and a `systemd.timer` recipe that each run unchanged against the released binary.~~ **Shipped** — `examples/integrations/`. |
| D | A `nami forecast` call shows both the historical-pattern *and* the demand-binned model side-by-side, with confidence and methodology labels distinct, and a documented `nami status` provider-availability matrix per region. |
