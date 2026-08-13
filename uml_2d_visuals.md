# UML, 2D systems visuals, and Benjamin Dicken Style explanations

**Date:** 2026-08-12
**Decision:** Workshop supports three peer authoring modes: semantic Mermaid/UML diagrams, explicitly composed static 2D systems maps, and polished Benjamin Dicken Style dynamic explanations. The agent chooses one or intentionally combines them according to the question.

## Why there are three modes

The distinction is not "ASCII versus UML." The three modes answer three different questions: **what is structurally true, where does it live, and how does it change over time**.

Mermaid is the precise, structured lens. It is appropriate when the meaning comes from a known diagram family: ordered messages, lifecycle states, types and relationships, entity relationships, or a conventional component/C4 view. Workshop's Grok-derived renderer already supports these families and produces deterministic SVG renditions.

The systems-map mode is the atlas. It is appropriate when the meaning comes partly from where things are placed: whole-machine topology, ownership boundaries, current versus target state, before/after columns, absent or unproven edges, implementation status, and editorial annotations. These maps resemble the clean spatial diagrams made with Monodraw or Excalidraw more than formal UML.

Benjamin Dicken Style is the guided dynamic explanation mode. It preserves deliberate 2D composition but adds time: staged reveals, state transitions, data motion, camera/viewport changes, emphasis, counters, and a narrative timeline. It is appropriate when a still image would make the reader mentally simulate the system.

[Ben Dicken's July 16 thread](https://x.com/BenjDicken/status/2077826040127422568) describes the reference workflow behind the dynamic kind in unusually concrete terms:

- rough static drawings in Excalidraw first, explicitly to settle layout and visual goals before animation;
- a screenshot plus a behavioral brief telling the coding agent what the visual must demonstrate;
- references to already-built visuals and reusable primitives such as boxes and connectors;
- inspiration assets converted into a reusable design-rules skill;
- several prompting/refinement passes, followed by manual sizing and publication polish;
- code as the final output ([elsewhere he identifies SVG + GSAP + JavaScript](https://x.com/BenjDicken/status/2077437733782557066)).

The important lesson is not "use Cursor." Workshop's subagent is the coding agent. The lesson is to separate storyboard, behavior, reusable visual grammar, timeline, and polish. Benjamin Dicken Style is therefore a code-backed visual artifact with a bounded runtime, not animated Mermaid and not a video pasted into the pane.

## The three first-class formats

| Format | Purpose | Layout authority | Typical output |
| --- | --- | --- | --- |
| `diagram.mermaid.v1` | UML and semantic diagrams | renderer | sequence, state, class, ER, flowchart, C4 |
| `diagram.systems.v1` | spatial systems maps | author-provided coordinates | topology, ownership, before/after, missing edges, system atlas |
| `diagram.systems.dynamic.v1` | Benjamin Dicken Style dynamic systems explanations | author storyboard + timeline | staged architecture, data motion, load/failure behavior, animated graphs |

All three are immutable visual revisions stored through the same Visual Registry. All appear in chat and the Visual pane, survive restart, expose canonical source, and refuse the live `stream` slot. Static formats render deterministic SVG. Benjamin Dicken Style renders in a sandboxed pane, supplies a deterministic poster/still SVG, and supports replay, pause, scrub, reduced motion, and export of the still.

## `diagram.systems.v1` source contract

The canonical source is bounded UTF-8 JSON. It is a small scene description designed for agents, not arbitrary SVG.

```json
{
  "version": 1,
  "title": "Agent-assisted query repair",
  "theme": "technical-dark",
  "canvas": { "width": 1200, "height": 680 },
  "groups": [
    { "id": "inputs", "x": 48, "y": 56, "width": 300, "height": 520, "label": "Evidence" }
  ],
  "nodes": [
    { "id": "mcp", "x": 92, "y": 250, "width": 210, "height": 72, "label": "PlanetScale MCP", "group": "inputs" },
    { "id": "agent", "x": 480, "y": 250, "width": 190, "height": 72, "label": "AI agent" },
    { "id": "github", "x": 870, "y": 250, "width": 190, "height": 72, "label": "GitHub" }
  ],
  "edges": [
    { "from": "mcp", "to": "agent", "label": "Insights + recommendations" },
    { "from": "agent", "to": "github", "label": "opens pull request" }
  ],
  "notes": [
    { "x": 480, "y": 370, "width": 300, "text": "Evidence remains linked to the recommendation." }
  ]
}
```

Required properties:

- `version` is `1`.
- `canvas.width` and `canvas.height` define the coordinate system.
- every node and group has an explicit, finite rectangle.
- edges reference existing node IDs.
- source order is stable and meaningful for paint order and deterministic output.

Supported visual vocabulary:

- groups with optional labels;
- rectangular nodes with multiline labels and optional semantic/status style;
- orthogonal or straight directed edges with labels;
- free-standing notes;
- light and `technical-dark` themes;
- solid, dashed, muted, warning, success, and missing/unproven edge treatments.

The renderer must escape all text, reject scripts/HTML/URLs, impose source/count/axis/output limits, and produce deterministic standalone SVG. Coordinates are not silently rearranged. Invalid or overlapping scenes return an explicit error and retain inspectable source; Workshop never substitutes a fake diagram.

## `diagram.systems.dynamic.v1` source contract

The canonical source is a bounded scene package with four explicit layers:

1. **Storyboard:** named beats and the claim each beat teaches.
2. **Scene:** explicit groups, nodes, edges, graphs, labels, and their stable IDs.
3. **Timeline:** deterministic changes to visibility, geometry, style, data flow, emphasis, and viewport over time, with bounded durations and easing for interpolated motion.
4. **Design rules:** typography, palette, spacing, connector grammar, motion easing, and reusable components.

The first implementation should keep this declarative where possible and use a vetted runtime rather than evaluating arbitrary page JavaScript. Its primitives must be rich enough to reproduce the reference qualities: dark technical canvas, precise typography, restrained color, labeled connectors, progressive reveals, animated flows, graph traces/areas, zoom or pan when useful, and crisp publication sizing.

Every dynamic visual must define:

- canvas dimensions and duration;
- a poster time or explicit poster state;
- a finite ordered set of beats with captions or accessible descriptions;
- deterministic initial and final state;
- pause, replay, scrub, and reduced-motion behavior;
- a static SVG fallback that communicates the core system without motion.

"4D" means authored 2D space plus time and system state. It does not mean WebGL, a 3D camera, decorative parallax, or animation for its own sake.

## Agent selection policy

Choose a **2D systems map** when the user asks to:

- map a whole system or repository;
- compare before and after;
- show ownership, deployment, containment, or trust boundaries;
- show what is wired, missing, dormant, unproven, or planned;
- create an architecture atlas where placement and negative space carry meaning;
- match a Monodraw/Excalidraw-style technical systems graphic.

Choose **Mermaid/UML** when the user asks to:

- show exact call order or concurrency: sequence;
- show lifecycle transitions: state;
- show types, interfaces, or inheritance: class/component;
- show entities and cardinality: ER;
- use a conventional C4 or automatically laid-out flowchart.

Choose a **Benjamin Dicken Style explanation** when the user asks to:

- show how data, work, failures, load, or control moves through a system;
- teach a multi-stage mechanism whose intermediate states matter;
- animate a performance curve, queue, shard, retry, replication, or optimizer process;
- produce a polished, presentation-ready technical explainer;
- use wording such as dynamic, animated, interactive, time-aware, or 4D.

Combine formats intentionally when one cannot answer the whole question. A useful set is a static 2D overview, a Mermaid sequence/state detail for exact semantics, and a Benjamin Dicken Style explanation only for the part whose evolution is the lesson. Do not create all three by reflex.

Explicit user wording wins. "UML," "sequence," or "state machine" must not silently become a systems map. "Systems map," "2D map," "before/after architecture," or "Monodraw/Excalidraw style" should prefer `diagram.systems.v1`.

## Presentation

Workshop labels the modes honestly:

- `SYSTEMS MAP · 2D`
- `BENJAMIN DICKEN STYLE`
- `UML · SEQUENCE`
- `UML · STATE`
- `UML · CLASS`
- `DIAGRAM · FLOWCHART` or `DIAGRAM · C4` when a Mermaid view is not UML

The systems-map pane uses the same core controls as Mermaid: zoom, fit, source, copy source, export SVG, and retry. The 4D pane adds play/pause, replay, scrub, beat navigation, and reduced-motion controls. Export should provide visible success or failure feedback and identify the saved file.

## Non-goals for the first cut

- importing arbitrary SVG;
- implementing the complete Excalidraw or tldraw schema;
- a drag-and-drop canvas editor;
- arbitrary JavaScript, remote scripts, or remote assets;
- a general-purpose video editor or full GSAP authoring environment;
- automatic conversion between systems maps and UML;
- treating all Mermaid diagrams as UML;
- replacing specialized live eval/trace templates.

## Acceptance

1. An agent can create and show a `diagram.systems.v1` scene through `synth_visuals.visual_manage`.
2. The scene renders as a non-empty deterministic SVG and reopens after restart.
3. Source, copy, zoom, fit, retry, and SVG export work in the Visual pane.
4. Invalid geometry, dangling edges, unsafe text/content, excessive counts, and oversized output fail closed.
5. `stream` binding is refused.
6. The authoring skill chooses systems maps for before/after topology and Mermaid sequence/state/class for formal views.
7. A CUA run can ask for multiple modes and receive distinct rendered visuals.
8. An agent can create and show a `diagram.systems.dynamic.v1` explainer with at least three meaningful beats.
9. The 4D visual can pause, replay, scrub, honor reduced motion, and reopen at a deterministic poster state.
10. The 4D visual has no network access or arbitrary-script escape and provides a useful deterministic SVG fallback.
