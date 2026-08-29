#!/usr/bin/env bash
# Local Workshop builds at a chosen maturity tier (contracts/release-tiers-v1.toml).
#
#   scripts/build-tier.sh <core|stable|beta|alpha|dev|all> [--debug]
#
# One command keeps the two envelope knobs aligned: the host compiles with
# --features tier-<tier> and the renderer bundle compiles the same tier via
# the WORKSHOP_TIER Vite define. `all` builds the four channel tiers
# (stable, beta, alpha, dev) sequentially into work/tier-builds/<tier>/,
# each with a tier-suffixed product name and bundle identifier so they
# install side by side; stable keeps the canonical identity. Every produced
# app carries a manifest.json binding it to the tier and source revision,
# and reports its own envelope in Settings → Build.
#
# core is a durability classification more than a channel; it builds
# individually (cargo needs --no-default-features for it) but is not part
# of `all`.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$ROOT/apps/synth_desktop"
OUT_ROOT="$ROOT/work/tier-builds"

REQUESTED="${1:-}"
PROFILE_FLAG=""
PROFILE="release"
[[ "${2:-}" == "--debug" ]] && { PROFILE_FLAG="--debug"; PROFILE="debug"; }

case "$REQUESTED" in
  core|stable|beta|alpha|dev) TIERS=("$REQUESTED") ;;
  all) TIERS=(stable beta alpha dev) ;;
  *) echo "usage: scripts/build-tier.sh <core|stable|beta|alpha|dev|all> [--debug]" >&2; exit 2 ;;
esac

# Shared packaged resources (same for every tier); stage once. Resolve the
# MLX release checkout the same way desktop-instance.sh does: prefer the
# pinned v0.8 siblings over a possibly-dirty synth-mlx-rl working copy.
REPO_SIBLING_ROOT="$(dirname "$ROOT")"
if [[ -z "${SYNTH_MLX_RL_PROJECT_ROOT:-}" ]]; then
  for candidate in synth-mlx-rl-v08-compat synth-mlx-rl-v08-pinned synth-mlx-rl; do
    if [[ -f "$REPO_SIBLING_ROOT/$candidate/pyproject.toml" ]]; then
      export SYNTH_MLX_RL_PROJECT_ROOT="$REPO_SIBLING_ROOT/$candidate"
      break
    fi
  done
fi
"$ROOT/scripts/stage-mlx-runtime-distribution.sh"
"$ROOT/scripts/stage-optimizer-runtime-distribution.sh"
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
  local features="tier-$tier" extra=()
  # tier-core sits below the default features (tier-stable); everything else
  # only widens them, so the default chain is harmless there.
  [[ "$tier" == "core" ]] && extra=(-- --no-default-features)
  (
    cd "$APP_DIR"
    WORKSHOP_TIER="$tier" npx tauri build $PROFILE_FLAG \
      --features "$features" \
      --bundles app \
      --config src-tauri/tauri.package.json \
      --config "$overlay" \
      ${extra[@]+"${extra[@]}"}
  )
  rm -f "$overlay"

  bundle_dir="$TARGET_ROOT/$PROFILE/bundle/macos"
  [[ -d "$bundle_dir/$product.app" ]] || { echo "[build-tier] expected bundle missing: $bundle_dir/$product.app" >&2; exit 1; }
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
