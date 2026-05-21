# Integration recipes

Self-contained, copy-paste templates for wrapping existing scheduled
workloads with `nami`. Per the roadmap (`docs/product-roadmap.md` Phase
C), these are **documentation only** — there is no published GitHub
Action, no Python package, no Kubernetes operator. Each recipe composes
the released `nami` binary with tooling you already run, and is meant to
be copied into your own repo / cron / systemd config and edited.

## What's here

| File | When to use |
|---|---|
| [`github-action.yml`](github-action.yml) | A workload that already runs in GitHub Actions on a schedule (nightly CI, weekly docs build, periodic reindex). Best with **short** `--within` windows and `nami preview` for advisory PR comments — see the runner-cost caveat below. |
| [`cron/nami-nightly.sh`](cron/nami-nightly.sh) + [`cron/crontab.example`](cron/crontab.example) | A workload running on a Linux host where you already use `cron`. The simplest path, and the one where long `--within` windows actually make sense. |
| [`systemd/`](systemd/) | A workload running on a modern Linux host with `systemd`. More secure secret handling (`LoadCredential=`), better logging integration (`journalctl -u nami-nightly`), and Type=oneshot semantics that compose cleanly. |

## Caveats to read first

1. **Long `--within` windows on hosted CI runners are expensive.** GitHub-
   hosted runners bill by the minute; `nami run --within 8h` will sit on
   the runner for up to 8 hours waiting for the chosen window. For
   GitHub Actions, prefer either a short `--within 1h`, or use
   `nami preview` for advisory output (it computes the recommendation
   instantly and returns).

2. **`nami` is a single-region scheduler.** Region resolution is the
   usual chain (`--region` → `NAMI_REGION` → config file). In CI, set
   the region as a workflow input or a repo variable.

3. **`EIA_API_KEY` is a secret.** Never commit it. GitHub Actions: use
   `secrets.EIA_API_KEY`. systemd: use `LoadCredential=` pointed at a
   `0600` file under `/etc/nami/`. cron: source an env file with
   restricted permissions (`chmod 600`).

4. **Reports auto-archive.** Every `nami run` drops a JSON report into
   `$XDG_STATE_HOME/nami/reports/<UTC-date>/` (or the explicit
   `--report` / `--report-dir`). For CI you almost certainly want
   `--report` or `--report-dir` set to a workspace path so it can be
   uploaded as an artifact.

5. **`nami doctor --strict` as a preflight.** All three recipes run it
   before scheduling so a missing cache or stale data fails *before*
   the workload starts, not after.

## Where to publish, if you want

Nothing in this directory is published anywhere. If your team finds one
of these recipes valuable and wants a versioned, distributable form, the
project's stance (per `docs/project-brief.md` non-goals) is that
external repositories or packages are the *user's* call — `nami` itself
won't take on a Python package, a JS-side action, or a Kubernetes
operator to maintain. The shipped binary is the deliverable.
