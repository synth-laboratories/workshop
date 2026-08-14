# Live-eval remaining acceptance and implementation status

## 2026-08-13 GEPA live-panel completion

The optimizer portion that was deliberately excluded from the original
Containers + Workshop pass is now implemented across the G1 GEPA runtime and
Workshop's durable right-panel projection:

- exhausted rollout retries persist typed failure evidence with null reward and
  cost; they never manufacture a zero-score rollout or a fake child stream;
- the runtime checkpoints after each consumed child result, tracks required,
  scored, failed, and pending coverage, preserves legacy pre-coverage
  checkpoints on reopen, and refuses heldout promotion without complete scored
  coverage;
- persisted pages are cursor-idempotent, sequence gaps trigger a durable reload,
  and a duplicate sequence with different content fails closed;
- the chat-bound live GEPA panel now presents best-so-far hill climb, Pareto
  search space, lineage and gate decisions, per-example rollouts/rewards,
  proposer reasoning, limits/cost, and an evidence-integrity panel whose failed
  samples are explicitly excluded from scores;
- the agent can open the durable optimizer visual in the active chat while the
  run continues; reopening preserves the same optimizer-run binding rather than
  creating a snapshot copy.

Rendered QA at 1440, 1024, 768, and 390 px reports zero document overflow. The
new deterministic GEPA QA surface is
`apps/synth_desktop/src/renderer/gepa-qa.html`; it exercises a
live-looking search with accepted, rejected, and evaluating candidates plus an
exhausted rollout. This is product/fixture validation, not a claim of a new paid
Banking77 uplift receipt. A fresh provider-backed run remains an external
acceptance receipt.

The remainder of this document records the 2026-08-12 Containers + Workshop
acceptance pass; optimizer execution was deliberately out of that historical
receipt and is superseded by the completion note above.

## Current receipt

The local real-evidence reference run is `roll_normalized_live_20260812`:

- 469 replayed envelopes (467 advancing evidence records plus subscription and
  reconciliation control/evidence);
- 330 `span.policy.data` partials folded into two policy calls;
- 13 durable PNG gameplay frames, each rendered at its real 256×256 size;
- 12 environment steps and 25 semantic replay checkpoints;
- Trace V5 digest
  `sha256:27f61806d79f0f967547bd9c6739d8002dd2255faa1999d0dbf26fbccc051bee`;
- Policy Focus renders five rows rather than hundreds. Selecting a call shows
  the real observation input, 850 characters of aggregated thinking, structured
  action arguments, and the truthful `Tool-only response (no text output)` state;
- CUA at 1440, 1024, 768, and 390 px measured
  `documentElement.scrollWidth === documentElement.clientWidth`; 390 px is also
  stricter than the effective layout width at 200% zoom on a 768 px surface;
- all 43 controls in the reference viewer have accessible names, trace rows have
  distinct semantic names, focus-visible and reduced-motion rules are present,
  and the trace status has a live status role.

Validation on this dirty WIP tip:

| Surface | Result |
|---|---|
| Workshop visual/reducer tests | **59 passed** |
| Workshop Rust library | **368 passed, 1 paid test ignored** |
| Workshop renderer | TypeScript and production Vite build **passed** |
| Containers | **257 passed, 8 platform-dependent skips** |
| Desktop named-instance contract | **passed** |

## Scope disposition

| Scope | Status after this pass |
|---|---|
| **A9 / V1** semantic trace | **Implemented and CUA-proven.** Policy partials fold into calls; input, thinking, output/tool-only state, tool arguments, usage, timestamps, and raw evidence remain inspectable. |
| **A10** responsive gate | **CUA-proven** at 1440/1024/768/390 with zero page overflow. Long rollout IDs own bounded horizontal scroll; mobile trace header/sequence collisions are fixed. |
| **A11** reconnect/reopen | **Implemented and in-process proven.** Workshop backfills every declared poll page after interruption, rejects cursor regress/no-progress, collapses exact replay, closes temporary out-of-order gaps, and reopens the persisted spool. A destructive live socket/container kill transcript remains an external drill. |
| **A12** idempotency | **Implemented and in-process proven.** Prepare/start retries reuse rollout identity; body conflicts fail closed; ambiguous starts recover authoritative state rather than replaying mutation. A second paid-call count is intentionally not claimed without a paid run. |
| **A13 / V3** isolation | **Implemented.** Explicit campaign/rollout scope is enforced before ingest in the reference viewer and before projection in the template; the ten-rollout path is tested and unrelated lanes cannot affect selection or metrics. |
| **A14** independent restart | **Implemented truthfully.** Policy restart advances only policy generation and retains logs. Environment restart now returns typed `environment_restart_unsupported` where no proven true checkpoint exists instead of fabricating restoration. |
| **A15 / A18** corruption/crash | **Implemented by the Containers journal/Trace V5 floor.** Persist-before-publish, exact-digest replay, gaps, conflicting duplicates, malformed/truncated journals, closed-spool resume, wrong high-water, and seal authority are covered by the green Containers suite. |
| **A16** transport equivalence | **Completed in the protocol floor.** Poll now has bounded `limit`, `cursor.next`, and `cursor.has_more`; control records do not consume the evidence page or advance the cursor. Workshop drains all pages before returning live. Poll/SSE/WS equivalence remains covered by Containers conformance. |
| **A17** retained artifacts | **Implemented and tested.** PNGs fsync before availability, remain served after runtime reconstruction/world stop for run retention, and invalid images stay explicit ASCII rather than fake availability. |
| **V2** synchronized replay | **Implemented and CUA-proven.** The main rail has 25 semantic moments for the 469-envelope run; policy deltas, observations, and frames do not become meaningless primary ticks. |
| **V4** truth states | **Implemented.** Pending, not emitted, not applicable, redacted, failed, present zero, unavailable image, and tool-only output are distinct states. |
| **V5** performance | **Partially complete.** A deterministic 100,017-envelope projection test finishes with bounded semantic rows, groups, and replay ticks; the visible step-group window is bounded and scope happens before prototype ingest. A browser heap/long-task benchmark under sustained live delivery is still required before calling this gate complete. |
| **V6** accessibility | **Implemented for the Craftax reference/template floor.** Accessible names, semantic row labels, focus visibility, status text beyond color, reduced motion, compact responsive inspector. A formal axe/screen-reader receipt remains external. |
| **O1–O4** operations | Existing budget, credential-broker, redaction, durable-spool, reconnect, and cancellation primitives are covered by unit/conformance tests, but the destructive parallel-budget/cancellation/auth-rotation drills below remain acceptance work. |
| **O5** rebuild identity | **Implemented and script-tested.** Named CUA bundles use one persistent local signer and `cua-run` relaunches the existing signed app. No rebuild/relaunch was performed in this pass, specifically to avoid causing another Keychain prompt while the mixed tree is dirty. |
| **W1–W3** agent workflow | **PASS on installed Tier 4 candidate.** Clean-source, signed isolated `v02golden` at `859110f` used legitimate GPT-5.6 Sol after operator-confirmed ChatGPT re-sync. W1 discovered/registered Craftax and completed exactly 10/10 real rollouts with no retries or replacements. W2 produced a substantive revision 2 with passing 1280×900 and 768×1024 reviews, then a one-start post-revision control smoke on the same installed SHA. W3 completed poll-503, immutable-frame-404, and policy-pin-403 through a fresh proxy-only registration with same-resource recovery and exactly one W3 engine start. Receipt: [`w1-w3-tier4-installed.json`](./receipts/2026-08-13/w1-w3-tier4-installed.json). |

The external drills remain described below as the acceptance contract. The
installed Tier 4 receipt now records their completion from an isolated data
root; they are no longer open golden-path work.

The biggest gap in [aug_12_update.md](/Users/joshuapurtell/Documents/GitHub/workshop/docs/aug_12_update.md) was that A1–A8 proved major integrations, but did not sufficiently test the viewer, recovery behavior, isolation, or operational failures.

I would add these acceptance tests.

## Highest priority

| ID | Test | Pass when |
|---|---|---|
| **A9** | **Semantic live visual CUA** | A real Craftax rollout is rendered as episode → step → policy call → action → reward. Streamed token deltas update one open call rather than creating hundreds of rows. Input, thinking, tool calls, usage, frames, reward, and closure are visible. Raw events remain available behind disclosure. |
| **A10** | **Responsive visual gate** | Real CUA at 1440, 1024, 768, and 390px. `scrollWidth <= clientWidth`; no overlapping rollout cards, titles, hashes, controls, frames, plots, or trace panels. Selected trace details remain visible while scrolling. |
| **A11** | **Disconnect, recover, and reopen** | Disconnect SSE during an open policy span, miss several events, reconnect using the last durable cursor, backfill through poll, deduplicate replayed events, and resume live delivery. Then kill the container and reopen the completed run using only durable storage. |
| **A12** | **Idempotent execution** | Repeat prepare and start calls with the same idempotency identity before, during, and after a timeout. Exactly one rollout, one paid policy execution, one event log, one reward receipt, and one Trace V5 seal exist. Conflicting retry bodies fail closed. |
| **A13** | **Rollout and campaign isolation** | Run at least 10 rollouts plus an unrelated campaign through the same façade. A visual scoped to one campaign never imports unrelated rollouts, usage, frames, rewards, selection state, or trace events. |
| **A14** | **Independent service restart** | Restart the policy service while keeping the environment alive, then restart the environment while retaining the policy service. Generations change independently, durable evidence remains readable, and unsupported restoration is reported rather than fabricated. |

A9 and A10 are essential because the current `visual_review` recorded `noOverflow: true` even though CUA measured 75px overflow at 1024px and 346px at 768px.

## Protocol and durability tests

### A15 — Duplicate, gap, and corruption handling

Inject:

- Duplicate event IDs
- Duplicate sequences with different digests
- Missing sequences
- Out-of-order delivery
- Truncated JSONL tail
- Corrupt frame digest
- `capture.closed` with the wrong high-water
- Seal digest that disagrees with the log

Pass when exact duplicates collapse, contradictions fail closed, gaps remain visible, and no trace is marked sealed until reconciliation succeeds.

### A16 — Transport equivalence under pagination

Consume the same rollout through:

- Poll with small pages
- SSE with reconnects
- WebSocket when advertised
- Poll → SSE transition
- SSE → poll fallback

Pass when all consumers reconstruct the same ordered evidence IDs and digests. Heartbeats, subscription records, and reconnect control messages must not advance the evidence cursor.

### A17 — Retention and artifact survival

Pass when:

- Every frame is retrievable and matches its advertised digest.
- Frames remain available for the declared retention period.
- A stopped world does not cause an unexpected 404.
- Expiration is explicit and machine-readable.
- The sealed trace distinguishes embedded artifacts from external retained artifacts.
- Reopening never silently falls back to fake ASCII or a placeholder image.

### A18 — Crash boundaries

Crash the producer:

1. Before persisting an event
2. After persisting but before publishing
3. During SSE publication
4. Before `capture.closed`
5. After closure but before sealing
6. During seal publication

Pass when recovery produces either the complete durable event once or no event. It must never publish evidence that cannot later be replayed.

## Visual behavior tests

### V1 — Policy-span folding

Use a fixture with hundreds of reasoning and tool deltas.

Pass when the primary trace displays one policy-call row that updates in place. Expanding it reveals:

- Input
- Aggregated thinking
- Text output or explicit tool-only status
- Structured tool calls
- Usage
- Latency
- Open and close timestamps
- Raw event range

### V2 — Through-time synchronization

At every environment-step cutoff, assert that these agree:

- Gameplay frame
- Observation
- Policy call
- Selected action
- Vitals and inventory
- Reward curve
- Achievement curve
- Trace selection
- Evaluation timestamp

Dragging the main slider must not land on thousands of meaningless token-delta positions.

### V3 — Multi-rollout visual

Run ten lanes with long rollout IDs.

Pass when:

- All ten remain selectable.
- The rollout strip has a clear scrolling affordance.
- The selected lane is stable as new events arrive.
- One lane’s terminal event does not force selection away from another.
- Aggregate metrics are clearly distinguished from selected-lane metrics.
- Sorting/filtering does not change evidence identity.

### V4 — Null and truthfulness matrix

Test missing:

- Reward
- Usage
- Cost
- Latency
- Frame
- Achievement list
- Policy output
- Trace seal

The visual must distinguish:

```text
pending
not emitted
not applicable
redacted
unavailable
failed
zero
```

These cannot all appear as `—`, `0`, or `not emitted`.

### V5 — Viewer performance

Suggested floor:

- 10 simultaneous rollouts
- 100,000 raw envelopes
- 10,000 policy deltas
- 1,000 frames
- 60 minutes of timestamps

Pass when:

- The visible trace is virtualized.
- Token deltas do not create one DOM button each.
- Incoming events are batched.
- Scrubbing remains responsive.
- Memory is bounded.
- Selection and playback remain stable.

### V6 — Accessibility

Pass when:

- Every control has an accessible name.
- Trace rows have distinct semantic labels.
- Keyboard users can select lanes, expand calls, scrub, and open details.
- Focus remains visible.
- Status is not communicated only by color.
- Reduced-motion behavior is respected.
- The inspector works at 200% zoom.

## Operational tests

### O1 — Budget enforcement

Set a small hard budget and run parallel rollouts.

Pass when:

- Allocation happens before mutation.
- Retries reuse the original allocation.
- The run stops before exceeding the authorized ceiling.
- Unknown cost remains unknown.
- Final observed cost reconciles with provider usage.
- Budget exhaustion is terminal evidence, not a generic error.

### O2 — Cancellation

Cancel:

- Before start
- During a policy call
- Between environment actions
- During reconnect
- After environment terminal but before seal

Pass when cancellation is idempotent, reaches the owning service once, preserves completed evidence, and records why subsequent actions did not occur.

### O3 — Backpressure and slow consumers

Use a slow visual consumer while the producer emits policy deltas and frames quickly.

Pass when:

- Producer progress does not depend on the browser.
- Durable storage remains authoritative.
- The consumer catches up by cursor.
- Memory does not grow without bound.
- Dropped optional live updates never imply dropped durable evidence.

### O4 — Authentication and secret handling

Pass when:

- Tokens never appear in events, URLs, logs, receipts, visual bindings, screenshots, or Trace V5.
- Auth rotation does not change stream identity.
- Expired auth yields a recoverable authorization state.
- A visual cannot fetch another workspace’s stream or frames.
- Redaction is verified against nested error payloads.

### O5 — Local Workshop rebuild identity

Given the Keychain issue, add a macOS development gate:

- Rebuild and relaunch the same named instance multiple times.
- Confirm the bundle identifier and signing identity remain stable.
- Confirm stored credentials are reused without repeated authorization prompts.
- Confirm a second isolated instance cannot access the first instance’s credential namespace.

## Discovery and agent workflow

### W1 — Find, inspect, register, run

Start from a clean Workshop workspace and give the agent only:

> Find the Craftax Rust container, register it, run ten rollouts, and visualize them.

Pass only if the agent:

1. Discovers the provider rather than guessing URLs.
2. Inspects capabilities and transport descriptors.
3. Creates and reviews the visual first.
4. Subscribes and receives `stream.subscribed`.
5. Starts exactly ten rollouts.
6. Uses the requested policy configuration.
7. Presents real evidence.
8. Produces a durable receipt.

### W2 — Visual iteration before paid work

Require at least one real visual revision before start:

```text
create
→ fixture/replay review
→ CUA finding
→ revise
→ wide review
→ compact review
→ ready
→ paid start
```

The gate must inspect actual rendered DOM/screenshots. Self-declared checks in visual metadata do not count.

### W3 — Tool failure recovery

Remove or fail one capability at a time:

- Visual MCP unavailable
- Containers MCP restarts
- SSE times out
- Poll returns a temporary 503
- Frame returns 404
- Policy service refuses the pin
- Visual review fails

Pass when the agent recovers safely or stops with a precise blocker. It must not guess a replacement URL, fabricate evidence, or begin paid execution without the visual.

## Receipt standard

Every A-test should produce the same machine-readable receipt bundle:

```text
receipt.json
requested-stream.json
bound-stream.json
cursor-transcript.jsonl
event-kind-counts.json
run-manifest.json
cost-reconciliation.json
trace-v5.json
screenshots/
  1440.png
  1024.png
  768.png
  390.png
cua-findings.json
```

`receipt.json` should include:

- Git revisions for Workshop and Containers
- Container/provider versions
- Environment, policy, harness, world, and task pins
- Rollout and stream IDs
- Requested and bound transports
- First subscription time
- First paid call time
- Cursor high-water and closure
- Reward and usage truth states
- Trace digest
- Visual ID and revision
- CUA viewport results
- Total observed cost

## Recommended order

I would prioritize the additions like this:

1. **A9 semantic visual CUA**
2. **A10 responsive visual gate**
3. **A11 reconnect/reopen**
4. **A12 idempotent execution**
5. **A13 campaign isolation**
6. **A15 corruption and gap handling**
7. **V2 synchronized replay**
8. **W1 clean-workspace agent run**
9. **O1 budget enforcement**
10. **O4 secrets and workspace isolation**

Those tests cover the most important gap in the current note: not merely proving that events exist, but proving that Workshop can reliably turn them into a truthful, usable, recoverable product experience.

## Native live acceptance receipt — 2026-08-12

The signed `livecraftax` Workshop instance passed a real visual-before-execution run against the registered normalized Craftax Rust container.

- Workshop opened visual `vis_b2665c9ec8f94b0f845b0a08ed9351de` before environment mutation and marked revision 7 ready.
- The binding preserved both declared transports: SSE for live delivery and poll for cursor-based recovery. No URL was guessed.
- Rollout `roll_workshop_live_craftax_20260812_2102` ran seed 8 with `react` / `muse_spark_medium` and completed for $0.0188208.
- The native canvas reconciled 148 durable envelopes into 22 semantic events and rendered 13 real Containers PNG frames, reward -0.20, achievements 0, policy usage, synchronized replay, plots, and the full Trace V5 viewer.
- The selected `policy.call` showed the real Craftax observation as input and the model's action JSON as output. The provider emitted neither hidden reasoning nor tool calls, and the viewer truthfully displayed `Not emitted` for both.
- Rebuilding and relaunching reused the stable signing identity and produced no repeat Keychain authorization prompt.

Machine-readable evidence is in [`docs/receipts/aug_12_live_visual/in_app_live_receipt.json`](receipts/aug_12_live_visual/in_app_live_receipt.json).
