#!/usr/bin/env bash
set -euo pipefail

workshop_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
github_root="$(dirname "$workshop_root")"
containers_root="${SYNTH_CONTAINERS_REPO:-$github_root/containers}"
default_bundle="$github_root/evals/data/results/harbor-coding-laguna-xs-modal-canary/20260804T032521Z/laguna-xs-edit-json/harbor/workspace/logs/trace_v5"
trace_bundle="${SYNTH_TRACE_V5_REAL_BUNDLE:-$default_bundle}"

if [[ ! -e "$trace_bundle" ]]; then
  echo "Trace V5 smoke bundle not found: $trace_bundle" >&2
  echo "Set SYNTH_TRACE_V5_REAL_BUNDLE to a bundle directory or deterministic ZIP." >&2
  exit 2
fi

"$containers_root/scripts/register-local-dev-build.sh" >/dev/null
containers_version=$(uv run --directory "$containers_root" python -c 'import tomllib; print(tomllib.load(open("pyproject.toml", "rb"))["project"]["version"])')
trace_cli="$HOME/.synth-desktop/dev-builds/synth-containers/$containers_version/current/.venv/bin/synth-trace"
if [[ ! -x "$trace_cli" ]]; then
  echo "synth-trace was not installed at $trace_cli" >&2
  exit 2
fi

staging="$(mktemp -d -t synth-trace-v5-real-smoke)"
cleanup() {
  rm -rf "$staging"
}
trap cleanup EXIT

staged_bundle="$staging/bundle"
if [[ -d "$trace_bundle" ]]; then
  cp -R "$trace_bundle" "$staged_bundle"
else
  "$trace_cli" extract "$trace_bundle" "$staged_bundle" >/dev/null
fi

# Older valid V5 bundles predate the canonical viewer packet. Project it from
# sealed authority in the temporary copy; never mutate the dogfood source.
"$trace_cli" project "$staged_bundle" --format rollout-inspector >/dev/null

inspection="$($trace_cli inspect-input "$staged_bundle")"
python3 -c 'import json,sys; p=json.load(sys.stdin); assert p["trusted"] and p["validation"]["valid"] and p["traces"], p' <<<"$inspection"

cd "$workshop_root"
SYNTH_TRACE_V5_REAL_BUNDLE="$staged_bundle" \
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml \
  --test trace_v5_e2e \
  imports_real_bundle_into_trusted_catalog_and_keeps_duplicate_identity \
  -- --ignored --nocapture
