#!/usr/bin/env bash
# Stage private visual templates from the developer's home into the build tree.
#
#   ./scripts/stage-internal-visuals.sh          stage
#   ./scripts/stage-internal-visuals.sh --clean  unstage
#   ./scripts/stage-internal-visuals.sh --check  exit 1 if anything is staged
#
# Source of truth is $SYNTH_INTERNAL_VISUALS_ROOT. Unset, the script prefers
# ~/.synth-desktop/visuals/templates and falls back to the pre-consolidation
# ~/.synth/visuals/templates, and prints which of the two it read either way.
#
# It reads both because pointing it at the new path alone was a silent
# regression: every internal template was still sitting in the old one, so the
# script exited 0 having staged nothing and the build simply had no internal
# visuals. Nothing here may succeed quietly while doing nothing — every path
# below names the directory it looked at, and staging zero templates from a
# root that exists is a warning, not a success.
#
# Templates are COPIED, not symlinked. A shell imports its chrome by relative
# path (`../../runtime/liveStream.ts`), and bundlers resolve a symlinked file
# from its real path by default (esbuild always, Vite unless
# resolve.preserveSymlinks). Through a symlink those imports resolve inside
# ~/.synth-desktop, where no chrome exists, and the build fails. Copying keeps the
# template inside the workspace where its relative imports are valid.
#
# The cost is that editing in ~/.synth-desktop needs a re-stage; run this again.
#
# Item 30 proposed deleting this script, because user templates now load from
# the same directory at runtime with no rebuild. That is blocked, not done: the
# runtime tier resolves imports against an eleven-specifier allowlist with no
# relative-path resolution, and both internal templates use four relative value
# imports it cannot serve. visuals/templates-internal/README.md has the table
# and the three changes that would unblock the deletion.
#
# A public release must run --check, not --clean: failing loudly beats silently
# deleting whatever a developer had staged.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/visuals/templates-internal"
PRIMARY="$HOME/.synth-desktop/visuals/templates"
LEGACY="$HOME/.synth/visuals/templates"
MODE="${1:-stage}"

staged_entries() {
  find "$DEST" -mindepth 1 -maxdepth 1 ! -name '.gitkeep' ! -name 'README.md' 2>/dev/null || true
}

# True when a root holds at least one directory with a template.json.
#
# Existence alone is the wrong test: the app creates
# ~/.synth-desktop/visuals/templates for the runtime user tier whether or not
# anything was ever migrated into it, so an empty new root would otherwise
# shadow a populated legacy one and reproduce the regression exactly.
has_templates() {
  local root="$1" dir
  [[ -d "$root" ]] || return 1
  for dir in "$root"/*/; do
    [[ -f "$dir/template.json" ]] && return 0
  done
  return 1
}

case "$MODE" in
  --check)
    found="$(staged_entries)"
    if [[ -n "$found" ]]; then
      echo "[internal-visuals] refusing: private templates are staged" >&2
      echo "$found" >&2
      echo "[internal-visuals] run ./scripts/stage-internal-visuals.sh --clean first" >&2
      exit 1
    fi
    echo "[internal-visuals] clean — no private templates staged"
    exit 0
    ;;
  --clean)
    while IFS= read -r entry; do
      [[ -n "$entry" ]] || continue
      rm -rf "$entry"
      echo "[internal-visuals] unstaged $(basename "$entry")"
    done <<< "$(staged_entries)"
    exit 0
    ;;
  stage) ;;
  *)
    echo "usage: $0 [--clean|--check]" >&2
    exit 2
    ;;
esac

mkdir -p "$DEST"

if [[ -n "${SYNTH_INTERNAL_VISUALS_ROOT:-}" ]]; then
  SRC="$SYNTH_INTERNAL_VISUALS_ROOT"
  SRC_LABEL="SYNTH_INTERNAL_VISUALS_ROOT"
  # An explicit override that does not exist is an operator mistake, not an
  # empty machine. Fail rather than fall back to a path nobody asked for.
  if [[ ! -d "$SRC" ]]; then
    echo "[internal-visuals] SYNTH_INTERNAL_VISUALS_ROOT=$SRC is not a directory" >&2
    exit 1
  fi
elif has_templates "$PRIMARY"; then
  SRC="$PRIMARY"
  SRC_LABEL="current"
elif has_templates "$LEGACY"; then
  SRC="$LEGACY"
  SRC_LABEL="legacy"
elif [[ -d "$PRIMARY" ]]; then
  SRC="$PRIMARY"
  SRC_LABEL="current, empty"
else
  echo "[internal-visuals] no private template root — nothing to stage"
  echo "[internal-visuals]   $PRIMARY (current) — absent"
  echo "[internal-visuals]   $LEGACY (legacy) — absent"
  exit 0
fi

echo "[internal-visuals] source: $SRC ($SRC_LABEL)"
if [[ "$SRC_LABEL" == legacy ]]; then
  echo "[internal-visuals] this is the pre-consolidation location." >&2
  echo "[internal-visuals] Staging from it works, but the runtime user-template" >&2
  echo "[internal-visuals] tier only reads $PRIMARY, so a template left here can" >&2
  echo "[internal-visuals] never load without a re-stage and a rebuild. Migrate:" >&2
  echo "[internal-visuals]   mkdir -p \"$PRIMARY\"" >&2
  echo "[internal-visuals]   mv \"$LEGACY\"/* \"$PRIMARY\"/" >&2
  echo "[internal-visuals] visuals/templates-internal/README.md has the detail." >&2
fi

count=0
for dir in "$SRC"/*/; do
  [[ -d "$dir" ]] || continue
  id="$(basename "$dir")"
  if [[ ! -f "$dir/template.json" || ! -f "$dir/shell.tsx" ]]; then
    echo "[internal-visuals] skip $id — needs both template.json and shell.tsx" >&2
    continue
  fi
  # A private template must not shadow a reviewed public one.
  if [[ -d "$ROOT/visuals/templates/$id" ]]; then
    echo "[internal-visuals] skip $id — a public template already owns that id" >&2
    continue
  fi
  rm -rf "${DEST:?}/$id"
  cp -R "${dir%/}" "$DEST/$id"
  echo "[internal-visuals] staged $id"
  count=$((count + 1))
done
if (( count == 0 )); then
  # The regression this script shipped looked exactly like this line, so it is
  # a warning on stderr rather than a cheerful zero.
  echo "[internal-visuals] WARNING: staged 0 private templates from $SRC" >&2
  echo "[internal-visuals] a template directory needs both template.json and shell.tsx" >&2
else
  echo "[internal-visuals] $count private template(s) staged from $SRC"
fi
