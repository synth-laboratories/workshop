#!/usr/bin/env bash
# Read-only production acceptance for the installed browser runtime, updater
# round-trip, and signed native Computer Use helper.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMMAND="${1:-help}"
APP="${2:-${SYNTH_INSTALLED_APP:-/Applications/Synth Desktop.app}}"
TEAM_ID="${SYNTH_TEAM_ID:-}"
RECEIPT="${SYNTH_ACCEPTANCE_RECEIPT:-${TMPDIR:-/tmp}/workshop-browser-production-gates.json}"

die() { echo "[browser-production] ERROR: $*" >&2; exit 1; }
note() { echo "[browser-production] $*"; }
sha256() { /usr/bin/shasum -a 256 "$1" | awk '{print $1}'; }

verify_code() {
  local target="$1" require_workshop_team="${2:-yes}"
  /usr/bin/codesign --verify --strict --deep "$target"
  if [[ -n "$TEAM_ID" && "$require_workshop_team" == yes ]]; then
    /usr/bin/codesign --verify -R "anchor apple generic and certificate leaf[subject.OU] = \"$TEAM_ID\"" "$target"
  fi
}

verify_notary() {
  local target="$1"
  /usr/sbin/spctl --assess --type execute -vv "$target" 2>&1 | grep -q 'source=Notarized Developer ID' \
    || die "Gatekeeper does not report Notarized Developer ID for $target"
  /usr/bin/xcrun stapler validate "$target" >/dev/null
}

verify_installed() {
  local app="$1" resources runtime node chrome helper
  [[ -d "$app" ]] || die "installed app is missing: $app"
  resources="$app/Contents/Resources"
  runtime="$resources/browser/runtime"
  node="$runtime/node/bin/node"
  [[ -d "$runtime/browsers" ]] || die "pinned browser runtime is missing"
  chrome="$(find "$runtime/browsers" -type f -path '*/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing' -print -quit)"
  helper="$resources/Synth Computer Use.app"
  [[ -x "$app/Contents/MacOS/synth-desktop" ]] || die "Desktop executable is missing"
  [[ -x "$app/Contents/MacOS/synth-browser-mcp" ]] || die "browser MCP adapter is missing"
  [[ -f "$resources/browser/playwright_backend.mjs" ]] || die "browser backend is missing"
  [[ -x "$node" && -x "$chrome" ]] || die "pinned Node/full Chromium runtime is incomplete"
  verify_code "$app"
  # Node retains the pinned upstream Node.js Foundation Developer ID. It is a
  # separately launched runtime, so require a valid signature but do not
  # pretend it belongs to Workshop's signing team.
  verify_code "$node" no
  verify_code "$(dirname "$(dirname "$(dirname "$chrome")")")"
  verify_notary "$app"
  SYNTH_BROWSER_RUNTIME_OUTPUT="$runtime" "$ROOT/scripts/build-browser-runtime.sh" verify
  verify_code "$app"
  if [[ -d "$helper" ]]; then
    verify_code "$helper"
    verify_notary "$helper"
  fi
  local version runtime_bytes
  version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
  runtime_bytes="$(du -sk "$runtime" | awk '{print $1 * 1024}')"
  jq -n --arg gate installed --arg app "$app" --arg version "$version" --argjson runtimeBytes "$runtime_bytes" \
    --arg nodeVersion "$("$node" --version)" --arg checkedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '{schema:"workshop.browser-production-gate.v1",gate:$gate,passed:true,app:$app,version:$version,nodeVersion:$nodeVersion,runtimeBytes:$runtimeBytes,checkedAt:$checkedAt}' > "$RECEIPT"
  note "installed-app gate passed; receipt: $RECEIPT"
}

verify_development_installed() {
  local app="$1" resources runtime node chrome version runtime_bytes
  [[ -d "$app" ]] || die "installed app is missing: $app"
  resources="$app/Contents/Resources"
  runtime="$resources/browser/runtime"
  node="$runtime/node/bin/node"
  chrome="$(find "$runtime/browsers" -type f -path '*/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing' -print -quit)"
  [[ -x "$app/Contents/MacOS/synth-desktop" ]] || die "Desktop executable is missing"
  [[ -x "$app/Contents/MacOS/synth-browser-mcp" ]] || die "browser MCP adapter is missing"
  [[ -f "$resources/browser/playwright_backend.mjs" && -x "$node" && -x "$chrome" ]] \
    || die "managed browser resources are incomplete"
  verify_code "$app"
  SYNTH_BROWSER_RUNTIME_OUTPUT="$runtime" "$ROOT/scripts/build-browser-runtime.sh" verify
  # Chromium launch is part of the gate. Re-check the outer resource seal
  # afterwards so framework-copy mutations cannot pass on first verification.
  verify_code "$app"
  version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
  runtime_bytes="$(du -sk "$runtime" | awk '{print $1 * 1024}')"
  jq -n --arg gate development-installed --arg app "$app" --arg version "$version" \
    --argjson runtimeBytes "$runtime_bytes" --arg nodeVersion "$("$node" --version)" \
    --arg checkedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '{schema:"workshop.browser-production-gate.v1",gate:$gate,passed:true,productionEligible:false,notarized:false,app:$app,version:$version,nodeVersion:$nodeVersion,runtimeBytes:$runtimeBytes,checkedAt:$checkedAt}' > "$RECEIPT"
  note "development installed-app smoke passed (not a production/notarization gate); receipt: $RECEIPT"
}

verify_updater() {
  local before="$2" after="$3" sentinel="$4" expected before_version after_version
  [[ -d "$before" && -d "$after" ]] || die "updater gate needs before and after .app bundles"
  [[ -f "$sentinel" ]] || die "profile sentinel is missing: $sentinel"
  expected="${SYNTH_PROFILE_SENTINEL_SHA256:-}"
  [[ -n "$expected" ]] || die "set SYNTH_PROFILE_SENTINEL_SHA256 to the pre-update sentinel digest"
  [[ "$(sha256 "$sentinel")" == "$expected" ]] || die "browser profile changed during updater acceptance"
  verify_installed "$before"
  before_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$before/Contents/Info.plist")"
  verify_installed "$after"
  after_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$after/Contents/Info.plist")"
  [[ "$before_version" != "$after_version" ]] || die "updater did not change the app version"
  "$before/Contents/Resources/browser/runtime/node/bin/node" "$ROOT/scripts/browser-profile-compat.mjs" \
    --before "$before" --after "$after" --require-version-change
  jq -n --arg gate updater --arg beforeVersion "$before_version" --arg afterVersion "$after_version" \
    --arg sentinel "$sentinel" --arg sentinelSha256 "$expected" --arg checkedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '{schema:"workshop.browser-production-gate.v1",gate:$gate,passed:true,beforeVersion:$beforeVersion,afterVersion:$afterVersion,profileSentinel:$sentinel,profileSentinelSha256:$sentinelSha256,checkedAt:$checkedAt}' > "$RECEIPT"
  note "updater/profile gate passed; receipt: $RECEIPT"
}

verify_profile_compat() {
  local before="$2" after="$3"
  [[ -d "$before" && -d "$after" ]] || die "profile compatibility needs before and after .app bundles"
  verify_development_installed "$before"
  verify_development_installed "$after"
  "$before/Contents/Resources/browser/runtime/node/bin/node" "$ROOT/scripts/browser-profile-compat.mjs" \
    --before "$before" --after "$after"
}

verify_soak() {
  local app="$2" duration="${3:-1800}"
  [[ -d "$app" ]] || die "Chromium soak needs a packaged .app bundle"
  "$app/Contents/Resources/browser/runtime/node/bin/node" "$ROOT/scripts/browser-soak.mjs" \
    --app "$app" --duration-seconds "$duration" --receipt "$RECEIPT"
}

verify_helper_live() {
  local helper="${2:-${SYNTH_HELPER_APP:-$APP/Contents/Resources/Synth Computer Use.app}}" binary grants
  [[ -x "$helper/Contents/MacOS/synth-computer-use" ]] || die "installed helper is missing: $helper"
  verify_code "$helper"
  verify_notary "$helper"
  binary="$helper/Contents/MacOS/synth-computer-use"
  grants="$("$binary" probe)"
  jq -e '.accessibility == "granted" and .screenRecording == "granted"' <<<"$grants" >/dev/null \
    || die "live helper probe reports missing Accessibility or Screen Recording"
  jq -n --arg gate native-helper-live --arg helper "$helper" --argjson grants "$grants" \
    --arg checkedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '{schema:"workshop.browser-production-gate.v1",gate:$gate,passed:true,helper:$helper,grants:$grants,checkedAt:$checkedAt}' > "$RECEIPT"
  note "signed live-helper gate passed; receipt: $RECEIPT"
}

verify_helper_development() {
  local helper="${2:-${SYNTH_HELPER_APP:-$APP/Contents/Resources/Synth Computer Use.app}}" binary grants signature
  [[ -x "$helper/Contents/MacOS/synth-computer-use" ]] || die "installed helper is missing: $helper"
  verify_code "$helper"
  binary="$helper/Contents/MacOS/synth-computer-use"
  grants="$("$binary" probe)"
  jq -e '.accessibility == "granted" and .screenRecording == "granted"' <<<"$grants" >/dev/null \
    || die "development helper probe reports missing Accessibility or Screen Recording"
  signature="$(/usr/bin/codesign -dvv "$helper" 2>&1 | awk -F= '/^Signature=/{print $2; exit}')"
  jq -n --arg gate native-helper-development --arg helper "$helper" --arg signature "$signature" \
    --argjson grants "$grants" --arg checkedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '{schema:"workshop.browser-production-gate.v1",gate:$gate,passed:true,productionEligible:false,notarized:false,helper:$helper,signature:$signature,grants:$grants,checkedAt:$checkedAt}' > "$RECEIPT"
  note "development live-helper gate passed (not a production/notarization gate); receipt: $RECEIPT"
}

case "$COMMAND" in
  installed) verify_installed "$APP" ;;
  development-installed) verify_development_installed "$APP" ;;
  profile-compat) [[ $# -eq 3 ]] || die "usage: $0 profile-compat BEFORE.app AFTER.app"; verify_profile_compat "$@" ;;
  soak) [[ $# -ge 2 && $# -le 3 ]] || die "usage: $0 soak APP [DURATION_SECONDS]"; verify_soak "$@" ;;
  updater) [[ $# -eq 4 ]] || die "usage: $0 updater BEFORE.app AFTER.app PROFILE_SENTINEL"; verify_updater "$@" ;;
  helper-live) verify_helper_live "$@" ;;
  development-helper-live) verify_helper_development "$@" ;;
  help|-h|--help)
    echo "Usage: $0 installed [APP] | development-installed [APP] | profile-compat BEFORE.app AFTER.app | soak APP [DURATION_SECONDS] | updater BEFORE.app AFTER.app PROFILE_SENTINEL | helper-live [HELPER.app] | development-helper-live [HELPER.app]"
    ;;
  *) die "unknown command: $COMMAND" ;;
esac
