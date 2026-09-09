#!/usr/bin/env bash
# Local Workshop packaged build. v0.10 has one production feature envelope.
#
#   scripts/build-tier.sh stable [--debug]
# The historical command name is retained for build.sh compatibility. Do not
# resurrect removed tier-* Cargo flags or imply different feature envelopes.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/mcp-adapters.sh"
APP_DIR="$ROOT/apps/synth_desktop"
OUT_ROOT="$ROOT/work/tier-builds"

REQUESTED="${1:-}"
PROFILE_FLAG=""
PROFILE="release"
[[ "${2:-}" == "--debug" ]] && { PROFILE_FLAG="--debug"; PROFILE="debug"; }

case "$REQUESTED" in
  stable) TIERS=(stable) ;;
  *) echo "usage: scripts/build-tier.sh stable [--debug] (v0.10 no longer has tier feature flags)" >&2; exit 2 ;;
esac
[[ $# -le 2 && ( $# -lt 2 || "$2" == --debug ) ]] || { echo "expected only optional --debug" >&2; exit 2; }

# Shared packaged resources (same for every tier); stage once. Resolve the
# MLX release checkout by the catalog revision, not by a historical name.
REPO_SIBLING_ROOT="$(dirname "$ROOT")"
if [[ -z "${SYNTH_MLX_RL_PROJECT_ROOT:-}" ]]; then
  mlx_pin="$(rg -o 'MLX_RUNTIME_SOURCE_REVISION: &str = "([0-9a-f]{40})"' --replace '$1' -m1 \
    "$APP_DIR/src-tauri/src/optimizers/mlx_runtime.rs")"
  for candidate in synth-mlx-rl synth-mlx-rl-v08-pinned synth-mlx-rl-v08-compat; do
    if [[ -f "$REPO_SIBLING_ROOT/$candidate/pyproject.toml" \
      && "$(git -C "$REPO_SIBLING_ROOT/$candidate" rev-parse HEAD)" == "$mlx_pin" ]]; then
      export SYNTH_MLX_RL_PROJECT_ROOT="$REPO_SIBLING_ROOT/$candidate"
      break
    fi
  done
fi
python3 "$ROOT/scripts/stage-trace-runtime.py" --containers "${SYNTH_CONTAINERS_PROJECT_ROOT:-$ROOT/../containers}"
"$ROOT/scripts/stage-mlx-runtime-distribution.sh"
"$ROOT/scripts/stage-optimizer-runtime-distribution.sh"
"$ROOT/scripts/build-browser-runtime.sh" assemble
if [[ ! -x "$ROOT/services/victoria-logs/victoria-logs" ]]; then
  "$ROOT/scripts/diagnostics/fetch-victorialogs.sh"
fi

TARGET_ROOT="${CARGO_TARGET_DIR:-$APP_DIR/src-tauri/target}"

title_case() { printf '%s' "$(tr '[:lower:]' '[:upper:]' <<<"${1:0:1}")${1:1}"; }

build_one() {
  local tier="$1" product identifier overlay bundle_dir out_dir
  if [[ "$tier" == "stable" ]]; then
    product="Synth Workshop"
    identifier="com.synth.desktop"
  else
    product="Synth Workshop $(title_case "$tier")"
    identifier="com.synth.desktop.$tier"
  fi
  overlay="$(mktemp -t "workshop-tier-$tier.XXXXXX.json")"
  printf '{"productName": "%s", "identifier": "%s"}\n' "$product" "$identifier" >"$overlay"

  echo "[build-tier] $tier ($PROFILE): $product · $identifier"
  (
    cd "$APP_DIR"
    npx tauri build $PROFILE_FLAG \
      --bundles app \
      --config src-tauri/tauri.package.json \
      --config "$overlay"
  )
  rm -f "$overlay"

  bundle_dir="$TARGET_ROOT/$PROFILE/bundle/macos"
  [[ -d "$bundle_dir/$product.app" ]] || { echo "[build-tier] expected bundle missing: $bundle_dir/$product.app" >&2; exit 1; }
  # Tauri's main app alone does not prove the agent adapters were built/copied.
  local cargo_profile=()
  [[ "$PROFILE" == release ]] && cargo_profile=(--release)
  cargo build --locked --manifest-path "$APP_DIR/src-tauri/Cargo.toml" \
    --bins ${cargo_profile[@]+"${cargo_profile[@]}"}
  local adapter
  for adapter in "${SYNTH_MCP_ADAPTERS[@]}"; do
    [[ -x "$TARGET_ROOT/$PROFILE/$adapter" ]] || { echo "missing adapter: $adapter" >&2; exit 1; }
    ditto "$TARGET_ROOT/$PROFILE/$adapter" "$bundle_dir/$product.app/Contents/MacOS/$adapter"
  done
  # Preserve Chromium framework symlinks and verify actual packaged startup.
  "$ROOT/scripts/finalize-browser-app.sh" "$bundle_dir/$product.app"
  out_dir="$OUT_ROOT/$tier"
  rm -rf "$out_dir"
  mkdir -p "$out_dir"
  # ditto preserves signatures and resource forks; cp -R can break codesign.
  ditto "$bundle_dir/$product.app" "$out_dir/$product.app"
  python3 - "$out_dir/manifest.json" <<PYEOF
import json, subprocess, sys
from pathlib import Path
from datetime import datetime, timezone
commit = subprocess.run(["git", "-C", "$ROOT", "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
dirty = bool(subprocess.run(["git", "-C", "$ROOT", "status", "--porcelain"], capture_output=True, text=True).stdout.strip())
if not commit:
    export_manifest = Path("$ROOT") / "PUBLIC_EXPORT_MANIFEST.json"
    if export_manifest.is_file():
        commit = json.loads(export_manifest.read_text(encoding="utf-8")).get("source", {}).get("commit", "")
json.dump({
    "tier": "$tier",
    "productName": "$product",
    "identifier": "$identifier",
    "profile": "$PROFILE",
    "commit": commit or "unknown",
    "treeDirty": dirty,
    "builtAt": datetime.now(timezone.utc).isoformat(),
}, open(sys.argv[1], "w"), indent=2)
PYEOF
  echo "[build-tier] staged $out_dir/$product.app"
}

for tier in "${TIERS[@]}"; do
  build_one "$tier"
done

echo "[build-tier] done:"
for tier in "${TIERS[@]}"; do
  cat "$OUT_ROOT/$tier/manifest.json"
  echo
done
