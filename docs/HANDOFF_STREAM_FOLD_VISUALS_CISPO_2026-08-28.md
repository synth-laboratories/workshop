# Handoff: live-stream fold, visual verification, CISPO templates

**Date:** 2026-08-28 · revised same day by a six-track verification pass — every factual claim
checked against code, corrections applied inline, open decisions resolved in "Decisions" below
**Verified against:** `workshop-v08-release` @ v0.8.0, branch `eval/inline-first-admission`
**Scope:** four related pieces of work — consolidating the live-eval fold into Rust (core),
giving the agent evidence that a live visual actually subscribed (visuals), promoting and completing
the CISPO template family (optimizers), and bringing the right panel to Codex-grade document
viewing (Part VII).

They are sequenced deliberately: the core change makes the visuals change nearly free, and both
together make the CISPO templates verifiable by construction.

---

# Part I — Core: one fold, in Rust

## The problem

The live-eval projection has one complete implementation and two partial, divergent shadows
(verified — the original "implemented three times" framing was too kind):

| # | Where | What it actually does |
| --- | --- | --- |
| 1 | `visuals/runtime/liveStream.ts` + `liveEvalReducer.ts` | the only complete pipeline: identity → dedupe → conflict detection → control filter → per-scope gap scan → fold |
| 2 | `src-tauri/src/storage/live_spool.rs` | dedupe **only** (~40 lines, `persist_live_envelopes` :81-112) — no fold, no gap scan, no conflict record — and its identity rule **contradicts** the TS one: `envelope_identity` (:39-79) treats a bare `event_id` as globally unique, exactly the bug `liveStream.ts:186-189` warns "silently drops all but one lane." For the multiplexed shape in `live_stream_contract.test.mjs:290-301` the spool persists one lane. |
| 3 | `src-tauri/src/reports/rollout_inspector.js` + `src-tauri/src/visuals/frozen_runtime.js` | **no fold at all.** `extractProjection` (rollout_inspector.js:241-254) is a locator for an already-computed trace projection; for live-eval, `frozen_runtime.js:15-25` renders `JSON.stringify(data.bindings)` into a `<pre>`. |

For a system whose value proposition is sealed reproducibility, the drift is not hypothetical — it
already happened: the spool can lane-collapse a multiplexed run today, and a sealed live-eval visual
would show raw envelope JSON. In practice it cannot even get that far: `freeze_bindings`
(`artifacts.rs:783-820`) requires a `snapshot` key that **no production code ever writes** (only
tests do), so sealing a live-eval visual fails outright — the live-eval export path is dead code.

## Scope beyond the three pipelines

The table above covers the three *pipelines*. The `sequence_fold_outside` conform ratchet (item 31),
run 2026-08-28, counts **16 further matches** — partial replay-skip / gap-detect folds re-implemented
outside any of them:

`optimizers/sidecar_training.rs` (5) · `cloud/intern/ingestion.rs` (3) · `optimizers/training.rs` (2)
· `optimizers/training_adapter.rs` (2) · `optimizers/eval_relay.rs` (2, a verbatim copy of the
`training_adapter.rs` fold) · `components/TerminalPanel.tsx` (2)

The renderer match is an independent defect: `TerminalPanel.tsx` keeps its own
`seen.current.has/add(event.sequence)` dedupe, which violates the style guide §8 rule that the
renderer renders projections and does not invent durable state. It should be removed under item 1
regardless of where the fold ultimately lives.

**Item 1 should be sized from all seven sites, not from the three pipelines.** The ratchet is the
inventory; it reaches zero only when the fold has one home.

## The target: pattern B

Three patterns already exist in-repo. Live-eval is the odd one out.

| | Rust owns | TSX owns | Used by |
| --- | --- | --- | --- |
| **A. Rust renders** | spec → deterministic SVG | display the rendition | `charts.rs`, `systems.rs`, `mermaid.rs` |
| **B. Rust projects** | bindings → literal values | render, interact, compose | `chart_data.rs` boundary |
| **C. Rust spools** | raw envelope persistence | dedupe, gaps, fold, project | **live-eval today** |

`charts.rs` already states the goal in the repo's own voice:

> The canonical source is the spec, the rendition is produced here in-process, and the pane displays
> that exact rendition. **One implementation means an agent capture and a human pane are the same
> pixels**, and review never needs a live window.

And `chart_data.rs` defines the boundary: resolution turns `from` blocks into literal values "so
`charts.rs` only ever renders literal values and **the renderer needs no knowledge of traces,
fixtures, or snapshots**."

**Move live-eval from C to B.** Rust owns the fold, the receipt, and sealing. TSX owns presentation
and interaction.

## The mechanics are already paved

Nothing here needs new infrastructure.

- **Typed boundary.** `tauri-specta` is wired and generating `src/renderer/src/generated/protocol.ts`
  (3,264 lines). `contract/specta.rs` carries a literal five-step recipe for migrating a command
  (:16-24), and already solves the awkward parts — `OpaqueJson` (:31-40), and an `OpaqueInteger<T>`
  wrapper (:54-62) written precisely because "every integer on this boundary is a sequence number, a
  cursor, a byte count, a token count or a millisecond timestamp." Budget one point of friction: the
  hand-maintained exported-command count assertion (`assert_eq!(exported, 276, …)`,
  specta.rs:494-497) and the committed `protocol.ts` staleness check must be bumped/regenerated with
  any signature change to `visual_stream_poll`.
- **The transport seam already passes through Rust.** `components/VisualHost.tsx:598-608` builds the
  ReplayClient over `bridges.visuals.pollStream({ visualId, pollUrl, after, limit })`; the loop runs
  at **500 ms** (`visuals/chrome/useLiveEvalStreams.ts:23` — the 750 ms timer at VisualHost.tsx:544
  is the separate optimizer-frames lane; don't instrument that one). The Rust handler is
  `visual_stream_poll` (`lib.rs:2360-2502`): it validates the URL against
  `visuals::declared_poll_urls`, fail-closed, performs the GET itself, and returns the page verbatim
  as `OpaqueJson`. The bytes already go through Rust; it just hands raw envelopes up.
- **Agent-facing shape.** `trace_query.rs` is the model — "Agents never receive SQL… An unknown field
  is an error rather than a passthrough, so storage can evolve without an agent's habits becoming a
  compatibility constraint."
- **Precedent on the optimizer side.** `optimizers/frames.rs` already puts `OptimizerFrameRef` /
  `OptimizerFrameDelta` on the specta boundary with `#[specta(type = Number)]` for `u64` sequences.

## The change

Port `ingestLiveEnvelopeBatch` + `projectLiveEval` into Rust behind the existing `pollStream` invoke,
and change what it returns:

```rust
struct LivePollResult {
    projection: LiveEvalProjection,   // cutoff-aware fold
    receipt: StreamReceipt,           // gaps, conflicts, per-scope sequences, ready
    cursor: ReplayCursor,
}
```

Two obligations the port must carry:

- **The fold is stateful across polls** (dedupe sets, per-scope sequences, conflicts). Rust holds it
  per `(visual_id, revision, stream)` keyed to the cursor, with a defined reset on revision change
  (the TS hook resets on `streamKey` change, `useLiveEvalStreams.ts:58-71`) and an idempotent
  "projection from cursor 0" answer so a renderer reload can resynchronize.
- **The same fold replaces `envelope_identity` in `live_spool.rs`** — port the TS identity/scope
  semantics (`streamId:sequence` first; never bare `event_id`). Existing spools persisted under the
  old rule may hold lane-collapsed data for multiplexed runs: version the spool schema
  (`synth.live-eval-spool.v1`) or re-spool; don't silently reinterpret. And preserve producer-cursor
  passthrough — the Craftax multiplexed fixture carries non-numeric string sequences, so cursors must
  never be recomputed from sequence numbers (`replayClient.ts:105-108` is the fallback to avoid).

Then **invert the export**: embed the projection in `synth-artifact-data` alongside the bindings, so
`frozen_runtime.js` stays a dumb renderer. This is not deduplicating an existing artifact fold — it
supplies the missing one (see the table above: today a live-eval seal fails outright, and would
render raw JSON if it didn't). Backward compat is a non-issue: sealed artifacts inline their own
runtime at seal time and are immutable CAS bundles, so a new `FROZEN_RUNTIME` changes
`runtime_digest` for new seals only. Keep the `bindings` key in `data.json` (add `projection` beside
it) and keep the `<pre>` fallback for unknown template ids.

## Costs and open decisions

1. **Sourced TSX gets less powerful.** Agent visuals currently receive events and may fold them
   however they like. A fixed projection means a novel aggregation needs a Rust change — landing on
   the axis where this system is already weakest.
   **Recommendation:** keep raw events available *alongside* the projection for sourced visuals; make
   the built-in templates and the `mark_ready` receipt authoritative on the Rust fold.
2. **Iteration slows.** Projection changes become a cargo rebuild instead of a Vite HMR round.
   Acceptable for the fold and receipt; presentation logic must stay in TSX.
3. **Hosts without Rust.** `runtime/diagnostics.ts` notes visuals run in "browser preview, the frozen
   runtime, exported artifacts." Embedding the projection at export time covers artifacts. Browser
   preview and fixture mode (`useFixtureReplay`, and both shipped shells) fold locally in TS and will
   keep doing so — the honest end-state is **one authoritative fold (Rust) plus a test-pinned TS
   mirror for fixtures**, policed permanently by a golden-fixture equivalence suite: the same
   checked-in fixtures (`cua-luna-low-10.json`, `examples/events.json`, every
   `live_stream_contract.test.mjs` scenario) run through both folds, outputs diffed canonically
   (sorted keys — `serde_json` here has no `preserve_order`, so never compare raw serializations).
   Treat WASM as a later option, not a prerequisite.
4. **RESOLVED — the cutoff is a per-stream cursor vector, not a sequence at all.** Verification
   killed both candidates: in the real multiplexed fixture
   (`live.craftax.v1/examples/cua-luna-low-10.json`, one stream, ten lanes), `sequence` is a
   **non-numeric string** (`"suites/…#s0:<uuid>:frame:0"`), so a scalar numeric cutoff is a no-op and
   `Record<scope, sequence>` cannot address the events either. The one durable total order that
   always exists is arrival order within a stream — already persisted verbatim by the spool and
   preserved by the fold. So: `cutoff: Record<streamId, count>`, a prefix length into each stream's
   deduped log; "projection at C" = fold of the first `C[streamId]` non-control envelopes per stream.
   The filmstrip orders snapshots by Σ counts (tie-break: streamId); `visual_timeline` expresses
   interesting points as cursor vectors captured when the interesting envelope was folded. Per-scope
   numeric sequences stay in the **receipt** for gap/conflict accounting — that is what they are
   actually good for. The shells already scrub by array index, not sequence
   (`live.harbor_eval.v1/shell.tsx:109-115`), so this matches how scrubbing already works.
5. **Component-local state breaks exact reconstruction.** A sourced visual can hold `useState` that
   isn't a function of the event prefix. Either mount reconstruction captures with fresh state and
   accept that interaction state isn't reconstructed, or make "render is a pure function of props" a
   `mark_ready` requirement. Prefer the former now.
6. **The port must replicate TS semantics exactly**, or the golden suite catches it on day one:
   `??` skips only null/undefined (treat `Value::Null` as absent); `Number()` is guarded by
   `String(x).length > 0` and `Number.isFinite` (sequences can be floats — model fold sequences as
   f64; only cursors are integers); Set/Map insertion order drives receipt ordering (use
   `IndexMap`/`Vec`, never `HashMap`); the conflict digest defaults to `JSON.stringify(event)` in
   document key order — define it canonically (sorted keys) in *both* implementations instead of
   porting the accident.

---

# Part II — Visuals: prove the stream actually worked

## Already computed, then discarded

`runtime/liveStream.ts` produces a full ingest state per batch:

```ts
export type LiveIngestState = {
  ids, digests, lastSequenceByScope, receivedSequencesByScope,
  gaps: Array<{ scope: string; after: number; before: number }>,
  conflicts: string[],
  ready
}
```

Per-scope sequence-gap scanning and duplicate-envelope-with-conflicting-digest detection, both
already implemented. And then `chrome/useLiveEvalStreams.ts:110-113`:

```ts
if (ingest.current.conflicts.length) {
  setError(ingest.current.conflicts.at(-1) ?? "Conflicting replay envelope");
} else if (ingest.current.gaps.length) {
  setError(`Evidence gap after sequence ${ingest.current.gaps.at(-1)?.after}`);
}
```

Structured evidence → one human-readable string → dropped. The agent never sees it.

**Dead code to wire:** `VISUAL_STREAM_CODES.streamReplayGap` (`runtime/diagnostics.ts:41`) and
`STREAM_REPLAY_GAP` (`src-tauri/src/diagnostics/codes.rs:38`) are declared in both languages and
**emitted from nowhere** on the live-eval replay path. (A renderer-local constant with the same
string fires on the optimizer lane — `VisualHost.tsx:993-994` — don't mistake it for coverage. The
remediation text for the Rust code already exists at `codes.rs:185-187`.)

**Already present:** `projectLiveEval(events, cutoffSequence)` is a pure fold that already filters
`seq > cutoffSequence` and reports `cutoff_sequence` in its output. Nobody calls it with a cutoff.

## 1. Stream receipt

Compute the receipt **in Rust at the `visual_stream_poll` seam** (`lib.rs:2360-2502`) — Rust already
holds every envelope byte the renderer folds, correlated to `visual_id` + `revision`, so the receipt
is neither renderer-reported nor agent-forgeable. (Browser preview bypasses the bridge via raw
`fetch`, but readiness cannot run there anyway — reviews need a Desktop capture — so "no server-side
fold observed" simply reads as "not ready", which is also the right answer for a pane never actually
rendered in Workshop.) Keep the existing renderer-side `RenderedVisualObservation` for what only the
DOM knows (rendered frame counts). Surface the receipt by merging it into the
`GET /v1/visuals/{id}/authoring` response (`visuals_ipc.rs:2455-2469`) — the agent already calls
`visual_authoring_context` in its loop; don't add a new tool:

- `state` from the six-state transport machine (`idle` / `declared` / `replaying` / `live` /
  `terminal` / `error`), plus time-in-state
- per stream: id, declared source, first-response latency, last sequence, envelope count, closed
- `gaps[]` and `conflicts[]`, verbatim
- `ready`, `recovered`, and whether the visual ever left `declared`
- envelope counts by `kind`, and non-control envelope count

The last two catch the failure modes that look identical on screen today: *declared ten streams and
opened none*, and *opened fine, received only control envelopes*. Both render as a plausible empty
visual.

## 2. Gate on it

Today's bar is stricter than "two reviews" but still transport-blind: `POST /v1/visuals/{id}/ready`
(`visuals_ipc.rs:2647-2765`) requires ≥2 reviews of the current revision at ≥2 distinct viewport
widths with capture-bound observation receipts, and — only for templates declaring an
`observationContract` — checks transport state against `READY_TRANSPORT_STATES = ["live","terminal"]`
plus per-template `minimumRolloutCount / minimumRenderedFrameCount / minimumSemanticEventCount`.
Extend it with the receipt check, slotted beside `certification_receipts` (~:2695-2702):

- **Conditional**: apply only when `declared_poll_urls(bindings)` is non-empty; fixture-only visuals
  (mermaid, charts, trace-bound) pass vacuously — the same conditionality pattern as
  `observation_contract.is_some()`.
- **Thresholds from the template's existing `readiness` knobs**, not hardcoded constants. "At least
  one non-control envelope" is already expressible as `minimumSemanticEventCount` (every shipped
  observation-contract template sets 1; `craftax.eval_matrix` deliberately sets
  `minimumRolloutCount: 0`) — reuse the knobs, or a legitimately-empty run gets adjudicated by a
  constant instead of by its template.
- **Fix two pre-existing hazards first, or the gate will hard-block honest runs**: the ingest fold
  skips control envelopes *before* recording their sequences (`liveStream.ts:245-248`), so any
  producer that sequences its heartbeats produces permanent phantom `gaps[]`; and the fold checks
  kind-only while the projector also honors a `control: true` flag (`liveEvalReducer.ts:51`) —
  reconcile the two while in there.
- Fail closed with a named reason; "no server-side fold observed" gets the
  `visual_observation_unavailable` treatment (show the visual in Desktop, then retry).
- Cheap immediate win, independent of everything above: `live.harbor_eval.v1/template.json` declares
  no `observationContract` at all — its gate today is only the two screenshots. Add the contract.

This is the forcing function; encouragement is not.

## 3. Filmstrip reconstruction

Because the projector is a pure fold over an ordered prefix, `render(project(events, n))` is already
well-defined.

- **`visual_capture_review({ visualId, viewport, atSequence })`** — same PNG-back-to-the-agent path,
  mounted against the projection at a logical cutoff.
- **`visual_timeline({ visualId })`** — the sequence numbers worth looking at, which the reducer can
  already identify: sequence 0, first non-control envelope, first `frame`, each `verifier`,
  `reward_signal`, `eval.run.terminal`, and both sides of every gap.

Catches what a single end-state screenshot cannot: broken empty state, off-by-one on the first
envelope, axes that only look right once data is dense, a spinner that never clears because `ready`
is computed from the wrong signal, and `lastKnownGood` pinned stale — visually identical to working,
at the end.

Storage exists: `content_store.put_bytes("previews", …)` and `preview_digest` per revision. A
filmstrip is preview digests keyed by `(visual_id, revision, cutoff)` — store them in CAS, not the
loose `visual-review-captures/` files today's review PNGs use — and `VisualSeal` already carries
`data_digest` + `runtime_digest`, so it is sealed reproducible evidence rather than a pile of
screenshots. `visual_timeline` needs no renderer work at all: the Rust fold (or the CAS spool for
finished rollouts — `eval_driver.rs:1725-1746` already replays every stream from `after=0` at rollout
end) answers it directly. `atSequence` on `visual_capture_review` is host→shell prop plumbing into
`projectLiveEval`'s already-live, never-called cutoff parameter.

## 4. Do not ask the agent to add logging

Agent-authored logging varies per visual, can't be trusted as evidence, and asserts about a
subscription the visual deliberately does not own. The seam already exists and is documented:
*"the host installs a sink and the visuals call it… No host, no sink, no error."*

**Instrument the seam, hand the agent a receipt.** The agent's job is to read a receipt and look at
pictures, not to narrate its own correctness.

## Smallest shippable version

Surface `gaps`/`conflicts`/`state` in `visual_authoring_context`, add `atSequence` to
`visual_capture_review`, add the receipt check to `visual_mark_ready`. Three small changes against
machinery that already exists.

---

# Part III — CISPO templates

## The gap

`contract/runtimes.rs:160-176` advertises `algorithms: ["gepa", "sft", "cispo"]` with three bounded
recipes (`cispo.mlx.v1`, `cispo.tinker.hosted.v1`, `cispo.slime.modal.v1`) and a typed
`CispoConfigV1` (`training_spec.rs:798`).

`visuals/families/optimizers/` contains `eval`, `gepa`, `sft`, `_shared` — **and no `cispo`.** But the
original framing here was wrong in a way that changes the work: **CISPO does not fall through to the
generic run shell.** `optimizers/service.rs:2721-2729` routes `"sft" | "cispo"` to
`optimizer.sft.live.v1`, pinned by a test (`service.rs:5875`), and that template's shared workspace
already carries a real CISPO mode: `projectEvents.ts:2341-2388` builds a full `projected.cispo`
state (objective, clip bounds, group size, reward variance, advantage mean±std, warm-start artifact,
checkpoint lineage, no-learning-signal) from `cispo.step.metrics` / `cispo.clip.identity` /
`cispo.no_learning_signal` / `cispo.warm_start` / `cispo.checkpoint.ready`, and
`overlays/sft/SftWorkspace.tsx` renders it — a dedicated `CispoIdentityPanel` (:604-646) with clip
identity pinned in the header and a no-signal row that reads "Stopped truthfully — uniform group, no
fabricated advantage." Covered by `visuals/tests/cispo_workspace.test.mjs`.

So the gap is narrower and stranger than "never authored": the visual exists but wears SFT's id; the
`algorithm_label` match has no `"cispo"` arm so the chrome says "Optimizer" (`service.rs:2710-2719`);
and the adapters treat CISPO events unevenly — **precisely, per match arm** (verified in both files,
which map identically; there is no sidecar-vs-hosted divergence in the mapping):

- `training.clip` → `cispo.clip.identity` and `cispo.no_learning_signal` **forward the payload
  wholesale** (`sidecar_training.rs:1020-1026` ≡ `training_adapter.rs:358-364`), and the projection
  reads `eps_low`/`eps_high` out of the wholesale clip payload — so clip identity and the no-signal
  state genuinely work on live runs, on both paths.
- The `training.metric` arm is a **four-field whitelist** in both files
  (`sidecar_training.rs:972-981`, `training_adapter.rs:321-328`): `step`, `train_loss`,
  `learning_rate`, `throughput`, built with `Map::from_iter` — anything else in the payload is
  dropped. The projection reads `group_size`, `reward_variance`, `advantage_mean`, `advantage_std`,
  `optimizer_step` from exactly this event (`cispo.step.metrics`, `projectEvents.ts:2363-2373`), so
  the variance/advantage half of the identity panel is fed by fixtures and tests today, not live runs
  (`optimizerSteps` degrades to a hardcoded 1 via the projection's fallback).

**Necessary vs sufficient:** widening the whitelist in both arms is the necessary Desktop-side fix,
but `group_size`/`reward_variance`/`advantage_*` appear nowhere in `mlx_runtime.rs` or either
adapter's tests — there is no in-repo evidence the MLX wheel puts them in `training.metric` payloads
at all. Confirm the wheel's payload on one real MLX run before assuming the adapter fix alone lights
the panel; if the wheel doesn't send them, this is item 14's emitter gap wearing a different hat.

## What the emitted data supports

From `training_spec.rs:742-840`:

- `CispoObjective` — `cispo_minimax` | `cispo_two_sided`
- `CispoReduction` — `sum` | `mean_tokens`
- `eps_low`, `eps_high`, `group_size` (validated `>= 2`), `rollout_budget`, `signal_attempts`
- `CISPO_NO_LEARNING_SIGNAL = "cispo.no_learning_signal"` (`training_spec.rs:89`), emitted from
  `sidecar_training.rs:1022` and `training_adapter.rs:360`

Clip identity is not a tuning knob — the validator refuses `cispo_minimax` with `eps_low < 1.0` as
"a different objective wearing CISPO's name." It belongs pinned in the chrome, not buried.

## Templates to author

1. **`optimizer.cispo.live.v1` — a promotion, not an authorship.** The CISPO workspace exists inside
   `optimizer.sft.live.v1`; give it its own id: a ~70-line shell mounting the shared workspace in
   CISPO mode, plus five registration touches — the family directory, `EXPECTED_IDS` in
   `visuals/tests/registry.test.mjs`, `runtimes.rs` `templates`, the `primary_visual_template` arm in
   `service.rs:2723` (split `"cispo"` out of `"sft"`), and the missing `algorithm_label("cispo")`
   arm. Prerequisite that makes it honest on live runs: **fix the two adapter arms to forward
   `group_size` / `reward_variance` / `advantage_mean` / `advantage_std` / `optimizer_step`** — a
   small, Desktop-local change. Rollouts-vs-budget renders from run config/limits, not events (the
   budget is never emitted); `signal_attempts` renders as "attempts allowed: N" plus the terminal
   no-signal state — a live climbing counter cannot be built from current events (see item 14).
2. **`optimizer.cispo.groups.v1` — re-scoped.** Per-group reward spread has the exact emitter gap
   this document warns about for KL: nothing emits per-group data, and the two group event constants
   (`CISPO_ROLLOUT_COLLECTED`, `CISPO_ADVANTAGES_COMPUTED`, `training_spec.rs:86-87`) are declared
   and emitted by nothing. Minimum viable version renders the **aggregate** variance/advantage stats
   the adapter fix unlocks; per-group spread is gated behind the item-14 emitter decision and must
   not be promised before it.
3. **`optimizer.cispo.rollouts.v1`** — mirrors `optimizer.sft.rollouts.v1` (an 80-line shell over
   `projected.sft.campaigns`); cheapest to author **if** CISPO runs actually emit the SFT-named
   campaign events it groups on (`sft.campaign.updated` / `sft.checkpoint_evaluation.*` —
   `training_spec.rs:70-75` explicitly warns CISPO reuses SFT event names). Verify on a real run
   before copying the shell.

**Warm-start lineage:** `optimizer.sft.lineage.v1` is not a graph — it renders a flat per-run
`{baseModel, adapter, checkpointId, deployable}` list fed solely by `sft.model.materialized`. The
cheap, honest version of the warm-start edge is a fourth node ("SFT warm-start artifact") above Base
— the projection already resolves `warmStartArtifactId` (`projectEvents.ts:2901-2902`). A true
cross-run artifact graph is a new data model; don't smuggle it in under this item.

## What NOT to build

Resist the generic RL dashboard. The emitted event vocabulary across the optimizer crate is `step`,
`reward`, `loss`, `learning_rate`, `epoch`, `tokens_per_second`, `throughput`, `reward_uplift`,
`reward_delta`. There is **no KL, no entropy, no advantage histogram, no grad_norm, no per-step clip
fraction** — the two `clip` hits are config bounds, not measurements. Verification extends the list
of things that do not exist in events today: **no per-group rewards**, **no per-step reward** (reward
lives on evaluation events — score/baseline/delta — not step metrics), **no signal-attempt progress**
(`signal_attempts` is a config knob sent in the request, `cispo.rs:323`; only the terminal
no-signal event exists), and **no rollout-budget consumption** (the budget is config, not events).

Panels for those would render em dashes forever, and by house convention that is correct but useless:
"absence survives the pipeline… `None` reaches the renderer as a gap, a hatched cell, or an em dash."

**The template gap is downstream of an emitter gap.** Clip-fraction, IS-ratio distribution, or KL
require changes in `sidecar_training.rs` / `training_adapter.rs` / `mlx_runtime.rs` **first**, and
they must land across all three placements or the visual is honest on MLX and blank on Tinker and
Modal.

## Sequencing

1. Author the three templates against data that exists today.
2. Register them in `runtimes.rs` `templates` for the `optimizers` runtime — host vocabulary, so no
   plugin release is needed (`optimizers/CONTRACT.md` §1).
3. The emitter decision (item 14) is now informed: none of the three training loops live in this
   repo — MLX is the signed, revision-pinned `synth-mlx-rl` wheelhouse; the Tinker lane is dark
   (`TINKER_CISPO_UNAVAILABLE`, `sidecar_training.rs:58`) pending an admission receipt; slime/Modal
   is dark (:59) behind a pinned external image. New metrics cannot land atomically across the three.
   What CAN land now, Desktop-side and atomically: the adapter-forwarding fix plus projection/UI
   plumbing that renders whatever arrives and em-dashes what doesn't. Defer the upstream emitters;
   when they ship, the panels light up without a Desktop release.

---

# What each run type looks like when this lands

All four share one spine: the contract-v1 events route
(`GET /runs/{run_id}/optimizer-events?after_sequence=&limit=`) → Rust dedupe, gap scan, fold, project
→ `{ projection, receipt, cursor }` → TSX shell. `OptimizerEventEnvelope` already carries
`algorithm_id`, `sequence_number`, `delta`, and `snapshot`, so the fold is one code path discriminated
by algorithm.

| Run type | Templates | Projection | Filmstrip payoff |
| --- | --- | --- | --- |
| **eval** | `optimizer.eval.live.v1` | rollouts, reward (verifier → reward_signal → terminal), usage/cost | reward that is only correct at terminal |
| **gepa** | `.live` `.frontier` `.candidate` `.evaluations` | candidate population, Pareto frontier, accept/reject | **the frontier at generation n** — a front that looks right at the end can have been wrong for 20 generations |
| **sft** | `.live` `.checkpoints` `.dataset` `.examples` `.lineage` `.rollouts` | loss curve, checkpoints, dataset, lineage | axis scaling that only looks right once dense |
| **cispo** | **wears `optimizer.sft.live.v1` today — promote to its own id** | identity panel (objective, clip bounds), aggregate variance/advantage (after the adapter fix), no-signal terminal state | the no-signal trip point |

---

# Work items — parts I–III

| # | Part | Item | Depends on |
| --- | --- | --- | --- |
| 1 | Core | Port ingest + fold to Rust behind `pollStream`; return `{projection, receipt, cursor}` | — |
| 2 | Core | **Decided:** cutoff = per-stream cursor vector (prefix counts); implement in fold + filmstrip | 1 |
| 3 | Core | Export projection (not bindings) into `synth-artifact-data`; strip projection from `frozen_runtime.js` | 1 |
| 4 | Core | Keep raw events available to sourced visuals alongside the projection | 1 |
| 5 | Visuals | Surface `gaps` / `conflicts` / `state` in `visual_authoring_context` | 1 |
| 6 | Visuals | Emit the dead `stream_replay_gap` diagnostic code | 1 |
| 7 | Visuals | Add `atSequence` to `visual_capture_review`; add `visual_timeline` | 1, 2 |
| 8 | Visuals | Gate `visual_mark_ready` on a clean receipt | 5 |
| 9 | CISPO | Promote the existing CISPO mode to `optimizer.cispo.live.v1` (thin shell + 5 registration touches, incl. `primary_visual_template` and `algorithm_label`); **widen the four-field whitelist in both `training.metric` arms** (clip/no-signal arms already forward wholesale) — and confirm on one MLX run that the wheel actually sends the step fields | — |
| 10 | CISPO | Author `optimizer.cispo.groups.v1` on **aggregate** stats; per-group spread waits on item 14's emitters | 9 |
| 11 | CISPO | Author `optimizer.cispo.rollouts.v1` — after verifying CISPO emits `sft.campaign.*` on a real run | — |
| 12 | CISPO | Register CISPO templates in `runtimes.rs` + `EXPECTED_IDS` | 9-11 |
| 13 | CISPO | Warm-start as a fourth node in the flat lineage (projection already resolves `warmStartArtifactId`); cross-run graph explicitly out of scope | — |
| 14 | CISPO | **Decided: defer upstream emitters** (three external release trains; Tinker and Modal lanes dark today); land Desktop-side adapter/projection plumbing now | — |

Items 9-13 do not block on Part I, but land better after it: they inherit the receipt, the filmstrip,
and the `mark_ready` gate, so the CISPO templates can be verified at t=0, at first group, at the
no-signal trip, and at terminal without anyone hand-watching a live run.

---

# Part IV — Reifying the right panel; plugin boundary as a data boundary

## The panel is already half-reified

`presentation.rs` opens with exactly the intent:

> **Deterministic right-panel presentation, shared by the native UI and the agent-facing MCP facades.**
> Visual lifecycle for a domain record — identity, eligibility, binding, reuse, and the show event —
> lives here rather than in whichever caller got there first. The renderer's `DataPage` grew its own
> copy of this logic; a second copy on the agent path would have drifted from it immediately.

The reification already happened once, under exactly the pressure you'd predict. The limit is that
it is a **special case, not a registry**: `TRACE_INSPECTOR_TEMPLATE`, `TRACE_CATALOG_TEMPLATE`,
`trace_inspectability()`, `trace_inspector_visual_id()`. A second domain means a second set of
constants and functions, or a third copy in whichever caller gets there first.

Two facts to carry into items 15-17: the module's only production callers are the agent-facing IPC
dispatcher (`visuals_ipc.rs` `dispatch_traces` — a single integration point), so the enum
generalization touches only the Rust agent path; and the renderer keeps a **by-hand mirror** of the
eligibility logic (`src/renderer/src/runtime/traceInspector.ts:13-40` — extracted from DataPage, not
generated), so the `TraceInspectability → Presentability` rename must keep wire labels stable or
update that mirror in the same change. Whether new providers' panes also appear in the native
DataPage catalog is separate renderer work — items 15-17 explicitly exclude it.

Note that `TraceInspectability`'s comment already states a **general** panel law, written in
trace-specific code: "The catalog shows every trace and names the reason rather than silently
omitting the unavailable ones." That should hold for containers and optimizer runs too.

## Four registries, none of them the panel

| Registry | Keyed by | Owns |
| --- | --- | --- |
| `plugins/registry.rs` | plugin id | install lifecycle (14 phases), channels, permissions, receipts |
| `contract/runtimes.rs` | runtime id | algorithms, templates, bounded recipes |
| `visuals/registry/index.ts` | template id | family discovery by glob, three-tier overlay |
| `presentation.rs` | **nothing — hardcoded to traces** | panel lifecycle |

They are already cross-linked: `plugins/registry.rs` imports `contract::runtimes::OPTIMIZERS as
CONTRACT`. And `plugins/registry.rs` states the intent to generalize — state is "one JSON file per
plugin, named for it… so a second one adds a catalog arm rather than a second registry."

## Proposed shape: panel host is core, panes are providers

`presentation.rs` → `presentation/` with a provider abstraction; traces becomes the first
implementation.

```rust
trait PanelProvider {
    fn provider_id(&self) -> &str;                              // joins to plugin id
    fn presentable(&self, record: &DomainRecord) -> Presentability;
    fn visual_id(&self, record: &DomainRecord) -> String;       // deterministic → reuse
    fn template_id(&self) -> &str;                              // host vocabulary
    fn projection_schema(&self) -> &str;
    fn bindings(&self, record: &DomainRecord) -> Vec<BindingDescriptor>;
}
```

`TraceInspectability` generalizes to `Presentability { Present, Unavailable(reason) }`.

**What must stay core.** `optimizers/CONTRACT.md` §1 already has the rule: "a runtime answers only
for itself, and host vocabulary is never round-tripped through a runtime" — and read the other way,
"the host must not answer on a runtime's behalf."

Core keeps the panel host and slot lifecycle, the fold, the receipt, the seal, capture / review /
`mark_ready`, revisions, annotations, secrets, approvals. **If a plugin could supply its own fold or
its own receipt, the receipt would attest nothing** — the same reason a host-served handshake is "a
digest of a host constant" whose anti-swap pin "attests nothing." A provider declares *what* to
present; core decides *whether* it is ready.

## The real coupling is the schema, not the modules

`storage/migrations.rs` is **3,432 lines defining 90 tables behind one global monotonic counter**,
and it already carries the scar:

> A version number is a promise about *this* lineage. Several v0.5 lanes were developed in parallel
> and **each numbered its own migration 23**, so an install that reached version 23 on another lane's
> DDL would skip this lane's table forever.

The mitigation is a `REQUIRED_TABLES` repair list plus `heal_missing_tables()`,
`heal_missing_columns()`, and `heal_experiment_graph_shape()` on every startup. **A plugin
architecture is parallel development made permanent**, so that failure mode stops being an incident
and becomes the steady state.

**A second schema already exists outside the lineage.** The `create_table_outside` ratchet counts 2,
both in `optimizers/local_lora.rs`: `LOCAL_LORA_DDL` (`local_lora_checkpoints`, 18 columns, :17) and
`HOSTED_LORA_OVERLAY_DDL` (`hosted_lora_overlays`, :41). Both are `#[allow(dead_code)] pub const` and
currently unreferenced — table definitions no migration ever creates or upgrades. The per-plugin
schema drift this part proposes to prevent has already arrived by accident. See item 22a.

Namespacing already exists informally and is enforced nowhere: `experiment_*` (12 tables),
`optimizer_*` (10), `report_*` (9), `trace_*` (8), `visual_*` (6), `secret_*` (4), `usage_*` (3),
`failure_*` (3), `evaluation_*` (3).

## Three planes, three owners

| Plane | Owner | Today |
| --- | --- | --- |
| **Log** — append, order, dedupe, seal, address | **core, always** | `event_journal`, `live_spool`, `content_store` |
| **Projection** — fold a log slice into a typed view | **plugin defines, core executes** | `projectLiveEval`, `craftaxTraceView`, `chart_data.rs` |
| **Presentation** — templates, panes | **plugin** | `visuals/families/*`, panel providers |

The middle row is Part I generalized: the plugin supplies a **view definition**; core owns view
lifecycle — running, caching, invalidating, digesting. A materialized view. The plugin never touches
durability, ordering, or the digest algorithm, which is why the receipt still attests something.

## Four data-side mechanisms

1. **Migration lineage per plugin.** Replace the global counter with `schema_migrations(plugin_id,
   version)`; core migrations run first, each plugin advances its own lineage. This kills the
   migration-23 class rather than healing it after the fact.
   Prefer **table prefixes over `ATTACH DATABASE`** given current access patterns — plugin tables
   join to core `traces` and `visuals` constantly, and attached-DB transactions get awkward. Revisit
   `ATTACH` only if "uninstall = delete one file" becomes a hard requirement.
2. **Projections registered by schema id.** Every contract is already named this way —
   `synth.visual-template.v1`, `synth.trace-stream-event.v1`, `synth.live-eval-spool.v1`,
   `synth.trace-projection.rollout-inspector.v1`, `synth.container.live-eval.v1`. A plugin declares
   `provides` / `consumes` / `filter`; core resolves input schema → fold → output schema, and each
   is versioned independently of the code producing it.
3. **Extensible query allowlist.** `trace_query.rs` already establishes the rule: "Agents never
   receive SQL… An unknown field is an error rather than a passthrough, so storage can evolve without
   an agent's habits becoming a compatibility constraint." Let a plugin contribute allowlisted
   columns over its own namespace. The agent learns one query language whose *vocabulary* grows and
   whose *shape* never changes.
4. **Composition through digests, not calls.** `content_digest`, `bindings_digest`, `data_digest`,
   `runtime_digest`, `receipt_digest`, `source_digest`, `preview_digest`. Plugins meet in the CAS.
   The CISPO warm-start referencing an SFT artifact id is a lineage edge in the log, not an API call
   into the SFT plugin — so optimizers ships without containers installed and the edge still resolves.

## The uninstall trap, and where it converges

If a seal stores "raw log + a fold that lived in the plugin," uninstalling optimizers makes every
historical GEPA artifact unrenderable — fatal for a system selling reproducibility.

**Seals must store the projection output, not a reference to a fold that may not exist later.** That
is Part I item 3 (export the projection into `synth-artifact-data`), arrived at from a second
direction. Land it for both reasons, and record which plugin and projection schema produced the seal.

## Two traps

- **Do not reuse `PluginStatus` for panel providers.** The plugin lifecycle is OS-facing — 14 phases
  including `downloading`, `verifying`, `needs_permissions` with macOS deep links, release channels.
  A panel provider is compiled in. Share the id namespace, not the status type, or every pane gets a
  `downloading` phase that cannot occur.
- **At N=2 providers a trait is worse than an enum.** Write step 1 as an enum with a match; lift to a
  trait when a third provider lands.

---

# Part V — First-class user visual templates

## The concept exists; the mechanism is the blocker

`visuals/templates-internal/README.md`:

> Staged, never authored here. **Source of truth is `~/.synth/visuals/templates/`.**
> `./scripts/stage-internal-visuals.sh` — symlink `~/.synth` templates in

So out-of-repo templates are already a concept, with `distribution: "internal"` and a no-shadow rule.
But installing one requires **a checkout, a symlink script, and a rebuild**, because
`registry/index.ts` discovers templates with `import.meta.glob`, which resolves at build time. The
symlink exists only to make the glob see them.

## The ladder, with one rung missing

| Rung | Where | Rebuild? | Exists |
| --- | --- | --- | --- |
| 1. sourced one-off | compiled in pane via `compileSourcedModule` | no | yes |
| 2. instance | `visuals/instances/*.tsx`, frozen bindings | no | yes |
| **3. user template** | **`~/.synth-desktop/visuals/templates/`** | **no** | **NO** |
| 4. internal template | symlinked into repo | yes | yes |
| 5. shipped family | `visuals/families/` | yes | yes |

Rung 3 is the only one reachable by a user without a checkout.

## Live bug found while investigating

The Rust registry **already has a runtime tier**. `visuals/templates.rs` `build_template_index()`
scans, in order: `families/` (recursive) → `templates/` → `templates-internal/` → a **managed**
registry from `managed_templates_root()`, which requires `template.json` + `renderer.html`, tags
`source_kind: "managed"`, and hard-errors on collision with a bundled id.

But:

```rust
fn managed_templates_root() -> PathBuf {
    std::env::var("SYNTH_DESKTOP_DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| visuals_root())   // ← returns <root>/visuals
        .join("visuals")                       // ← doubled
        .join("templates")
}
```

With `SYNTH_DESKTOP_DATA_ROOT` unset, this resolves to `<repo>/visuals/visuals/templates` — a path
that never exists. **An unset data root silently disables the entire managed template registry** —
and verification found it is worse than "disabled" on both axes:

- **Not dev-only — the opposite.** The env var is set only by the dev-instance launcher
  (`scripts/desktop-instance.sh`) and tests. Canonical installs have no env; descriptor-launched
  bundles get their data root from `Contents/Resources/instance.json` precisely because
  LaunchServices drops env. So the broken cases are **production installs and descriptor bundles**;
  the env-setting dev launcher is the only path that works today.
- **Post-import it is actively wrong, not just dead.** `import_managed_template` (`templates.rs:243`)
  `create_dir_all`s the doubled path, after which the scan tier "works" — from
  `<repo>/visuals/visuals/templates` in dev (repo pollution), or from **inside the signed .app
  bundle's Resources** when packaged, which is lost on update and invalidates the code signature.
  Ship a one-time cleanup for strays and a release-note line.

The correct helper already exists and is used everywhere else: `instance::state_root()`, which honors
instance identity and otherwise returns `~/.synth-desktop`. This is also precisely the local-copy
drift that `instance_paths.rs` warns about — "Nine copies of one rule is nine places for it to drift,
and it did."

**Fix:** `crate::instance::state_root().join("visuals").join("templates")`. Verified exact:
`state_root()` resolves instance identity whose env arm reads the same `SYNTH_DESKTOP_DATA_ROOT`, so
the one case that works today (env set) is **byte-identical** after the fix, descriptor bundles and
canonical installs are repaired, and a raw `env::var` read disappears (the direction Part VI's a2
conform check wants). `templates.rs` compiles only into the app crate (the MCP adapter proxies over
HTTP), so `crate::instance::` is available and the adapter-purity lock test is not implicated. Add a
regression test pinning `managed_templates_root()` against `IsolatedDataRoot` (`instance.rs:954`).
That single change makes the managed registry work *and* puts it exactly where user templates belong
— beside `config.toml` and `.env`.

## Proposed design

```
~/.synth-desktop/
  config.toml
  .env
  visuals/templates/<template-id>/
      template.json                  # synth.visual-template.v1 — same schema as families
      shell.tsx                      # sourced TSX  → source_kind "user"     (NEW)
      renderer.html                  # sandboxed HTML → source_kind "managed" (exists)
      examples/                      # optional fixtures
```

One directory, two shapes. The scan already validates `schemaVersion == synth.visual-template.v1`
and id-equals-directory, and `shell.tsx` is already detected (`shell_path`, `templates.rs:449-451`).
Two overstated reusables to fix while in there: **the managed-tier scan follows symlinks** (only the
families recursion and the import path refuse them — add `symlink_metadata` checks to a scan loop
that now reads an agent-writable directory), and `MANAGED_TEMPLATE_MAX_BYTES` gates only
`import_managed_template`, not the scan — decide whether hand-edited files get the same cap.

**Rendering rides the existing sourced path.** `VisualHost.tsx:741` already branches on
`isSourcedTemplate(templateId)` → `compileSourcedModule(source)`. A user template is agent TSX with a
manifest, so it inherits the whole capability model for free: eleven allowlisted imports, no
`fetch` / `EventSource` / `WebSocket` / `eval` / `window` / `import.meta`, globals shadowed to
`undefined`, 256 KiB cap, no guessed stream URLs.

**Do not add a second allowlist validator in Rust.** Rust does structural validation (manifest, id,
size, regular files, no symlinks); the pane keeps semantic validation via `validateSourcedSource`,
which already fails closed and renders `sourcedInvalidShell` with the exact message. One
implementation, consistent with Part I.

**Registry merge.** The TS glob cannot see runtime files, so `listTemplates()` must become
`static families ∪ runtime user templates`, with Rust serving the runtime set — another reason the
registry wants to live in Rust. The metadata half already exists: `visuals_templates_list/get`
(`lib.rs:2628-2637`) return `TemplateMeta` including `shellPath` and `sourceKind`. The genuinely
missing piece is one command returning **shell.tsx source text** (read only under
`managed_templates_root()`, size- and symlink-checked), plus switching VisualHost's branch from id
equality (`isSourcedTemplate` is literally `templateId === "sourced.visual.v1"`) to
`meta.source_kind === "user"`, and un-hardcoding the "sourced.visual.v1 requires content" error
string.

## Agent flow

Today `visual_save_tsx` writes an **instance** (frozen bindings). Nothing promotes an instance to a
reusable template. Add:

- `visual_save_template({ id, manifest, source })` → writes `~/.synth-desktop/visuals/templates/<id>/`
- `visual_create_template --from <family-id>` → scaffold by copying a shipped family
- `visual_validate_template` → manifest schema, id ↔ directory, allowlist scan, missing shell

The authoring loop then terminates somewhere durable: sourced visual → review at two viewports →
`mark_ready` → **promote to user template** that survives app updates and needs no rebuild.

## Developer experience

- **Hot reload.** `notify` is not currently a dependency — fs-watch is a new crate. Since every
  agent write flows through the item-26 tools, start with emit-event-on-save plus rescan-on-focus for
  hand edits; buy the watcher only if that feels laggy. sucrase is transform-only, so recompile is
  instant either way.
- **Errors in the pane.** Reuse `sourcedInvalidShell` / `visual-sourced-invalid` verbatim, so a bad
  user template says why, where the user is looking, in the same words the agent gets.
- **Per-instance, not shared — decision reversed by verification.** `state_root()` intentionally
  collapses into the instance's private data root for named instances; sharing would require a
  hard-coded `~/.synth-desktop` literal (the exact drift Part VI's conform check bans, with
  `whisper.rs` as the lone bad precedent), would let an agent in one instance plant templates every
  instance renders, and — because the managed tier hard-errors on bundled-id collision — one bad
  shared template could poison `build_template_index()` for **all** instances. Per-instance is also
  what the item-23 fix already implements. If cross-instance sharing is ever wanted, add an explicit
  named helper in `instance.rs`, never a literal.

## Traps

1. **Sealing.** A visual sealed against a user template references code on one machine. The seal must
   embed the template **source** (or its digest with the source in CAS). Same convergence as the
   plugin-uninstall problem.
2. **An agent writing here persists code that runs at every launch** — a different act from rendering
   in the pane. `internal_readme.md` sets the precedent for this class: entering an API key "creates
   persistent access and must follow the Computer Use confirmation policy." Concretely: a new
   `ApprovalKind` variant routed through the existing `ApprovalBroker` (`session/approval.rs:129`),
   modeled on `PluginLifecycle` — it already carries publisher, digest, size, retention: exactly the
   fields a "persist agent-written renderer code" card wants. No new confirmation machinery.
3. **Keep no-shadow, add fork.** Users will want to override a shipped template; let them fork to a
   new id via `--from` rather than shadow an id, so a shipped id always means one thing.
4. **Deletion, not addition.** `stage-internal-visuals.sh` and the symlink dance go away; internal
   templates become an ordinary user-template directory the team happens to share.

## `~/.synth` vs `~/.synth-desktop` — smaller than expected

Only three sites, and **the data is not actually split**:

| Site | Says | Reality |
| --- | --- | --- |
| `renderer/src/components/VoiceRecognitionSettings.tsx:12` | `DEFAULT_MODELS_ROOT = "~/.synth/whisper-models"` | `whisper.rs:382` uses `~/.synth-desktop/models/whisper`. **The UI displays a path the app does not use.** |
| `scripts/stage-internal-visuals.sh:26` | `$HOME/.synth/visuals/templates` | staging source only |
| `visuals/templates-internal/README.md:3` | `~/.synth/visuals/templates/` | documents the above |

Everything real already lives under `~/.synth-desktop` — `whisper.rs`, `laguna.rs`,
`training_models.rs`, `trace_ingest.rs`, `instance.rs`. So consolidation is a label fix and a script
path, not a data migration. The one genuine defect is `managed_templates_root()` above — though note
`whisper.rs:376-384` hard-codes its home-relative path and ignores instance identity entirely, unlike
`training_models.rs`; folding it into `state_root()` is cheap while item 30 is open.

---

# Work items — parts IV and V

| # | Part | Item | Depends on |
| --- | --- | --- | --- |
| 15 | IV | `presentation.rs` → `presentation/` + provider enum; move trace bits to `presentation/trace.rs` | — |
| 16 | IV | Generalize `TraceInspectability` → `Presentability { Present, Unavailable(reason) }` | 15 |
| 17 | IV | Add a second provider (optimizer runs) — proves the abstraction | 15, 16 |
| 18 | IV | `schema_migrations(plugin_id, version)`; split core vs plugin migration lineages | — |
| 19 | IV | Enforce table-prefix namespacing per plugin | 18 |
| 20 | IV | Register projections by schema id (`provides` / `consumes` / `filter`) | 1 |
| 21 | IV | Let plugins contribute allowlisted columns to `trace_query.rs` | 19 |
| 22 | IV | Record producing plugin + projection schema in the seal | 3 |
| 22a | IV | Fold `LOCAL_LORA_DDL` / `HOSTED_LORA_OVERLAY_DDL` (`optimizers/local_lora.rs`) into the migration lineage, or delete them | 18 |
| 23 | V | **Fix `managed_templates_root()` to use `instance::state_root()`** — live bug | — |
| 24 | V | Accept `template.json` + `shell.tsx` (`source_kind: "user"`) in the registry scan; add symlink refusal + size cap to the scan tier | 23 |
| 25 | V | Merge runtime user templates into `listTemplates()` (new shell-source command; branch on `source_kind`, not template id); compile via `compileSourcedModule` | 24 |
| 26 | V | `visual_save_template`, `visual_create_template --from`, `visual_validate_template` | 24 |
| 27 | V | Reload on save-event + rescan-on-focus (fs-watch only if needed — `notify` is a new dep); in-pane errors via `sourcedInvalidShell` | 25 |
| 28 | V | Embed template source in the seal for user templates | 24 |
| 29 | V | New `ApprovalKind` variant through the existing `ApprovalBroker` (model: `PluginLifecycle`) for agent-written persistent templates | 26 |
| 30 | V | Retire `stage-internal-visuals.sh`; fix the three `~/.synth` label sites; fold `whisper.rs` path into `state_root()` | 24 |

Item 23 is a one-line fix that unblocks the rest and repairs a currently dead code path.

---

# Part VI — Boundaries, and enforcing them in SynthStyle

## Which boundaries are actually defended

| Boundary | Stated where | Enforced how | Status |
| --- | --- | --- | --- |
| Renderer ↔ Host | style guide §2, §8 | 4 conform checks + typecheck | **Real** |
| Host ↔ Runtime sidecar | `optimizers/CONTRACT.md` §1 | capability derivation, anti-swap pins | **Real**, optimizers only |
| Host ↔ Agent | `trace_query.rs`, capability stripping | column allowlist, unknown = error | **Real** |
| Log ↔ Projection | — | — | **Absent** — the fold exists 3× |
| Core ↔ Plugin (in-process) | — | — | **Absent** — 90 tables, one counter |
| Build-time ↔ Run-time | — | — | **Absent** — glob, staging symlinks |
| Bundled ↔ User-authored | registry tier order | tier order, no-shadow | **Half** — root broken, TSX unsupported |

Every defect in this document sits on one of the four undefended rows. The three defended boundaries
did not drift. That is the argument for the rest of this part.

## The enforcement mechanism already exists

`scripts/conform-desktop.sh` is a **ratchet by convention, not by mechanism**: it counts
anti-patterns via ripgrep (`count_rg` sums per-file `rg -c` output), prints one heredoc line per
check, and **always exits 0** — there are no stored baselines and no failure mode; "may only
decrease" is enforced by humans reading the CI output. Item 31 is therefore six `count_rg`
assignments plus six heredoc lines in the existing idiom; converting the ratchet to an asserted
baseline is a separate, larger change worth its own decision. Run by `./scripts/desktop.sh conform`
in CI on every PR (`.github/workflows/desktop-conform.yml`). Ten checks today:

`map_err_to_string` · `to_string_contains` · `status_magic` · `target_json_kind` · `static_once_lock` ·
`client_new` · `window_synth` · `is_tauri_ternary` · `use_state_app` · `invoke_string`

Read together they enforce one boundary — **the renderer may not own truth, and errors may not
collapse into strings.** That boundary is well defended. The others have no ratchet, which is why
they drifted.

## Proposed conform checks

Same idiom: start each at its current count; it may only decrease. All six were dry-run against the
tree — the patterns and counts below are real, not estimates.

| Check | Pattern (run from `apps/synth_desktop`) | Count | Verdict |
| --- | --- | --- | --- |
| a1. `.synth-desktop` literals | `'\.synth-desktop'` in `src-tauri/src src/renderer/src`, excluding `instance_paths.rs` / `instance.rs` / tests | 22 | Land — duplicated path truth (laguna, whisper, UI label strings) |
| a2. raw data-root env reads | `'env::var\("SYNTH_DESKTOP_DATA_ROOT"'` excluding `instance_paths.rs` | **1 — the single match IS the item-23 bug line** (`visuals/templates.rs:206`) | Land — → 0 after item 23 |
| b. fold copies | token count (`lastSequenceByScope\|receivedSequencesByScope\|extractProjection`) outside `liveStream.ts` | 3 (today's copies) | Land **only as a consolidation progress meter** — the three folds share no common token, so no grep catches a fourth written with fresh names. Real enforcement is the golden-fixture equivalence suite (Part I): same envelope stream through every fold, assert identical projection. Precedent for a source-scanning lock test: `instance.rs:1371-1389` |
| c. `CREATE TABLE` outside migrations | `-i 'CREATE TABLE'` in `src-tauri/src`, excluding `storage/migrations.rs` + its tests | 4 — **2 are live production DDL consts in `optimizers/local_lora.rs`**, found by the dry run | Land |
| d. `import.meta.glob` | scan `../../visuals src src-tauri/src` | 7 (6 in `visuals/registry/index.ts`, 1 in `live.craftax.v1/shell.tsx`) | Land — cleanest of the six |
| e. template-root re-derivation | `'join\("families"\)\|templates-internal'` outside `visuals/templates.rs` | 0 | Land as a stay-at-0 guard (the naive `visuals_root()` pattern punishes legitimate callers of the canonical helper — don't use it) |
| f. hardcoded template ids | `'"optimizer\.(gepa\|sft\|eval\|dag\|run)\.(live\.)?v[0-9]+"'` outside `contract/runtimes.rs`, **never scanning `visuals/`** — a template's own manifest and shell legitimately carry its id | 17 (service.rs 12, eval_recipes.rs 2, manager.rs 1, workspace_recipe.rs 1, VisualHost.tsx 1) | Land with exactly this scope |

**Correction to the original claim:** the `.synth-desktop` ratchet (a1) would **not** have caught the
`managed_templates_root()` bug — that function contains no such literal; its defect is a raw env
read plus a local path re-derivation. The check that catches the bug class is a2, whose entire
current count of 1 is the bug itself. Land a1 and a2 as separate lines and credit a2 as the item-23
guard.

## Style guide additions

**§2 "Source of truth"** (the guide is `WORKSHOP_QUALITY_STYLE_GUIDE.md`; `workshop_style.md` is the
short triage companion) — extend the canonical-source table with the missing rows: the fold, path
resolution, template roots, migration lineage, panel presentation. And generalize the closing line,
which currently names three instances of one rule ("Do not create a second token system, a
component-local preference store, or a renderer-only source of runtime truth"). The rule is: **do not
create a second source of truth for anything**; those are examples.

**§8 "Runtime and data boundaries"** — four additions in the existing voice:

- One fold per event stream. A projection has exactly one implementation.
- One path helper per root. A local re-derivation is a defect regardless of whether it is correct.
- A seal stores outputs, never a reference to code that may not exist later.
- Plugins declare vocabulary; they never supply trust primitives.

## The data-boundary idiom to reuse

Wherever a boundary is real, the same pattern appears: **enumerate what is allowed, error on unknown,
never pass through.** `trace_query.rs` allowlisted columns; `sourcedValidate.ts` allowlisted imports
plus a forbidden-token scan; `container_capabilities.rs` tri-state with claims stripped from
agent-supplied metadata; `validate_managed_renderer` networkless scan; `live_eval.rs` declared stream
sources only.

Apply it to the undefended rows: a namespace allowlist for plugin tables, a schema-id registry for
projections, a tier allowlist for template roots, one helper for path resolution.

## Observation

The best architectural rules in this repo live in **module doc-comments** — `instance_paths.rs` on
copies of a rule, `presentation.rs` on the second copy that would have drifted, `CONTRACT.md` §1 on
ownership, `chart_data.rs` on where resolution stops. They are among the clearest engineering prose
here, and each was written after something broke. They are also invisible to anyone who has not opened
that file, and unenforced.

Promoting the recurring ones into §8 with a ratchet line each converts hard-won lessons from folklore
into structure. That matters more than usual now: the next readers of this code are people who were
not here when it broke.

---

# Work items — part VI

| # | Item | Depends on |
| --- | --- | --- |
| 31 | Add the six conform checks to `conform-desktop.sh` at current baselines | — |
| 32 | Extend style guide §2 table with fold / paths / template roots / migrations / presentation | — |
| 33 | Add the four §8 boundary rules | — |
| 34 | Generalize the §2 "no second source of truth" line | — |

Items 31–34 are small, self-contained, and best landed **first**, so items 1–30 arrive against
enforced boundaries rather than intended ones.

---

# Part VII — Right panel: Codex-grade document viewing

Requirement added 2026-08-28: the right panel should be as seamless as the Codex desktop right panel
— tabs, a clickable breadcrumb path, markdown typeset by default with a View-source toggle, code
blocks with language badge and copy, an "Open ▾" in-external dropdown, and any file the agent
produces or references openable there instantly.

## Where the panel actually is

The right panel is **`VisualPane`** (`components/VisualHost.tsx:1450-1818`), mounted by the
workbench route grid (`routes.tsx:584-595`) — not `DataPage`, which is the full-page Data catalog.
Pane state is a single `openArtifact: ArtifactRef | null` (`routes.tsx:140`). The agent→panel rail
already exists and is instant — MCP `visual_show` → durable `visual.show` event → listener opens the
pane (`useAppController.ts:1500-1533`) — but only for **visual records**. There is no file viewer and
no markdown renderer anywhere in the app (reports render their markdown as raw text,
`ReportsPage.tsx:104-105`), no tabs, no breadcrumbs. Expand/collapse exists
(`VisualHost.tsx:1708-1718`); the opener plugin is installed and permitted, used today only from chat
file chips (`ChatTranscript.tsx:695`).

## Gap table

| Reference behavior | Status | Seam |
| --- | --- | --- |
| Tab strip, per-tab close, "+" | missing | lift `openArtifact` to an ordered `openArtifacts[]` in `useAppController`/`useShellLayout`; strip renders in the `VisualPane` header |
| Clickable breadcrumbs | missing | needs path-backed content plus a workspace-scoped dir-list command |
| Markdown typeset + View source | missing | one md dependency + a `document.markdown.v1` branch in the `VisualHost` dispatch (:1379) |
| Code blocks: language badge, copy | partial | copy exists for chat (`CopyMessageButton`); same seam as markdown |
| "Open ▾" external | partial | plugin + capability already granted; add the dropdown once pane content carries a path |
| Expand / collapse | **exists** | cosmetic alignment only |
| Agent doc → panel instantly | partial | the full rail exists for visuals; missing only the file/document provider |

## The design constraint that makes this Part IV's second provider

The renderer has no fs access, by design: every byte crosses a typed command gated by a grant
derived from declared bindings (`visual_poll_stream`'s declared-URL rule; `visual_read_media`'s
bound-run + type-allowlist + size-cap + digest-reverify rule), and workspace file access is mediated
by `workspace_scope.rs` session roots with read-only/read-write attachment modes. **Do not add
`tauri-plugin-fs`.** A document viewer is: `workspace_read_file` / `workspace_list_dir` commands
resolving through `workspace_scope::session_roots` (size-capped, media-typed, mirroring
`visual_read_media`), a `workspace_file` binding kind so a pane reads only paths its visual
declares, and `ensure_document_viewer(...)` in `presentation/` — identity from canonical path +
content digest, eligibility = inside workspace scope, named-reason unavailability. That is the
second/third `PanelProvider`, which is exactly what item 17 wanted: a provider that is not
trace-shaped, forcing the abstraction honestly — with a user-visible payoff instead of an abstract
proof.

## Work items — part VII

| # | Item | Depends on |
| --- | --- | --- |
| 35 | `workspace_read_file` + `workspace_list_dir` commands via `workspace_scope` (no fs plugin) | — |
| 36 | `ensure_document_viewer` provider in `presentation/` (identity, eligibility, `workspace_file` binding, show) | 15, 16, 35 |
| 37 | Markdown/code renderer branch in the `VisualHost` dispatch — typeset default, View-source toggle, language badge + copy | 35 |
| 38 | Pane chrome: breadcrumbs (segments → `workspace_list_dir`) and "Open ▾" (existing `openPath` / `revealItemInDir`) | 36, 37 |
| 39 | Tabs: `openArtifact` → ordered tab list, per-tab close, "+", persisted in layout prefs | 37 |
| 40 | MCP `document_show(path)` beside `visual_show`; chat file chips open in-panel, external-open demoted to the dropdown | 36 |
| 41 | Reports polish: render `report.prose.v1` blocks with the item-37 renderer; offer reports as pane tabs | 37 |

---

# Decisions — resolved by the verification pass

| Decision | Resolution |
| --- | --- |
| Item 2 — cutoff shape for multiplexed runs | **Per-stream cursor vector** (prefix counts into deduped log order). Scalar is a no-op and per-scope numeric vectors are unaddressable on the real Craftax fixture (string sequences). Part I, cost 4. |
| Item 14 — emitter change across three placements | **Defer the upstream emitters** (three external release trains; Tinker and Modal lanes are dark today). Land the Desktop-side adapter forwarding + projection plumbing now — atomic, and it unblocks the CISPO panels on live MLX runs. |
| Phase 7 acceptance runs | **One real MLX run** + golden fixtures for the dark lanes; budget the Tinker/Modal runs when those lanes open. |
| User templates: shared vs per-instance | **Per-instance** (`state_root()`), reversing the earlier lean — see Part V for the three reasons (literal drift, cross-instance planting, shared-registry poisoning). |
| SYN-3243 — community-build telemetry | Resolved in the issue itself (rescoped 2026-08-28): a community build **records nothing and transmits nothing** — neither Optional nor Essential — with the (not-yet-built) upload path gated on the SYN-3242 provenance flag so it can never transmit regardless of user setting. Only open sub-question: whether `install_id` is generated at all in community builds. |

---

# The plan

## Five contours

1. **Every invariant gets exactly one home.** The fold ×3, dedupe ×2, `DataPage`'s copy of
   presentation, nine adapter path fallbacks, three lanes numbering migration 23, a local copy of a
   path rule that silently killed a registry — every defect here is the same defect. Make single-home
   structural rather than remembered.
2. **Core owns trust; plugins own vocabulary.** The boundary is about what can be *attested*, not
   about code layout. If a plugin can supply its own receipt, the receipt attests nothing.
3. **Verification becomes an artifact, not an afternoon.** Extend the evidence discipline already
   applied to data — `uploaded: false`, `"claim": "NOT A2"`, derived capabilities — inward, to the
   visuals themselves. Harbor is the proof of what the manual cost buys you: nothing, for weeks.
4. **Move the boundary from build time to run time.** The glob, the staging symlink, compiled-in
   templates. Mostly deletion.
5. **Reproducibility must survive removal.** Seal outputs, not references. One change answers plugin
   uninstall, user templates, and the frozen runtime's third fold.

## Phases

| Phase | Items | Why here |
| --- | --- | --- |
| **0 — Enforce** | 31–34 | Small; makes everything after it land against real boundaries. The script never fails CI — the six lines are counters; an asserted baseline is a separate decision |
| **1 — Unblock** | 23 | One line; repairs a registry broken **in production installs** (not dev-only), plus stray-directory cleanup |
| **2 — Evidence** | 5–8 | Converts acceptance from manual to mechanical. Fix the two phantom-gap hazards before gating; add `live.harbor_eval.v1`'s missing `observationContract` immediately |
| **3 — Harbor** | *(no code)* | One real run; self-evidencing after phase 2 |
| **4 — Consolidate** | 1–4 | The Rust fold; also replaces the spool's divergent identity rule and revives the dead live-eval seal path |
| **5 — User templates** | 24–30 | Rides the fixed root and the sourced compiler |
| **6 — Panel** | 15–17, 35–41 | Providers + the Codex-grade document viewer; the document provider is the concrete second provider that proves the abstraction |
| **7 — CISPO** | 9–14 | Inherits fold, receipt, filmstrip, gate. Acceptance: one real MLX run + fixtures for the dark lanes |
| **later** | 18–22 | Plugin schema boundaries — only once a real second plugin exists |

## What to resist

- **Schema namespacing and per-plugin migrations before a second plugin exists.** Most speculative
  item on the list; easiest to over-build.
- **Traits at N=2 providers.** Enum and match; lift when the third lands.
- **The CISPO RL dashboard before the emitter change.** KL and clip-fraction panels would render em
  dashes forever, and the house rule makes that correct-but-useless.
- **Letting the Rust consolidation eat agent expressiveness.** Keep raw events beside the projection.
  Expressiveness is already the weak axis; do not spend more of it on tidiness.
- **`tauri-plugin-fs` for the document viewer.** The capability file is minimal on purpose; file
  access goes through `workspace_scope` session roots like every other byte the renderer sees.
- **The groups template before the adapter fix.** `optimizer.cispo.groups.v1` on today's live event
  flow would render em dashes forever — the same correct-but-useless trap as the KL panels.

## Honest summary

Almost none of this is new capability. It is consolidation, one dead-path fix, and extending an
evidence discipline already practiced into the places it has not reached. That is also why it is good
pre-OSS work: it is precisely what makes the repo legible to someone who did not build it.

---

# Work by repository

Verified 2026-08-28 against sibling checkouts in `~/GitHub`. Workshop was read deeply this session;
the other five were inspected via git log, directory structure, and targeted greps. The CISPO emitter
finding and the route ownership below were confirmed directly.

| Repo | Share of plan | Blocks others? | Last commit |
| --- | --- | --- | --- |
| **workshop** | ~85% — Parts I–VII | no | 2026-08-28 |
| **synth-mlx-rl** | the CISPO emitter gap | **yes — blocks item 9** | 2026-08-23 |
| **optimizers** | producer-side contract + CISPO passthrough | yes — mid-chain | 2026-08-21 |
| **containers** | Harbor acceptance + stream producer | **yes — blocks phase 3** | 2026-08-27 |
| **backend** | small; mostly decisions | no | 2026-08-23 |
| **optimizers-beta** | **no code — needs a disposition decision** | no | 2026-08-13 |

## workshop

Everything in this document except the emitter work: phase 0 (items 31–34), item 23, the receipt and
gate (5–8), the Rust fold (1–4), user templates (24–30), panel providers and the document layer
(15–17, 35–41), and the four-field whitelist widening in the two symmetric adapter arms. Plus the OSS
launch project (SYN-3231–3247), tracked separately.

## synth-mlx-rl — the real blocker for CISPO

**26 CISPO files**; this is where the trainer lives. The decisive finding:

> `advantage_mean`, `reward_variance`, and `advantage_std` appear in **zero files across
> synth-mlx-rl, optimizers, and optimizers-beta.**

Nothing anywhere in the stack computes them. This is stronger than "the wheel may not send them" —
the fields do not exist. Widening Workshop's whitelist alone would surface nulls.

**Work:** compute and emit `group_size`, `reward_variance`, `advantage_mean`, `advantage_std`, and
`optimizer_step` in the CISPO metric payload. This is also the natural home for item 14 (clip
fraction, IS-ratio distribution, KL) if those are judged worth the cost.

**Order matters:** the chain is synth-mlx-rl → optimizers → workshop. Starting at the Workshop end
produces em dashes.

## optimizers

Owns the contract routes — `GET /runs/{run_id}/optimizer-events` with `after_sequence` is served from
`rust/crates/synth_gepa/src/service.rs:1494`. CISPO orchestration lives in four Python files
(`hosted.py`, `training.py`, `__init__.py`, and tests).

1. **Pass the wider metric payload through** to the events route once synth-mlx-rl emits it.
2. **Emit `page` + `cursor` consistently.** Workshop's `parseReplayPage` normalizes three producer
   shapes today, with a COMPAT note that the bare-array arm can be retired "once every producer emits
   `page`+`cursor`." That retirement is a producer-side change, and it is what makes the receipt's gap
   detection trustworthy rather than best-effort.

## containers

Most active sibling repo, and it owns Harbor.

1. **Harbor acceptance (phase 3).** One real run through `container_prepare_rollout` → bind →
   `container_start_prepared_rollout` → sealed trace, replacing Workshop's 1.6 KB fixture.
   **No Workshop code is required** — the classifier, family-match assertion, `live_frames` refusal,
   and C5-02 policy pins all exist and are enforced. `tests/test_harbor_docker.py` is explicit that it
   proved the Docker fold headless and *not* in Desktop; that is the gap.
2. **Producer-side stream guarantees** for the receipt: the `stream.subscribed` control envelope,
   monotonic gap-free sequences per scope, and `page`+`cursor` as above.
3. Continue advertising `live_frames` as non-native for Harbor — Workshop refuses `native` rather than
   inventing a Craftax view.

## backend

Smallest surface; mostly decisions rather than code.

- **`usesynth.ai/releases/<channel>/latest.json`** — the manifest `update_check.rs` polls. Its
  behavior determines what community builds do (SYN-3245).
- **Artifact upload** (`synth.workshop-artifact-upload.v1`) — seals that embed projections (item 3)
  and user-template source (item 28) grow the payload. Check size limits before those land.
- **Telemetry sink** — deliberately absent. SYN-3243 resolves that community builds record and
  transmit nothing; the open question is whether an official-build sink is ever built.
- Intern endpoints: unchanged.

## optimizers-beta — a decision, not a change

Zero CISPO files, last commit **2026-08-13** (eight days behind `optimizers`), carrying GELO and a
"wip: preserve hosted optimizer and MAPO integration" state. Nothing in this plan touches it.

It is also an OSS liability — another repo to audit, or another absence to explain. Resolve its
disposition (merge what matters into `optimizers`, or archive) **before** the launch rather than
during it.

## Cross-cutting

The inter-repo contract is the schema id. Any change to `synth.trace-stream-event.v1`, the optimizer
event envelope, or the metric payload shape is a coordinated version bump across repos, not a local
edit. That is the strongest practical argument for the schema-id projection registry in Part IV.

## Caveat on this document

Traced from the Rust sources, the contract tables, the visuals package, and the template manifests.
The original caveat — that the GEPA and SFT shells were never opened — is closed: the 2026-08-28
verification pass opened every optimizer shell. All are thin (60–81 line) wrappers over
`_shared/optimizer.run.v1`'s `OptimizerFamilyShell` + `projectEvents.ts` (2,924 lines, the shared
projection); the panels live in `overlays/`. The material discovery from that audit — CISPO already
renders through `optimizer.sft.live.v1`'s CISPO mode, fed today by fixtures rather than live runs —
is folded into Part III above.

