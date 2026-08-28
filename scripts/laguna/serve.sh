#!/usr/bin/env bash
# Independent Synth Laguna sidecar (Poolside-compatible OpenAI API).
# Does NOT use Poolside.app — only optionally reuses weights under models-dir.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VENV="${SYNTH_LAGUNA_VENV:-$HOME/.synth-desktop/laguna/.venv}"

# Default: reuse Poolside weight layout if present (independent process)
if [[ -z "${SYNTH_LAGUNA_MODELS_DIR:-}" ]]; then
  if [[ -d "$HOME/.config/poolside/models/poolside/Laguna-XS-2.1-NVFP4-mlx" ]]; then
    export SYNTH_LAGUNA_MODELS_DIR="$HOME/.config/poolside/models"
  else
    export SYNTH_LAGUNA_MODELS_DIR="$HOME/.synth-desktop/models"
  fi
fi

if [[ -f "$HOME/.synth-desktop/laguna/env.sh" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.synth-desktop/laguna/env.sh"
fi

if [[ -x "$VENV/bin/python" ]]; then
  PYTHON="$VENV/bin/python"
else
  PYTHON="${SYNTH_PYTHON:-python3}"
fi

export SYNTH_LAGUNA_HOST="${SYNTH_LAGUNA_HOST:-127.0.0.1}"
export SYNTH_LAGUNA_PORT="${SYNTH_LAGUNA_PORT:-7333}"
export SYNTH_LAGUNA_BACKEND="${SYNTH_LAGUNA_BACKEND:-mlx_lm}"
export SYNTH_LAGUNA_AUTO_LOAD="${SYNTH_LAGUNA_AUTO_LOAD:-1}"
export SYNTH_LAGUNA_REQUIRE_AUTH="${SYNTH_LAGUNA_REQUIRE_AUTH:-1}"
export PYTHONPATH="$ROOT/services/laguna-daemon${PYTHONPATH:+:$PYTHONPATH}"

exec "$PYTHON" -m laguna_daemon \
  --host "$SYNTH_LAGUNA_HOST" \
  --port "$SYNTH_LAGUNA_PORT" \
  --models-dir "$SYNTH_LAGUNA_MODELS_DIR" \
  --default-model "${SYNTH_LAGUNA_DEFAULT_MODEL:-poolside/Laguna-XS-2.1-NVFP4-mlx}" \
  ${SYNTH_LAGUNA_API_KEY:+--api-key "$SYNTH_LAGUNA_API_KEY"} \
  --backend "$SYNTH_LAGUNA_BACKEND" \
  "$@"
