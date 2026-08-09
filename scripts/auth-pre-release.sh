#!/usr/bin/env bash
set -euo pipefail

WORKSHOP_URL="${SYNTH_WORKSHOP_URL:-https://www.usesynth.ai}"
RELEASE_ARTIFACT="${1:-${SYNTH_DESKTOP_RELEASE_ARTIFACT:-}}"

if [[ -z "$RELEASE_ARTIFACT" || ! -e "$RELEASE_ARTIFACT" ]]; then
  echo "[auth-gate] pass the actual release .app or zip as argument 1" >&2
  exit 2
fi

content_type_file="$(mktemp)"
body_file="$(mktemp)"
cleanup() { rm -f "$content_type_file" "$body_file"; }
trap cleanup EXIT

status="$(curl --silent --show-error --output "$body_file" \
  --dump-header "$content_type_file" --write-out '%{http_code}' \
  --request POST "$WORKSHOP_URL/api/auth/device/init")"
if [[ "$status" != "200" ]] || ! grep -qi '^content-type: application/json' "$content_type_file"; then
  echo "[auth-gate] device init must return 200 JSON; got $status" >&2
  sed -n '1,8p' "$body_file" >&2
  exit 1
fi

python3 -m json.tool "$body_file" >/dev/null

if [[ -z "${SYNTH_AUTH_E2E_COMMAND:-}" ]]; then
  echo "[auth-gate] SYNTH_AUTH_E2E_COMMAND is required for the live artifact pass" >&2
  exit 2
fi

export SYNTH_DESKTOP_RELEASE_ARTIFACT="$RELEASE_ARTIFACT"
eval "$SYNTH_AUTH_E2E_COMMAND"

echo "[auth-gate] PASS artifact=$RELEASE_ARTIFACT workshop=$WORKSHOP_URL at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
