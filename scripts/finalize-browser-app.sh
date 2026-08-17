#!/usr/bin/env bash
# Repair Tauri's resource copy of the Chromium framework, seal the outer app,
# and prove that first launch does not invalidate its signature.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="${1:-$ROOT/apps/synth_desktop/src-tauri/target/release/bundle/macos/Synth Desktop.app}"
SOURCE_RUNTIME="$ROOT/apps/synth_desktop/browser/runtime"
DEST_RUNTIME="$APP/Contents/Resources/browser/runtime"
IDENTITY="${SYNTH_APP_SIGN_IDENTITY:-${APPLE_SIGNING_IDENTITY:-${SYNTH_SIGN_IDENTITY:--}}}"

die() { echo "[browser-bundle] ERROR: $*" >&2; exit 1; }
note() { echo "[browser-bundle] $*"; }

[[ -d "$APP" && "$APP" == *.app ]] || die "application bundle is missing: $APP"
[[ -f "$SOURCE_RUNTIME/manifest.json" ]] || die "assembled browser runtime is missing"
[[ -d "$DEST_RUNTIME" ]] || die "Tauri did not bundle the browser runtime"

# Tauri's generic resource copier dereferences versioned framework symlinks.
# Chromium then has an invalid framework layout even if an initial deep seal
# appears to pass. Replace only the generated bundle copy with ditto, which
# preserves the signed framework structure and extended attributes.
rm -rf "$DEST_RUNTIME"
mkdir -p "$(dirname "$DEST_RUNTIME")"
/usr/bin/ditto "$SOURCE_RUNTIME" "$DEST_RUNTIME"

if [[ "$IDENTITY" != "-" ]]; then
  /usr/bin/codesign --force --sign "$IDENTITY" --options runtime --timestamp "$APP"
else
  /usr/bin/codesign --force --sign - "$APP"
fi
/usr/bin/codesign --verify --strict --deep "$APP"

# Runtime verification launches browser and renderer processes. Verify the app
# again afterwards so a first-launch mutation can never receive a passing
# packaging receipt.
SYNTH_BROWSER_RUNTIME_OUTPUT="$DEST_RUNTIME" "$ROOT/scripts/build-browser-runtime.sh" verify
/usr/bin/codesign --verify --strict --deep "$APP"
note "finalized and launch-verified $APP"
