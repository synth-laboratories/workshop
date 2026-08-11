#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_ROOT="$(mktemp -d)"
OWNER_PID=""
cleanup() {
  [[ -z "$OWNER_PID" ]] || kill "$OWNER_PID" 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

export SYNTH_DESKTOP_INSTALL_LOCK_DIR="$TEST_ROOT/canonical-install"
"$ROOT/scripts/desktop.sh" _lock-probe 5 &
OWNER_PID="$!"

for _ in 1 2 3 4 5 6 7 8 9 10; do
  [[ -f "$SYNTH_DESKTOP_INSTALL_LOCK_DIR/pid" ]] && break
  sleep 0.05
done
[[ "$(cat "$SYNTH_DESKTOP_INSTALL_LOCK_DIR/pid")" == "$OWNER_PID" ]]

if "$ROOT/scripts/desktop.sh" _lock-probe 0 >/dev/null 2>&1; then
  echo "concurrent canonical install lock was accepted" >&2
  exit 1
fi

kill "$OWNER_PID"
wait "$OWNER_PID" 2>/dev/null || true
OWNER_PID=""

mkdir -p "$SYNTH_DESKTOP_INSTALL_LOCK_DIR"
printf '999999\n' >"$SYNTH_DESKTOP_INSTALL_LOCK_DIR/pid"
"$ROOT/scripts/desktop.sh" _lock-probe 0
[[ ! -e "$SYNTH_DESKTOP_INSTALL_LOCK_DIR" ]]

echo "desktop canonical install lock: ok"
