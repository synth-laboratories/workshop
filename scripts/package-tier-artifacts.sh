#!/usr/bin/env bash
# Package already-built Workshop channel apps without changing their bytes.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/dist/v0.9.2-channels}"
VERSION="$(node -p "require('$ROOT/apps/synth_desktop/package.json').version")"
ARCH="$(uname -m)"

mkdir -p "$OUT"

for tier in stable beta alpha dev; do
  case "$tier" in
    stable) product="Synth Workshop" ;;
    beta) product="Synth Workshop Beta" ;;
    alpha) product="Synth Workshop Alpha" ;;
    dev) product="Synth Workshop Dev" ;;
  esac
  app="$ROOT/work/tier-builds/$tier/$product.app"
  build_manifest="$ROOT/work/tier-builds/$tier/manifest.json"
  base="Synth-Workshop-v${VERSION}-${tier}-macOS-${ARCH}-UNNOTARIZED"
  archive="$OUT/$base.zip"
  manifest="$OUT/$base.json"
  checksum="$archive.sha256"
  [[ -d "$app" ]] || { echo "missing tier app: $app" >&2; exit 1; }
  [[ -f "$build_manifest" ]] || { echo "missing tier build manifest: $build_manifest" >&2; exit 1; }
  source_commit="$(jq -er '.commit' "$build_manifest")"
  [[ "$(jq -er '.treeDirty' "$build_manifest")" == false ]] || { echo "tier app came from a dirty tree: $tier" >&2; exit 1; }
  /usr/bin/codesign --verify --deep --strict "$app"
  rm -f "$archive" "$manifest" "$checksum"
  (cd "$(dirname "$app")" && /usr/bin/ditto -c -k --sequesterRsrc --keepParent "$(basename "$app")" "$archive")
  sha256="$(/usr/bin/shasum -a 256 "$archive" | awk '{print $1}')"
  printf '%s  %s\n' "$sha256" "$(basename "$archive")" > "$checksum"
  jq -n \
    --arg version "$VERSION" --arg channel "$tier" --arg product "$product" \
    --arg architecture "$ARCH" --arg archive "$(basename "$archive")" \
    --arg sourceCommit "$source_commit" --arg sha256 "$sha256" \
    --argjson archiveBytes "$(stat -f '%z' "$archive")" \
    --arg builtAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '{schema:"workshop.local-distribution.v1",product:$product,version:$version,
      channel:$channel,platform:"macOS",architecture:$architecture,
      sourceCommit:$sourceCommit,sourceTreeDirty:false,archive:$archive,
      archiveBytes:$archiveBytes,sha256:$sha256,signature:"ad-hoc",
      notarization:"none",builtAt:$builtAt}' > "$manifest"
done

echo "Packaged Workshop $VERSION channels in $OUT"
