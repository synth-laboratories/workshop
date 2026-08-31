#!/usr/bin/env bash
set -euo pipefail

COMMAND="${1:-verify}"
OUTPUT="${SYNTH_BROWSER_RUNTIME_OUTPUT:-$(cd "$(dirname "$0")/.." && pwd)/apps/synth_desktop/browser/runtime}"
MANIFEST="$OUTPUT/manifest.json"

[[ "$COMMAND" == "verify" ]] || {
  echo "[browser-runtime] unsupported command: $COMMAND" >&2
  exit 2
}
[[ -f "$MANIFEST" ]] || {
  echo "[browser-runtime] missing manifest: $MANIFEST" >&2
  exit 1
}

while IFS=$'\t' read -r relative expected_digest expected_bytes; do
  file="$OUTPUT/$relative"
  [[ -f "$file" ]] || {
    echo "[browser-runtime] missing declared file: $relative" >&2
    exit 1
  }
  actual_digest="$(shasum -a 256 "$file" | awk '{print $1}')"
  actual_bytes="$(stat -f %z "$file")"
  [[ "$actual_digest" == "$expected_digest" ]] || {
    echo "[browser-runtime] digest mismatch: $relative" >&2
    exit 1
  }
  [[ "$actual_bytes" == "$expected_bytes" ]] || {
    echo "[browser-runtime] size mismatch: $relative" >&2
    exit 1
  }
done < <(jq -r '.files[] | [.path, .sha256, (.bytes | tostring)] | @tsv' "$MANIFEST")

"$OUTPUT/node/bin/node" --version >/dev/null
echo "[browser-runtime] verified $(jq '.files | length' "$MANIFEST") sealed files"
