#!/usr/bin/env bash
# Run the Muse Glimmer engine by hand.
#
# Synth Desktop does not use this script: it spawns llama-server directly (see
# `spawn_muse_engine` in apps/synth_desktop/src-tauri/src/laguna.rs), because
# the installed app ships no checkout to read a script from. The two argument
# lists must stay in agreement — change one, change the other.
#
# The engine is a backend for the Laguna daemon on :7333, never a destination
# for Codex or Desktop. Point clients at :7333.
set -euo pipefail

MUSE_HOME="${SYNTH_MUSE_HOME:-$HOME/.synth-desktop/muse}"
SERVER="$MUSE_HOME/runtime/llama-dd1ea524333b1e697489067d7a4c39c60d32beee/llama-server"
MODEL_DIR="${SYNTH_MUSE_MODEL_PATH:-$HOME/.synth-desktop/models/meta-models/Muse-Glimmer-30B-GGUF}"
MODEL="$MODEL_DIR/muse-glimmer-30B-kquant-17gb.gguf"
MMPROJ="$MODEL_DIR/mmproj-kquant.gguf"
DRAFT="$MODEL_DIR/dflash-kquant.gguf"

[[ -x "$SERVER" ]] || { echo "[muse:serve] managed llama.cpp runtime is missing; repair it from Settings > Models" >&2; exit 1; }
[[ -f "$MODEL" ]] || { echo "[muse:serve] 4-bit model not found at $MODEL" >&2; exit 1; }
[[ -f "$MMPROJ" ]] || { echo "[muse:serve] vision projector not found at $MMPROJ" >&2; exit 1; }
[[ -f "$DRAFT" ]] || { echo "[muse:serve] DFlash draft not found at $DRAFT" >&2; exit 1; }

# Optional, and what Desktop always does: guard the engine with the same bearer
# token as the daemon so no other local process can reach the weights directly.
AUTH=()
if [[ -n "${SYNTH_MUSE_API_KEY:-}" ]]; then
  AUTH=(--api-key "$SYNTH_MUSE_API_KEY")
fi

exec "$SERVER" \
  --model "$MODEL" \
  --mmproj "$MMPROJ" \
  --model-draft "$DRAFT" \
  --spec-type draft-dflash \
  --alias meta-models/Muse-Glimmer-30B-GGUF \
  --host 127.0.0.1 \
  --port "${SYNTH_MUSE_PORT:-7334}" \
  --ctx-size "${SYNTH_MUSE_CONTEXT_LENGTH:-131072}" \
  --jinja \
  --reasoning-format deepseek \
  "${AUTH[@]}" \
  --temp 1.0 \
  --top-p 0.95 \
  --top-k 64 \
  --n-gpu-layers 999 \
  --n-gpu-layers-draft 999
