#!/usr/bin/env bash
# Run the bounded native-MLX reliability soak against a live Laguna daemon.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BASE_URL="${SYNTH_LAGUNA_BASE_URL:-http://127.0.0.1:7333}"
DATA_DIR="${SYNTH_LAGUNA_DATA_DIR:-$HOME/.synth-desktop/laguna}"
VENV="${SYNTH_LAGUNA_VENV:-$DATA_DIR/.venv}"

if [[ ${1:-} == http://* || ${1:-} == https://* ]]; then
  BASE_URL="$1"
  shift
fi

if [[ -z ${SYNTH_LAGUNA_API_KEY:-} && -f "$DATA_DIR/api_key" ]]; then
  export SYNTH_LAGUNA_API_KEY="$(tr -d '\n' <"$DATA_DIR/api_key")"
fi

PYTHON="$VENV/bin/python"
[[ -x "$PYTHON" ]] || PYTHON="python3"

exec "$PYTHON" "$ROOT/services/laguna-daemon/scripts/soak_mlx.py" \
  --base-url "$BASE_URL" "$@"
