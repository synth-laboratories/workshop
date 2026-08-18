#!/usr/bin/env bash
# Drive one crash-recovery checkpoint end to end against a named instance.
#
# Crashes are expected; the bug this drill guards is Workshop lying about what
# survived one. It kills the app at a named point in a turn, relaunches it, and
# reports what the next launch believes — read straight out of the durable
# stores, because that is what the sidebar reads.
#
# Usage:
#   ./scripts/crash-recovery-drill.sh <checkpoint> [instance]
#   ./scripts/crash-recovery-drill.sh --inspect [instance]
#
# Checkpoints (see docs/crash_recovery_contract.md):
#   after_turn_start  after_first_activity  before_tool_dispatch
#   after_tool_dispatch  after_tool_receipt  after_rollout_launch
#   after_rollout_terminal  before_final_message
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHECKPOINT="${1:-}"
NAME="${2:-codex}"
RELEASE_SLUG="v05"
INSTANCE_ROOT="${SYNTH_DESKTOP_INSTANCES_ROOT:-$HOME/.synth-desktop/instances}/$RELEASE_SLUG/$NAME"
DB="$INSTANCE_ROOT/data/synth.sqlite3"
THREADS="$INSTANCE_ROOT/codex/threads.json"

if [[ -z "$CHECKPOINT" ]]; then
  sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi

require_db() {
  if [[ ! -f "$DB" ]]; then
    echo "[drill:$NAME] no database at $DB — run the instance at least once" >&2
    exit 1
  fi
}

# What the next launch will show. Read from storage rather than from the UI:
# if these disagree with the sidebar, the renderer is the thing that is wrong.
inspect() {
  require_db
  echo "== sessions still claiming to run"
  sqlite3 "$DB" \
    "SELECT id, status, COALESCE(active_run_id, '-') FROM sessions WHERE status = 'running' OR active_run_id IS NOT NULL;"
  echo "== live ownership claims"
  sqlite3 "$DB" \
    "SELECT session_id, owner_instance_id, lease_expires_at FROM turn_ownership;"
  echo "== pending recovery notices"
  sqlite3 "$DB" \
    "SELECT id,
            json_extract(metadata_json, '\$.recovery.reason'),
            json_extract(metadata_json, '\$.recovery.restartable'),
            json_extract(metadata_json, '\$.recovery.needsAttention')
     FROM sessions WHERE json_extract(metadata_json, '\$.recovery') IS NOT NULL;"
  echo "== unsettled external actions"
  sqlite3 "$DB" \
    "SELECT tool_call_id, operation, status, COALESCE(external_object_id, '-')
     FROM action_receipts WHERE settled_at IS NULL;"
  if [[ -f "$THREADS" ]]; then
    echo "== Codex records still marked running (must be empty after a relaunch)"
    python3 - "$THREADS" <<'PY'
import json, sys
records = json.load(open(sys.argv[1]))
stale = [key for key, value in records.items() if value.get("status") == "running"]
print("\n".join(stale) if stale else "(none)")
PY
  fi
}

if [[ "$CHECKPOINT" == "--inspect" ]]; then
  NAME="${2:-codex}"
  inspect
  exit 0
fi

echo "[drill:$NAME] state before the crash"
inspect || true

echo
echo "[drill:$NAME] launching with SYNTH_DESKTOP_CRASH_AT=$CHECKPOINT"
echo "[drill:$NAME] drive one turn in the UI; the process will abort at the checkpoint."
SYNTH_DESKTOP_CRASH_AT="$CHECKPOINT" "$ROOT/scripts/desktop-instance.sh" dev "$NAME" || true

echo
echo "[drill:$NAME] state after the crash, before any relaunch"
inspect

cat <<EOF

[drill:$NAME] now relaunch normally and check the invariant:

  ./scripts/desktop-instance.sh dev $NAME

  - no chat may show Working in the first hydrated frame;
  - every abandoned chat must read Interrupted or Recovering;
  - Archive must be enabled on all of them;
  - a chat with an unsettled action above must read Needs attention and
    must NOT offer Restart.

Then re-run:  ./scripts/crash-recovery-drill.sh --inspect $NAME
Both "sessions still claiming to run" and "Codex records still marked running"
must be empty.
EOF
