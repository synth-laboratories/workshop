# W3 local failure-injection runbook

This is a no-CUA preparation aid for the W1 Craftax/Containers path. It leaves
the upstream service untouched and scopes every injected failure to one exact
rollout id. Never point an existing shared container registration at this
proxy; register a fresh W3-only entry.

## Start with all faults off

Use a fresh temporary state directory and a free loopback port:

```bash
W3_STATE_DIR="$(mktemp -d /tmp/workshop-w3.XXXXXX)"
W3_STATE_FILE="$W3_STATE_DIR/faults.json"
printf '%s\n' '{}' > "$W3_STATE_FILE"
python3 scripts/w3-container-fault-proxy.py \
  --listen 127.0.0.1:18097 \
  --upstream http://127.0.0.1:8097 \
  --state-file "$W3_STATE_FILE"
```

Register `http://127.0.0.1:18097` as a new W3-only container. Discover and
prepare through that registration. Put the returned stable rollout id in each
toggle below. Do not substitute or guess a route.

## One fault at a time

Temporary poll 503 (before start):

```bash
printf '%s\n' '{"rollout_id":"ROLL_ID","poll_503":true}' > "$W3_STATE_FILE"
```

Expected: the agent names the declared poll URL failure and does not start.
Recovery: write `{}`; retry that exact declared poll URL and stable rollout id.

Immutable frame 404 (after an unmodified control rollout has emitted a frame
URL, using a separate W3 rollout if paid execution is involved):

```bash
printf '%s\n' '{"rollout_id":"ROLL_ID","frame_404":true}' > "$W3_STATE_FILE"
```

Expected: the agent reports the missing declared frame and does not fabricate a
frame or replace it with a screenshot/latest URL. Recovery: write `{}` and GET
the exact same immutable frame URL.

Policy-pin refusal (before start):

```bash
printf '%s\n' '{"rollout_id":"ROLL_ID","policy_pin_refusal":true}' > "$W3_STATE_FILE"
```

Expected: `POST /rollouts` returns 403 `bind_refused` with affordance
`bind_policy_config`; the agent must not change the requested policy or start a
new rollout. Recovery: write `{}` and inspect the stable rollout status before
replaying the exact start. If it remains prepared, replay is safe; otherwise do
not start again.

Only rewrite the state file between requests—never during a request. Restore
with:

```bash
printf '%s\n' '{}' > "$W3_STATE_FILE"
```

Then stop the proxy with Ctrl-C and remove only the temporary directory printed
above after preserving the W3 receipt.

## Visual MCP unavailable

For an isolated Desktop instance only, stop its `synth-visuals-mcp` child or
temporarily move aside that instance's visuals IPC descriptor, run discovery,
then restore the same descriptor at the same path. The prior `w1sol` receipt
already used the descriptor move safely. Do not touch another instance's IPC
file, do not replace it with Containers IPC, and do not proceed to prepare or
start while visuals discovery is unavailable.
