#!/usr/bin/env bash
# Build a reproducible, unnotarized friends artifact without ever launching the
# staged app. The four commands are intentionally separable and ordered:
# stage -> record -> zip -> install.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMMAND="${1:-help}"
OUTPUT="${2:-${SYNTH_RELEASE_ROOT:-${TMPDIR:-/tmp}/synth-desktop-v0.4-release}}"
APP_NAME="Synth Desktop.app"
STAGE_ROOT="$OUTPUT/stage"
STAGED_APP="$STAGE_ROOT/$APP_NAME"
ZIP_PATH="$OUTPUT/Synth-Desktop-v0.4.0-macOS-arm64-UNNOTARIZED.zip"
PROVENANCE="$OUTPUT/PROVENANCE.json"
BUILT_APP="$ROOT/apps/synth_desktop/src-tauri/target/release/bundle/macos/$APP_NAME"
INSTALLED_APP="${SYNTH_RELEASE_INSTALL_APP:-/Applications/$APP_NAME}"
CONTAINERS_ROOT="${SYNTH_CONTAINERS_ROOT:-$(dirname "$ROOT")/containers}"

usage() {
  cat <<EOF
Usage: ./scripts/release-artifact.sh <stage|record|zip|install|all> [output-root]

  stage    Require clean source, build, copy adapters, and sign a never-run app
  record   Record source, bundle, executable, frontend, and Containers identity
  zip      Create the friends ZIP and verify signature/CDHash after extraction
  install  Install only from the verified ZIP (backs up an existing app)
  all      Run stage -> record -> zip -> install

Default output: $OUTPUT
EOF
}

die() { echo "[release-artifact] ERROR: $*" >&2; exit 1; }
note() { echo "[release-artifact] $*"; }
sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
cdhash() { /usr/bin/codesign -dvvv "$1" 2>&1 | awk -F= '/^CDHash=/{print $2; exit}'; }

require_clean_source() {
  local status
  status="$(git -C "$ROOT" status --porcelain=v1 --untracked-files=all)"
  [[ -z "$status" ]] || die "source tree is dirty; commit or move every change before cutting bytes"
  git -C "$ROOT" diff --quiet --ignore-submodules -- || die "unstaged source drift"
  git -C "$ROOT" diff --cached --quiet --ignore-submodules -- || die "staged source drift"
}

verify_resource_hygiene() {
  local path
  for path in visuals/families visuals/chrome visuals/components visuals/runtime \
    visuals/ambient.d.ts visuals/package.json visuals/tsconfig.json; do
    git -C "$ROOT" ls-files --error-unmatch "$path" >/dev/null 2>&1 \
      || git -C "$ROOT" ls-files "$path/**" | grep -q . \
      || die "release visual resource is not source-controlled: $path"
    if find "$ROOT/$path" -type f -print0 2>/dev/null \
      | xargs -0 git -C "$ROOT" check-ignore -q -- 2>/dev/null; then
      die "ignored file exists inside release visual allowlist: $path"
    fi
  done
  [[ ! -e "$ROOT/visuals/instances" || -d "$ROOT/visuals/instances" ]] || die "invalid visuals/instances"
}

verify_app() {
  local app="$1"
  [[ -d "$app" ]] || die "app is missing: $app"
  [[ -x "$app/Contents/MacOS/synth-desktop" ]] || die "main executable is missing"
  /usr/bin/codesign --verify --deep --strict "$app"
  [[ -n "$(cdhash "$app")" ]] || die "could not read app CDHash"
}

stage_artifact() {
  require_clean_source
  verify_resource_hygiene
  mkdir -p "$OUTPUT"
  [[ ! -e "$STAGE_ROOT" ]] || die "stage already exists: $STAGE_ROOT"
  note "building clean source $(git -C "$ROOT" rev-parse HEAD)"
  (cd "$ROOT" && npm run build --workspace @synth/synth-desktop)
  [[ -d "$BUILT_APP" ]] || die "Tauri did not produce $BUILT_APP"
  mkdir -p "$STAGE_ROOT"
  /usr/bin/ditto "$BUILT_APP" "$STAGED_APP"
  for adapter in synth-containers-mcp synth-visuals-mcp synth-optimizers-mcp; do
    local source="$ROOT/apps/synth_desktop/src-tauri/target/release/$adapter"
    [[ -x "$source" ]] || die "release adapter is missing: $source"
    /usr/bin/ditto "$source" "$STAGED_APP/Contents/MacOS/$adapter"
  done
  /usr/bin/codesign --force --deep --sign - "$STAGED_APP"
  verify_app "$STAGED_APP"
  note "staged and never launched: $STAGED_APP"
}

record_artifact() {
  require_clean_source
  verify_app "$STAGED_APP"
  mkdir -p "$OUTPUT"
  local source_sha source_tree container_sha container_tree executable executable_sha bundle_id version signing cd_hash frontend_hash
  source_sha="$(git -C "$ROOT" rev-parse HEAD)"
  source_tree="$(git -C "$ROOT" rev-parse 'HEAD^{tree}')"
  if [[ -d "$CONTAINERS_ROOT/.git" ]]; then
    [[ -z "$(git -C "$CONTAINERS_ROOT" status --porcelain=v1 --untracked-files=all)" ]] \
      || die "Containers pin tree is dirty: $CONTAINERS_ROOT"
    container_sha="$(git -C "$CONTAINERS_ROOT" rev-parse HEAD)"
    container_tree="$(git -C "$CONTAINERS_ROOT" rev-parse 'HEAD^{tree}')"
  else
    die "Containers repository is unavailable: $CONTAINERS_ROOT"
  fi
  executable="$STAGED_APP/Contents/MacOS/synth-desktop"
  executable_sha="$(sha256 "$executable")"
  bundle_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$STAGED_APP/Contents/Info.plist")"
  version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$STAGED_APP/Contents/Info.plist")"
  signing="$(/usr/bin/codesign -dvv "$STAGED_APP" 2>&1 | awk -F= '/^Signature=/{print $2; exit}')"
  cd_hash="$(cdhash "$STAGED_APP")"
  frontend_hash="$(cd "$ROOT/apps/synth_desktop/dist" && find . -type f -print | LC_ALL=C sort \
    | while IFS= read -r file; do shasum -a 256 "$file"; done \
    | shasum -a 256 | awk '{print $1}')"
  jq -n \
    --arg generatedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    --arg sourceSha "$source_sha" --arg sourceTree "$source_tree" \
    --arg containersSha "$container_sha" --arg containersTree "$container_tree" \
    --arg app "$STAGED_APP" --arg bundleId "$bundle_id" --arg version "$version" \
    --arg signing "$signing" --arg cdHash "$cd_hash" \
    --arg executableSha256 "$executable_sha" --arg frontendSha256 "$frontend_hash" \
    '{schema:"synth.desktop-release-provenance.v1", generatedAt:$generatedAt,
      stage:{path:$app, launched:false}, source:{workshopCommit:$sourceSha, workshopTree:$sourceTree,
      containersCommit:$containersSha, containersTree:$containersTree}, app:{bundleId:$bundleId,
      version:$version, signing:$signing, cdHash:$cdHash, executableSha256:$executableSha256,
      frontendSha256:$frontendSha256}, zip:null, roundTrip:null}' > "$PROVENANCE.tmp"
  mv "$PROVENANCE.tmp" "$PROVENANCE"
  note "recorded $PROVENANCE"
}

zip_artifact() {
  verify_app "$STAGED_APP"
  [[ -f "$PROVENANCE" ]] || die "record first: $PROVENANCE"
  [[ "$(jq -r '.app.cdHash' "$PROVENANCE")" == "$(cdhash "$STAGED_APP")" ]] || die "stage drifted after provenance"
  [[ ! -e "$ZIP_PATH" ]] || die "ZIP already exists: $ZIP_PATH"
  (cd "$STAGE_ROOT" && /usr/bin/ditto -c -k --sequesterRsrc --keepParent "$APP_NAME" "$ZIP_PATH")
  local roundtrip roundtrip_app before after zip_sha zip_bytes
  roundtrip="$(mktemp -d "${TMPDIR:-/tmp}/synth-release-roundtrip.XXXXXX")"
  trap 'rm -rf "$roundtrip"' RETURN
  /usr/bin/ditto -x -k "$ZIP_PATH" "$roundtrip"
  roundtrip_app="$roundtrip/$APP_NAME"
  verify_app "$roundtrip_app"
  before="$(jq -r '.app.cdHash' "$PROVENANCE")"
  after="$(cdhash "$roundtrip_app")"
  [[ "$before" == "$after" ]] || die "CDHash changed across ZIP round-trip: $before != $after"
  zip_sha="$(sha256 "$ZIP_PATH")"
  zip_bytes="$(stat -f '%z' "$ZIP_PATH")"
  jq --arg path "$ZIP_PATH" --arg sha "$zip_sha" --argjson bytes "$zip_bytes" --arg cdHash "$after" \
    '.zip={path:$path,sha256:$sha,bytes:$bytes} | .roundTrip={codesignVerified:true,cdHash:$cdHash}' \
    "$PROVENANCE" > "$PROVENANCE.tmp"
  mv "$PROVENANCE.tmp" "$PROVENANCE"
  rm -rf "$roundtrip"
  trap - RETURN
  note "verified ZIP round-trip: $ZIP_PATH"
}

install_artifact() {
  [[ -f "$ZIP_PATH" && -f "$PROVENANCE" ]] || die "zip and provenance are required before install"
  [[ "$(sha256 "$ZIP_PATH")" == "$(jq -r '.zip.sha256' "$PROVENANCE")" ]] || die "ZIP digest does not match provenance"
  local extracted candidate expected actual backup=""
  extracted="$(mktemp -d "${TMPDIR:-/tmp}/synth-release-install.XXXXXX")"
  /usr/bin/ditto -x -k "$ZIP_PATH" "$extracted"
  candidate="$extracted/$APP_NAME"
  verify_app "$candidate"
  expected="$(jq -r '.app.cdHash' "$PROVENANCE")"
  actual="$(cdhash "$candidate")"
  [[ "$expected" == "$actual" ]] || die "install candidate CDHash mismatch"
  if [[ -e "$INSTALLED_APP" ]]; then
    backup="${INSTALLED_APP%.app}.backup-$(date '+%Y%m%d-%H%M%S').app"
    mv "$INSTALLED_APP" "$backup"
    note "backed up previous app: $backup"
  fi
  if ! /usr/bin/ditto "$candidate" "$INSTALLED_APP"; then
    [[ -n "$backup" && ! -e "$INSTALLED_APP" ]] && mv "$backup" "$INSTALLED_APP"
    die "install failed; previous app restored when possible"
  fi
  verify_app "$INSTALLED_APP"
  [[ "$(cdhash "$INSTALLED_APP")" == "$expected" ]] || die "installed app CDHash mismatch"
  rm -rf "$extracted"
  note "installed without launching: $INSTALLED_APP"
}

case "$COMMAND" in
  stage) stage_artifact ;;
  record) record_artifact ;;
  zip) zip_artifact ;;
  install) install_artifact ;;
  all) stage_artifact; record_artifact; zip_artifact; install_artifact ;;
  help|-h|--help) usage ;;
  *) usage >&2; exit 2 ;;
esac
