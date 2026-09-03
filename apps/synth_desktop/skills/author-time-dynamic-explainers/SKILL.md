---
name: author-time-dynamic-explainers
description: Author and refine polished time-dynamic technical explainers in Workshop with diagram.systems.dynamic.v1. Use for algorithms, databases, distributed systems, optimizer loops, data movement, scale changes, retries, sharding, trees, queues, or any mechanism whose changing state is the lesson; especially when the user asks for a Benjamin Dicken-style visual, an animated systems explanation, or a visual that teaches how something works over time.
---

# Author time-dynamic explainers

Create an explanation that changes the scene to teach the mechanism. Do not
animate a completed flowchart.

Before authoring, read:

- `references/ben-dicken-playbook.md` for the source-derived production
  playbook and prompt handoff.
- `references/visual-grammar.md` for the visual style: field, primitives,
  typography, color, boundaries, and connector treatment.
- `references/information-dynamics-philosophy.md` for information architecture,
  meaningful sections, cross-section interaction, and causal data flow over
  time.
- `references/motion-grammar.md` for storyboard and transition rules.
- `references/pattern-library.md` for reusable compositions and close visual
  matching.
- `references/reference-coverage.md` when validating that the skill can
  reconstruct the reference family before applying it to new subject matter.
- `references/review-checklist.md` before capture review.

Read `references/observed-sources.md` only when auditing provenance or refreshing
the derived grammar.

When matching the Ben Dicken family, use the **post-pipeline primary set** in
`references/observed-sources.md`. Older chalkboard, talking-head, and generic
product-demo examples are provenance only and must not drive the composition.

## Workflow

1. Make rough static diagrams first. Lock the layout and visual goal before
   implementing animation.
2. Write the one-sentence lesson and the user knowledge gap.
3. Draw the information map before the scene: name the three to five meaningful
   systems, the persistent objects each owns, and the evidence that crosses each
   boundary.
4. Identify the smallest persistent object model that can teach it. Choose one
   dominant mechanism or shared substrate around which the other sections are
   organized.
5. Storyboard three to seven beats. Give every beat one state change and one
   claim.
6. Choose a spatial metaphor from the mechanism itself: split, route, compare,
   grow, queue, traverse, replicate, accept/reject, or converge.
7. Author bounded declarative source for `diagram.systems.dynamic.v1` with a
   useful poster time and reduced-motion state.
8. Reuse the visual family's existing primitives, connectors, boxes, and
   components; do not redraw near-duplicates per scene.
9. Create with the real evidence bound, show it, and inspect
   `authoring_context`.
10. Capture wide and compact views. Inspect the PNGs, update the same visual ID,
   and repeat until the review checklist passes.
11. Record both screenshot-backed reviews and mark ready only at the reviewed
   revision.

## Non-negotiable composition rules

- Start with one focal object or comparison, not a dashboard of finished steps.
- Keep two to five focal objects visible in a beat; reveal detail as it becomes
  relevant.
- Preserve object identity across beats so the viewer can track what changed.
- Group content into a few sections that correspond to real systems or
  responsibilities. Do not make a panel for every noun.
- Keep sets and subsets separate when membership changes over time. Show the
  full population and the changing selected/frontier subset as distinct,
  aligned structures.
- Route cross-section interactions on short orthogonal paths or shared edge
  buses. Never run a connector through a record, label, or unrelated section.
- Use negative space as timing and hierarchy, not as room for extra labels.
- Put labels on or immediately beside the object they describe.
- Prefer short noun labels and one concise sentence of narration per beat.
- Use color to encode role or changed state. Keep inactive structure quiet.
- Make the final frame explain the mechanism without requiring motion.

## Non-negotiable motion rules

- Animate mechanism verbs: insert, split, route, copy, grow, select, reject,
  merge, or update.
- Let each beat establish a before state, perform one transformation, and settle
  into an inspectable after state.
- Move data or control along visible paths; do not substitute pulsing glows for
  causality.
- Animate the evidence lifecycle in causal order: create, enqueue, dispatch,
  execute, return, persist, read, decide, and update. Skip stages only when the
  omitted stage is irrelevant to the lesson.
- Use cuts or scene replacement when the explanatory scale changes materially.
- Keep motion deterministic and bounded. Never loop infinitely.
- Avoid simultaneous movement that asks the viewer to track more than two
  independent changes.

## Failure modes

Reject and revise a visual when it is:

- a static flowchart with opacity changes;
- a dense architecture poster whose every node exists from frame one;
- mostly containers, cards, chrome, legends, or numbered boxes;
- dependent on tiny type, hover, or transcript text for the primary claim;
- decorative motion without a state transition;
- a close copy of any cited source rather than a new composition for the user's
  mechanism; or
- illegible at either required capture viewport.

Use no arbitrary JavaScript, HTML, remote assets, or live `stream` binding.

## Reference reconstruction audits

For internal skill validation, reproduce a reference composition closely enough
to test primitives, hierarchy, pacing, and poster state. Label the output as a
study and keep it out of production task assets. Production visuals must apply
the learned grammar to an original composition for the requested mechanism.
