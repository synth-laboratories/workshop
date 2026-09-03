---
name: use-live-annotators
description: Configure, run, inspect, control, and visualize observe-only annotators that stream provisional findings beside active Synth container rollouts. Use for live annotation protocols, hot-swaps, operator notes, logical-time replay, or post-seal reconciliation; use trace-v5-annotate when annotation begins only after sealing.
---

# Use Live Annotators

Live annotators observe active rollouts and publish a separate durable stream.
They never act as the policy, change environment state, rewrite reward, or turn a
provisional finding into sealed evidence.

## Run the lane

1. Discover and probe the registered container. Preserve its `container_id` and
   declared capabilities; never substitute or construct a loopback URL.
2. Inspect the installed protocol with `annotation_manage` operation
   `annotation_protocol_get`.
3. Use a recipe with `[live_annotation]`. Before dispatch, require a pinned
   `protocol_revision_id` and both rollout and annotation stream descriptors.
4. Create `live.annotated_rollouts.v1`, bind both declared streams for every
   rollout, and wait for the visual subscription acknowledgement before starting
   paid work.
5. Follow the rollout and findings on their shared logical clock. A live request
   is incomplete if findings appear only after every rollout has terminated.
6. After sealing, call `annotation_provisional_list` and report reconciliation
   without promoting provisional rows to sealed evidence.

Every annotation MCP call has the shape:

```json
{
  "operation": "annotation_protocol_get",
  "arguments": { "container_id": "<registered container id>" }
}
```

Change `operation` and include only the arguments required by that operation.
Use returned run, rollout, protocol revision, and stream identities verbatim.

## Admit safely

- Confirm the registered container advertises live annotation and prepare
  declares the annotation sibling stream. Health or generic SSE is insufficient.
- Protocol source is workspace-relative and digest-pinned. Configuration may
  identify a model route but must not contain a credential or secret value.
- Provider inference runs from the container through Workshop's run-scoped
  proxy. The container receives a capability route and public sentinel, never a
  provider key. Do not use Keychain, paste a key into configuration, or expose a
  proxy token.
- Fail closed if a requested protocol pin, annotation channel, or visual
  subscription is missing.
- Keep live `[live_annotation]` and post-hoc `[annotation]`/Trace V5 work
  distinct. The sealed trace and post-seal verifier remain authoritative.

## Operate the protocol

Use `annotation_manage`:

- `annotation_protocol_get` inspects identity, digests, and judge metadata.
- `annotation_protocol_update` installs source/configuration. With `run_id`, it
  advances undispatched rollouts; explicit `rollout_ids` hot-swap selected active
  lanes. Carry state only when continuity is intended.
- `annotation_control_send` sends one durable control to one rollout. `message`
  accepts `note`, `judge_now`, or `set`; `protocol.update` selects an installed
  revision; `stop` stops annotation, not the rollout.
- `annotation_provisional_list` reads current/history plus reconciliation.

Never guess an annotation endpoint. Use descriptors returned by prepare. A
control took effect only when its acknowledgement appears in the annotation
stream; the immediate HTTP response is not sufficient proof.

## Preserve meaning and time

- Preserve finding ID, confidence, basis, cited rollout sequences, and protocol
  revision.
- Supersession and retraction are append-only history. A default view may hide
  inactive findings only when the operator can reveal them.
- Maintain one deterministic logical arrival clock across rollout and annotation
  streams. Also retain producer timestamp, producer sequence, and the rollout
  sequence each annotation observed. Logical time orders replay; it does not
  replace provenance.
- Link a finding to an LLM call only through an explicit request/call/span ID or
  an exact cited source sequence. Time proximity is not provenance.
- Missing findings, scores, calls, reasoning, timestamps, model failures,
  abstentions, and dropped channels remain visible missing/failure states.

## Present the live visual

The first view answers: what is running, is it healthy, what is emerging, and
what should be inspected next. Keep two to four primary metrics visible; place
diagnostics behind a stable disclosure.

The shared visual provides:

- aggregate campaign state and one selectable progress lane per rollout;
- task-specific outcome and state through a thin adapter;
- frames or low-bandwidth clips synchronized with logical time when emitted;
- findings, milestones, failures, supersessions, and retractions on that clock;
- call-level Trace V5 inputs, retained reasoning, tools/results, output, and
  explicitly linked annotations when those envelopes exist;
- honest empty states when policy spans or annotation evidence are absent.

Do not render a chart from one point or a matrix with no dimensions. Do not
repeat the same missing prerequisite in every panel. Verify the real rendered
right panel at normal zoom as well as a wide view.

## Seal, capture, and reopen

Terminal settlement must drain both streams and bind a sealed trace, captured
replay, or durable projection before the producer can disappear. Reopen the
visual with the producer stopped; a completed visual that returns to
`connecting` is not durable proof.

Capture subscribed, early-live, mid-run, terminal, selected-detail,
evidence-detail, and durable-reopen states when applicable. Record visual and
run identity, revision, logical cursor, source revision, viewport/placement,
evidence mode, timestamp, digest, and what the image proves.

Reconciliation classifies findings as `resolved`, `corroborated`, `unresolved`,
or `unsealed`. Report protocol revision, rollout/run IDs, stream completion,
control acknowledgements, and counts. A successful rollout without its requested
annotation channel is incomplete.

