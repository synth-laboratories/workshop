#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
failed=0
check() {
  local name="$1" command_name="$2"
  if command -v "$command_name" >/dev/null 2>&1; then
    printf 'ok   %s: %s\n' "$name" "$(command -v "$command_name")"
  else
    printf 'miss %s: install %s\n' "$name" "$command_name" >&2
    failed=1
  fi
}

check Git git
check Node node
check npm npm
check Rust rustc
check Cargo cargo
check Python python3
check jq jq
check curl curl

[[ "$(uname -s)" == "Darwin" ]] || { echo "miss platform: Workshop v0.8 builds on macOS." >&2; failed=1; }
[[ -f "$repo_root/apps/synth_desktop/src/renderer/src/generated/protocol.ts" ]] || { echo "miss generated protocol bindings" >&2; failed=1; }
[[ -f "$repo_root/contracts/release-tiers-v1.toml" ]] || { echo "miss release tier contract" >&2; failed=1; }

if [[ "$failed" -ne 0 ]]; then exit 1; fi
echo "Workshop build prerequisites are ready."
