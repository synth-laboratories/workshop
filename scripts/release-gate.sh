#!/usr/bin/env bash
# Release gate for a chosen maturity tier (contracts/release-tiers-v1.toml).
#
#   scripts/release-gate.sh <core|stable|beta|alpha|dev> [--required-only]
#
# Resolves the tier's verification plan through workshop-tier-plan, runs the
# required items (and the recommended ones unless --required-only, whose
# omission is then recorded with a reason), and writes a receipt binding the
# results to the exact source revision. Manual items are never auto-passed:
# they land in the receipt as needs-human and block promotion until a human
# attests them there. Promotion is explicit — this script renders a verdict;
# it does not move any feature between tiers.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TIER="${1:-}"
MODE="${2:-}"
case "$TIER" in
  core|stable|beta|alpha|dev) ;;
  *) echo "usage: scripts/release-gate.sh <core|stable|beta|alpha|dev> [--required-only]" >&2; exit 2 ;;
esac

echo "[release-gate] resolving the $TIER plan"
PLAN="$(cargo run --quiet \
  --manifest-path "$ROOT/apps/synth_desktop/src-tauri/Cargo.toml" \
  --bin workshop-tier-plan -- "$TIER")"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RECEIPT_DIR="$ROOT/work/release-gates"
RECEIPT="$RECEIPT_DIR/$TIER-$STAMP.json"
mkdir -p "$RECEIPT_DIR"

COMMIT="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
DIRTY=0
[[ -n "$(git -C "$ROOT" status --porcelain 2>/dev/null)" ]] && DIRTY=1

PLAN_JSON="$PLAN" RECEIPT="$RECEIPT" TIER="$TIER" COMMIT="$COMMIT" DIRTY="$DIRTY" \
STAMP="$STAMP" RECEIPT_DIR="$RECEIPT_DIR" MODE="$MODE" ROOT="$ROOT" python3 - <<'PYEOF'
import json, os, subprocess, sys
from datetime import datetime, timezone

plan = json.loads(os.environ["PLAN_JSON"])
mode = os.environ["MODE"]
root = os.environ["ROOT"]
receipt_dir = os.environ["RECEIPT_DIR"]
stamp = os.environ["STAMP"]
tier = os.environ["TIER"]

items = []
promote = True

def run(item, disposition):
    global promote
    name, kind, command = item["name"], item["kind"], item["command"]
    entry = {"name": name, "kind": kind, "disposition": disposition, "command": command}
    if kind == "manual":
        entry["status"] = "needs-human"
        entry["reason"] = "attest by hand in this receipt before promoting"
        if disposition == "required":
            promote = False
    elif disposition == "recommended" and mode == "--required-only":
        entry["status"] = "skipped"
        entry["reason"] = "--required-only run; record why this omission is acceptable"
    else:
        log = os.path.join(receipt_dir, f"{tier}-{stamp}.{name}.log")
        print(f"[release-gate] {disposition} {name}: {command}", flush=True)
        with open(log, "wb") as sink:
            code = subprocess.call(["bash", "-c", command], cwd=root, stdout=sink, stderr=sink)
        entry["status"] = "passed" if code == 0 else "failed"
        entry["log"] = os.path.relpath(log, root)
        if code != 0 and disposition == "required":
            promote = False
    items.append(entry)

for item in plan["verification"]["required"]:
    run(item, "required")
for item in plan["verification"]["recommended"]:
    run(item, "recommended")
for group in ("optional", "excluded"):
    for item in plan["verification"][group]:
        items.append({"name": item["name"], "kind": item["kind"], "disposition": group, "status": "not-run"})

receipt = {
    "schema": 1,
    "contractVersion": plan["contractVersion"],
    "tier": tier,
    "commit": os.environ["COMMIT"],
    "treeDirty": os.environ["DIRTY"] == "1",
    "generatedAt": datetime.now(timezone.utc).isoformat(),
    "features": plan["features"],
    "items": items,
    "promote": promote,
}
with open(os.environ["RECEIPT"], "w") as sink:
    json.dump(receipt, sink, indent=2)
    sink.write("\n")

print(f"[release-gate] receipt: {os.environ['RECEIPT']}")
for entry in items:
    print(f"[release-gate]   {entry['status']:>14}  {entry['disposition']:<11} {entry['name']}")
print(f"[release-gate] verdict: {'PROMOTE-OK' if promote else 'DO-NOT-PROMOTE'}"
      + (" (dirty tree)" if receipt["treeDirty"] else ""))
sys.exit(0 if promote else 1)
PYEOF
