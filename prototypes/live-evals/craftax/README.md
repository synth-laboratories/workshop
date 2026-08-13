# Craftax live eval reference prototype

This is a dependency-free local reference visual for the live-eval design in
[`docs/live_evals.md`](../../../docs/live_evals.md). It consumes real SSE and frame
endpoints. It does not load fixture events.

## Run

Serve the directory from a loopback origin:

```sh
python3 -m http.server 4188 --directory prototypes/live-evals/craftax
```

Open `http://127.0.0.1:4188`.

For a real Evals ReAct run, start the JSONL-tail server before starting the runner:

```sh
python3 prototypes/live-evals/craftax/serve.py \
  --events /absolute/path/to/containers-storage-root \
  --container-base http://127.0.0.1:8099 \
  --bundle-root /absolute/path/to/trace-v5-bundles
```

`--events` accepts either the normalized eval JSONL or a Containers storage root
containing `event_logs/*.jsonl` and `seals/*.trace-v5.json`. The bridge unwraps
the durable envelopes and attaches each rollout's lane identity without changing
their payloads. Connect the visual to `http://127.0.0.1:4188/api/events`, confirm it says live,
then start the Evals Craftax `stream --policy react` command with the same JSONL path.
The container service must have replay/frame capture enabled if PNG frames are required;
otherwise the visual renders the real symbolic ASCII frames carried by the eval stream.
When the bundle root gains a sealed trace index, the bridge emits one stable
`trace.reconciled` projection from that real index so the visual can transition from
unsealed live evidence to its durable Trace V5 identity.

You can either paste one or more absolute SSE endpoints or use **Transport smoke helper**
against the local Craftax Rust service. The helper creates a real telemetry-enabled
rollout and lets you send manual environment actions. Those actions test transport,
frames, event ordering, reward projection, and replay; they are not evidence of a ReAct
or other model policy.

A native Craftax endpoint looks like:

```text
http://127.0.0.1:8098/rollouts/<rollout-id>/stream
```

Multiple endpoints can be supplied one per line. A normalized run-level
`evals.event-stream.v1` endpoint is also accepted.

## Through-time behavior

The prototype stores real events by stable identity, merges streams by occurrence time,
and uses sequence as a deterministic tie-breaker. Within a selected rollout, sequence is
authoritative when present. The timeline can:

- follow newly arriving events;
- scrub to an earlier global event cutoff;
- replay captured events using their observed timestamp gaps;
- switch lanes while remaining at the same global cutoff.

Frames, rewards, achievements, policy metadata, usage, and cost are all projected only
from events visible at the selected cutoff. Missing policy data remains visibly
unavailable.

The trace projection uses that same cutoff. It accepts live eval transcript events and
Containers `trace.raw` / `trace.visual` events, labels them partial and unsealed, and
switches to a sealed identity only after a matching `trace.reconciled` event. Prompt,
reply, reasoning, and raw observation text are withheld from the structural reference
projection.

## Policy and trace source

The raw Craftax Rust stream owns environment state and frames. The opinionated
Containers ReAct runtime joins those world events with provider calls in its one durable
`synth.trace-stream-event.v1` rollout journal. Pointing this bridge at the Containers
storage root therefore populates the policy panel and world replay from the same rollout
identity; it does not join unrelated streams in the browser.

Containers persists the terminal `synth.trace.v5` seal beside the live journal. The
bridge emits `trace.reconciled` only from that persisted seal, allowing the visual to
move from live, unsealed evidence to the exact matching digest without changing sources.
