#!/usr/bin/env bash
set -euo pipefail

WORKSHOP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTANCE_NAME="${1:-livecraftax}"
INSTANCE_ROOT="${SYNTH_DESKTOP_INSTANCE_ROOT:-$HOME/.synth-desktop/instances/v02/$INSTANCE_NAME}"
BUILD_ROOT="$INSTANCE_ROOT/build/target/debug"
IPC_FILE="$INSTANCE_ROOT/data/visuals-ipc.json"
PROMPT_FILE="$WORKSHOP_ROOT/scripts/prompts/live_visual_authoring_acceptance.md"
RECEIPT_ROOT="${SYNTH_VISUAL_HARNESS_RECEIPT_ROOT:-/tmp/synth-live-visual-authoring-$INSTANCE_NAME}"
WIDE_EVIDENCE="$RECEIPT_ROOT/wide.png"
COMPACT_EVIDENCE="$RECEIPT_ROOT/compact.png"

for required in \
  "$BUILD_ROOT/synth-visuals-mcp" \
  "$BUILD_ROOT/synth-containers-mcp" \
  "$IPC_FILE" \
  "$PROMPT_FILE" \
  "$WIDE_EVIDENCE" \
  "$COMPACT_EVIDENCE"; do
  if [[ ! -e "$required" ]]; then
    echo "missing harness prerequisite: $required" >&2
    exit 2
  fi
done

mkdir -p "$RECEIPT_ROOT"

codex exec \
  --json \
  --ephemeral \
  --strict-config \
  --dangerously-bypass-hook-trust \
  --skip-git-repo-check \
  --sandbox read-only \
  --model gpt-5.6-sol \
  --image "$WIDE_EVIDENCE" \
  --image "$COMPACT_EVIDENCE" \
  --cd "$WORKSHOP_ROOT" \
  --config 'model_reasoning_effort="medium"' \
  --config 'model_verbosity="high"' \
  --config 'service_tier="fast"' \
  --config 'approval_policy="never"' \
  --config "mcp_servers.synth_visuals.command=\"$BUILD_ROOT/synth-visuals-mcp\"" \
  --config "mcp_servers.synth_visuals.env={SYNTH_VISUALS_IPC_FILE=\"$IPC_FILE\"}" \
  --config "mcp_servers.synth_containers.command=\"$BUILD_ROOT/synth-containers-mcp\"" \
  --config "mcp_servers.synth_containers.env={SYNTH_DESKTOP_IPC_FILE=\"$IPC_FILE\"}" \
  --output-last-message "$RECEIPT_ROOT/final.md" \
  - < "$PROMPT_FILE" | tee "$RECEIPT_ROOT/events.jsonl"

echo "harness receipts: $RECEIPT_ROOT"
