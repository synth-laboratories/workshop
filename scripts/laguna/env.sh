#!/usr/bin/env bash
# Source this before starting Synth Desktop so local Laguna is used.
#   source scripts/laguna/env.sh
export SYNTH_LAGUNA_BASE_URL="${SYNTH_LAGUNA_BASE_URL:-http://127.0.0.1:7333}"
export SYNTH_LAGUNA_BACKEND="${SYNTH_LAGUNA_BACKEND:-mlx_lm}"
export SYNTH_LAGUNA_MODEL="${SYNTH_LAGUNA_MODEL:-poolside/Laguna-XS-2.1-NVFP4-mlx}"
export SYNTH_LAGUNA_REVISION="${SYNTH_LAGUNA_REVISION:-841778bda563a36104dd521e37d99218e46f4f25}"
echo "[laguna] SYNTH_LAGUNA_BASE_URL=$SYNTH_LAGUNA_BASE_URL"
