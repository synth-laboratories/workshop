#!/usr/bin/env bash
# User-facing local build and launch commands for Synth Workshop.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="Synth Workshop Local.app"
BUNDLE_ROOT="${CARGO_TARGET_DIR:-$ROOT/apps/synth_desktop/src-tauri/target}/release/bundle"
APP_PATH="$BUNDLE_ROOT/macos/$APP"
VERSION="$(node -p "require('$ROOT/apps/synth_desktop/package.json').version")"
DMG_PATH="$BUNDLE_ROOT/dmg/Synth Workshop Local_${VERSION}_aarch64.dmg"

usage() {
  cat <<'EOF'
Usage: ./scripts/workshop.sh <build | run | build-and-run>

  build          Create a local Apple Silicon .app and .dmg without release credentials
  run            Open the existing local build
  build-and-run  Build, then open it

Local builds are ad-hoc signed and not notarized. Official downloads remain at
https://www.usesynth.ai/download.
EOF
}

fail() {
  printf '[workshop] ERROR: %s\n' "$*" >&2
  exit 1
}

build() {
  "$ROOT/scripts/install.sh" --check
  command -v uv >/dev/null || fail "Install uv before building Workshop."
  [[ -d "$ROOT/node_modules" ]] || fail "Dependencies are not installed. Run: ./scripts/install.sh"
  printf '[workshop] building local app and DMG (no release credentials)\n'
  source "$ROOT/scripts/prepare-build-sources.sh"
  SYNTH_APP_SIGN_IDENTITY=- SYNTH_SIGN_IDENTITY=- APPLE_SIGNING_IDENTITY=- \
    "$ROOT/scripts/build-tier.sh" local
  [[ -d "$APP_PATH" ]] || fail "Build completed without producing $APP_PATH"
  printf '[workshop] applying ad-hoc local signature\n'
  /usr/bin/codesign --force --sign - "$APP_PATH"
  /usr/bin/codesign --verify --deep --strict "$APP_PATH"

  local dmg_root
  dmg_root="$(mktemp -d "${TMPDIR:-/tmp}/workshop-local-dmg.XXXXXX")"
  trap 'rm -rf "$dmg_root"' RETURN
  /usr/bin/ditto "$APP_PATH" "$dmg_root/$APP"
  ln -s /Applications "$dmg_root/Applications"
  mkdir -p "$(dirname "$DMG_PATH")"
  hdiutil create -volname "Synth Workshop Local $VERSION" \
    -srcfolder "$dmg_root" -ov -format UDZO "$DMG_PATH" >/dev/null
  rm -rf "$dmg_root"
  trap - RETURN
  printf '[workshop] app: %s\n' "$APP_PATH"
  printf '[workshop] dmg: %s\n' "$DMG_PATH"
  printf '[workshop] local build is ad-hoc signed and not notarized; do not redistribute it as an official release\n'
}

run() {
  [[ -d "$APP_PATH" ]] || fail "No local build found. Run: ./scripts/workshop.sh build"
  printf '[workshop] opening %s\n' "$APP_PATH"
  /usr/bin/open -na "$APP_PATH"
}

case "${1:-}" in
  build) build ;;
  run) run ;;
  build-and-run) build; run ;;
  --help|-h|help) usage ;;
  *) usage >&2; fail "Choose build, run, or build-and-run." ;;
esac
