# Static 2D systems map

Use `diagram.systems.v1` when deliberate placement carries meaning: broad topology, ownership and trust boundaries, before/after composition, or missing and unproven edges.

Compose for a reading path rather than maximum coverage. Use groups for a few meaningful boundaries, 5–7 primary nodes when possible, short labels, and whitespace as structure. Move exact paths, IDs, and caveats into compact notes only when they are necessary to interpret the map. If the map needs many sequential facts, use a Mermaid sequence or a Benjamin Dicken Style explanation instead of forcing them into one poster.

After creation, use the same screenshot-backed wide/compact QA loop described by the parent skill. Explicit coordinates make the author responsible for label fit, connector crossings, balance, and density; a valid rectangle is not evidence that the composition is readable.

The `content` value is bounded JSON, serialized as a string for `visual_manage`:

```json
{
  "version": 1,
  "title": "Before and after",
  "theme": "technical-dark",
  "canvas": { "width": 1200, "height": 680 },
  "groups": [
    { "id": "before", "x": 40, "y": 50, "width": 500, "height": 560, "label": "Before" },
    { "id": "after", "x": 660, "y": 50, "width": 500, "height": 560, "label": "After" }
  ],
  "nodes": [
    { "id": "old-agent", "x": 190, "y": 270, "width": 200, "height": 72, "label": "Agent", "group": "before" },
    { "id": "new-agent", "x": 810, "y": 270, "width": 200, "height": 72, "label": "Agent", "group": "after" },
    { "id": "evidence", "x": 730, "y": 110, "width": 200, "height": 72, "label": "Evidence", "group": "after" }
  ],
  "edges": [
    { "from": "evidence", "to": "new-agent", "label": "grounding" }
  ],
  "notes": []
}
```

Give every group and node an explicit finite rectangle. Keep IDs stable, reference only existing nodes, and use source order intentionally for paint order. Prefer orthogonal connectors. Use dashed/muted/missing styling for absent, planned, or unproven paths; never render them as established facts. Coordinates are authoritative, so fix collisions in the source rather than expecting automatic layout.

Create with `template_id: "diagram.systems.v1"`, then show the returned visual ID. Never fall back to arbitrary SVG or `blank.canvas.v1`.
