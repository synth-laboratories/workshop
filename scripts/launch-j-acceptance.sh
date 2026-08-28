#!/usr/bin/env bash
# Launch Workshop instance `j` for the NanoHorizon Craftax acceptance run.
#
# Detached on purpose: `cua-run` execs into the app, so the app *becomes* this
# process. Started as a supervised background job it dies with its supervisor,
# which killed one attempt mid-request and took the pending approval with it.
#
# SYNTH_DESKTOP_ALLOW_AGENT_HUMAN_APPROVALS=1 lets a non-human caller settle
# spending and credential consent. That is a real weakening: the approval
# receipt then records agent consent, not operator consent. It is here because
# the operator asked for it for this run; it is not a default.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
LOG="${SYNTH_J_LAUNCH_LOG:-/tmp/j-run.log}"

cd "$ROOT"
nohup env \
  SYNTH_DESKTOP_USE_DEV_SIGNER=0 \
  SYNTH_DESKTOP_ALLOW_AGENT_HUMAN_APPROVALS=1 \
  CONTAINERS_ROOT=/Users/joshuapurtell/GitHub/containers-nanohorizon-e2e-final \
  bash scripts/desktop-instance.sh cua-run j \
  >"$LOG" 2>&1 </dev/null &
disown
echo "[launch-j] started; log: $LOG"
