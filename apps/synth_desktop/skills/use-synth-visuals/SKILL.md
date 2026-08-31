---
name: use-synth-visuals
description: Use when creating, updating, inspecting, or opening a Synth Desktop visual from task, rollout, trace, or eval evidence.
---

# Use Synth Visuals

Choose the visual grammar from the evidence. Treat registered templates as optional shortcuts, not mandates. For ad-hoc quantitative analysis, use `visual_manage` with operation `chart`; it creates or revises `analysis.chart.v1`, renders the chart, and returns a review PNG in one call. For other ad-hoc analysis, use `analysis.visual.v1` and author its ordered `spec.blocks` at creation time. For a live event log plus inspect overlay, prefer `compose.visual.v1` with advertised components; do not hang a `stream` input on `analysis.visual.v1`. For a custom pane that still uses those components and host-owned ingest, author TSX on `sourced.visual.v1`. Use `blank.canvas.v1` when none of those grammars express the composition cleanly. If the artifact is a system, UML, flow picture, or time-aware technical explainer, load `author-synth-diagrams`. It chooses among `diagram.mermaid.v1`, `diagram.systems.v1`, `diagram.systems.dynamic.v1`, or a focused combination; do not dump SVG/HTML/JavaScript into a canvas.

Optimizer visuals are a strict exception to the authoring workflow below. The `optimizer.*` family is product-owned and already configured by `use-synth-optimizers`. Report `visualEvidence.state` (`ready` | `reviewed` | `partial` | `failed`); never loop capture/repair. `partial` and `failed` never block task completion. Only call `show` when that workflow asks you to recover a missing subscription receipt. Never call `authoring_context`, `capture_review`, `review`, `update`, or `mark_ready` for an optimizer-owned visual.

## Intended approach

A visual is a concise explanation of evidence, not a graphical evidence dump.

1. **Ground:** establish the exact facts, their provenance, and what is missing. Never use design to imply unavailable evidence.
2. **Claim:** write the one sentence the visual should make obvious. If there are several independent claims, use beats, details, or separate focused visuals.
3. **Reduce:** choose the smallest grammar and fewest marks that communicate that claim. Keep exact records available as detail instead of placing every identifier and event on the primary surface.
4. **Compose:** create hierarchy before decoration—primary result, supporting mechanism, then provenance/caveats. Prefer direct labels, whitespace, and progressive disclosure.
5. **Render:** show the artifact in its real Desktop pane. Source validity is not visual validity.
6. **Critique:** capture, open, and inspect wide and compact screenshots. Revise the same visual ID until the claim is legible without collisions, ambiguous truncation, or excessive density.
7. **Certify:** record screenshot-backed reviews and mark ready only after automated findings and human-visible problems are resolved.

The first draft is expected to be revised. Do not describe a visual as polished merely because it rendered or passed schema validation.

Codex advertises one compact custom tool,
`mcp__synth_visuals__visual_manage`, instead of all visual schemas on every
turn. In code mode call it as
`tools.mcp__synth_visuals__visual_manage({ operation, arguments })`. There is
**no** top-level `method` field. Do not use `resources/list`, `resources/read`,
or filesystem search to discover this tool.

| Operation | Arguments |
| --- | --- |
| `list_templates` | `{ "genre"?: string }` — returns `inputs` (and a `slots` copy), `bindingSchema`, and `components[]` (empty when the template does not advertise parts). There is no `list_components` verb. |
| `list` | `{ "search"?: string, "status"?: string, "session_id"?: string, "scope"?: "session" \| "instance" }` — defaults to this task; `scope: "instance"` is labeled cross-task discovery |
| `get` | `{ "visual_id": string }` |
| `create` | `{ "template_id": string, "title"?: string, "display_name"?: string, "content"?: string, "props"?: object, "session_id"?: string, "instance_id"?: string }` — `display_name` is the short human-readable Outputs label; `sourced.visual.v1` requires `content` (allowlisted TSX). |
| `create_with_bind` | `{ "template_id": string, "title"?: string, "input": string, "kind": string, "data"?: object, "source"?: string, "schema"?: string }` — atomic create plus the first required input. Prefer this for `experiment.overview.v1`, `analysis.visual.v1`, and `compose.visual.v1` (`spec`). Bind `stream` (eval) or `optimizer_run` (GEPA/SFT/CISPO) separately. `slot` still binds on stored envelopes; new writers emit `input`. If both names are present and disagree, fail closed. |
| `chart` | `{ "visual_id"?: string, "title"?: string, "display_name"?: string, "spec": object, "bindings"?: object, "input"?: string, "kind"?: string, "source"?: string, "data"?: object, "viewport"?: {"width": number}, "capture"?: boolean, "presentation"?: "canvas" \| "pane" }` — create or revise an ad-hoc `analysis.chart.v1`. Bind optimizer runs or traces in the same call for provenance. The default capture returns the rendered PNG for inspection. |
| `update` | `{ "visual_id": string, "title"?: string, "display_name"?: string, "content"?: string, "bindings"?: object, "status"?: string }` — `bindings` must be the canonical envelope; prefer `bind` |
| `bind` | `{ "instance_id": string, "input": string, "kind": string, "source"?: string, "data"?: object, "poll_url"?: string, "path"?: string, "schema"?: string, "mode"?: "replace" \| "append", "bindings"?: [{ "kind": string, "source"?: string, "data"?: object, "poll_url"?: string }] }` — inline inputs require `data`; other kinds require `source`. `slot` still binds on stored envelopes; new writers emit `input`. Two malformed binds must not block a corrected bind. |
| `show` | `{ "visual_id": string, "session_id"?: string }` |
| `fork` | `{ "visual_id": string, "title"?: string }` |
| `archive` | `{ "visual_id": string }` |
| `experiment_create` | `{ "request_id": string, "title": string, "task"?: string, "model"?: string }` — creates or reopens the current task's durable experiment record; `request_id` is the idempotency key. |
| `experiment_attach_evidence` | `{ "experiment_id": string, "evidence_id": string, "kind": "trace" \| "visual" \| "artifact" \| "container", "label": string, ... }` — attaches a durable evidence reference; a container evidence item requires `container_id`. |
| `experiment_finalize` | `{ "experiment_id": string, "status": "completed" \| "partial" \| "failed", "result": object, "assessment"?: object }` — record measured results honestly; use `null` for missing values. |
| `authoring_context` | `{ "visual_id": string }` |
| `capture_review` | `{ "visual_id": string, "viewport": {"width": number, "height": number} }` — returns an attached PNG plus `screenshot_path` |
| `review` | `{ "visual_id": string, "revision": number, "viewport": {"width": number, "height": number}, "checks": object, "findings": string[], "screenshot_path"?: string }` (`screenshot_path` is required for systems visuals) |
| `mark_ready` | `{ "visual_id": string, "revision": number }` |

For example, list templates with
`{"operation":"list_templates","arguments":{}}`.
Do not invent separate callable names such as
`mcp__synth_visuals__visual_create`; legacy MCP names remain compatible for
other clients but are intentionally not advertised to Codex.
Use the same `visual_manage` facade for experiment lifecycle operations; do
not call a separate `experiment_create` tool.

## Workflow

1. Inspect the available evidence before choosing a chart: task metadata, rollout count, seeds, traces, reward components, achievements, costs, tokens, latency, and failure state.
2. State the analytical question in one sentence: “Which arm achieves more per dollar?”, “Where do rewards diverge?”, or “What happened during this rollout?”
3. Choose only visual forms that answer that question. Read [visual-recipes.md](references/visual-recipes.md) for mappings. For a one-off quantitative comparison, also read [ad-hoc-visuals.md](references/ad-hoc-visuals.md). It is the canonical guide for chart selection, style, metric denominators, and evidence/event-source bindings.
4. Create the smallest useful visual with a stable ID, a descriptive title that names the task and comparison, and a short sensible `display_name` (normally 2–6 words, such as “GLM Craftax Results” or “Reward by Seed”). Keep names distinct within the task and never use raw IDs as names. Use operation `chart` for quantitative comparisons and `create`/`create_with_bind` for other registered templates. Use `presentation: "canvas"` for gameplay, trace workbenches, and dense live dashboards.
   If instance-scope discovery finds a useful visual owned by another task, call `fork` first and revise the returned current-task visual ID. Never update or `show` the other task's original: its presentation event routes to its owner, not this chat.
5. Show exact units and provenance. Preserve small costs rather than rounding them to `$0.00`.
6. Call the `show` operation after creation or update so the result opens in the Desktop pane.
7. Inspect the rendered visual in Desktop canvas mode. Fix clipped labels, empty sections, misleading encodings, weak hierarchy, and excessive whitespace.
8. Perform at least two explicit render-and-critique iterations at distinct viewport widths. Call `capture_review` for authored visual families—evals (`analysis.*`, `live.*`, `craftax.*`), UML/Mermaid, static 2D systems maps, and Benjamin Dicken Style dynamic systems visuals. Do **not** capture, review, or `mark_ready` `optimizer.*` product visuals; report `visualEvidence.state` instead and never loop capture/repair. For each viewport inspect the PNG attached to the tool result. Pass its returned `screenshot_path` to `review`; never shell-search for captures, invent a path, or submit checks without looking at the image. Do not reuse a review after the visual revision changes. For systems visuals, resolve every deterministic finding returned by `authoring_context` before readiness.
9. Call `mark_ready` only when all required landmarks pass. A trusted live template is configured through `visual_config`. `sourced.visual.v1` compiles allowlisted `content` TSX and mounts it as the pane Shell. `blank.canvas.v1` stays HTML/SVG with no scripts.

## Composition rules

- Lead with task identity and the question answered, not the template name.
- Establish context with a compact task card when environment state or objective affects interpretation.
- Put the most decision-relevant result above the fold.
- Prefer direct labels over legends. If a legend is necessary, keep arm names short and consistent.
- Use color for identity or signed meaning, never decoration. Keep one accent and one neutral comparison color by default.
- Include sample count (`n`), seeds, aggregation, and uncertainty near the result.
- Label single-rollout comparisons as exploratory. A 0% or 100% observation from one rollout is not a stable frequency estimate.
- Distinguish missing from zero and failed from scored.
- Keep raw trace detail available through a scrubber or table, but summarize the important transition first.

For a request to compare model settings, policies, candidates, seeds, scores,
cost, tokens, latency, or efficiency, **default to operation `chart` even when
some requested evidence is missing**. Represent unavailable numeric values as
`null` and explain the gap in a note panel. Do not fall back to
`analysis.visual.v1` merely because one arm is unavailable. Use the legacy
ordered-block grammar only when the requested result is primarily a narrative
record rather than a quantitative chart.

## Never do this

- Do not draw a line or smoothed curve across unordered model arms.
- Do not invent a Pareto frontier, trend, confidence interval, or distribution from insufficient observations.
- Do not use a heatmap for one achievement or one arm.
- Do not make an area proportional to a value unless area is the intended encoding.
- Do not truncate labels into ambiguity or overlay labels on marks.
- Do not report cost as `$0.00` when a nonzero micro-cost is known.
- Do not treat reward, achievement count, and pass rate as interchangeable.

## `analysis.visual.v1`

Required input: **`spec`**. Author it as `kind: "inline"` with `data` containing a short narrative and ordered `blocks`. Do not bind this template on input `experiment`. `list_templates` returns `inputs` (and a `slots` copy), `bindingSchema`, and `components[]` (empty when the template does not advertise parts); `example_binding` is the canonical create+bind payload.

Available blocks:

- `note`: context, caveat, or conclusion.
- `metrics`: exact headline values with optional details.
- `ranked-bars`: ordered magnitudes with a meaningful zero baseline.
- `frequency-diff`: two-arm per-achievement frequencies and percentage-point deltas.
- `table`: exact multi-field comparison or provenance.
- `scatter`: independent observations on two quantitative axes. Never add connecting lines.

Create with bind:

```js
await tools.mcp__synth_visuals__visual_manage({
  operation: "create_with_bind",
  arguments: {
    template_id: "analysis.visual.v1",
    title: "HealthBench smoke · policy vs scorer",
    input: "spec",
    kind: "inline",
    data: {
      schemaVersion: "synth.visual.analysis_spec.v1",
      title: "HealthBench smoke · policy vs scorer",
      blocks: [{ type: "metrics", items: [{ label: "Train mean", value: null }] }]
    }
  }
});
```

## `compose.visual.v1`

Required input: **`spec`** (`synth.visual.compose_spec.v1`). Optional inputs: **`stream`** (`live_sse` / `fixture` / `inline`) for container eval envelopes, and **`optimizer_run`** (`optimizer_run` / `fixture` / `inline`) for `optimizer_event.v1` (GEPA, SFT, CISPO). Bind the dialect a placement consumes. Do not mash eval traces into `optimizer_run`. Do not hang a live input on `analysis.visual.v1`. `live.eval_stream.v1` remains a whole-pane shortcut. Product `optimizer.gepa.live.v1` / `optimizer.sft.live.v1` / `optimizer.eval.live.v1` stay product-owned. Hosted RLVR is **CISPO** (`algorithmId: cispo`, `cispo.*` events) — do not invent `rlvr.*`. Unknown `component` ids fail closed. `list_templates` echoes this template's `components[]` (id, kind, protocolId, consumes, emits) next to `inputs`. There is no `list_components` verb.

Advertised components (kind is the render contract; `protocolId` is the bind dialect):

- `event_stream.v1` — consumes `stream` or `optimizer_run` (placement `input` selects one), emits cursor. Optional `config.includeKinds` matches envelope `kind` or `type`.
- `detail_modal.v1` — consumes cursor via `from` (must name an `event_stream.v1` placement). In-pane overlay, not a second visual.

Create the spec, then bind the declared dialect. Guessed `/events` URLs still fail closed.

```js
const created = await tools.mcp__synth_visuals__visual_manage({
  operation: "create_with_bind",
  arguments: {
    template_id: "compose.visual.v1",
    title: "Harbor smoke · live stream",
    input: "spec",
    kind: "inline",
    data: {
      schemaVersion: "synth.visual.compose_spec.v1",
      title: "Harbor smoke · live stream",
      placements: [
        { id: "log", component: "event_stream.v1", input: "stream" },
        { id: "inspect", component: "detail_modal.v1", from: "log" }
      ]
    }
  }
});
await tools.mcp__synth_visuals__visual_manage({
  operation: "bind",
  arguments: {
    instance_id: created.id,
    input: "stream",
    kind: "live_sse",
    source: declaredSseUrl,
    poll_url: declaredPollUrl
  }
});
await tools.mcp__synth_visuals__visual_manage({
  operation: "show",
  arguments: { visual_id: created.id }
});
```

Optimizer event log (same kit; not product `optimizer.*` chrome):

```js
const created = await tools.mcp__synth_visuals__visual_manage({
  operation: "create_with_bind",
  arguments: {
    template_id: "compose.visual.v1",
    title: "CISPO clip · optimizer_run",
    input: "spec",
    kind: "inline",
    data: {
      schemaVersion: "synth.visual.compose_spec.v1",
      title: "CISPO clip · optimizer_run",
      placements: [
        {
          id: "log",
          component: "event_stream.v1",
          input: "optimizer_run",
          config: { includeKinds: ["candidate.accepted", "sft.training.metrics", "cispo.clip.identity"] }
        },
        { id: "inspect", component: "detail_modal.v1", from: "log" }
      ]
    }
  }
});
await tools.mcp__synth_visuals__visual_manage({
  operation: "bind",
  arguments: {
    instance_id: created.id,
    input: "optimizer_run",
    kind: "optimizer_run",
    source: optimizerRunId
  }
});
await tools.mcp__synth_visuals__visual_manage({
  operation: "show",
  arguments: { visual_id: created.id }
});
```

## `sourced.visual.v1`

Kind `sourced_visual`. Protocol `whole_file.v1`. The agent authors a pane; Desktop **runs it**. Register-then-show: pass the module as `content`, bind `stream` if the module consumes host replay, then `show`. Do not recompile per seed.

Allowlisted imports only:

- `react` / `react-dom` / `react/jsx-runtime`
- `@synth/visuals/chrome`
- `@synth/visuals/chrome/useLiveEvalStream` — consumes host `ReplayClient`; does not discover URLs
- `@synth/visuals/components/event_stream.v1`
- `@synth/visuals/components/detail_modal.v1`

Unknown import, `fetch`, `EventSource`, `eval`, or a guessed `/events` URL fails closed in the pane. Host still builds `ReplayClient` and passes `replay`, `events`, `state`. Layout the advertised parts; do not own ingest. `blank.canvas.v1` is not this path.

```js
const created = await tools.mcp__synth_visuals__visual_manage({
  operation: "create",
  arguments: {
    template_id: "sourced.visual.v1",
    title: "Harbor smoke · custom log",
    content: sourcedTsx
  }
});
await tools.mcp__synth_visuals__visual_manage({
  operation: "bind",
  arguments: {
    instance_id: created.id,
    input: "stream",
    kind: "live_sse",
    source: declaredSseUrl,
    poll_url: declaredPollUrl
  }
});
await tools.mcp__synth_visuals__visual_manage({
  operation: "show",
  arguments: { visual_id: created.id }
});
```

## `experiment.overview.v1`

Required input: **`experiment`**, not `spec`. Accepts `inline`, `fixture`, or `local_cas`. Inline binds **must include `data`**. A bind without `data` returns `visual_binding_invalid` / `inline visual binding requires data`; correct the same visual — do not abandon it after two malformed binds.

Use the experiment overview when several runs or optimizer candidates answer one
research question. It is the canonical right-pane summary for the experiment;
do not create one overview per seed or per candidate.

Create the visual when the experiment identity and question are known, then
update the **same visual id** as progress and evidence arrive. Mint a new visual
only for a distinct experiment identity or research question. The inline
`experiment` projection uses schema `synth.experiment.overview.v1`:

- `experimentId`, `title`, `question`, `hypothesis`, and `status` establish identity.
- `progress` may include `phase`, `completed`, `total`, `elapsed`, `eta`, `usage`, and `cost`.
- `metrics` contains exact decision values such as baseline, selected result, heldout, and lift.
- `arms` contains all compared variants; mark the baseline and selected candidate explicitly.
- `evidence` links the distributions, failures, traces, replays, curves, or other visuals that support the result.
- `lineage` is an ordered compact projection, not a substitute for a full trace or DAG.
- `limitations` records missing baselines, incomplete heldout evidence, failed runs, and other caveats.

The decision core stays small: identity plus `hypotheses` (each with `claim`,
`verdict`, `confidence`, and `why`). Add the following typed modules only when
the evidence exists and it helps the reader. Missing and empty modules do not
render:

- `results.metrics` and `results.rollouts` add exact result values and compact per-rollout rows. A rollout may include `seed`, `reward`, `steps`, `achievements`, `stopReason`, and `traceId`.
- `traces` adds durable trace references with concise summaries, reward/step context, `traceId`, or `visualId`. Prefer references over embedding transcripts.
- `task` records task/benchmark identity and version, objective, split, or harness revision.
- `runtime` records model, reasoning effort, limits, container/image identity, digest, and run timing.
- `artifacts` adds files or durable objects via `path`, `visualId`, `traceId`, or `containerId`; do not paste long paths into prose.
- `provenance` records repository, commit, dirty state, configuration digest, and other reproducibility facts.

`traces` and `artifacts` accept either a plain array or
`{ prominence: "detail" | "summary", items: [...] }`. `detail` is the default
and stays collapsed. Use `summary` sparingly when that module is central to the
claim; Workshop then opens that one module initially. Workshop owns layout and
reference-chip rendering—the agent supplies typed facts, short summaries, and
durable IDs. Never add empty placeholder modules merely to make the record look
complete.

Missing measurements must be omitted or `null`, never written as zero. Do not
mark an arm selected merely because it is latest, and do not describe an
experiment as improved without baseline and comparison evidence. Keep every
seed/rollout in the underlying eval visual; the experiment overview summarizes
the distribution and links to that evidence rather than flattening it.

```js
await tools.mcp__synth_visuals__visual_manage({
  operation: "create_with_bind",
  arguments: {
    template_id: "experiment.overview.v1",
    title: "Banking77 baseline eval",
    input: "experiment",
    kind: "inline",
    data: {
      schemaVersion: "synth.experiment.overview.v1",
      experimentId: "exp.banking77.baseline.v1",
      title: "Banking77 baseline eval",
      question: "What is scored accuracy on 10 labeled examples?",
      status: "running",
      progress: { phase: "scoring", completed: 0, total: 10 },
      limitations: ["Baseline-only. No candidate generation and no uplift claim."]
    }
  }
});
```

Use specialized rollout or trace templates only when their interaction is genuinely useful. Use `craftax.rollout_scrub.v1` for step-by-step environment inspection and `trace.rollout_inspector.v1` for event/tool/message filtering.

For a Trace V5 record, bind the canonical `synth.trace-projection.rollout-inspector.v1` projection and let the first-class inspector preserve the trace hierarchy. Start in **Focus** for agent messages, tool activity, failures, and evaluation evidence; switch to **Full** only when model-call and lifecycle provenance matter. Put verdicts and grader rationale in **Evidence**, and identity, digests, visibility, token usage, and lane coverage in **Metadata**. Search commands and outputs or jump to an exact sequence instead of flattening the run into a generic chart. Never reconstruct missing events, expose content above the projection's visibility ceiling, or imply that an incomplete lane has complete coverage.

## Writing bindings

Bindings are always the canonical envelope:

```json
{"schemaVersion": "synth.visual-bindings.v1",
 "inputs": [{"input": "stream", "kind": "live_sse", "source": "...", "poll_url": "..."}]}
```

Write `input` / `inputs`. `slot` / `slots` still bind on stored envelopes; new writers omit them. If both names are present and disagree, fail closed.

Use the `bind` operation, which writes that envelope for you. A slot-keyed
object such as `{"stream": [...]}` is a legacy shape: it is upgraded with a
warning today and will be refused. Do not hand-build binding objects through
`update` when `bind` can express what you need.

For an input the template declares `multiple` — such as ten rollout streams on one
`stream` input — bind them in one call:

```js
await tools.mcp__synth_visuals__visual_manage({
  operation: "bind",
  arguments: {
    instance_id: visualId,
    input: "stream",
    mode: "append",
    bindings: rolloutIds.map((id) => ({
      kind: "live_sse",
      source: `${base}/rollouts/${id}/stream`,
      poll_url: `${base}/rollouts/${id}/events`,
      schema: "synth.trace-stream-event.v1"
    }))
  }
});
```

`mode` defaults to `replace`, which drops existing bindings on that input. Use
`append` when adding to a `multiple` input across several calls.

## Live container evals

Use the task-family live template for an eval that is still running:
`live.craftax.v1` or `live.harbor_eval.v1`. Bind its required
`stream` input as `live_sse` with the absolute SSE endpoint and exact poll
endpoint declared by rollout preparation. Every live binding needs `poll_url`:
the durable poll authority is what lets a completed run replay, and a stream
bound without one cannot be reopened after it closes. The stream emits
`synth.trace-stream-event.v1`; never guess or rewrite either route. Discover the
container with `container_list` / `container_probe` and inspect advertised
transports before binding.

Prepare in this order:

1. Discover the provider and inspect capabilities. Do not construct `/events`.
2. Call `container_prepare_rollout`; it must return the exact descriptor and `visual_binding` without starting execution.
3. Create the task-family visual with `presentation: "canvas"`, bind the returned `stream` input, and call `show`. Review at least twice, then `mark_ready`.
4. Get `authoring_context`. Use a prior real trace or the template's example only to develop layout; label example evidence and replace it with the declared stream before readiness.
5. Wait until the control envelope reports `stream.subscribed` with `ready: true`. HTTP 200 and heartbeats are not ready.
6. Call `container_start_prepared_rollout` with the exact prepared stream descriptor, `visual_id`, `task_instance_id` or `seed`, and `policy_ref` (`harness` + `config`). Desktop refuses a missing pin, a stale visual receipt, and still waits for control-only `stream.subscribed`. The host does not pick `luna_med`.
7. **Refuse start** if visuals MCP is down, declared poll returns 503, the URL was guessed, or `stream.subscribed` is missing. Never fabricate evidence, frames, rewards, or usage.
8. Leave the canvas open through terminal status and confirm every lane finishes or exposes its named failure.

Harbor: open `live.harbor_eval.v1` from register `metadata.liveEval` before trial start. Two `policy_ref`s (`luna_med` and `sol_med`). `live_frames=native` fails.

VisualsBench keeps that Harbor outer card but grades the separate product
visual authored by Codex. Its register metadata pins `harbor_fused` + Codex
with `mcp_bind: synth_visuals`; do not start without that bind, and never use
the `stream` input on the product visual.

## Iteration rubric

Every review supplies these booleans: `rendered`, `noOverflow`, `primarySurfaceVisible`, `temporalControls`, `traceInspector`, and `realEvidence`. Systems visuals additionally require `noTextCollisions`, `focalDensity`, and `screenshotInspected`; their reviews require a PNG/JPEG screenshot path, and readiness is refused while deterministic authoring findings remain. `live.craftax.v1` additionally requires `imageReplay`, which is true only when ordered Containers PNG frame URLs render and can be scrubbed or played. Also critique:

- Is the primary environment or decision surface dominant above the fold?
- Can the operator tell environment facts from policy facts and evaluator authority?
- Do evaluation-time and rollout-time controls share an explicit cutoff?
- Are reward, achievements, usage, failures, and missing values visually distinct?
- Is raw trace evidence selectable without taking over the main story?
- Does compact mode preserve the task, frame/state, live status, and critical outcome?

The live view should emphasize true environment step progress, rollout state,
cumulative reward, achievement count, vitals, usage/cost when present, and the
latest semantic engine event. Craftax also exposes a per-lane through-time
cutoff and policy span partials. Never substitute elapsed time for step progress,
invent missing values, or fill the pane with raw JSON; retain the journal and
sealed Trace V5 for deeper inspection and post-run reopening.

## Live optimizer runs

Optimizer visuals are owned by the optimizer service. Start an allowlisted recipe
through `use-synth-optimizers` with `open_visual: true`, or call its `open_visual`
operation for an existing run. Do not create a parallel `analysis.visual.v1` or
bind an optimizer feed manually. The host selects the GEPA, GELO, or SFT family,
binds input `optimizer_run`, shows the same durable visual ID in the current chat's
right pane, and keeps reading the optimizer event cursor while the agent continues
to talk or poll. Reopening after a restart must reuse that ID and replay persisted
events; unknown score, reward, cost, coverage, or evidence integrity remains missing.

Ad-hoc analysis is different from the product-owned live viewer. After the user
asks for a cross-run comparison, operation `chart` may bind completed or
running runs as separate `optimizer_run` slots and derive panels from those
snapshots. Label a read from an unsealed run as a snapshot. If a requested run
is unavailable in the current catalog or task scope, do not substitute another
run or copy values from prose: keep that arm null and name the unavailable run
in a note. The source matrix and metric rules are in
[ad-hoc-visuals.md](references/ad-hoc-visuals.md).

## `blank.canvas.v1`

Use the blank canvas for dense dashboards, environment-state illustrations, unusual trace layouts, or publication-style visual stories that are not a Mermaid diagram. Supply a `document` containing:

- `html`: semantic, self-contained HTML or inline SVG;
- `css`: optional scoped presentation CSS;
- `description`: a short accessible explanation;
- `height`: the intended pane height, normally 480–900 px;
- `background`: optional canvas color.

The canvas runs in a sandbox without scripts, network, forms, popups, or parent-page access. Build interactivity with a registered trusted template instead. Do not embed secrets, remote assets, untrusted HTML, or controls that appear interactive but cannot work.

Prefer semantic HTML and SVG with meaningful `<title>`, `<desc>`, headings, labels, and table structure. Design for the narrow side pane first, avoid fixed desktop widths, and keep all essential evidence readable without hover.

## Quality gate

Before marking a visual ready, verify:

- every mark maps to real data;
- the chart form matches the sample structure;
- units and denominators are visible;
- the title says what is compared;
- the key difference is readable without hovering;
- caveats are adjacent to the claim they qualify;
- the pane remains useful at narrow Desktop width.
