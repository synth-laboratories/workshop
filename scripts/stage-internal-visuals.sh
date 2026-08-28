#!/usr/bin/env bash
# Stage private visual templates from ~/.synth-desktop into the build tree.
#
#   ./scripts/stage-internal-visuals.sh          stage
#   ./scripts/stage-internal-visuals.sh --clean  unstage
#   ./scripts/stage-internal-visuals.sh --check  exit 1 if anything is staged
#
# Source of truth is $SYNTH_INTERNAL_VISUALS_ROOT (default
# ~/.synth-desktop/visuals/templates).
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
# A public release must run --check, not --clean: failing loudly beats silently
# deleting whatever a developer had staged.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/visuals/templates-internal"
SRC="${SYNTH_INTERNAL_VISUALS_ROOT:-$HOME/.synth-desktop/visuals/templates}"
MODE="${1:-stage}"

staged_entries() {
  find "$DEST" -mindepth 1 -maxdepth 1 ! -name '.gitkeep' ! -name 'README.md' 2>/dev/null || true
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
if [[ ! -d "$SRC" ]]; then
  echo "[internal-visuals] no private template root at $SRC — nothing to stage"
  exit 0
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
echo "[internal-visuals] $count private template(s) staged from $SRC"
