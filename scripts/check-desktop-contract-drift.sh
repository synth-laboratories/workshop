#!/usr/bin/env bash
# Fail when Rust contract command/event consts drift from TS protocolConstants.
#
# Compares:
#   apps/synth_desktop/src-tauri/src/contract/{commands,events}.rs
#   apps/synth_desktop/src/renderer/src/bridge/protocolConstants.ts
#
# Usage (from repo root):
#   ./scripts/check-desktop-contract-drift.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUST_COMMANDS="$ROOT/apps/synth_desktop/src-tauri/src/contract/commands.rs"
RUST_EVENTS="$ROOT/apps/synth_desktop/src-tauri/src/contract/events.rs"
TS_CONSTANTS="$ROOT/apps/synth_desktop/src/renderer/src/bridge/protocolConstants.ts"

for path in "$RUST_COMMANDS" "$RUST_EVENTS" "$TS_CONSTANTS"; do
  if [[ ! -f "$path" ]]; then
    echo "[contract-drift] missing $path" >&2
    exit 1
  fi
done

python3 - "$RUST_COMMANDS" "$RUST_EVENTS" "$TS_CONSTANTS" <<'PY'
import re
import sys
from pathlib import Path

rust_commands_path, rust_events_path, ts_path = map(Path, sys.argv[1:3] + [sys.argv[3]])

def parse_rust_consts(text: str) -> dict[str, str]:
    # pub const NAME: &'static str = "value";
    return dict(re.findall(
        r"pub const ([A-Z][A-Z0-9_]+):\s*&'static str\s*=\s*\"([^\"]+)\"",
        text,
    ))

def parse_ts_object(text: str, name: str) -> dict[str, str]:
    m = re.search(rf"export const {name} = \{{([\s\S]*?)\}} as const;", text)
    if not m:
        raise SystemExit(f"[contract-drift] TS object {name} not found")
    return dict(re.findall(r"([A-Z][A-Z0-9_]+):\s*\"([^\"]+)\"", m.group(1)))

rust_cmd = parse_rust_consts(rust_commands_path.read_text())
# Event channel consts live on EventChannel; origin string consts on EventOrigin.
rust_events_all = parse_rust_consts(rust_events_path.read_text())
rust_event_channels = {
    k: v for k, v in rust_events_all.items()
    if k not in {"PROVIDER", "DESKTOP"}
}
rust_origins = {k: v for k, v in rust_events_all.items() if k in {"PROVIDER", "DESKTOP"}}

ts = ts_path.read_text()
ts_cmd = parse_ts_object(ts, "COMMANDS")
ts_events = parse_ts_object(ts, "EVENT_CHANNELS")
ts_origins = parse_ts_object(ts, "EVENT_ORIGINS")

errors: list[str] = []

def compare(label: str, rust: dict[str, str], ts_map: dict[str, str]) -> None:
    rust_keys = set(rust)
    ts_keys = set(ts_map)
    only_rust = sorted(rust_keys - ts_keys)
    only_ts = sorted(ts_keys - rust_keys)
    if only_rust:
        errors.append(f"{label}: only in Rust: {', '.join(only_rust)}")
    if only_ts:
        errors.append(f"{label}: only in TS: {', '.join(only_ts)}")
    for key in sorted(rust_keys & ts_keys):
        if rust[key] != ts_map[key]:
            errors.append(
                f"{label}: {key} value mismatch rust={rust[key]!r} ts={ts_map[key]!r}"
            )

compare("COMMANDS", rust_cmd, ts_cmd)
compare("EVENT_CHANNELS", rust_event_channels, ts_events)
compare("EVENT_ORIGINS", rust_origins, ts_origins)

if errors:
    print("[contract-drift] FAIL — Rust ↔ TS boundary consts out of sync:", file=sys.stderr)
    for err in errors:
        print(f"  - {err}", file=sys.stderr)
    sys.exit(1)

print(
    f"[contract-drift] OK — "
    f"{len(ts_cmd)} commands, {len(ts_events)} event channels, "
    f"{len(ts_origins)} origins match"
)
PY
