---
name: use-synth-visuals
description: Use when creating, updating, inspecting, or opening a Synth Desktop visual from task, rollout, trace, or eval evidence.
---

# Use Synth Visuals

Choose the visual grammar from the evidence. Treat registered templates as optional shortcuts, not mandates. For ad-hoc analysis, prefer `analysis.visual.v1` and author its ordered `spec.blocks` at creation time. Use `blank.canvas.v1` when the composition cannot be expressed cleanly with those blocks. If the artifact is a system, UML, or flow picture, load `author-synth-diagrams` and create `diagram.mermaid.v1` instead of dumping SVG into a canvas.

Codex advertises one compact custom tool, `mcp__synth_visuals`, instead of all
visual schemas on every turn. Call it with `method: "visual_manage"`, an
`operation`, and an operation-specific `arguments` object:

| Operation | Arguments |
| --- | --- |
| `list_templates` | `{ "genre"?: string }` |
| `list` | `{ "search"?: string, "status"?: string, "session_id"?: string }` |
| `get` | `{ "visual_id": string }` |
| `create` | `{ "template_id": string, "title"?: string, "content"?: string, "props"?: object, "session_id"?: string, "instance_id"?: string }` |
| `update` | `{ "visual_id": string, "title"?: string, "content"?: string, "bindings"?: object, "status"?: string }` |
| `bind` | `{ "instance_id": string, "slot": string, "kind": string, "source": string, "path"?: string, "schema"?: string }` |
| `show` | `{ "visual_id": string, "session_id"?: string }` |
| `fork` | `{ "visual_id": string, "title"?: string }` |
| `archive` | `{ "visual_id": string }` |
| `authoring_context` | `{ "visual_id": string }` |
| `review` | `{ "visual_id": string, "revision": number, "viewport": {"width": number, "height": number}, "checks": object, "findings": string[], "screenshot_path"?: string }` |
| `mark_ready` | `{ "visual_id": string, "revision": number }` |

For example, list templates with
`{"method":"visual_manage","operation":"list_templates","arguments":{}}`.
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
8. Perform at least two explicit render-and-critique iterations at distinct viewport widths. Record each with `review`; do not reuse a review after the visual revision changes.
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

Author a `spec` with a short narrative and ordered blocks. Available blocks:

- `note`: context, caveat, or conclusion.
- `metrics`: exact headline values with optional details.
- `ranked-bars`: ordered magnitudes with a meaningful zero baseline.
- `frequency-diff`: two-arm per-achievement frequencies and percentage-point deltas.
- `table`: exact multi-field comparison or provenance.
- `scatter`: independent observations on two quantitative axes. Never add connecting lines.

Use specialized rollout or trace templates only when their interaction is genuinely useful. Use `craftax.rollout_scrub.v1` for step-by-step environment inspection and `trace.rollout_inspector.v1` for event/tool/message filtering.

For a Trace V5 record, bind the canonical `synth.trace-projection.rollout-inspector.v1` projection and let the first-class inspector preserve the trace hierarchy. Start in **Focus** for agent messages, tool activity, failures, and evaluation evidence; switch to **Full** only when model-call and lifecycle provenance matter. Put verdicts and grader rationale in **Evidence**, and identity, digests, visibility, token usage, and lane coverage in **Metadata**. Search commands and outputs or jump to an exact sequence instead of flattening the run into a generic chart. Never reconstruct missing events, expose content above the projection's visibility ceiling, or imply that an incomplete lane has complete coverage.

## Live container evals

Use the task-family live template for an eval that is still running:
`live.craftax.v1`, `live.harbor_eval.v1`, or `live.digbench.v1`. Bind its required
`stream` slot as `live_sse` with the absolute SSE endpoint and exact poll
endpoint declared by rollout preparation. The stream emits
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

dig.bench: open `live.digbench.v1` before `start_session`. Basic ReAct and agentic Codex + `digbench-mcp` on the same game. No frames. Token never in bindings or screenshots. `/reward` is `completed` → 1, `game_over` → 0, incomplete → null.

## Iteration rubric

Every review supplies these booleans: `rendered`, `noOverflow`, `primarySurfaceVisible`, `temporalControls`, `traceInspector`, and `realEvidence`. `live.craftax.v1` additionally requires `imageReplay`, which is true only when ordered Containers PNG frame URLs render and can be scrubbed or played. Also critique:

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

Before showing a visual, verify:

- every mark maps to real data;
- the chart form matches the sample structure;
- units and denominators are visible;
- the title says what is compared;
- the key difference is readable without hovering;
- caveats are adjacent to the claim they qualify;
- the pane remains useful at narrow Desktop width.
