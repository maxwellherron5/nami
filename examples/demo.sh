#!/usr/bin/env bash
#
# End-to-end nami demo: a real scikit-learn training run governed by the
# schedule nami generates from live EIA-930 grid data.
#
#   ./examples/demo.sh
#
# Requires EIA_API_KEY in the environment or in ./.env (free key:
# https://www.eia.gov/opendata/register.php). Honesty note: this demo
# does NOT fake a deferral. `nami preview` is run with a realistic 6h
# deadline so you see the *actual* scheduling decision over real grid
# data (defer to a cleaner hour, or run-now if nothing materially
# cleaner exists). `nami run` then uses a deliberately short deadline so
# the training actually starts within minutes — the wrap/decide/execute/
# report mechanism is identical either way.

set -euo pipefail
cd "$(dirname "$0")/.."   # workspace root

BIN=target/debug/nami
[ -x "$BIN" ] || cargo build -q -p nami-cli

if [ -z "${EIA_API_KEY:-}" ] && [ -f .env ]; then
    set -a; . ./.env; set +a
fi
: "${EIA_API_KEY:?set EIA_API_KEY (or put it in ./.env) to run the demo}"

iso_in() {  # $1 = timedelta expression (minutes); prints RFC3339 UTC
    python3 -c "import datetime,sys; print((datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(minutes=int(sys.argv[1]))).strftime('%Y-%m-%dT%H:%M:%SZ'))" "$1"
}

echo "==> 1/5  nami refresh — live EIA-930, CAISO, 8 weeks"
"$BIN" refresh --region CAISO --weeks 8

echo; echo "==> 2/5  nami status — cache freshness & data sources"
"$BIN" status

echo; echo "==> 3/5  nami forecast — historical-pattern curve, next 12h"
"$BIN" forecast --region CAISO --horizon 12h

PREVIEW_DEADLINE=$(iso_in 360)   # +6h: realistic flexibility
echo; echo "==> 4/5  nami preview — REAL scheduling decision (6h deadline, no execution)"
"$BIN" preview --region CAISO --deadline "$PREVIEW_DEADLINE" --duration 1m \
    -- python3 examples/sklearn_train.py

RUN_DEADLINE=$(iso_in 10)        # +10m: bounded so training starts promptly
REPORT="${TMPDIR:-/tmp}/nami-demo-report.json"
echo; echo "==> 5/5  nami run — training executes under nami (10m deadline), report -> $REPORT"
# nami exits with the child's code; don't let errexit abort before we
# capture it (a failed/refused run is still a valid demo outcome).
set +e
"$BIN" run --region CAISO --deadline "$RUN_DEADLINE" --duration 1m \
    --report "$REPORT" -- python3 examples/sklearn_train.py
RUN_RC=$?
set -e

echo; echo "==> run report summary"
"$BIN" status --report "$REPORT" | sed -n '/Run report/,$p'
echo; echo "nami exit code (propagated from the training process): $RUN_RC"
