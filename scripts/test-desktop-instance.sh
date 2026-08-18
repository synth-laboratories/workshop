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
mkdir -p "$TEST_ROOT/home/.codex" "$TEST_ROOT/home/.synth-desktop"
printf '{"tokens":{"access_token":"fixture"}}\n' >"$TEST_ROOT/home/.codex/auth.json"
printf 'SYNTH_API_KEY=synth-fixture\nOPENROUTER_API_KEY=openrouter-fixture\nKEEP=yes\n' >"$TEST_ROOT/test-credentials.env"
export SYNTH_DESKTOP_TEST_CREDENTIALS_FILE="$TEST_ROOT/test-credentials.env"
export SYNTH_DESKTOP_DEV_OAUTH_FILE="$TEST_ROOT/home/.codex/auth.json"
export SYNTH_DESKTOP_SHARED_ROOT="$TEST_ROOT/home/.synth-desktop/shared"

alpha="$($ROOT/scripts/desktop-instance.sh print alpha)"
beta="$($ROOT/scripts/desktop-instance.sh print beta)"
default_instance="$($ROOT/scripts/desktop-instance.sh print)"

[[ "$(printf '%s' "$alpha" | jq -r .name)" == "alpha" ]]
[[ "$(printf '%s' "$beta" | jq -r .name)" == "beta" ]]
[[ "$(printf '%s' "$default_instance" | jq -r .name)" == "codex" ]]
printf '%s' "$default_instance" | jq -e '
  .mode == "development" and
  .product == "workshop" and
  .releaseLine == "v0.5" and
  .appVersion == "0.5.0" and
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
printf '%s' "$alpha" | jq -e '
  (.appBundle | endswith("/Synth Workshop v0.5 · alpha.app")) and
  (.executable | endswith("/debug/synth-desktop"))
' >/dev/null

# Refreshing an instance contract after a build or identity assertion must not
# erase the binary provenance those earlier phases recorded.
alpha_manifest="$TEST_ROOT/instances/v05/alpha/instance.json"
jq '.provenance={phase:"bundle-signed", executableDigest:"sha256:fixture"} | .executable="/tmp/Synth Workshop.app/Contents/MacOS/synth-desktop" | .executableDigest="sha256:fixture"' \
  "$alpha_manifest" >"$alpha_manifest.tmp"
mv "$alpha_manifest.tmp" "$alpha_manifest"
alpha_refreshed="$($ROOT/scripts/desktop-instance.sh print alpha)"
printf '%s' "$alpha_refreshed" | jq -e '
  .provenance.phase == "bundle-signed" and
  .provenance.executableDigest == "sha256:fixture" and
  .executable == "/tmp/Synth Workshop.app/Contents/MacOS/synth-desktop" and
  .executableDigest == "sha256:fixture"
' >/dev/null
alpha_env="$TEST_ROOT/instances/v05/alpha/data/.env"
[[ "$(stat -f '%Lp' "$alpha_env")" == "600" ]]
rg -q '^SYNTH_API_KEY=' "$alpha_env"
rg -q '^OPENROUTER_API_KEY=' "$alpha_env"

# Credential refresh updates only the allowlist and preserves instance-local
# non-secret values.
printf 'SYNTH_API_KEY=stale\nOPENROUTER_API_KEY=stale\nLOCAL_ONLY=yes\n' >"$alpha_env"
$ROOT/scripts/desktop-instance.sh print alpha >/dev/null
rg -q '^LOCAL_ONLY=yes$' "$alpha_env"
rg -q '^SYNTH_API_KEY=.synth-fixture.$' "$alpha_env"
rg -q '^OPENROUTER_API_KEY=.openrouter-fixture.$' "$alpha_env"

# Packaged apps must run exclusively from their isolated instance. A cwd or
# runtime fallback under ~/Documents causes macOS Files & Folders prompts.
awk '
  /if \[\[ "\$COMMAND" == "cua"/{in_cua=1}
  in_cua && /cd "\$INSTANCE_ROOT"/{safe_cwd=NR}
  in_cua && /exec "\$app_executable"/{exec_line=NR}
  END { exit !(safe_cwd && exec_line && safe_cwd < exec_line) }
' "$ROOT/scripts/desktop-instance.sh"
rg -q 'if \(\$0 == exe \|\| \$0 == cua_exe\)' "$ROOT/scripts/desktop-instance.sh"
rg -q 'SYNTH_DESKTOP_DEV_OAUTH_FILE' "$ROOT/scripts/desktop-instance.sh"
rg -q 'SYNTH_DESKTOP_DEV_OAUTH_STATE_FILE' "$ROOT/scripts/desktop-instance.sh"
rg -q '\.synth-desktop/shared' "$ROOT/scripts/desktop-instance.sh"
if rg -q 'SYNTH_DESKTOP_DEV_SHARE_CANONICAL_OAUTH|synth-desktop-dev-\$NAME' "$ROOT/scripts/desktop-instance.sh"; then
  echo "Desktop CUA launcher still contains a Keychain credential path" >&2
  exit 1
fi
if rg -n 'home\.join\("Documents/|dirs::home_dir\(\).*Documents/|\.join\("Documents/' \
  "$ROOT/apps/synth_desktop/src-tauri/src/optimizers/recipes.rs" \
  "$ROOT/apps/synth_desktop/src-tauri/src/optimizers/sft_recipes.rs" \
  "$ROOT/apps/synth_desktop/src-tauri/src/trace_ingest.rs" >/dev/null; then
  echo "Desktop runtime still probes protected Documents paths" >&2
  exit 1
fi

if "$ROOT/scripts/desktop-instance.sh" print '../unsafe' >/dev/null 2>&1; then
  echo "unsafe instance name was accepted" >&2
  exit 1
fi
if SYNTH_DESKTOP_RELEASE_LINE=v0.1 "$ROOT/scripts/desktop-instance.sh" print alpha >/dev/null 2>&1; then
  echo "non-v0.5 release line was accepted by the v0.5 launcher" >&2
  exit 1
fi

jq -e '
  .identifier == "com.synth.desktop.v05.dev.alpha" and
  .productName == "Synth Workshop v0.5 · alpha" and
  .version == "0.5.0" and
  (.bundle.icon | length) == 2 and
  .bundle.targets == ["app"] and
  (.bundle.resources | to_entries | map(.value) | sort) == [
    "cookbooks/optimizers/gepa/banking77_container",
    "cookbooks/optimizers/gepa/crafter_container"
  ] and
  .bundle.macOS.minimumSystemVersion == "14.0"
' \
  "$TEST_ROOT/instances/v05/alpha/generated/tauri.instance.json" >/dev/null
jq -e '.bundle.macOS.minimumSystemVersion == "14.0"' \
  "$ROOT/apps/synth_desktop/src-tauri/tauri.conf.json" >/dev/null
jq -e '.bundle.resources["generated-resources/cookbooks"] == "cookbooks"' \
  "$ROOT/apps/synth_desktop/src-tauri/tauri.conf.json" >/dev/null

# Local CUA builds sign with the stable local certificate by default so TCC
# and Keychain grants survive rebuilds; ad-hoc is an explicit opt-out. The
# deprecated `--deep` signing flag stamps the outer identifier onto nested
# code and must never return; the bundle signs under its own $BUNDLE_ID and
# every build asserts its designated requirement is not cdhash-anchored.
rg -q 'SYNTH_DESKTOP_USE_DEV_SIGNER:-1' "$ROOT/scripts/desktop-instance.sh"
! rg -q -- 'codesign --force --deep' "$ROOT/scripts/desktop-instance.sh"
rg -q -- '--identifier "\$BUNDLE_ID" "\$app_bundle"' "$ROOT/scripts/desktop-instance.sh"
rg -q 'assert_bundle_identity' "$ROOT/scripts/desktop-instance.sh"
rg -q 'SYNTH_DESKTOP_REBUILD_ADAPTERS:-0' "$ROOT/scripts/desktop-instance.sh"
rg -q 'SYNTH_OPTIMIZER_USE_LOCAL_SOURCE:-0' "$ROOT/scripts/desktop-instance.sh"
rg -q 'optimizer runtime=immutable installed plugin' "$ROOT/scripts/desktop-instance.sh"
rg -q 'verify_packaged_provenance' "$ROOT/scripts/desktop-instance.sh"
rg -q 'runtime_executable=.*lsof' "$ROOT/scripts/desktop-instance.sh"
! rg -q 'bundle_cdhash.*exit|/\^CDHash=/\{print \$2; exit\}' "$ROOT/scripts/desktop-instance.sh"

# Canonical lifecycle commands must never stop an arbitrary copied app or a
# named development instance. Exact executable paths are the process authority.
# The stand-in binary must be compiled locally: copies of Apple-signed system
# binaries (e.g. /bin/sleep) are SIGKILLed by AMFI on Apple Silicon.
UNRELATED_APP="$TEST_ROOT/Unrelated/Synth Desktop.app"
mkdir -p "$UNRELATED_APP/Contents/MacOS"
printf '#include <unistd.h>\n#include <stdlib.h>\nint main(int argc, char **argv) { sleep(argc > 1 ? (unsigned)atoi(argv[1]) : 30); return 0; }\n' > "$TEST_ROOT/unrelated_sleep.c"
cc -o "$UNRELATED_APP/Contents/MacOS/synth-desktop" "$TEST_ROOT/unrelated_sleep.c"
"$UNRELATED_APP/Contents/MacOS/synth-desktop" 30 &
UNRELATED_PID="$!"
SYNTH_DESKTOP_APP_PATH="$TEST_ROOT/Canonical.app" "$ROOT/scripts/desktop.sh" stop >/dev/null
kill -0 "$UNRELATED_PID"

echo "desktop instance contract: ok"
