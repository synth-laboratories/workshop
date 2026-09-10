#!/usr/bin/env bash
# Shared by public distributions and first-class local builds; no Keychain use.
set -euo pipefail
trace_import="$1/Contents/MacOS/synth_trace_import"
[[ -f "$trace_import" ]] || { echo "Packaged trace importer missing" >&2; exit 1; }
# The CLI links the same ad-hoc Ghostty dylib as the app. Tauri's nested signing
# leaves hardened library validation enabled unless it is explicitly resealed.
codesign --force --sign - "$trace_import"
if import_probe="$("$trace_import" 2>&1)"; then
  echo "Trace importer unexpectedly accepted an empty invocation" >&2
  exit 1
fi
[[ "$import_probe" == *"usage: synth-trace-import BUNDLE"* ]] || {
  echo "Packaged trace importer failed its executable load check: $import_probe" >&2
  exit 1
}
