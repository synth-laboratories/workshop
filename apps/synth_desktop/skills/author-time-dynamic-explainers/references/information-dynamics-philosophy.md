# Information presentation and dynamics philosophy

This is the information-architecture layer of a time-dynamic technical
explainer. It governs what receives space, what stays persistent, how sections
interact, and what changes when time advances. Use `visual-grammar.md` for the
surface style.

## Begin with the viewer's missing mental model

Write two sentences before drawing:

1. What does the viewer already know?
2. What causal relationship should the viewer be able to explain after watching?

The second sentence is the visual's claim. Every persistent object, section,
and beat must help the viewer reconstruct that claim without the transcript.

## Organize by meaningful systems

- Divide the canvas into three to five large sections based on real ownership,
  responsibility, or state boundaries. Examples include a shared workspace, a
  queue, a worker service, a candidate population, and a selected subset.
- Give each section one job. Its heading should name the system or collection,
  not a vague topic such as `status` or `details`.
- Choose one dominant mechanism. Give it the most space and use the surrounding
  sections to show its inputs, outputs, and consequences.
- Use a dashed boundary only for a true logical scope such as a workspace,
  region, transaction, or service. Repeated records do not each need a card.
- Preserve a stable reading order. Prefer a top-to-bottom or left-to-right
  causal sweep over a symmetrical poster that leaves the entry point unclear.

## Show the actual nouns

- Represent data with recognizable classical objects: document records,
  candidate rows, seed cells, queue slots, workers, endpoint labels, traces,
  and charts.
- Name concrete identities whenever identity matters: candidate and generation,
  seed or row, worker slot, trace generation, endpoint, and returned value.
- Show multiplicity through repeated objects. A queue should contain jobs; a
  worker service should contain concurrent slots; a population should contain
  rows.
- Show missing as missing. Use an em dash or an explicit unknown state; never
  coerce unavailable evidence into zero, an empty bar, or a completed state.

## Separate population, subset, and selection

When a collection has a changing subset, do not force both meanings into one
rail.

- Keep the full population visible as the durable history.
- Show the current subset in a separate aligned area with its fraction of the
  full population.
- Show selection as a temporary role on a subset member, not as a new identity.
- When membership changes, move or restyle the member in the subset while
  leaving its population record intact.
- If one or two subset members inspire a new object, mirror those identities
  into compact input slots near the consumer. Avoid diagram-spanning parent
  wires.

## Make interactions cross boundaries visibly

Every important cross-section interaction should answer three questions:

1. What object crosses the boundary?
2. Which system sends it and which system receives it?
3. What durable state changes after it arrives?

Use short orthogonal connectors for local handoffs and a shared edge bus for
many returns to the same destination. Attach labels to the path they describe.
Never route a line through a record, text label, chart, or unrelated section.
If routes would cross, change the section layout, mirror a compact identity, or
consolidate returns before adding another connector.

## Animate the evidence lifecycle

Use time to reveal causality, not to decorate a finished diagram. A strong
general lifecycle is:

1. A producer creates a concrete object.
2. The object expands into work and enters a queue.
3. Workers pull from the head and execute concurrently.
4. Results return as concrete evidence.
5. Evidence becomes durable shared state.
6. A decision-maker reads that state together with selected prior objects.
7. A new object is emitted.
8. A complete evidence set crosses a synchronization barrier.
9. Population, subset, or summary histories update in place.

Do not update a downstream decision before its required evidence is complete.
Make the synchronization barrier visible through completion of rows, tokens,
or records rather than a caption alone.

## Preserve identity while state changes

- Keep persistent systems and records at stable positions across beats.
- Move copies only when the viewer must see a handoff; preserve the source
  identity until the transfer is understood.
- Use stable semantic color: active work, selected input, accepted/healthy,
  rejected/failed, and inactive structure retain the same meanings throughout.
- Prefer a settled before-state, one transformation, and an inspectable
  after-state. Avoid more than two independent simultaneous motions.

## Pair mechanism with measurement

Use a chart only when it explains a consequence of the mechanism.

- Place compact histories after the system state they summarize.
- Use shared axes and stable series identities.
- For optimization, distinguish exploration (the union of successes ever
  achieved) from exploitation (the best single object so far).
- Make monotonic quantities visually monotonic. Do not use decorative progress
  bars when the shape over time matters.
- Keep exact evidence in the records or matrix and use the chart for the trend;
  do not make the chart carry both jobs.

## Show, then label

Prefer visible state transitions over prose:

- records entering a workspace rather than `results saved`;
- trace items accumulating rather than `agent is working`;
- queue slots filling and draining rather than `rollouts running`;
- reward cells returning rather than `candidate scored`;
- frontier membership changing rather than `candidate dominated`.

Use captions to name the interpretation, not to substitute for the mechanism.

## Review the whole temporal composition

Review at least four representative states: setup, work in flight, decision or
subset update, and settled result. A poster alone cannot validate queue growth,
dispatch, return paths, synchronization, or reuse of prior state.

Reject the visual if:

- the large sections do not correspond to real systems;
- the dominant mechanism is visually secondary;
- interactions are described but not depicted;
- connectors overlap labels or unrelated objects;
- the same record changes identity merely to simplify a transition;
- missing evidence looks like zero or completion;
- time can advance without a meaningful state change; or
- the final state cannot be inspected without replay.
