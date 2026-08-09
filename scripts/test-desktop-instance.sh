#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_ROOT="$(mktemp -d)"
UNRELATED_PID=""
cleanup() {
  [[ -z "$UNRELATED_PID" ]] || kill "$UNRELATED_PID" 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

export SYNTH_DESKTOP_INSTANCES_ROOT="$TEST_ROOT/instances"

alpha="$($ROOT/scripts/desktop-instance.sh print alpha)"
beta="$($ROOT/scripts/desktop-instance.sh print beta)"
default_instance="$($ROOT/scripts/desktop-instance.sh print)"

[[ "$(printf '%s' "$alpha" | jq -r .name)" == "alpha" ]]
[[ "$(printf '%s' "$beta" | jq -r .name)" == "beta" ]]
[[ "$(printf '%s' "$default_instance" | jq -r .name)" == "codex" ]]
printf '%s' "$default_instance" | jq -e '
  .mode == "development" and
  (.sourceRoot | length > 0) and
  (.sourceRevision | length > 0) and
  .hotReload.renderer == true and
  .hotReload.rust == true and
  .hotReload.viteUrl == .viteUrl
' >/dev/null
[[ "$(printf '%s' "$alpha" | jq -r .bundleId)" != "$(printf '%s' "$beta" | jq -r .bundleId)" ]]
[[ "$(printf '%s' "$alpha" | jq -r .dataRoot)" != "$(printf '%s' "$beta" | jq -r .dataRoot)" ]]
[[ "$(printf '%s' "$alpha" | jq -r .workspace)" != "$(printf '%s' "$beta" | jq -r .workspace)" ]]
[[ "$(printf '%s' "$alpha" | jq -r .cargoTargetDir)" != "$(printf '%s' "$beta" | jq -r .cargoTargetDir)" ]]
[[ "$(printf '%s' "$alpha" | jq -r .viteUrl)" != "$(printf '%s' "$beta" | jq -r .viteUrl)" ]]
[[ "$(printf '%s' "$alpha" | jq -r .iconLabel)" == "1" ]]
[[ "$(printf '%s' "$beta" | jq -r .iconLabel)" == "2" ]]
[[ -f "$(printf '%s' "$alpha" | jq -r .icon)" ]]

if "$ROOT/scripts/desktop-instance.sh" print '../unsafe' >/dev/null 2>&1; then
  echo "unsafe instance name was accepted" >&2
  exit 1
fi

jq -e '.identifier == "com.synth.desktop.dev.alpha" and .productName == "Synth Desktop · alpha" and (.bundle.icon | length) == 2' \
  "$TEST_ROOT/instances/alpha/generated/tauri.instance.json" >/dev/null

# Canonical lifecycle commands must never stop an arbitrary copied app or a
# named development instance. Exact executable paths are the process authority.
UNRELATED_APP="$TEST_ROOT/Unrelated/Synth Desktop.app"
mkdir -p "$UNRELATED_APP/Contents/MacOS"
cp /bin/sleep "$UNRELATED_APP/Contents/MacOS/synth-desktop"
"$UNRELATED_APP/Contents/MacOS/synth-desktop" 30 &
UNRELATED_PID="$!"
SYNTH_DESKTOP_APP_PATH="$TEST_ROOT/Canonical.app" "$ROOT/scripts/desktop.sh" stop >/dev/null
kill -0 "$UNRELATED_PID"

echo "desktop instance contract: ok"
