#!/usr/bin/env bash
#
# nami nightly carbon-aware job (cron wrapper)
#
# 1. Save this script somewhere like ~/.local/bin/nami-nightly.sh
#    and `chmod +x` it.
# 2. Put your `EIA_API_KEY` and optional `NAMI_REGION` in
#    ~/.config/nami/env (with `chmod 600`):
#
#        EIA_API_KEY=...
#        NAMI_REGION=MISO
#
# 3. Add the crontab entry from `crontab.example` next to this file.
#
# Behavior:
#   - Refreshes the cache once at the start (cheap).
#   - Runs the workload under `nami run --within 6h --duration 1h` so it
#     waits up to 6 hours for the lowest-carbon hour-aligned window.
#   - Auto-archives the RunReport to $XDG_STATE_HOME/nami/reports/ (the
#     nami default), where `nami report summary` can later aggregate
#     across runs.

set -euo pipefail

ENV_FILE="${NAMI_ENV_FILE:-$HOME/.config/nami/env}"
LOG_FILE="${NAMI_LOG_FILE:-$HOME/.local/state/nami/nightly.log}"

if [ ! -r "$ENV_FILE" ]; then
    echo "nami-nightly: cannot read env file $ENV_FILE" >&2
    exit 1
fi

# Source the env file in a subshell-safe way: only reads `KEY=value`
# lines, ignores comments and blanks. (Don't use `source` if the env
# file might contain arbitrary shell.)
set -a
# shellcheck disable=SC1090
. "$ENV_FILE"
set +a

mkdir -p "$(dirname "$LOG_FILE")"

# Preflight: fail fast if anything is missing. --strict means warnings
# (stale cache, unset key) also exit nonzero.
nami doctor --strict >>"$LOG_FILE" 2>&1

# Refresh once (cheap; no-op if you already refreshed today).
nami refresh >>"$LOG_FILE" 2>&1

# Run the actual workload. Edit the command after `--` to taste.
# --within 6h sits in the wait phase up to 6 hours; the wait is
# cancellable by SIGINT/SIGTERM/SIGHUP and never crosses the deadline.
exec nami run \
    --within 6h \
    --duration 1h \
    --log "$LOG_FILE" \
    -- /usr/bin/make -C "$HOME/projects/my-repo" integration-test
