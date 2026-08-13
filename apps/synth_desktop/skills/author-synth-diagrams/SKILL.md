---
name: author-synth-diagrams
description: Author Mermaid/UML, static 2D systems maps, or Benjamin Dicken Style dynamic explanations into the right Visual pane for this question.
---

# Author Synth diagrams

Use when the user needs a **picture of a system**. Choose the grammar that answers this question: renderer-laid-out Mermaid for exact semantics, an explicitly positioned 2D map for spatial architecture, or a bounded 4D explainer when change over time is the lesson. Do not open a stock visual.

Live eval evidence stays on `live.*.v1`. Optimizer runs stay on `optimizer.*.v1`. Do not route system pictures through `blank.canvas.v1`.

## Intended approach

- Start with one visual claim and the evidence that establishes it.
- Preserve semantic truth before style: direction, containment, time, absence, and uncertainty must match the source evidence.
- Use the least expressive sufficient grammar. Mermaid owns formal UML semantics; static 2D owns deliberate spatial composition; Benjamin Dicken Style owns explanation through time.
- Design a reading path. The first glance should reveal the claim, the second the mechanism, and the third the exact evidence or caveat.
- Prefer progressive disclosure over completeness on one frame. A diagram is not improved by showing every available identifier, connector, or result simultaneously.
- Treat rendering as the start of review. Inspect the actual pixels at wide and compact sizes, revise, and only then certify readiness.

## Workflow

1. State the visual claim, then choose a mode from the selection rules below. The common path does not require template discovery, MCP resources, or filesystem search.
2. Read only the matching reference: [families.md](references/families.md), [systems-map.md](references/systems-map.md), or [dynamic-systems.md](references/dynamic-systems.md).
3. Call `mcp__synth_visuals__visual_manage` directly with `operation: "create"` and **new** `content` in `arguments`. Do not put source in `props`.
4. Read the returned `visual.id`, then call the same tool with `operation: "show"` and that ID so the visual lands in the right pane of this chat.
5. Call `authoring_context`. Treat every `automatedFindings` entry as revision feedback, not a warning to ignore.
6. Inspect the actual rendered Desktop visual—not the JSON source. For UML/Mermaid, static 2D systems maps, and Benjamin Dicken Style dynamic visuals alike, call `capture_review` at wide and compact viewport sizes; it returns each real PNG as tool image content and its absolute `screenshot_path`. Look at both attached images, check text collisions, truncation, hierarchy, edge crossings, and focal density, then revise the same ID with `update` + new `content`. Do not shell-search for screenshots.
7. Record both screenshot paths with `review`. Systems visuals must pass `noTextCollisions`, `focalDensity`, and `screenshotInspected`; never self-report those checks without opening the images. Repeat render → screenshot → critique → update until both reviews pass, then call `mark_ready`.

Do not call `resources/list` or `resources/read`: `synth_visuals` is a tool-only MCP server. Do not shell-search for the tool, MCP registration, or skill implementation. Repository inspection required to answer the user's question is allowed, but keep it bounded and call the visual tool as soon as the necessary evidence is gathered. Reference files are optional syntax help only when the requested family is unfamiliar.

In code mode, do not guess the facade shape or search `ALL_TOOLS`. Use the
registered callable directly:

```js
const created = await tools.mcp__synth_visuals__visual_manage({
  operation: "create",
  arguments: {
    template_id: "diagram.mermaid.v1",
    title: "Exact request order",
    content: "sequenceDiagram\n  Agent->>MCP: request\n  MCP->>Registry: create",
    presentation: "pane"
  }
});
text(created);
```

Read `visual.id` from that result, then call the same exact tool with
`{ operation: "show", arguments: { visual_id: "vis_..." } }`.

```json
{
  "operation": "create",
  "arguments": {
    "template_id": "diagram.mermaid.v1",
    "title": "How policy_ref reaches the container",
    "content": "sequenceDiagram\nAgent->>MCP: policy_ref\nMCP->>IPC: start\nIPC->>Container: POST /rollouts",
    "presentation": "pane",
    "session_id": "ses_..."
  }
}
```

Then call `visual_manage` again with `{"operation":"show","arguments":{"visual_id":"vis_..."}}`.

## Rules

- `diagram.mermaid.v1` content is UTF-8 Mermaid. `diagram.systems.v1` and `diagram.systems.dynamic.v1` content is bounded UTF-8 JSON. Never paste arbitrary SVG, HTML, or JavaScript into a canvas.
- Default `presentation` is `"pane"`.
- Never bind slot `stream`.
- Missing `content` fails closed. Do not retry as `blank.canvas.v1`.
- Preserve stable IDs across revisions. In 2D/4D sources, every group and node has an explicit finite rectangle and every edge references existing nodes.
- Prefer 5–7 focal elements per beat. Stage secondary evidence through later beats instead of showing every node and connector in the poster state.
- Keep node labels short enough to fit their rectangles and edge labels at 24 characters or fewer. Put exact identifiers and long evidence in the beat description or a detail note, not across a connector.
- Do not claim an edge, state, or animation beat that the evidence does not establish. Use a missing/unproven treatment when that absence is the point.

## Selection rules

- **Mermaid/UML** (`diagram.mermaid.v1`): exact call order or concurrency → sequence; lifecycle → state; types/interfaces/inheritance → class; entities/cardinality → ER. Use Mermaid flowchart/C4 when the user wants a conventional, automatically laid-out topology or context view.
- **Static 2D systems map** (`diagram.systems.v1`): broad topology, whole-repository maps, ownership/deployment/trust boundaries, before/after, placement, containment, missing edges, dormant/unproven/planned paths, or Monodraw/Excalidraw-style composition.
- **Benjamin Dicken Style dynamic explanation** (`diagram.systems.dynamic.v1`): data/work/failure/load/control moving through time; intermediate states; queues, shards, retries, replication, optimizer steps, animated graphs; or explicit Benjamin Dicken Style/dynamic/animated/interactive/4D wording.
- **Both (or a focused set):** use a 2D overview plus Mermaid sequence/state detail when spatial context and exact behavior are both material. Add 4D only for the mechanism whose evolution is the lesson. Do not create all three by reflex.
- Explicit wording wins. “UML,” “sequence,” and “state machine” stay Mermaid; “systems map,” “2D,” “before/after architecture,” and “Monodraw/Excalidraw style” prefer static 2D.

## Dynamic authoring pipeline

For `diagram.systems.dynamic.v1`, do not jump directly to motion:

When subagents are available, delegate storyboard plus scene/timeline authoring to one bounded subagent. The parent agent owns evidence selection, integration, safety validation, creation, and `show`.

1. Write a storyboard of at least three named beats, each with one behavioral claim and accessible description.
2. Define reusable primitives and design rules: boxes, connectors, graph marks, typography, palette, spacing, easing, and emphasis. Reuse these consistently; Workshop's subagent is the coding collaborator.
3. Build a bounded deterministic timeline with exact `durationMs`, `posterTimeMs`, ordered beats, timestamped target changes, and `reducedMotion: "poster" | "final"`. Timeline items may add `durationMs` and an allowed `easing`; animate only visibility, position, opacity, emphasis, and supported style changes.
4. Make the poster fallback useful by itself. It must communicate the core system as deterministic SVG with no motion.
5. Refine in passes: first behavioral correctness, then label/connector clarity, then pacing and publication polish. Verify pause, replay, scrub, beat navigation, reduced motion, and poster rendering.

Supported Mermaid families remain flowchart, sequence, class, state, ER, and C4. Sankey, Gantt, git, mindmap, pie, and other Mermaid families are unsupported—say so rather than creating an empty pane.
