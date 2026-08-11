#!/usr/bin/env bash
# Provision the central Trace V5 format-authority CLI for a named eval instance.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESTINATION="${1:?usage: prepare-synth-trace-cli.sh DESTINATION}"
CONTAINERS_ROOT="${SYNTH_CONTAINERS_ROOT:-$ROOT/../containers}"

if [[ ! -f "$CONTAINERS_ROOT/pyproject.toml" ]]; then
  echo "[trace-cli] missing synth-containers checkout: $CONTAINERS_ROOT" >&2
  exit 1
fi

UV_BIN="${SYNTH_UV_BIN:-$(command -v uv || true)}"
if [[ -z "$UV_BIN" || ! -x "$UV_BIN" ]]; then
  echo "[trace-cli] uv is required to run synth-trace" >&2
  exit 1
fi

# Verify the exact command before publishing it. A broken launcher must stop
# instance startup instead of surfacing after ten expensive rollouts.
"$UV_BIN" run --project "$CONTAINERS_ROOT" synth-trace --help >/dev/null

mkdir -p "$(dirname "$DESTINATION")"
TEMP="$DESTINATION.tmp.$$"
printf '#!/bin/sh\nexec %q run --project %q synth-trace "$@"\n' \
  "$UV_BIN" "$CONTAINERS_ROOT" >"$TEMP"
chmod 755 "$TEMP"
"$TEMP" --help >/dev/null
mv "$TEMP" "$DESTINATION"
echo "[trace-cli] provisioned $DESTINATION"

