#!/usr/bin/env bash
# Build a Developer ID signed and notarized release without launching the
# staged app. Commands are intentionally separable and ordered:
# stage -> notarize -> record -> zip -> install.
#
# Candidate commands are deliberately separate. They produce a locally signed,
# clearly labelled candidate for pre-notary acceptance without weakening any
# official release gate.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/mcp-adapters.sh
source "$ROOT/scripts/mcp-adapters.sh"
COMMAND="${1:-help}"
APP_VERSION="$(node -p "require('$ROOT/apps/synth_desktop/package.json').version")"
ARCH="$(uname -m)"
OUTPUT="${2:-${SYNTH_RELEASE_ROOT:-${TMPDIR:-/tmp}/synth-desktop-v${APP_VERSION}-release}}"
APP_NAME="Synth Workshop.app"
STAGE_ROOT="$OUTPUT/stage"
STAGED_APP="$STAGE_ROOT/$APP_NAME"
ZIP_PATH="$OUTPUT/Synth-Workshop-v${APP_VERSION}-macOS-${ARCH}.zip"
CANDIDATE_ZIP_PATH="$OUTPUT/Synth-Workshop-v${APP_VERSION}-macOS-${ARCH}-CANDIDATE.zip"
NOTARY_ZIP="$OUTPUT/Synth-Workshop-v${APP_VERSION}-notary-submission.zip"
NOTARIZED_MARKER="$OUTPUT/.notarized-cdhash"
CANDIDATE_MARKER="$OUTPUT/.candidate-cdhash"
PROVENANCE="$OUTPUT/PROVENANCE.json"
BUILT_APP="$ROOT/apps/synth_desktop/src-tauri/target/release/bundle/macos/$APP_NAME"
INSTALLED_APP="${SYNTH_RELEASE_INSTALL_APP:-/Applications/$APP_NAME}"
CONTAINERS_ROOT="${SYNTH_CONTAINERS_ROOT:-$(dirname "$ROOT")/containers}"
SIGN_IDENTITY="${SYNTH_RELEASE_SIGN_IDENTITY:-}"
NOTARY_PROFILE="${SYNTH_RELEASE_NOTARY_PROFILE:-}"
CANDIDATE_SIGN_IDENTITY="${SYNTH_CANDIDATE_SIGN_IDENTITY:-}"
CANDIDATE_INSTALLED_APP="${SYNTH_CANDIDATE_INSTALL_APP:-/Applications/Synth Workshop Candidate.app}"

usage() {
  cat <<EOF
Usage: ./scripts/release-artifact.sh <stage|notarize|record|zip|install|all> [output-root]
       ./scripts/release-artifact.sh <candidate-stage|candidate-record|candidate-zip|candidate-install|candidate-all> [output-root]

  stage    Require clean source, build, copy adapters, and Developer ID sign
  notarize Submit to Apple, wait, staple, and verify Gatekeeper acceptance
  record   Record source, bundle, executable, frontend, and Containers identity
  zip      Create the release ZIP and verify signature/CDHash after extraction
  install  Install only from the verified ZIP (backs up an existing app)
  all      Run stage -> notarize -> record -> zip -> install

  candidate-stage   Build and locally sign an explicit pre-notary candidate
  candidate-record  Record candidate provenance with notarized=false
  candidate-zip     Create and verify the candidate ZIP round trip
  candidate-install Install the candidate without replacing the official app
  candidate-all     Run all candidate steps (never submits to Apple)

Default output: $OUTPUT

Required environment:
  SYNTH_RELEASE_SIGN_IDENTITY  Developer ID Application identity
  SYNTH_RELEASE_NOTARY_PROFILE notarytool keychain profile
  SYNTH_CANDIDATE_SIGN_IDENTITY local codesigning identity for candidate commands
  SYNTH_CANDIDATE_INSTALL_APP   candidate install path (default: $CANDIDATE_INSTALLED_APP)
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

verify_official_app() {
  local app="$1" report authority
  verify_app "$app"
  report="$(/usr/bin/codesign --display --verbose=4 "$app" 2>&1)"
  authority="$(printf '%s\n' "$report" | awk -F= '/^Authority=/{print $2; exit}')"
  [[ "$authority" == Developer\ ID\ Application:* ]] \
    || die "official app is not signed by a Developer ID Application identity"
  printf '%s\n' "$report" | grep -Eq 'flags=.*\(runtime\)' \
    || die "official app does not enable the hardened runtime"
}

stage_artifact() {
  require_clean_source
  verify_resource_hygiene
  [[ -n "$SIGN_IDENTITY" ]] || die "SYNTH_RELEASE_SIGN_IDENTITY is required"
  [[ "$SIGN_IDENTITY" == Developer\ ID\ Application:* ]] \
    || die "SYNTH_RELEASE_SIGN_IDENTITY must be a Developer ID Application identity"
  security find-identity -v -p codesigning 2>/dev/null | grep -F "\"$SIGN_IDENTITY\"" >/dev/null \
    || die "Developer ID identity is unavailable in the keychain search list"
  mkdir -p "$OUTPUT"
  [[ ! -e "$STAGE_ROOT" ]] || die "stage already exists: $STAGE_ROOT"
  note "building clean source $(git -C "$ROOT" rev-parse HEAD)"
  (cd "$ROOT" && npm run build --workspace @synth/synth-desktop)
  [[ -d "$BUILT_APP" ]] || die "Tauri did not produce $BUILT_APP"
  mkdir -p "$STAGE_ROOT"
  /usr/bin/ditto "$BUILT_APP" "$STAGED_APP"
  for adapter in "${SYNTH_MCP_ADAPTERS[@]}"; do
    local source="$ROOT/apps/synth_desktop/src-tauri/target/release/$adapter"
    [[ -x "$source" ]] || die "release adapter is missing: $source"
    /usr/bin/ditto "$source" "$STAGED_APP/Contents/MacOS/$adapter"
  done
  /usr/bin/codesign --force --deep --options runtime --timestamp \
    --sign "$SIGN_IDENTITY" "$STAGED_APP"
  verify_official_app "$STAGED_APP"
  note "staged and never launched: $STAGED_APP"
}

stage_candidate_artifact() {
  require_clean_source
  verify_resource_hygiene
  [[ -n "$CANDIDATE_SIGN_IDENTITY" ]] || die "SYNTH_CANDIDATE_SIGN_IDENTITY is required"
  security find-identity -v -p codesigning 2>/dev/null | grep -F "\"$CANDIDATE_SIGN_IDENTITY\"" >/dev/null \
    || die "candidate signing identity is unavailable in the keychain search list"
  mkdir -p "$OUTPUT"
  [[ ! -e "$STAGE_ROOT" ]] || die "stage already exists: $STAGE_ROOT"
  note "building candidate from clean source $(git -C "$ROOT" rev-parse HEAD)"
  (cd "$ROOT" && npm run build --workspace @synth/synth-desktop)
  [[ -d "$BUILT_APP" ]] || die "Tauri did not produce $BUILT_APP"
  mkdir -p "$STAGE_ROOT"
  /usr/bin/ditto "$BUILT_APP" "$STAGED_APP"
  for adapter in "${SYNTH_MCP_ADAPTERS[@]}"; do
    local source="$ROOT/apps/synth_desktop/src-tauri/target/release/$adapter"
    [[ -x "$source" ]] || die "release adapter is missing: $source"
    /usr/bin/ditto "$source" "$STAGED_APP/Contents/MacOS/$adapter"
  done
  /usr/bin/codesign --force --deep --options runtime \
    --sign "$CANDIDATE_SIGN_IDENTITY" "$STAGED_APP"
  verify_app "$STAGED_APP"
  cdhash "$STAGED_APP" > "$CANDIDATE_MARKER"
  note "candidate staged and never launched: $STAGED_APP"
}

notarize_artifact() {
  require_clean_source
  verify_official_app "$STAGED_APP"
  [[ -n "$NOTARY_PROFILE" ]] || die "SYNTH_RELEASE_NOTARY_PROFILE is required"
  [[ ! -e "$NOTARY_ZIP" ]] || die "notary submission already exists: $NOTARY_ZIP"
  (cd "$STAGE_ROOT" && /usr/bin/ditto -c -k --sequesterRsrc --keepParent "$APP_NAME" "$NOTARY_ZIP")
  xcrun notarytool submit "$NOTARY_ZIP" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$STAGED_APP"
  xcrun stapler validate "$STAGED_APP"
  /usr/sbin/spctl --assess --type execute -vv "$STAGED_APP" 2>&1 \
    | grep -q 'source=Notarized Developer ID' \
    || die "Gatekeeper does not recognize the staged app as notarized"
  cdhash "$STAGED_APP" > "$NOTARIZED_MARKER"
  rm -f "$NOTARY_ZIP"
  note "notarized and stapled: $STAGED_APP"
}

record_artifact() {
  require_clean_source
  verify_official_app "$STAGED_APP"
  [[ -f "$NOTARIZED_MARKER" ]] || die "notarize first: marker is missing"
  [[ "$(<"$NOTARIZED_MARKER")" == "$(cdhash "$STAGED_APP")" ]] \
    || die "staged app changed after notarization"
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
  signing="$(/usr/bin/codesign -dvv "$STAGED_APP" 2>&1 | awk -F= '/^Authority=/{print $2; exit}')"
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
      version:$version, signing:$signing, notarized:true, stapled:true, cdHash:$cdHash, executableSha256:$executableSha256,
      frontendSha256:$frontendSha256}, zip:null, roundTrip:null}' > "$PROVENANCE.tmp"
  mv "$PROVENANCE.tmp" "$PROVENANCE"
  note "recorded $PROVENANCE"
}

record_candidate_artifact() {
  require_clean_source
  verify_app "$STAGED_APP"
  [[ -f "$CANDIDATE_MARKER" ]] || die "candidate-stage first: marker is missing"
  [[ "$(<"$CANDIDATE_MARKER")" == "$(cdhash "$STAGED_APP")" ]] \
    || die "staged candidate changed after signing"
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
  signing="$(/usr/bin/codesign -dvv "$STAGED_APP" 2>&1 | awk -F= '/^Authority=/{print $2; exit}')"
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
    '{schema:"synth.desktop-release-provenance.v1", distribution:"candidate", generatedAt:$generatedAt,
      stage:{path:$app, launched:false}, source:{workshopCommit:$sourceSha, workshopTree:$sourceTree,
      containersCommit:$containersSha, containersTree:$containersTree}, app:{bundleId:$bundleId,
      version:$version, signing:$signing, notarized:false, stapled:false, cdHash:$cdHash, executableSha256:$executableSha256,
      frontendSha256:$frontendSha256}, zip:null, roundTrip:null}' > "$PROVENANCE.tmp"
  mv "$PROVENANCE.tmp" "$PROVENANCE"
  note "recorded candidate provenance: $PROVENANCE"
}

zip_artifact() {
  verify_official_app "$STAGED_APP"
  [[ -f "$NOTARIZED_MARKER" ]] || die "notarize first: marker is missing"
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
  xcrun stapler validate "$roundtrip_app"
  /usr/sbin/spctl --assess --type execute -vv "$roundtrip_app" 2>&1 \
    | grep -q 'source=Notarized Developer ID' \
    || die "Gatekeeper rejected the ZIP round-trip app"
  before="$(jq -r '.app.cdHash' "$PROVENANCE")"
  after="$(cdhash "$roundtrip_app")"
  [[ "$before" == "$after" ]] || die "CDHash changed across ZIP round-trip: $before != $after"
  zip_sha="$(sha256 "$ZIP_PATH")"
  zip_bytes="$(stat -f '%z' "$ZIP_PATH")"
  local release_id
  release_id="workshop-v${APP_VERSION}-${zip_sha:0:16}"
  jq --arg path "$ZIP_PATH" --arg sha "$zip_sha" --argjson bytes "$zip_bytes" \
    --arg cdHash "$after" --arg releaseId "$release_id" \
    '.releaseId=$releaseId | .zip={path:$path,sha256:$sha,bytes:$bytes} | .roundTrip={codesignVerified:true,cdHash:$cdHash}' \
    "$PROVENANCE" > "$PROVENANCE.tmp"
  mv "$PROVENANCE.tmp" "$PROVENANCE"
  rm -rf "$roundtrip"
  trap - RETURN
  note "verified ZIP round-trip: $ZIP_PATH"
}

zip_candidate_artifact() {
  verify_app "$STAGED_APP"
  [[ -f "$CANDIDATE_MARKER" ]] || die "candidate-stage first: marker is missing"
  [[ -f "$PROVENANCE" ]] || die "candidate-record first: $PROVENANCE"
  [[ "$(jq -r '.distribution' "$PROVENANCE")" == "candidate" ]] || die "provenance is not for a candidate"
  [[ "$(jq -r '.app.cdHash' "$PROVENANCE")" == "$(cdhash "$STAGED_APP")" ]] || die "candidate stage drifted after provenance"
  [[ ! -e "$CANDIDATE_ZIP_PATH" ]] || die "candidate ZIP already exists: $CANDIDATE_ZIP_PATH"
  (cd "$STAGE_ROOT" && /usr/bin/ditto -c -k --sequesterRsrc --keepParent "$APP_NAME" "$CANDIDATE_ZIP_PATH")
  local roundtrip roundtrip_app before after zip_sha zip_bytes release_id
  roundtrip="$(mktemp -d "${TMPDIR:-/tmp}/synth-candidate-roundtrip.XXXXXX")"
  trap 'rm -rf "$roundtrip"' RETURN
  /usr/bin/ditto -x -k "$CANDIDATE_ZIP_PATH" "$roundtrip"
  roundtrip_app="$roundtrip/$APP_NAME"
  verify_app "$roundtrip_app"
  before="$(jq -r '.app.cdHash' "$PROVENANCE")"
  after="$(cdhash "$roundtrip_app")"
  [[ "$before" == "$after" ]] || die "candidate CDHash changed across ZIP round trip: $before != $after"
  zip_sha="$(sha256 "$CANDIDATE_ZIP_PATH")"
  zip_bytes="$(stat -f '%z' "$CANDIDATE_ZIP_PATH")"
  release_id="workshop-v${APP_VERSION}-candidate-${zip_sha:0:16}"
  jq --arg path "$CANDIDATE_ZIP_PATH" --arg sha "$zip_sha" --argjson bytes "$zip_bytes" \
    --arg cdHash "$after" --arg releaseId "$release_id" \
    '.releaseId=$releaseId | .zip={path:$path,sha256:$sha,bytes:$bytes} | .roundTrip={codesignVerified:true,cdHash:$cdHash}' \
    "$PROVENANCE" > "$PROVENANCE.tmp"
  mv "$PROVENANCE.tmp" "$PROVENANCE"
  rm -rf "$roundtrip"
  trap - RETURN
  note "verified candidate ZIP round trip: $CANDIDATE_ZIP_PATH"
}

install_artifact() {
  [[ -f "$ZIP_PATH" && -f "$PROVENANCE" ]] || die "zip and provenance are required before install"
  [[ "$(sha256 "$ZIP_PATH")" == "$(jq -r '.zip.sha256' "$PROVENANCE")" ]] || die "ZIP digest does not match provenance"
  local extracted candidate expected actual backup=""
  extracted="$(mktemp -d "${TMPDIR:-/tmp}/synth-release-install.XXXXXX")"
  /usr/bin/ditto -x -k "$ZIP_PATH" "$extracted"
  candidate="$extracted/$APP_NAME"
  verify_official_app "$candidate"
  xcrun stapler validate "$candidate"
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
  /usr/sbin/spctl --assess --type execute -vv "$INSTALLED_APP" 2>&1 \
    | grep -q 'source=Notarized Developer ID' \
    || die "Gatekeeper rejected the installed app"
  [[ "$(cdhash "$INSTALLED_APP")" == "$expected" ]] || die "installed app CDHash mismatch"
  rm -rf "$extracted"
  note "installed without launching: $INSTALLED_APP"
}

install_candidate_artifact() {
  [[ -f "$CANDIDATE_ZIP_PATH" && -f "$PROVENANCE" ]] || die "candidate ZIP and provenance are required before install"
  [[ "$(jq -r '.distribution' "$PROVENANCE")" == "candidate" ]] || die "provenance is not for a candidate"
  [[ "$(sha256 "$CANDIDATE_ZIP_PATH")" == "$(jq -r '.zip.sha256' "$PROVENANCE")" ]] || die "candidate ZIP digest does not match provenance"
  local extracted candidate expected actual backup=""
  extracted="$(mktemp -d "${TMPDIR:-/tmp}/synth-candidate-install.XXXXXX")"
  /usr/bin/ditto -x -k "$CANDIDATE_ZIP_PATH" "$extracted"
  candidate="$extracted/$APP_NAME"
  verify_app "$candidate"
  expected="$(jq -r '.app.cdHash' "$PROVENANCE")"
  actual="$(cdhash "$candidate")"
  [[ "$expected" == "$actual" ]] || die "candidate install CDHash mismatch"
  if [[ -e "$CANDIDATE_INSTALLED_APP" ]]; then
    backup="${CANDIDATE_INSTALLED_APP%.app}.backup-$(date '+%Y%m%d-%H%M%S').app"
    mv "$CANDIDATE_INSTALLED_APP" "$backup"
    note "backed up previous candidate: $backup"
  fi
  if ! /usr/bin/ditto "$candidate" "$CANDIDATE_INSTALLED_APP"; then
    [[ -n "$backup" && ! -e "$CANDIDATE_INSTALLED_APP" ]] && mv "$backup" "$CANDIDATE_INSTALLED_APP"
    die "candidate install failed; previous candidate restored when possible"
  fi
  verify_app "$CANDIDATE_INSTALLED_APP"
  [[ "$(cdhash "$CANDIDATE_INSTALLED_APP")" == "$expected" ]] || die "installed candidate CDHash mismatch"
  rm -rf "$extracted"
  note "installed candidate without launching: $CANDIDATE_INSTALLED_APP"
}

case "$COMMAND" in
  stage) stage_artifact ;;
  notarize) notarize_artifact ;;
  record) record_artifact ;;
  zip) zip_artifact ;;
  install) install_artifact ;;
  all) stage_artifact; notarize_artifact; record_artifact; zip_artifact; install_artifact ;;
  candidate-stage) stage_candidate_artifact ;;
  candidate-record) record_candidate_artifact ;;
  candidate-zip) zip_candidate_artifact ;;
  candidate-install) install_candidate_artifact ;;
  candidate-all) stage_candidate_artifact; record_candidate_artifact; zip_candidate_artifact; install_candidate_artifact ;;
  help|-h|--help) usage ;;
  *) usage >&2; exit 2 ;;
esac
