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
  local target="$1"
  /usr/bin/codesign --verify --strict --deep "$target"
  if [[ -n "$TEAM_ID" ]]; then
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
  verify_code "$node"
  verify_code "$(dirname "$(dirname "$(dirname "$chrome")")")"
  verify_notary "$app"
  SYNTH_BROWSER_RUNTIME_OUTPUT="$runtime" "$ROOT/scripts/build-browser-runtime.sh" verify
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
  jq -n --arg gate updater --arg beforeVersion "$before_version" --arg afterVersion "$after_version" \
    --arg sentinel "$sentinel" --arg sentinelSha256 "$expected" --arg checkedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '{schema:"workshop.browser-production-gate.v1",gate:$gate,passed:true,beforeVersion:$beforeVersion,afterVersion:$afterVersion,profileSentinel:$sentinel,profileSentinelSha256:$sentinelSha256,checkedAt:$checkedAt}' > "$RECEIPT"
  note "updater/profile gate passed; receipt: $RECEIPT"
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

case "$COMMAND" in
  installed) verify_installed "$APP" ;;
  updater) [[ $# -eq 4 ]] || die "usage: $0 updater BEFORE.app AFTER.app PROFILE_SENTINEL"; verify_updater "$@" ;;
  helper-live) verify_helper_live "$@" ;;
  help|-h|--help)
    echo "Usage: $0 installed [APP] | updater BEFORE.app AFTER.app PROFILE_SENTINEL | helper-live [HELPER.app]"
    ;;
  *) die "unknown command: $COMMAND" ;;
esac
