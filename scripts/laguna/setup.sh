#!/usr/bin/env bash
# Install Laguna daemon deps and download NVFP4 MLX weights (mlxfast v2 reference).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DAEMON="$ROOT/services/laguna-daemon"
VENV="${SYNTH_LAGUNA_VENV:-$HOME/.synth-desktop/laguna/.venv}"
MODEL="${SYNTH_LAGUNA_MODEL:-poolside/Laguna-XS-2.1-NVFP4-mlx}"
REVISION="${SYNTH_LAGUNA_REVISION:-841778bda563a36104dd521e37d99218e46f4f25}"

mkdir -p "$(dirname "$VENV")" "$HOME/.synth-desktop/laguna"

echo "[laguna:setup] workshop=$ROOT"
echo "[laguna:setup] venv=$VENV"
echo "[laguna:setup] model=$MODEL@$REVISION"

if ! command -v uv >/dev/null 2>&1; then
  echo "uv is required (https://github.com/astral-sh/uv)" >&2
  exit 1
fi

uv venv "$VENV"
# shellcheck disable=SC1091
source "$VENV/bin/activate"

uv pip install -e "$DAEMON"
if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]]; then
  echo "[laguna:setup] installing mlx + mlx-lm extras…"
  uv pip install "mlx>=0.26" "mlx-lm>=0.26" "huggingface-hub>=0.26"
else
  echo "[laguna:setup] non-Apple-Silicon — skipping mlx extras (mock backend only)"
fi

# Verify mlx import before downloading multi-GB weights
if ! python -c "import mlx, mlx_lm" 2>/dev/null; then
  echo "[laguna:setup] mlx/mlx_lm import failed — aborting weight download" >&2
  exit 1
fi

python - <<PY
from huggingface_hub import snapshot_download
import os
model = os.environ.get("SYNTH_LAGUNA_MODEL", "$MODEL")
revision = os.environ.get("SYNTH_LAGUNA_REVISION", "$REVISION")
print(f"[laguna:setup] downloading {model}@{revision} (≈21.6 GB)…", flush=True)
path = snapshot_download(
    repo_id=model,
    revision=revision,
)
print(f"[laguna:setup] ready at {path}", flush=True)
PY

cat > "$HOME/.synth-desktop/laguna/env.sh" <<EOF
export SYNTH_LAGUNA_BASE_URL="http://127.0.0.1:7333"
export SYNTH_LAGUNA_BACKEND="mlx_lm"
export SYNTH_LAGUNA_MODEL="$MODEL"
export SYNTH_LAGUNA_REVISION="$REVISION"
export SYNTH_LAGUNA_AUTO_LOAD="1"
export PATH="$VENV/bin:\$PATH"
EOF

echo
echo "[laguna:setup] done."
echo "  1) ./scripts/laguna/serve.sh"
echo "  2) source ~/.synth-desktop/laguna/env.sh"
echo "  3) npm run dev --workspace @synth/synth-desktop"
