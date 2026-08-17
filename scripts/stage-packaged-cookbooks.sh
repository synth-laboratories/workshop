#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOURCE_ROOT="${SYNTH_COOKBOOKS_SOURCE_ROOT:-}"
if [[ -z "$SOURCE_ROOT" ]]; then
  SOURCE_ROOT="$(dirname "$ROOT")/synth-cookbooks-public"
  git_common_dir="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
  primary_sibling=""
  if [[ -n "$git_common_dir" ]]; then
    primary_sibling="$(dirname "$(dirname "$git_common_dir")")/synth-cookbooks-public"
  fi
  if [[ ! -d "$SOURCE_ROOT/cookbooks/optimizers/gepa" && -n "$primary_sibling" ]]; then
    SOURCE_ROOT="$primary_sibling"
  fi
fi
DESTINATION="$ROOT/apps/synth_desktop/src-tauri/generated-resources/cookbooks/optimizers/gepa"

stage_cookbook() {
  local name="$1"
  local source="$SOURCE_ROOT/cookbooks/optimizers/gepa/$name"
  local destination="$DESTINATION/$name"
  if [[ ! -f "$source/gepa.toml" || ! -f "$source/synth_service_app.py" ]]; then
    echo "packaged cookbook is unavailable: $source" >&2
    exit 1
  fi
  mkdir -p "$destination"
  rsync -a --delete \
    --exclude '.venv' \
    --exclude '.pytest_cache' \
    --exclude '__pycache__' \
    --exclude '.banking77-runs' \
    --exclude 'runs' \
    "$source/" "$destination/"
}

stage_cookbook banking77_container
stage_cookbook crafter_container

[[ -f "$DESTINATION/crafter_container/crafter_text_env.py" ]]
[[ -f "$DESTINATION/crafter_container/uv.lock" ]]
