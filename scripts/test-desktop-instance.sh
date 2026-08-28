#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_ROOT="$(mktemp -d)"
UNRELATED_PID=""
LOCK_HOLDER=""
cleanup() {
  [[ -z "$UNRELATED_PID" ]] || kill "$UNRELATED_PID" 2>/dev/null || true
  [[ -z "$LOCK_HOLDER" ]] || kill "$LOCK_HOLDER" 2>/dev/null || true
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
[[ "$(printf '%s' "$default_instance" | jq -r .name)" == "codex-$(printf '%s' "$default_instance" | jq -r .worktreeHash)" ]]
[[ "$(printf '%s' "$default_instance" | jq -r .name)" =~ ^codex-[a-f0-9]{8}$ ]]
[[ "$(printf '%s' "$default_instance" | jq -r .releaseSlug)" == "v08" ]]
[[ "$(printf '%s' "$default_instance" | jq -r .instanceRoot)" == "$TEST_ROOT/instances/v08/$(printf '%s' "$default_instance" | jq -r .name)" ]]
printf '%s' "$default_instance" | jq -e '
  .mode == "development" and
  .product == "workshop" and
  .releaseLine == "v0.8" and
  .appVersion == "0.8.0" and
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
  (.appBundle | endswith("/Synth Workshop v0.8 · alpha.app")) and
  (.executable | endswith("/debug/synth-desktop"))
' >/dev/null

# Refreshing an instance contract after a build or identity assertion must not
# erase the binary provenance those earlier phases recorded.
alpha_manifest="$TEST_ROOT/instances/v08/alpha/instance.json"
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
alpha_env="$TEST_ROOT/instances/v08/alpha/data/.env"
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
dev_instance_body="$(sed -n '/^dev_instance()/,/^}/p' "$ROOT/scripts/desktop-instance.sh")"
awk '
  /cd "\$INSTANCE_ROOT"/ && !safe_cwd {safe_cwd=NR}
  /exec_isolated_cua_bundle/ && !launch_line {launch_line=NR}
  END { exit !(safe_cwd && launch_line && safe_cwd < launch_line) }
' <<<"$dev_instance_body"
# The helper itself must end in an environment-scrubbed exec of the recorded
# bundle executable; checking a removed inline exec made this gate stale while
# missing the stronger isolation contract.
isolated_exec="$(sed -n '/^exec_isolated_cua_bundle()/,/^}/p' "$ROOT/scripts/desktop-instance.sh")"
grep -q 'exec env -i' <<<"$isolated_exec"
grep -q 'PWD="\$INSTANCE_ROOT"' <<<"$isolated_exec"
grep -q 'SYNTH_OPTIMIZER_PROJECT_ROOT="\$optimizer_project_root"' <<<"$isolated_exec"
grep -q 'CONTAINERS_ROOT="\$containers_root"' <<<"$isolated_exec"
grep -q '"\$CUA_EXE"' <<<"$isolated_exec"
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
if SYNTH_DESKTOP_RELEASE_LINE=v0.6 "$ROOT/scripts/desktop-instance.sh" print alpha >/dev/null 2>&1; then
  echo "non-v0.8 release line was accepted by the v0.8 launcher" >&2
  exit 1
fi

jq -e '
  .identifier == "com.synth.desktop.v08.dev.alpha" and
  .productName == "Synth Workshop v0.8 · alpha" and
  .version == "0.8.0" and
  (.bundle.icon | length) == 2 and
  .bundle.targets == ["app"] and
  .bundle.resources == {} and
  .bundle.macOS.minimumSystemVersion == "14.0"
' \
  "$TEST_ROOT/instances/v08/alpha/generated/tauri.instance.json" >/dev/null
jq -e '.bundle.macOS.minimumSystemVersion == "14.0"' \
  "$ROOT/apps/synth_desktop/src-tauri/tauri.conf.json" >/dev/null
# Packaged resources live in the packaging overlay, not the base config, so a
# fresh worktree can `cargo check` without staging cookbooks or the helper.
jq -e '.bundle | has("resources") | not' \
  "$ROOT/apps/synth_desktop/src-tauri/tauri.conf.json" >/dev/null
jq -e '.bundle.resources | has("generated-resources/cookbooks") | not' \
  "$ROOT/apps/synth_desktop/src-tauri/tauri.package.json" >/dev/null

# Local CUA builds sign with the stable local certificate by default so TCC
# and Keychain grants survive rebuilds; ad-hoc is an explicit opt-out. The
# deprecated `--deep` signing flag stamps the outer identifier onto nested
# code and must never return; the bundle signs under its own $BUNDLE_ID and
# every build asserts its designated requirement is not cdhash-anchored.
rg -q 'SYNTH_DESKTOP_USE_DEV_SIGNER:-1' "$ROOT/scripts/desktop-instance.sh"
! rg -q -- 'codesign --force --deep' "$ROOT/scripts/desktop-instance.sh"
rg -q -- '--identifier "\$BUNDLE_ID" "\$app_bundle"' "$ROOT/scripts/desktop-instance.sh"
rg -q 'assert_bundle_identity' "$ROOT/scripts/desktop-instance.sh"
# An explicit ad-hoc rebuild must replace any certificate-backed signing
# record retained by write_contract instead of leaving stale manifest truth.
rg -q 'record_bundle_signing "\$app_bundle"' "$ROOT/scripts/desktop-instance.sh"
rg -Fq -- '--arg identity "${host_authority:-adhoc}"' "$ROOT/scripts/desktop-instance.sh"
rg -q 'SYNTH_DESKTOP_REBUILD_ADAPTERS:-0' "$ROOT/scripts/desktop-instance.sh"
rg -q 'SYNTH_OPTIMIZER_USE_LOCAL_SOURCE:-0' "$ROOT/scripts/desktop-instance.sh"
rg -q 'SYNTH_COMPUTER_USE_PARENT_REQUIREMENT=' "$ROOT/scripts/desktop-instance.sh"
rg -q 'optimizer runtime=immutable installed plugin' "$ROOT/scripts/desktop-instance.sh"
rg -q 'verify_packaged_provenance' "$ROOT/scripts/desktop-instance.sh"
rg -q 'packaging_preflight' "$ROOT/scripts/desktop-instance.sh"
rg -q 'missing Computer Use helper bundle' "$ROOT/scripts/desktop-instance.sh"
! rg -q 'missing staged cookbooks' "$ROOT/scripts/desktop-instance.sh"
rg -q 'dirty source tree' "$ROOT/scripts/desktop-instance.sh"
rg -q 'insufficient disk' "$ROOT/scripts/desktop-instance.sh"
rg -q 'signing identity not in keychain' "$ROOT/scripts/desktop-instance.sh"
rg -q 'CARGO_TARGET_DIR:-' "$ROOT/scripts/build-computer-use-helper.sh"
rg -q 'runtime_executable=.*lsof' "$ROOT/scripts/desktop-instance.sh"
! rg -q 'bundle_cdhash.*exit|/\^CDHash=/\{print \$2; exit\}' "$ROOT/scripts/desktop-instance.sh"

# ID-R-14: one export_instance_env(), called once, and both launch paths
# (build/launch vs run-only) export the same variable names. Names only; the
# dry-run hook prints no values.
[[ "$(rg -c '^\s*export SYNTH_DESKTOP_DATA_ROOT=' "$ROOT/scripts/desktop-instance.sh")" == "1" ]]
export_env_calls="$(rg -c '^\s*export_instance_env$' "$ROOT/scripts/desktop-instance.sh")"
[[ "$export_env_calls" -ge 2 ]]
env_names_for() {
  SYNTH_DESKTOP_OPERATION_DRY_RUN=1 "$ROOT/scripts/desktop-instance.sh" "$1" alpha \
    | sed -n 's/^\[desktop:alpha\] dry-run env_names=//p'
}
build_names="$(env_names_for cua-build)"
run_names="$(env_names_for cua-run)"
rebuild_names="$(env_names_for rebuild-run)"
[[ -n "$build_names" && "$build_names" == "$run_names" && "$run_names" == "$rebuild_names" ]]
for required in SYNTH_DESKTOP_DATA_ROOT SYNTH_DESKTOP_CONFIG SYNTH_CODEX_HOME \
  SYNTH_DESKTOP_SOURCE_REVISION \
  SYNTH_DESKTOP_DEV_OAUTH_STATE_FILE SYNTH_COMPUTER_USE_PARENT_REQUIREMENT CARGO_TARGET_DIR; do
  [[ ",$build_names," == *",$required,"* ]] || { echo "launch env missing $required" >&2; exit 1; }
done

# Official releases fail closed unless Developer ID signing, Apple notarization,
# stapling, Gatekeeper, and immutable provenance all succeed.
rg -q 'SYNTH_RELEASE_SIGN_IDENTITY is required' "$ROOT/scripts/release-artifact.sh"
rg -q 'SYNTH_RELEASE_NOTARY_PROFILE is required' "$ROOT/scripts/release-artifact.sh"
rg -q 'notarytool submit.*--wait' "$ROOT/scripts/release-artifact.sh"
rg -q 'stapler staple' "$ROOT/scripts/release-artifact.sh"
rg -q 'source=Notarized Developer ID' "$ROOT/scripts/release-artifact.sh"
rg -q 'notarized:true, stapled:true' "$ROOT/scripts/release-artifact.sh"
! rg -q 'UNNOTARIZED' "$ROOT/scripts/release-artifact.sh"
! rg -q 'awk.*\{print \$2; exit\}' "$ROOT/scripts/release-artifact.sh"
! rg -q '\[\[ -d "\$CONTAINERS_ROOT/.git" \]\]' "$ROOT/scripts/release-artifact.sh"

# Pre-notary acceptance uses an explicit candidate lane. It records the lack of
# notarization, preserves the official app, and never calls Apple's notary API.
rg -q 'candidate-all' "$ROOT/scripts/release-artifact.sh"
rg -q 'distribution:"candidate"' "$ROOT/scripts/release-artifact.sh"
rg -q 'notarized:false, stapled:false' "$ROOT/scripts/release-artifact.sh"
rg -q 'Synth Workshop Candidate.app' "$ROOT/scripts/release-artifact.sh"
rg -q 'com.synth.desktop.v08.candidate' "$ROOT/apps/synth_desktop/src-tauri/tauri.candidate.conf.json"
candidate_case="$(sed -n '/candidate-stage)/,/help|-h|--help)/p' "$ROOT/scripts/release-artifact.sh")"
! grep -q 'notarize_artifact' <<<"$candidate_case"

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

# P1-9: exclusive instance operation lease. A second cua-build is refused
# with the owner printed; status --verbose shows that owner. The first
# process is a dry-run that holds the lock — not a 10-minute cua-build.
lock_file="$TEST_ROOT/instances/v08/alpha/operation.lock"
SYNTH_DESKTOP_OPERATION_DRY_RUN=1 SYNTH_DESKTOP_OPERATION_LOCK_HOLD=1 \
  "$ROOT/scripts/desktop-instance.sh" cua-build alpha >/dev/null &
LOCK_HOLDER="$!"
lock_wait=0
while [[ ! -f "$lock_file" && "$lock_wait" -lt 50 ]]; do
  sleep 0.1
  lock_wait=$((lock_wait + 1))
done
[[ -f "$lock_file" ]] || { echo "operation lock was not created" >&2; kill "$LOCK_HOLDER" 2>/dev/null || true; exit 1; }
jq -e '.instance == "alpha" and (.pid | type == "number") and
  (.process_start_time | length > 0) and (.worktree | length > 0) and
  (.repo_revision | length > 0) and .operation == "cua-build" and
  (.created_at | length > 0)' "$lock_file" >/dev/null
status_verbose="$($ROOT/scripts/desktop-instance.sh status --verbose alpha)"
case "$status_verbose" in
  *"owner instance=alpha pid="*) ;;
  *) echo "status --verbose did not print the lock owner: $status_verbose" >&2; kill "$LOCK_HOLDER" 2>/dev/null || true; exit 1 ;;
esac
set +e
refuse_out="$(SYNTH_DESKTOP_OPERATION_DRY_RUN=1 "$ROOT/scripts/desktop-instance.sh" cua-build alpha 2>&1)"
refuse_status=$?
set -e
[[ "$refuse_status" -ne 0 ]] || { echo "second cua-build was not refused" >&2; kill "$LOCK_HOLDER" 2>/dev/null || true; exit 1; }
case "$refuse_out" in
  *"owner instance=alpha pid="*" worktree="*" operation=cua-build"*) ;;
  *) echo "second cua-build did not print the owner: $refuse_out" >&2; kill "$LOCK_HOLDER" 2>/dev/null || true; exit 1 ;;
esac
kill "$LOCK_HOLDER" 2>/dev/null || true
wait "$LOCK_HOLDER" 2>/dev/null || true
LOCK_HOLDER=""

# ID-R-15: workshop-qa must not signal by instance name or path substring.
if rg -n 'kill -TERM|\[\[ "\$command" == \*"\$staged_root"\* \]\]' "$ROOT/scripts/workshop-qa" >/dev/null; then
  echo "workshop-qa still sweeps processes by path substring" >&2
  exit 1
fi
rg -q 'SYNTH_WORKSHOP_INSTANCE_ID' "$ROOT/scripts/desktop-instance.sh"
rg -q 'qa-\$WORKTREE_HASH|qa-\$\{WORKTREE_HASH\}' "$ROOT/scripts/workshop-qa"
rg -q 'codex-\$WORKTREE_HASH|codex-\$\{WORKTREE_HASH\}' "$ROOT/scripts/crash-recovery-drill.sh"

# P1-5: cua-build writes a bundle-resident descriptor with the W3b contract
# and records bootEpoch + processStartIdentity on the launcher manifest.
SYNTH_DESKTOP_OPERATION_DRY_RUN=1 "$ROOT/scripts/desktop-instance.sh" cua-build alpha >/dev/null
descriptor="$TEST_ROOT/instances/v08/alpha/generated/descriptor-preview.app/Contents/Resources/instance.json"
[[ -f "$descriptor" ]] || { echo "bundle descriptor was not written" >&2; exit 1; }
jq -e '
  .schemaVersion == "synth.desktop.instance-descriptor.v1" and
  .instance_id == "alpha" and
  (.instance_root | endswith("/instances/v08/alpha")) and
  (.config_path | endswith("/instances/v08/alpha/data/config.toml")) and
  (.data_root | endswith("/instances/v08/alpha/data")) and
  .bundle_id == "com.synth.desktop.v08.dev.alpha" and
  .release_line == "v0.8" and
  (.source_revision | length > 0) and
  (.generated_at | length > 0)
' "$descriptor" >/dev/null
jq -e '
  (.runtime.bootEpoch | startswith("inst_")) and
  (.runtime.processStartIdentity | length > 0)
' "$TEST_ROOT/instances/v08/alpha/instance.json" >/dev/null
rg -q 'write_bundle_descriptor "\$app_bundle"' "$ROOT/scripts/desktop-instance.sh"

# ID-R-05: task scripts source RELEASE_SLUG from the launcher print contract.
if rg -q 'RELEASE_SLUG="v05"|/v05/\$NAME|== "0\.5\.0"' "$ROOT/scripts/workshop-qa" "$ROOT/scripts/crash-recovery-drill.sh"; then
  echo "workshop-qa or crash-recovery-drill still hard-codes v05/0.5.0" >&2
  exit 1
fi
rg -q 'jq -r \.releaseSlug' "$ROOT/scripts/workshop-qa"
rg -q 'jq -r \.releaseSlug' "$ROOT/scripts/crash-recovery-drill.sh"

# P1-8: rebuild-run exists, composes the recorded launch path, and cua-run's
# drift refusal names it. Do not launch or package a .app here.
rebuild_help="$($ROOT/scripts/desktop-instance.sh help 2>&1 || true)"
case "$rebuild_help" in
  *"rebuild-run"*) ;;
  *) echo "usage does not mention rebuild-run" >&2; exit 1 ;;
esac
rebuild_dry="$(SYNTH_DESKTOP_OPERATION_DRY_RUN=1 "$ROOT/scripts/desktop-instance.sh" rebuild-run alpha)"
case "$rebuild_dry" in
  *"rebuild-run steps=build,bundle,sign,record,verify,launch,wait-health,print-runtime"*) ;;
  *) echo "rebuild-run did not compose the recorded steps: $rebuild_dry" >&2; exit 1 ;;
esac
case "$rebuild_dry" in
  *"/health.instance == alpha"*) ;;
  *) echo "rebuild-run did not wait for /health.instance: $rebuild_dry" >&2; exit 1 ;;
esac
rg -q 'wait_for_health_instance' "$ROOT/scripts/desktop-instance.sh"
rg -q 'print_runtime_identity' "$ROOT/scripts/desktop-instance.sh"
rg -q 'verify_packaged_provenance' "$ROOT/scripts/desktop-instance.sh"
rebuild_body="$(sed -n '/^rebuild_run_instance()/,/^}/p' "$ROOT/scripts/desktop-instance.sh")"
case "$rebuild_body" in
  *"observe_rebuild_readiness &"*"exec_isolated_cua_bundle"*) ;;
  *) echo "rebuild-run did not keep the app on cua-run's foreground exec path" >&2; exit 1 ;;
esac
case "$rebuild_body" in
  *'exec_isolated_cua_bundle &'*|*'"$CUA_EXE" &'*) echo "rebuild-run launched the app asynchronously" >&2; exit 1 ;;
  *) ;;
esac
readiness_body="$(sed -n '/^observe_rebuild_readiness()/,/^}/p' "$ROOT/scripts/desktop-instance.sh")"
grep -q 'trap - EXIT' <<<"$readiness_body"
grep -q 'wait_for_health_instance' <<<"$readiness_body"
grep -q 'print_runtime_identity' <<<"$readiness_body"
set +e
drift_out="$($ROOT/scripts/desktop-instance.sh cua-run alpha 2>&1)"
drift_status=$?
set -e
[[ "$drift_status" -ne 0 ]] || { echo "cua-run accepted an unrecorded bundle" >&2; exit 1; }
case "$drift_out" in
  *"bundle was not produced by cua-build; run desktop-instance.sh rebuild-run alpha"*) ;;
  *) echo "cua-run drift message did not name rebuild-run: $drift_out" >&2; exit 1 ;;
esac

echo "desktop instance contract: ok"
