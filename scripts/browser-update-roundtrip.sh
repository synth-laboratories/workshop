#!/usr/bin/env bash
# Build two distinct Workshop app bundles, install them through isolated staged
# replacement, and prove profile compatibility across update and rollback.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION_BEFORE="${SYNTH_UPDATE_BEFORE_VERSION:-0.5.1}"
VERSION_AFTER="${SYNTH_UPDATE_AFTER_VERSION:-0.5.2}"
RECEIPT="${SYNTH_ACCEPTANCE_RECEIPT:-${TMPDIR:-/tmp}/workshop-browser-update-roundtrip.json}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/workshop-update-roundtrip.XXXXXX")"
GENERATED="$WORK/generated"
ARTIFACTS="$WORK/artifacts"
INSTALL_ROOT="$WORK/Applications"
INSTALLED="$INSTALL_ROOT/Synth Desktop.app"
BUNDLE="$ROOT/apps/synth_desktop/src-tauri/target/release/bundle/macos/Synth Desktop.app"
BEFORE="$ARTIFACTS/Synth Desktop-$VERSION_BEFORE.app"
AFTER="$ARTIFACTS/Synth Desktop-$VERSION_AFTER.app"
BACKUP="$ARTIFACTS/Synth Desktop-rollback.app"

cleanup() { /bin/rm -rf "$WORK"; }
trap cleanup EXIT
mkdir -p "$GENERATED" "$ARTIFACTS" "$INSTALL_ROOT"

build_version() {
  local version="$1" destination="$2" config="$GENERATED/tauri-$version.json"
  printf '{"version":"%s"}\n' "$version" > "$config"
  (cd "$ROOT/apps/synth_desktop" && npx tauri build --bundles app --config "$config")
  "$ROOT/scripts/finalize-browser-app.sh" "$BUNDLE"
  /usr/bin/ditto "$BUNDLE" "$destination"
  /usr/bin/codesign --verify --strict --deep "$destination"
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$destination/Contents/Info.plist")" == "$version" ]]
}

install_stage() {
  local source="$1" stage="$INSTALL_ROOT/.Synth Desktop.stage.app"
  [[ ! -e "$stage" ]] || { echo "stale install stage: $stage" >&2; return 1; }
  /usr/bin/ditto "$source" "$stage"
  /usr/bin/codesign --verify --strict --deep "$stage"
  [[ ! -e "$INSTALLED" ]] || mv "$INSTALLED" "$BACKUP"
  mv "$stage" "$INSTALLED"
  /usr/bin/codesign --verify --strict --deep "$INSTALLED"
}

build_version "$VERSION_BEFORE" "$BEFORE"
build_version "$VERSION_AFTER" "$AFTER"
[[ "$(/usr/bin/shasum -a 256 "$BEFORE/Contents/MacOS/synth-desktop" | awk '{print $1}')" != \
   "$(/usr/bin/shasum -a 256 "$AFTER/Contents/MacOS/synth-desktop" | awk '{print $1}')" ]] \
  || { echo "two versioned builds produced the same desktop executable" >&2; exit 1; }

install_stage "$BEFORE"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$INSTALLED/Contents/Info.plist")" == "$VERSION_BEFORE" ]]
install_stage "$AFTER"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$INSTALLED/Contents/Info.plist")" == "$VERSION_AFTER" ]]

# Roll back with the exact backup made by the staged forward installation.
/bin/rm -rf "$INSTALLED"
mv "$BACKUP" "$INSTALLED"
/usr/bin/codesign --verify --strict --deep "$INSTALLED"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$INSTALLED/Contents/Info.plist")" == "$VERSION_BEFORE" ]]

PROFILE_RECEIPT="$GENERATED/profile.json"
"$BEFORE/Contents/Resources/browser/runtime/node/bin/node" "$ROOT/scripts/browser-profile-compat.mjs" \
  --before "$BEFORE" --after "$AFTER" --rollback "$INSTALLED" --require-version-change --receipt "$PROFILE_RECEIPT"

jq -n \
  --arg beforeVersion "$VERSION_BEFORE" --arg afterVersion "$VERSION_AFTER" \
  --arg beforeExecutableSha256 "$(/usr/bin/shasum -a 256 "$BEFORE/Contents/MacOS/synth-desktop" | awk '{print $1}')" \
  --arg afterExecutableSha256 "$(/usr/bin/shasum -a 256 "$AFTER/Contents/MacOS/synth-desktop" | awk '{print $1}')" \
  --arg rollbackExecutableSha256 "$(/usr/bin/shasum -a 256 "$INSTALLED/Contents/MacOS/synth-desktop" | awk '{print $1}')" \
  --argjson profile "$(cat "$PROFILE_RECEIPT")" --arg checkedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
  '{schema:"workshop.browser-update-roundtrip.v1",passed:true,productionEligible:false,notarized:false,mechanism:"isolated-staged-bundle-replacement",beforeVersion:$beforeVersion,afterVersion:$afterVersion,beforeExecutableSha256:$beforeExecutableSha256,afterExecutableSha256:$afterExecutableSha256,rollbackExecutableSha256:$rollbackExecutableSha256,rollbackRestoredOriginal:($beforeExecutableSha256 == $rollbackExecutableSha256),profile:$profile,checkedAt:$checkedAt}' > "$RECEIPT"
echo "[browser-update] two-build update/rollback passed; receipt: $RECEIPT"
cat "$RECEIPT"
