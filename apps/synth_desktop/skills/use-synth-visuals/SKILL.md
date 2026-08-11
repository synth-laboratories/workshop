---
name: use-synth-visuals
description: Use when creating, updating, inspecting, or opening a Synth Desktop visual from task, rollout, trace, or eval evidence.
---

# Use Synth Visuals

Choose the visual grammar from the evidence. Treat registered templates as optional shortcuts, not mandates. For ad-hoc analysis, prefer `analysis.visual.v1` and author its ordered `spec.blocks` at creation time. Use `blank.canvas.v1` when the composition cannot be expressed cleanly with those blocks. Read [visual-style-guide.md](references/visual-style-guide.md) before authoring any user-facing visual; it defines the pane-first hierarchy, density, typography, color, and canonical compositions.

Codex advertises one compact custom tool, normally
`mcp__synth_visuals__visual_manage`, instead of all visual schemas on every
turn. Call it with an `operation` and an operation-specific `arguments` object.
Use the exact tool exposed in the current tool catalog; do not search generic
MCP resources after the visual tool is already available.

| Operation | Arguments |
| --- | --- |
| `list_templates` | `{ "genre"?: string }` |
| `list` | `{ "search"?: string, "status"?: string, "session_id"?: string }` |
| `get` | `{ "visual_id": string }` |
| `create` | `{ "template_id": string, "title"?: string, "props"?: object, "session_id"?: string, "instance_id"?: string }` |
| `update` | `{ "visual_id": string, "title"?: string, "bindings"?: object, "status"?: string }` |
| `bind` | `{ "instance_id": string, "slot": string, "kind": string, "source": string, "path"?: string, "schema"?: string }` |
| `save` | `{ "visual_id": string, "tsx"?: string }` |
| `show` | `{ "visual_id": string, "session_id"?: string }` |
| `fork` | `{ "visual_id": string, "title"?: string }` |
| `archive` | `{ "visual_id": string }` |

For example, list templates with
`{"operation":"list_templates","arguments":{}}`.
Do not invent separate callable names such as
`mcp__synth_visuals__visual_create`; legacy MCP names remain compatible for
other clients but are intentionally not advertised to Codex.

## Workflow

1. Inspect the available evidence before choosing a chart: task metadata, rollout count, seeds, traces, reward components, achievements, costs, tokens, latency, and failure state.
2. State the analytical question in one sentence: “Which arm achieves more per dollar?”, “Where do rewards diverge?”, or “What happened during this rollout?”
3. Choose only visual forms that answer that question. Read [visual-recipes.md](references/visual-recipes.md) for mappings and [visual-style-guide.md](references/visual-style-guide.md) for presentation.
4. Sketch the first pane-height: conclusion, one primary visual, and only the metrics needed to interpret it.
5. Create the smallest useful visual with the `create` operation, a stable ID, and a title that names the task and comparison.
6. Show exact units and provenance. Preserve small costs rather than rounding them to `$0.00`.
7. Call the `show` operation after creation or update so the result opens in the Desktop pane.
8. Inspect the rendered pane at its actual width. Fix clipped labels, awkward wrapping, empty sections, misleading encodings, excessive whitespace, and evidence that should be collapsed or moved below the fold before reporting completion.

## Composition rules

- Lead with task identity and the question answered, not the template name.
- Establish context with a compact task card when environment state or objective affects interpretation.
- Put the most decision-relevant result above the fold.
- Give each screenful one dominant question and one dominant visual. Supporting cards explain the graphic; they do not replace it.
- Prefer direct labels over legends. If a legend is necessary, keep arm names short and consistent.
- Use color for identity or signed meaning, never decoration. Keep one accent and one neutral comparison color by default.
- Include sample count (`n`), seeds, aggregation, and uncertainty near the result.
- Label single-rollout comparisons as exploratory. A 0% or 100% observation from one rollout is not a stable frequency estimate.
- Distinguish missing from zero and failed from scored.
- Keep raw trace detail available through a scrubber or table, but summarize the important transition first.
- Keep audit detail last. Show a concise provenance line near the claim and put filenames, IDs, and field-level sourcing in the final block or an expandable surface.

## Never do this

- Do not draw a line or smoothed curve across unordered model arms.
- Do not invent a Pareto frontier, trend, confidence interval, or distribution from insufficient observations.
- Do not use a heatmap for one achievement or one arm.
- Do not make an area proportional to a value unless area is the intended encoding.
- Do not truncate labels into ambiguity or overlay labels on marks.
- Do not report cost as `$0.00` when a nonzero micro-cost is known.
- Do not treat reward, achievement count, and pass rate as interchangeable.
- Do not lead with four equal metric cards when one result matters most; establish a clear visual hierarchy.
- Do not use a table as the primary visual when position, slope, or length would reveal the comparison faster.
- Do not expose long paths, rollout IDs, or methodology paragraphs above the primary result.

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

Use `live.container_rollouts.v1` for an eval that is still running. Bind its required `stream` slot as `live_sse` with an absolute endpoint that emits native `evals.event-stream.v1` events.

Prepare in this order:

1. Start the SSE bridge or eval service without starting rollouts.
2. Create the visual and bind the `stream` slot.
3. Call the `show` operation and confirm the pane says it is waiting or connected.
4. Start the eval only after the pane is ready.
5. Leave the pane open through terminal status and confirm every lane finishes or exposes its named failure.

The live view should emphasize true `progress.done / progress.total`, rollout state, cumulative reward, achievement count, vitals, usage/cost when present, and the latest semantic engine event. Never substitute elapsed time for step progress. Do not fill the pane with raw JSON or full Craftax maps; show a concise recent-activity tail and retain the underlying stream for deeper inspection.

## `blank.canvas.v1`

Use the blank canvas for bespoke diagrams, dense dashboards, environment-state illustrations, unusual trace layouts, or publication-style visual stories. Supply a `document` containing:

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
- the first pane-height contains a conclusion and a real visual, not only prose and cards;
- secondary evidence and provenance do not compete with the main result;
- the pane remains useful at narrow Desktop width.
