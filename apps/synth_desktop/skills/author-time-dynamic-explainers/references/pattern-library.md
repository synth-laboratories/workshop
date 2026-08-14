# Pattern library

Use these patterns to reproduce the observed visual grammar closely while
creating an original scene for the mechanism at hand.

## Object-first canvas

- Remove in-canvas navigation, cards, title chrome, legends, and metric panels.
- Begin with a nearly empty off-white (`#f7f6f2`) or near-black (`#101112`)
  canvas.
- Place the first meaningful object near the visual center with ample space for
  the transformation that follows.
- Add a short caption outside the mechanism rather than a paragraph inside it.

## Repeated infrastructure topology

Use for shards, replicas, workers, candidates, or trials:

- Define one crisp primitive and duplicate it at even intervals.
- Build hierarchy row by row: source unit, partitioned units, then shared lower
  infrastructure.
- Use thin dotted vertical stems for containment or ownership and solid paths
  for active data movement.
- Keep inactive outlines blue-gray; activate one row with a restrained mustard,
  green, or red accent.
- Reveal multiplicity through repeated placement, not a label saying “many.”

## Dense data plane with sparse control plane

Use when many workers, shards, candidates, or examples sit beneath a small
coordinating layer:

- Establish scale with a dense, evenly spaced matrix of quiet repeated units.
- Place only a few bright control objects above it: clients, routers,
  selectors, or optimizers.
- Preserve the matrix as a stable coordinate system while one or two particles
  travel from control plane to selected units.
- Reveal routes only when active; avoid a permanent web of connectors.
- Use particle fan-out to demonstrate distribution and particle convergence to
  demonstrate aggregation.

## Parallel lifecycle lanes

Use for sharded backups, parallel trials, evaluation arms, or optimizer
candidates:

- Give every lane the same spatial grammar and advance them in lockstep.
- Stage the lifecycle left-to-right or top-to-bottom: provision, restore,
  catch-up, freeze, persist, retire.
- Keep production/source objects stable and animate work onto temporary objects
  so operational isolation is visible.
- Mark the synchronization barrier with one shared time line or freeze marker.
- Collapse or remove temporary objects after completion so lifecycle ownership
  is unambiguous.

## Direct-manipulation tree

Use for search trees, evolutionary lineages, and candidate ancestry:

- Keep parent and child positions stable while values change.
- Draw values inside small rectangular records rather than separate callouts.
- Place the incoming value or candidate above the structure, then animate it
  into position.
- When capacity is exceeded, visibly split or branch the affected object and
  settle the resulting geometry before continuing.
- Use dark, thin, gently curved connectors on a light field.

## Nested scale comparison

Use when relative quantity or capacity is the lesson:

- Encode magnitude with nested area or length on a shared origin.
- Put the object name and value directly inside its shape.
- Introduce comparisons one at a time and preserve earlier outlines.
- Leave the unused canvas empty so the size relationship dominates.
- Avoid axes, KPI cards, or a separate legend unless exact scale demands them.

## Routed regional system

Use for availability zones, evaluator splits, modules, or feedback paths:

- Delimit logical regions with thin dashed boundaries.
- Use a few small semantic object icons or labeled primitives inside each region.
- Animate one packet, record, prompt, or control token along an explicit path.
- Keep non-participating regions quiet until the route reaches them.
- Use color to distinguish role, not decoration.

## Closely matched pacing

- Start from a simple stable state.
- Add one object or one layer every one to three seconds.
- Hold the changed state briefly before the next transformation.
- Build complexity cumulatively for topology and scale explanations.
- Use a clean scene cut when changing explanatory scale.
- End on the most informative settled state, then stop or replay explicitly.

## Designer-refinement comparison

Use this pattern when auditing or demonstrating the visual pipeline itself:

- Stack `before`, static design reference, and `after` vertically on one
  uninterrupted near-black canvas.
- Preserve the exact object topology across all three panels so polish is the
  only changing variable.
- Use small downward arrows between panels rather than cards or prose.
- In the rough state, use thin solid connectors, outline clients, and one warm
  moving request particle.
- In the static design state, strengthen fills and spacing while holding the
  same topology. Keep this panel static.
- In the final state, quiet inactive objects and connectors, promote the focal
  server with semantic green, and animate fewer, subtler traffic particles in
  the same accent.
- Animate the rough `before` and implemented `after` panels. Keep the Figma
  panel static: it is the design source between two executions, not a third
  execution.

## GEPA mapping

For VisualsBench task `001`, use an evolutionary lineage rather than a circular
flowchart:

1. Show one base candidate prompt as a small record/tree root.
2. Reveal per-instance score markers around the candidate population.
3. Select one Pareto winner by moving it into the active work area.
4. Route a small minibatch through it; let textual feedback accumulate beside
   the exact module prompt that produced the trace.
5. Transform that prompt record into a revised child candidate.
6. Compare parent and child; either dissolve the child or attach it to the
   lineage and update its per-instance wins.
7. Repeat once with a different winner so the value of population diversity is
   visible, then settle on the best-average final candidate when budget reaches
   zero.

Implementation-specific gates observed in Workshop's optimizer projection:

- Establish the seed with `seed_full_train` evaluation.
- Select a Pareto member as parent; preserve its identity in the lineage.
- Evaluate `parent_minibatch_reference` and `candidate_minibatch` on the same
  example IDs so the comparison is visually paired.
- Reflect on rollout traces and failed examples, then edit one materialized
  prompt/program module to create a child candidate.
- Reject at the minibatch gate unless the proposal strictly improves.
- Run `candidate_full_train` only after the minibatch gate; accepted candidates
  become incumbents and update the frontier.
- Treat `heldout` as terminal measurement, not another selection signal.
- Render missing rewards as missing (`—`), never as zero.

For a population/frontier explainer, keep these relationships simultaneously
legible:

- Show every candidate in the population and outline the Pareto frontier as a
  changing subset; do not hide dominated candidates.
- When the canvas is dense, separate the persistent candidate population and
  the current Pareto frontier into two aligned structures. Animate promotion
  into the frontier and removal from that subset while keeping dominated
  candidates visible in the population. Do not run parent, child, reward, or
  trace connectors through candidate records.
- Encode the Pareto vector as per-seed reward cells. Mark cells on which each
  candidate is currently best; mean reward is secondary context, not an axis.
- Render those cells as a candidate-pool matrix: candidates are rows, Pareto
  seeds are columns, returned rewards are explicit, and missing rollouts remain
  `—`. Separately show the frontier subset and its fraction of the full pool
  (count and percentage).
- Show the proposer reading returned traces/results and appending a new child to
  an explicit evaluation queue.
- Route one or two current frontier members into the proposer as inspiration;
  preserve their identities so the child visibly descends from selected
  examples rather than appearing from nowhere.
- Animate those one or two chosen frontier records into the Codex proposer
  coding agent. Never choose an in-flight, incomplete, or non-frontier row as a
  parent merely because it is visually nearby.
- In dense compositions, mirror the selected identities into compact proposer
  input slots and animate the short final handoff. Avoid permanent
  diagram-spanning parent wires. Consolidate container returns into one edge
  evidence bus when separate reward and trace paths would cross other objects.
- Expand the rollout queue when proposals arrive. Show one rollout slot per
  candidate × seed and allow multiple candidates to be in flight.
- For Banking77, depict the queue as a real FIFO of concrete
  `candidate × train:{seed}` jobs. A candidate expands into row-specific jobs;
  container workers pull from the head, invoke `POST /rollout` with the
  candidate overlay and concrete dataset row, and return reward, usage, and
  trace evidence. Show the attached `stream_id` and declared
  `/reward?rollout_id=…` relationship without inventing a parallel scoring
  path. The pinned Banking77 recipe launches one container service with
  `BANKING77_POLICY_CONCURRENCY=4`; render that as four simultaneous rollout
  slots inside the service, not as four unrelated containers.
- Label the concrete Banking77 routes on the flow. The service supports
  `POST /rollouts/prepare`, blocking or async `POST /rollout` (with
  `/rollouts` as an alias), `GET /rollouts/{id}` and `/state`, cursor-based
  `GET /rollouts/{id}/events?after=n`, and
  `GET /reward?rollout_id={id}`. The reward endpoint returns `reward: null`
  while running, a numeric value when scored, and null with `status: absent`
  when terminal scoring evidence is missing.
- Treat the complete per-seed reward vector as a synchronization barrier. Do
  not update frontier membership while any rollout for that candidate remains
  missing.
- Render this as a classical technical diagram: a clock/ticker, FIFO rail,
  document-shaped candidate records, rollout tokens, worker/server primitives,
  return documents, and a frontier rail. Avoid dashboard cards, giant status
  words, novelty fonts, toy-like pills, and strike-through X marks; use precise
  rectangular records, clean sans-serif typography, position, and muted styling
  to show a candidate leaving the frontier.
- Draw the optimizer and container/evaluator as separate bounded systems. Move
  candidates outward for rollout execution and trace/reward evidence back.
- When a new candidate Pareto-dominates a frontier member, visibly remove the
  old member from the frontier while retaining it in the population.
- Track two monotonic histories in a compact line chart: the percentage of
  seeds ever achieved and the percentage solved by the best single candidate
  yet. Usually grow the union first, then show a later candidate consolidating
  that coverage. Never replace these histories with decorative progress bars.
- Preserve candidate generations and the durable Codex proposer history. Show
  one `proposer_workspaces/generation_NNN` record per generation, streamed
  proposer deltas while active, then a sealed Trace V5 sequence (input,
  thinking, tool, artifact/output) linked to the candidate or candidates it
  produced. Label these records explicitly as `CODEX TRACE V5`; do not leave
  the viewer to infer that identity from generic `streaming` or `sealed` text.
- Show the shared run workspace as the durable handoff between evaluation and
  proposal. Persist rollout events, rewards, usage, and trace records there;
  then route those records into the Codex proposer coding agent together with
  selected frontier candidates. Do not imply that the proposer reads directly
  from an ephemeral worker response.
- Make that shared workspace the dominant visual mechanism, not a small label.
  Animate rollout records entering it, the Codex coding agent reading those
  records and selected parents, Trace V5 items accumulating during the turn,
  and the emitted child leaving the workspace for the FIFO rollout queue.
  Prefer moving records and stateful objects over captions that merely narrate
  those relationships.
