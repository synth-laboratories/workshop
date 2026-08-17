---
name: use-synth-visuals
description: Use when creating, updating, inspecting, or opening a Synth Desktop visual from task, rollout, trace, or eval evidence.
---

# Use Synth Visuals

Choose the visual grammar from the evidence. Treat registered templates as optional shortcuts, not mandates. For ad-hoc analysis, prefer `analysis.visual.v1` and author its ordered `spec.blocks` at creation time. Use `blank.canvas.v1` when the composition cannot be expressed cleanly with those blocks. If the artifact is a system, UML, flow picture, or time-aware technical explainer, load `author-synth-diagrams`. It chooses among `diagram.mermaid.v1`, `diagram.systems.v1`, `diagram.systems.dynamic.v1`, or a focused combination; do not dump SVG/HTML/JavaScript into a canvas.

Optimizer visuals are a strict exception to the authoring workflow below. They are product-owned and already configured by `use-synth-optimizers`: only call `show` when that workflow asks you to recover a missing subscription receipt. Never call `authoring_context`, `capture_review`, `review`, `update`, or `mark_ready` for an optimizer-owned visual.

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
| `list_templates` | `{ "genre"?: string }` |
| `list` | `{ "search"?: string, "status"?: string, "session_id"?: string }` |
| `get` | `{ "visual_id": string }` |
| `create` | `{ "template_id": string, "title"?: string, "content"?: string, "props"?: object, "session_id"?: string, "instance_id"?: string }` |
| `create_with_bind` | `{ "template_id": string, "title"?: string, "slot": string, "kind": string, "data"?: object, "source"?: string, "schema"?: string }` — atomic create plus the first required slot. Prefer this for `experiment.overview.v1` and `analysis.visual.v1`. |
| `update` | `{ "visual_id": string, "title"?: string, "content"?: string, "bindings"?: object, "status"?: string }` — `bindings` must be the canonical envelope; prefer `bind` |
| `bind` | `{ "instance_id": string, "slot": string, "kind": string, "source"?: string, "data"?: object, "poll_url"?: string, "path"?: string, "schema"?: string, "mode"?: "replace" \| "append", "bindings"?: [{ "kind": string, "source"?: string, "data"?: object, "poll_url"?: string }] }` — inline slots require `data`; other kinds require `source`. Two malformed binds must not block a corrected bind. |
| `show` | `{ "visual_id": string, "session_id"?: string }` |
| `fork` | `{ "visual_id": string, "title"?: string }` |
| `archive` | `{ "visual_id": string }` |
| `authoring_context` | `{ "visual_id": string }` |
| `capture_review` | `{ "visual_id": string, "viewport": {"width": number, "height": number} }` — returns an attached PNG plus `screenshot_path` |
| `review` | `{ "visual_id": string, "revision": number, "viewport": {"width": number, "height": number}, "checks": object, "findings": string[], "screenshot_path"?: string }` (`screenshot_path` is required for systems visuals) |
| `mark_ready` | `{ "visual_id": string, "revision": number }` |

For example, list templates with
`{"operation":"list_templates","arguments":{}}`.
Do not invent separate callable names such as
`mcp__synth_visuals__visual_create`; legacy MCP names remain compatible for
other clients but are intentionally not advertised to Codex.

## Workflow

1. Inspect the available evidence before choosing a chart: task metadata, rollout count, seeds, traces, reward components, achievements, costs, tokens, latency, and failure state.
2. State the analytical question in one sentence: “Which arm achieves more per dollar?”, “Where do rewards diverge?”, or “What happened during this rollout?”
3. Choose only visual forms that answer that question. Read [visual-recipes.md](references/visual-recipes.md) for mappings.
4. Create the smallest useful visual with the `create` operation, a stable ID, and a title that names the task and comparison. Use `presentation: "canvas"` for gameplay, trace workbenches, and dense live dashboards.
5. Show exact units and provenance. Preserve small costs rather than rounding them to `$0.00`.
6. Call the `show` operation after creation or update so the result opens in the Desktop pane.
7. Inspect the rendered visual in Desktop canvas mode. Fix clipped labels, empty sections, misleading encodings, weak hierarchy, and excessive whitespace.
8. Perform at least two explicit render-and-critique iterations at distinct viewport widths. Call `capture_review` for every visual family—evals (`analysis.*`, `live.*`, `craftax.*`), optimizers (`optimizer.*`), UML/Mermaid, static 2D systems maps, and Benjamin Dicken Style dynamic systems visuals. For each viewport inspect the PNG attached to the tool result. Pass its returned `screenshot_path` to `review`; never shell-search for captures, invent a path, or submit checks without looking at the image. Do not reuse a review after the visual revision changes. For systems visuals, resolve every deterministic finding returned by `authoring_context` before readiness.
9. Call `mark_ready` only when all required landmarks pass. A trusted live template is configured through `visual_config`; saved arbitrary TSX is retained as source evidence but is never executed by Desktop.

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

## Never do this

- Do not draw a line or smoothed curve across unordered model arms.
- Do not invent a Pareto frontier, trend, confidence interval, or distribution from insufficient observations.
- Do not use a heatmap for one achievement or one arm.
- Do not make an area proportional to a value unless area is the intended encoding.
- Do not truncate labels into ambiguity or overlay labels on marks.
- Do not report cost as `$0.00` when a nonzero micro-cost is known.
- Do not treat reward, achievement count, and pass rate as interchangeable.

## `analysis.visual.v1`

Required slot: **`spec`**. Author it as `kind: "inline"` with `data` containing a short narrative and ordered `blocks`. Do not bind this template on slot `experiment`. `list_templates` returns `slots` and `bindingSchema`; `example_binding` is the canonical create+bind payload.

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
    slot: "spec",
    kind: "inline",
    data: {
      schemaVersion: "synth.visual.analysis_spec.v1",
      title: "HealthBench smoke · policy vs scorer",
      blocks: [{ type: "metrics", items: [{ label: "Train mean", value: null }] }]
    }
  }
});
```

## `experiment.overview.v1`

Required slot: **`experiment`**, not `spec`. Accepts `inline`, `fixture`, or `local_cas`. Inline binds **must include `data`**. A bind without `data` returns `visual_binding_invalid` / `inline visual binding requires data`; correct the same visual — do not abandon it after two malformed binds.

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
    slot: "experiment",
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
 "slots": [{"slot": "stream", "kind": "live_sse", "source": "...", "poll_url": "..."}]}
```

Use the `bind` operation, which writes that envelope for you. A slot-keyed
object such as `{"stream": [...]}` is a legacy shape: it is upgraded with a
warning today and will be refused. Do not hand-build binding objects through
`update` when `bind` can express what you need.

For a slot the template declares `multiple` — such as ten rollout streams on one
`stream` slot — bind them in one call:

```js
await tools.mcp__synth_visuals__visual_manage({
  operation: "bind",
  arguments: {
    instance_id: visualId,
    slot: "stream",
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

`mode` defaults to `replace`, which drops existing bindings on that slot. Use
`append` when adding to a `multiple` slot across several calls.

## Live container evals

Use the task-family live template for an eval that is still running:
`live.craftax.v1`, `live.harbor_eval.v1`, or `live.digbench.v1`. Bind its required
`stream` slot as `live_sse` with the absolute SSE endpoint and exact poll
endpoint declared by rollout preparation. Every live binding needs `poll_url`:
the durable poll authority is what lets a completed run replay, and a stream
bound without one cannot be reopened after it closes. The stream emits
`synth.trace-stream-event.v1`; never guess or rewrite either route. Discover the
container with `container_list` / `container_probe` and inspect advertised
transports before binding.

Prepare in this order:

1. Discover the provider and inspect capabilities. Do not construct `/events`.
2. Call `container_prepare_rollout`; it must return the exact descriptor and `visual_binding` without starting execution.
3. Create the task-family visual with `presentation: "canvas"`, bind the returned `stream` slot, and call `show`. Review at least twice, then `mark_ready`.
4. Get `authoring_context`. Use a prior real trace or the template's example only to develop layout; label example evidence and replace it with the declared stream before readiness.
5. Wait until the control envelope reports `stream.subscribed` with `ready: true`. HTTP 200 and heartbeats are not ready.
6. Call `container_start_prepared_rollout` with the exact prepared stream descriptor, `visual_id`, `task_instance_id` or `seed`, and `policy_ref` (`harness` + `config`). Desktop refuses a missing pin, a stale visual receipt, and still waits for control-only `stream.subscribed`. The host does not pick `luna_med`.
7. **Refuse start** if visuals MCP is down, declared poll returns 503, the URL was guessed, or `stream.subscribed` is missing. Never fabricate evidence, frames, rewards, or usage.
8. Leave the canvas open through terminal status and confirm every lane finishes or exposes its named failure.

Harbor: open `live.harbor_eval.v1` from register `metadata.liveEval` before trial start. Two `policy_ref`s (`luna_med` and `sol_med`). `live_frames=native` fails.

VisualsBench keeps that Harbor outer card but grades the separate product
visual authored by Codex. Its register metadata pins `harbor_fused` + Codex
with `mcp_bind: synth_visuals`; do not start without that bind, and never use
the `stream` slot on the product visual.

dig.bench: open `live.digbench.v1` before `start_session`. Basic ReAct and agentic Codex + `digbench-mcp` on the same game. No frames. Token never in bindings or screenshots. `/reward` is `completed` → 1, `game_over` → 0, incomplete → null.

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
binds slot `optimizer_run`, shows the same durable visual ID in the current chat's
right pane, and keeps reading the optimizer event cursor while the agent continues
to talk or poll. Reopening after a restart must reuse that ID and replay persisted
events; unknown score, reward, cost, coverage, or evidence integrity remains missing.

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
