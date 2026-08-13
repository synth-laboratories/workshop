# Benjamin Dicken Style dynamic explanation

Use `diagram.systems.dynamic.v1` only when time or changing state teaches something a still cannot: movement of work/data/control, queue pressure, retries, replication, shard behavior, optimizer steps, or a graph evolving with the mechanism.

Author in five passes:

1. **Storyboard:** at least three named beats, each with one behavioral claim and an accessible description.
2. **Visual grammar:** reusable nodes, connectors, graph marks, typography, palette, spacing, easing, and emphasis rules.
3. **Bounded timeline:** explicit canvas, `durationMs`, `posterTimeMs`, deterministic initial/final state, beat times, and only schema-supported changes.
4. **Poster fallback:** `posterTimeMs` must select a state that communicates the core system as deterministic SVG; `reducedMotion` is exactly `"poster"` or `"final"`.
5. **Refinement:** verify behavior first, then connector/label clarity, then pacing and publication polish.

## Required visual QA loop

After every create or update:

1. Call `show`, then `authoring_context`; resolve every automated finding.
2. Capture the rendered Desktop pane at a wide viewport and at a compact viewport.
3. Open and inspect both screenshots. Fail the review for any touching/overlapping text, ambiguous truncation, label crossing, hidden primary claim, or poster state with more than roughly 5–7 focal elements.
4. Update the same visual ID and repeat. Record the real screenshot paths with `review`; call `mark_ready` only after both images pass.

JSON validity and bounded rectangles are not visual approval. A review without an inspected rendered screenshot is invalid.

Illustrative source shape (follow the registered schema when it is stricter):

```json
{
  "version": 1,
  "title": "A retry becomes queue pressure",
  "canvas": { "width": 1200, "height": 680 },
  "theme": "technical-dark",
  "groups": [],
  "nodes": [
    { "id": "client", "x": 80, "y": 270, "width": 180, "height": 72, "label": "Client" },
    { "id": "queue", "x": 500, "y": 270, "width": 200, "height": 72, "label": "Retry queue" },
    { "id": "worker", "x": 920, "y": 270, "width": 180, "height": 72, "label": "Worker" }
  ],
  "edges": [
    { "id": "enqueue", "from": "client", "to": "queue", "label": "retry" },
    { "id": "drain", "from": "queue", "to": "worker", "label": "work" }
  ],
  "notes": [],
  "durationMs": 9000,
  "posterTimeMs": 9000,
  "reducedMotion": "poster",
  "beats": [
    { "id": "request", "atMs": 0, "durationMs": 2500, "caption": "Request fails", "description": "The failed request enters retry handling." },
    { "id": "pressure", "atMs": 3000, "durationMs": 3000, "caption": "Retries accumulate", "description": "Retries arrive faster than the worker drains them." },
    { "id": "recovery", "atMs": 6500, "caption": "Worker recovers", "description": "Drain rate rises and queue depth falls." }
  ],
  "timeline": [
    { "atMs": 0, "durationMs": 600, "easing": "ease-out", "target": "queue", "changes": { "opacity": 0.45 } },
    { "atMs": 3000, "durationMs": 900, "easing": "ease-in-out", "target": "queue", "changes": { "opacity": 1, "emphasis": true, "style": "warning" } },
    { "atMs": 6500, "durationMs": 700, "easing": "ease-in", "target": "worker", "changes": { "emphasis": true, "style": "success" } }
  ]
}
```

The dynamic schema extends the static scene at the root. Use these exact fields: `durationMs`, `posterTimeMs`, `beats[{id, atMs, durationMs?, caption, description?}]`, `timeline[{atMs, durationMs?, easing?, target, changes:{visible?, x?, y?, opacity?, emphasis?, style?}}]`, and `reducedMotion: "poster" | "final"`. Timeline easing is limited to `linear`, `ease-in`, `ease-out`, `ease-in-out`, `step-start`, or `step-end`; omit it for the renderer default.

Motion is explanatory, not decorative. Define pause, replay, scrub, beat navigation, reduced-motion behavior, and a useful poster. Never include arbitrary JavaScript, HTML, remote scripts/assets, a live `stream` binding, WebGL, decorative parallax, or an implicit infinite animation.
