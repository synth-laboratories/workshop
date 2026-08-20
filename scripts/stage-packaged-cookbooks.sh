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

# The staged cookbooks are what a Banking77 or Crafter run actually executes,
# and they come from a sibling working tree rather than a submodule -- so
# without this receipt the bundle cannot say which cookbook it carries, and two
# builds of the same Workshop commit are indistinguishable.
if git -C "$SOURCE_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  cookbooks_sha="$(git -C "$SOURCE_ROOT" rev-parse HEAD)"
  cookbooks_ref="$(git -C "$SOURCE_ROOT" rev-parse --abbrev-ref HEAD)"
  cookbooks_dirty=false
  [[ -z "$(git -C "$SOURCE_ROOT" status --porcelain=v1 --untracked-files=no)" ]] || cookbooks_dirty=true
else
  cookbooks_sha=""
  cookbooks_ref=""
  cookbooks_dirty=true
fi
cat > "$DESTINATION/COOKBOOKS_SOURCE.json" <<JSON
{
  "schema": "synth.packaged-cookbooks-source.v1",
  "commit": "$cookbooks_sha",
  "ref": "$cookbooks_ref",
  "dirty": $cookbooks_dirty,
  "cookbooks": ["banking77_container", "crafter_container"]
}
JSON

[[ -f "$DESTINATION/crafter_container/crafter_text_env.py" ]]
[[ -f "$DESTINATION/crafter_container/uv.lock" ]]
