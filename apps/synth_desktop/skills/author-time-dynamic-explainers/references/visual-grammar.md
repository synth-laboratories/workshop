# Visual style

These rules are derived from multiple public technical explainers cited in
`observed-sources.md`. They define the surface language. Apply the grammar to
new subject matter; do not reproduce a source composition. Use
`information-dynamics-philosophy.md` for section architecture and causal flow.

## Canvas

- Use a plain off-white or near-black field with minimal framing.
- Reserve generous empty space around the active mechanism.
- Let the mechanism occupy the canvas; avoid title bars, dashboards, and UI
  panels inside the visual.
- Design the main scene at a publication-friendly landscape ratio, then author
  an explicit compact composition or viewport plan rather than shrinking all
  text uniformly.

## Objects

- Draw the real conceptual objects: records, pages, shards, replicas, prompts,
  candidates, examples, or scores.
- Use simple geometric primitives with thin outlines and modest corner radii.
- Repeat a primitive to communicate multiplicity and scale.
- Nest shapes when containment or relative size is the lesson.
- Use dashed boundaries for logical regions, groups, or temporary scope.
- Keep connectors thin and mostly orthogonal or gently curved.
- Prefer classical technical-diagram objects over novelty illustration:
  document records, queue rails, worker slots, server/service boundaries,
  matrices, timelines, and small direct charts.
- Let a repeated family share exact geometry. Differences should encode state
  or identity, not casual styling variation.

## Typography

- Prefer direct labels of one to four words.
- Use a clean, neutral sans-serif for titles and object labels; reserve
  monospace for values or identifiers. Do not use Comic Sans, faux handwriting,
  or Excalidraw-like lettering. The authored quality should come from spacing,
  hierarchy, and vector construction rather than a novelty typeface.
- Keep labels horizontal and near their objects.
- Avoid paragraphs on the canvas. Put the current claim in one short caption.
- Never rely on text smaller than 12 screen pixels in a capture.

## Color

- Use one quiet structural color and one or two semantic accents.
- Assign color consistently by role: selected, incoming, accepted, rejected,
  replica, or changed.
- Fade inactive context without making it unreadable.
- Prefer flat fills and crisp strokes over gradients, glow, or glass effects.

## Hierarchy

- A beat should have one unmistakable active object or relationship.
- Add complexity through repetition over time, not by showing the final dense
  state immediately.
- Make the changed property visually measurable: count, position, size, path,
  partition, membership, or color.
- Size sections by explanatory importance. The causal center should dominate;
  supporting systems and summaries should be visibly secondary.

## Boundaries and routing

- Use a few large regions for real systems, not a grid of dashboard cards.
- Align sections to a shared column or baseline so the visual reads as one
  composed figure.
- Route local handoffs with short orthogonal paths. Consolidate repeated returns
  into one edge bus before they cross the composition.
- Put connector labels on quiet stretches of their own path.
- Leave a clear gutter around text and objects. A path that touches a label,
  record, or unrelated boundary is an overlap, even if it remains technically
  readable.
- Prefer mirrored input tokens near a consumer to long parent or dependency
  wires across the canvas.

## Finish quality

- Keep corner radii small and consistent; avoid toy-like pills and oversized
  rounded cards.
- Show state through position, membership, fill, and restrained stroke changes.
  Do not use giant X marks, novelty icons, or playful strike-throughs for
  rejection.
- Avoid shadows, gradients, glow, glass effects, ornamental textures, and
  decorative animation.
- Use precise alignment, even intervals, short labels, and negative space as
  the sources of polish.

## Dark infrastructure family

- Use a near-black canvas around `#101211`.
- Use cool blue-gray outlines for stable servers, databases, and connectors.
- Use warm orange for the single active route or temporary work object.
- Use muted green for synchronized/healthy state and muted red for stale or
  failed state.
- Keep strokes crisp and thin; avoid shadows, glow, gradients, and ornamental
  texture.
- Combine a short, precisely typeset title with small, quiet explanatory text.
- Keep the control plane sparse above or beside a dense repeated data plane.
