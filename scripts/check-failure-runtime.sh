#!/usr/bin/env bash
# Mechanical enforcement for the failure runtime.
# See notes/specifications/workshop/failure_runtime.md
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/apps/synth_desktop/src-tauri/src"
fail=0

deny() {
  local pattern="$1"
  local path="$2"
  local msg="$3"
  if rg -n --glob '*.rs' --glob '!**/emergency_sink.rs' "$pattern" "$path" >/dev/null; then
    echo "FAIL: $msg"
    rg -n --glob '*.rs' --glob '!**/emergency_sink.rs' "$pattern" "$path" || true
    fail=1
  fi
}

deny 'eprintln!' "$SRC" "eprintln! outside emergency_sink.rs"
deny 'AppError::message' "$SRC" "AppError::message must not return"
deny 'impl From<String> for AppError' "$SRC/error.rs" "From<String> for AppError is forbidden"
deny 'impl From<&str> for AppError' "$SRC/error.rs" "From<&str> for AppError is forbidden"
deny 'error: Option<Value>' "$SRC/platform" "raw error JSON on platform contracts"
deny 'error: Option<String>' "$SRC/platform" "raw error strings on platform contracts"

if rg -n 'fn .*unknown' "$ROOT/apps/synth_desktop/src/renderer/src/runtime/publicError.ts" | rg -v 'reason: unknown' >/dev/null; then
  true
fi

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi
echo "failure-runtime checks passed"
