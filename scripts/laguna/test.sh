#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VENV="${SYNTH_LAGUNA_VENV:-$HOME/.synth-desktop/laguna/.venv}"
export PYTHONPATH="$ROOT/services/laguna-daemon${PYTHONPATH:+:$PYTHONPATH}"
if [[ -x "$VENV/bin/python" ]]; then
  exec "$VENV/bin/python" -m unittest discover -s "$ROOT/services/laguna-daemon/tests" -v
fi
exec python3 -m unittest discover -s "$ROOT/services/laguna-daemon/tests" -v
