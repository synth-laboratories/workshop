#!/usr/bin/env bash
# Workshop v0.2 Wave 0 — SynthStyle CONFORM CHECKS for apps/synth_desktop.
#
# Prints labeled baseline counts for every check in SynthStyle
# § WORKSHOP CONFORMANCE AUDIT → CONFORM CHECKS. Each count may only decrease
# over time. Patterns are relative to apps/synth_desktop (src-tauri/src,
# src/renderer/src).
#
# Usage (from repo root):
#   ./scripts/conform-desktop.sh
#   ./scripts/desktop.sh conform
#
# Paragons cited by Wave 0 scaffolding: objective_tests, real_fixtures
# (enforcement only — no product refactor in this script).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/apps/synth_desktop"

if [[ ! -d "$DESKTOP/src-tauri/src" || ! -d "$DESKTOP/src/renderer/src" ]]; then
  echo "[conform] missing apps/synth_desktop sources under $DESKTOP" >&2
  exit 1
fi

if ! command -v rg >/dev/null 2>&1; then
  echo "[conform] ripgrep (rg) is required" >&2
  exit 1
fi

# Sum rg -c output (path:count lines, or a bare count for a single file).
count_rg() {
  local pattern="$1"
  shift
  local out
  out="$(rg -c --no-messages "$pattern" "$@" 2>/dev/null || true)"
  if [[ -z "$out" ]]; then
    printf '%s' 0
    return
  fi
  printf '%s\n' "$out" | awk '
    {
      if (match($0, /:[0-9]+$/)) {
        s += substr($0, RSTART + 1) + 0
      } else if ($0 ~ /^[0-9]+$/) {
        s += $0 + 0
      }
    }
    END { print s + 0 }
  '
}

cd "$DESKTOP"

map_err_to_string="$(count_rg 'map_err\(\|e\| e\.to_string\(\)\)' src-tauri/src)"
to_string_contains="$(count_rg '\.to_string\(\)\.contains\(' src-tauri/src)"
# Pattern as published in SynthStyle CONFORM CHECKS (Wave 1 magic status strings).
status_magic="$(count_rg 'status == "|"running"|"interrupted"' src-tauri/src/codex.rs)"
target_json_kind="$(count_rg 'target_json\["kind"\]|\bkind\b == "codex"|== "intern"' src-tauri/src)"
static_once_lock="$(count_rg 'static .*OnceLock' src-tauri/src)"
client_new="$(count_rg 'Client::new\(\)' src-tauri/src)"
window_synth="$(count_rg 'window\.synth' src/renderer/src --glob '!runtime/desktopBridge.ts')"
is_tauri_ternary="$(count_rg 'isTauri \?' src/renderer/src)"
env_d_ts_lines="$(wc -l < src/renderer/src/env.d.ts | tr -d '[:space:]')"
use_state_app="$(count_rg 'useState' src/renderer/src/App.tsx)"
invoke_string="$(count_rg 'invoke\("' src/renderer/src --glob '!**/generated/**')"

cat <<EOF
[conform] apps/synth_desktop SynthStyle CONFORM CHECKS (counts may only decrease)
[conform] map_err_to_string          ${map_err_to_string}    # W6 → 0   rg map_err(|e| e.to_string())
[conform] to_string_contains         ${to_string_contains}    # W6 → 0   rg .to_string().contains(
[conform] status_magic_codex         ${status_magic}    # W1 → 0   rg status == "|"running"|"interrupted" codex.rs
[conform] target_json_kind           ${target_json_kind}    # W1 → 0   rg target_json["kind"]|kind == "codex"|== "intern"
[conform] static_once_lock           ${static_once_lock}    # W5 → 0   rg static .*OnceLock
[conform] client_new                 ${client_new}    # W5 → 0   rg Client::new()
[conform] window_synth               ${window_synth}    # W3 ↓     rg window.synth (excl. desktopBridge.ts)
[conform] is_tauri_ternary           ${is_tauri_ternary}    # W3/W9 → 0  rg 'isTauri ?'
[conform] env_d_ts_lines             ${env_d_ts_lines}    # W2 → <100  wc -l env.d.ts
[conform] use_state_app              ${use_state_app}    # W3 → <10  rg useState App.tsx
[conform] invoke_string              ${invoke_string}    # W2 → 0   rg invoke(" (excl. generated/)
EOF
