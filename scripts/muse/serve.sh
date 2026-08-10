#!/usr/bin/env bash
set -euo pipefail

MUSE_HOME="${SYNTH_MUSE_HOME:-$HOME/.synth-desktop/muse}"
SERVER="$MUSE_HOME/runtime/llama-b10342/llama-server"
MODEL_DIR="${SYNTH_MUSE_MODEL_PATH:-$HOME/.synth-desktop/models/meta-models/Muse-Glimmer-30B-GGUF}"
MODEL="$MODEL_DIR/muse-glimmer-30B-kquant-17gb.gguf"
MMPROJ="$MODEL_DIR/mmproj-kquant.gguf"
DRAFT="$MODEL_DIR/dflash-kquant.gguf"

[[ -x "$SERVER" ]] || { echo "[muse:serve] managed llama.cpp runtime is missing; repair it from Settings > Models" >&2; exit 1; }
[[ -f "$MODEL" ]] || { echo "[muse:serve] 4-bit model not found at $MODEL" >&2; exit 1; }
[[ -f "$MMPROJ" ]] || { echo "[muse:serve] vision projector not found at $MMPROJ" >&2; exit 1; }
[[ -f "$DRAFT" ]] || { echo "[muse:serve] DFlash draft not found at $DRAFT" >&2; exit 1; }

exec "$SERVER" \
  --model "$MODEL" \
  --mmproj "$MMPROJ" \
  --model-draft "$DRAFT" \
  --spec-type draft-dflash \
  --alias meta-models/Muse-Glimmer-30B-GGUF \
  --host 127.0.0.1 \
  --port "${SYNTH_MUSE_PORT:-7334}" \
  --ctx-size "${SYNTH_MUSE_CONTEXT_LENGTH:-131072}" \
  --temp 1.0 \
  --top-p 0.95 \
  --top-k 64 \
  --n-gpu-layers 999 \
  --n-gpu-layers-draft 999
