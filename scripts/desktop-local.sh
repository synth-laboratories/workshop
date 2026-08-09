#!/usr/bin/env bash
# Launch Synth Desktop against real local Laguna on the Mac host.
#
# Vanilla mlx_lm cannot load model_type=laguna. Our sidecar (:7333) matches
# Poolside's OpenAI API and, by default, proxies to Poolside's MLX binary
# (already running with weights loaded) via backend=external. Swap upstream
# later for mere-run / Arena without changing Desktop.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LAGUNA_HOME="${HOME}/.synth-desktop/laguna"

if [[ -f "$LAGUNA_HOME/env.sh" ]]; then
  # shellcheck disable=SC1091
  source "$LAGUNA_HOME/env.sh"
fi

export SYNTH_LAGUNA_BASE_URL="${SYNTH_LAGUNA_BASE_URL:-http://127.0.0.1:7333}"
if [[ -z "${SYNTH_LAGUNA_API_KEY:-}" && -f "$LAGUNA_HOME/api_key" ]]; then
  export SYNTH_LAGUNA_API_KEY="$(tr -d '\n' <"$LAGUNA_HOME/api_key")"
fi

if ! curl -sf -H "Authorization: Bearer ${SYNTH_LAGUNA_API_KEY}" \
  "$SYNTH_LAGUNA_BASE_URL/health" >/dev/null; then
  echo "Laguna sidecar not healthy at $SYNTH_LAGUNA_BASE_URL"
  echo "Start it with:  cd $ROOT && ./scripts/laguna/serve.sh"
  echo "(Keep Poolside.app's MLX sidecar running if using backend=external.)"
  exit 1
fi

# Prefer an already-running compose/host runtime if healthy; else Tauri starts one.
if [[ -n "${SYNTH_RUNTIME_URL:-}" ]]; then
  :
elif curl -sf -H "Authorization: Bearer ${SYNTH_RUNTIME_TOKEN:-dev-runtime-token}" \
  "http://127.0.0.1:8765/v1/health" >/dev/null 2>&1; then
  export SYNTH_RUNTIME_URL="http://127.0.0.1:8765"
  export SYNTH_RUNTIME_TOKEN="${SYNTH_RUNTIME_TOKEN:-dev-runtime-token}"
fi

echo "[desktop-local] laguna=$SYNTH_LAGUNA_BASE_URL runtime=${SYNTH_RUNTIME_URL:-spawn}"
cd "$ROOT"
exec npm run dev --workspace @synth/synth-desktop
