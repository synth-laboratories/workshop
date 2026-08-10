#!/usr/bin/env bash
# Run the live MLX integration suite against a running Laguna daemon.
#
#   ./scripts/laguna/live_test.sh                    # default daemon on :7333
#   ./scripts/laguna/live_test.sh http://127.0.0.1:7335
#
# The suite exercises real weights: expect it to take minutes and to hold the
# single GPU slot throughout. Do not run it against a daemon another Synth
# instance is using — drain or assign one first.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BASE_URL="${SYNTH_LAGUNA_LIVE_BASE_URL:-http://127.0.0.1:7333}"
if [[ ${1:-} == http://* || ${1:-} == https://* ]]; then
  BASE_URL="$1"
  shift
fi
VENV="${SYNTH_LAGUNA_VENV:-$HOME/.synth-desktop/laguna/.venv}"
DATA_DIR="${SYNTH_LAGUNA_DATA_DIR:-$HOME/.synth-desktop/laguna}"

# Read the daemon's own key file rather than scraping it from a process list,
# where it would end up in shell history and any captured report.
API_KEY="${SYNTH_LAGUNA_LIVE_API_KEY:-${SYNTH_LAGUNA_API_KEY:-}}"
if [[ -z "$API_KEY" && -f "$DATA_DIR/api_key" ]]; then
  API_KEY="$(tr -d '\n' <"$DATA_DIR/api_key")"
fi

if ! curl -sf -m 10 -H "Authorization: Bearer $API_KEY" "$BASE_URL/health" >/dev/null; then
  echo "No healthy Laguna daemon at $BASE_URL" >&2
  echo "Start one with ./scripts/laguna/serve.sh, then re-run." >&2
  exit 1
fi

PYTHON="$VENV/bin/python"
[[ -x "$PYTHON" ]] || PYTHON="python3"

export PYTHONPATH="$ROOT/services/laguna-daemon${PYTHONPATH:+:$PYTHONPATH}"
export SYNTH_LAGUNA_LIVE_BASE_URL="$BASE_URL"
export SYNTH_LAGUNA_LIVE_API_KEY="$API_KEY"
export SYNTH_LAGUNA_LIVE_REPORT="${SYNTH_LAGUNA_LIVE_REPORT:-${TMPDIR:-/tmp}/laguna-live-report.json}"

echo "Running the live MLX suite against $BASE_URL"
echo "Measurements will be written to $SYNTH_LAGUNA_LIVE_REPORT"
if (($#)); then
  tests=()
  for test_name in "$@"; do
    if [[ "$test_name" == tests.* ]]; then
      tests+=("$test_name")
    else
      tests+=("tests.integration.test_live_mlx.$test_name")
    fi
  done
else
  tests=(tests.integration.test_live_mlx)
fi

exec "$PYTHON" -m unittest -v "${tests[@]}"
