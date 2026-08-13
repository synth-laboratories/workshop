# Visuals issues: live GEPA and Trace V5

Date: 2026-08-13
Status: implementation backlog from live visual QA
Reviewed run: `healthbench_groq_gepa_aug13i`
Reviewed surface: `gepa-qa.html` at desktop, 680 px, and 480 px widths

This note records what the optimizer visual currently communicates well, where it is misleading or difficult to use, and the acceptance bar for putting it in Workshop's right panel during a live run. It is deliberately evidence-driven: the issues below were found against the durable HealthBench GEPA journal, not invented mock data.

## What already works

- The visual reconstructs a useful live workspace from durable optimizer events: run status, stages, candidates, rollouts, proposer activity, hill-climb context, GEPA's per-example Pareto frontier, and Trace V5 artifacts.
- The proposer trace shows semantic input, visible reasoning summaries, tool calls and results, file changes, and final output. Long bodies are collapsed, searchable, and switchable between focused and full views.
- Generations have persistent text labels and colors, and generation tabs plus a scrubber make individual proposer traces navigable.
- Terminal state overrides a stale run record. The reviewed run correctly says it terminated after its rolling failure rate exceeded the tolerance, and it does not fabricate a heldout score or cost.
- Candidate links open the inspector. At 680 px and 480 px, the page had no horizontal document overflow.
- The Pareto section uses GEPA's meaningful object—non-dominated per-example reward vectors—rather than treating a generic aggregate-score scatter plot as “the Pareto frontier.”

These are a strong base. The remaining problems are mostly about decision authority, lineage, terminal accounting, hierarchy, and scale.

## P0 — truthfulness and algorithm semantics

### VIS-GEPA-001: Rejected-candidate comparisons use the wrong scores

**Observed**

The inspector can say a candidate was rejected despite a positive delta against its parent. For example, Gen 0 proposal 2 is presented approximately as `0.41 → 0.62, +0.21`, followed by “requires strict improvement.” That explanation contradicts the decision.

**Cause**

The projection prefers `candidate_minibatch_reward` and `parent_minibatch_reward` when summarizing a rejection. A full-train rejection is authoritative over a different comparison: challenger selection score versus the incumbent selection score at decision time. The durable event for this candidate reports approximately:

- challenger selection score: `0.494`
- incumbent selection score: `0.576`
- selection delta: `-0.081`
- comparison: `incumbent_dominates`

The original parent and the decision-time incumbent are not necessarily the same candidate.

**Required behavior**

- Treat the authoritative decision event as the source of truth.
- For a full-train gate, show challenger and decision-time incumbent selection scores, objective name, true delta, incumbent ID, and comparison rationale.
- For a minibatch gate, show minibatch evidence and label it explicitly as provisional.
- Never recompute an authoritative decision from whichever score fields happen to be present first.

**Acceptance**

- The reviewed Gen 0 proposal 2 reads approximately `0.494 vs incumbent 0.576 · Δ -0.081 · incumbent dominates`.
- Gen 0 proposal 3 likewise uses its true full-train comparison, not its parent minibatch delta.
- Fixture tests cover parent != incumbent, minibatch rejection, full-train rejection, missing scores, and alternative selection objectives.

### VIS-GEPA-002: The hill-climb line implies false incumbent transitions

**Observed**

The hill-climb chart connects every scored candidate in event order. In the reviewed run it draws a clean monotonic climb through sibling proposals that were later rejected. It therefore looks like four accepted optimization steps, when only one proposal became the incumbent.

**Required behavior**

Separate two views:

1. **Incumbent trajectory:** only seed and authoritative accepted/promoted incumbent transitions.
2. **Candidate scores:** all scored candidates as generation-colored points, without connecting them into the incumbent line.

Parent-child lineage may be shown with thin dashed links. A sibling evaluation must never become a segment of the accepted hill climb merely because it scored before another sibling.

**Acceptance**

- The orange/best line changes only on an authoritative incumbent transition.
- Rejected candidates remain visible as points but cannot move the best-so-far line.
- Hover/focus states identify generation, proposal, parent, gate, score, and decision.
- Tests shuffle event arrival order and produce the same trajectory.

### VIS-GEPA-003: Complete rejected candidates are omitted from the Pareto population

**Observed**

The Pareto section says some rejected candidates are “awaiting complete full-train vectors,” even though they completed 60/60 full-train rollouts. The chart only makes the authoritative frontier member easy to see and loses valuable dominated/rejected evidence.

**Required behavior**

- Use authoritative `frontier.updated` events for frontier membership.
- Plot every candidate with a complete comparable reward vector.
- Render frontier members prominently; render complete dominated/rejected candidates in subdued generation color or gray.
- Reserve “awaiting complete vector” for genuinely incomplete or incomparable candidates.
- Explain why a complete candidate is outside the frontier: dominated, rejected by gate, incomparable objective/schema, or awaiting a decision.

**Acceptance**

- All complete 60-example candidates in the reviewed run appear in the matrix or chart.
- Only authoritative members are called frontier members.
- A rejected but complete candidate is labeled `dominated/rejected`, never `awaiting vector`.
- Aggregate mean remains context, not a Pareto axis or substitute for the per-example vector.

### VIS-GEPA-004: Attempts, scored examples, failures, and pending work are conflated

**Observed**

The header reports `332 / 710` rollouts while the rollout browser reports `306 / 306`. Evidence integrity reports Required 300, Scored 296, Failed 1, Pending 3, alongside 9 exhausted rollout attempts. All values may be internally derivable, but the nouns and denominators are not explicit. On a terminated run, `Pending` is especially misleading.

**Required behavior**

Use separate, named counters:

- examples required
- examples scored
- rollout attempts launched
- attempts succeeded
- attempts retrying
- attempts exhausted
- examples aborted by termination
- examples still pending only while the run is active

After terminal failure, unresolved required work becomes `aborted` or `not run`, not `pending`.

**Acceptance**

- Every numerator and denominator has a visible noun or tooltip.
- The header and rollout browser reconcile from the same projection.
- A terminal fixture has zero pending work and explicitly reports aborted/not-run work.
- Retries increase attempt counts without increasing required-example counts.

### VIS-GEPA-005: “Passes” and “Failures” infer semantics from reward sign

**Observed**

The rollout browser treats `reward > 0` as a pass and `reward <= 0` as a failure. That is not task- or optimizer-general. A zero or negative reward can be a valid scored outcome, while an infrastructure failure may have no reward at all.

**Required behavior**

- Prefer an evaluator-provided outcome (`pass`, `fail`, graded class, rubric result) when one exists.
- Otherwise use neutral filters: `positive reward`, `non-positive reward`, `unscored`, and `operational error`.
- Keep outcome quality separate from transport, provider, timeout, cancellation, and scoring failures.
- A null reward must carry a state: pending, aborted, failed, missing, or not applicable.

**Acceptance**

- Tests cover a valid negative reward, a zero reward, a null retry, a terminal aborted rollout, and an operational error.
- No UI label calls reward sign a pass/failure unless the evaluator contract defines that threshold.

### VIS-GEPA-006: Cost needs value and completeness, not an implicit zero convention

**Observed**

The UI correctly renders unknown cost as `unavailable`, but a numeric field alone cannot distinguish a genuine known `$0.00` from an upstream placeholder zero.

**Required behavior**

- Preserve unknown provider cost as `null` end to end.
- If a real zero is possible, pair the value with provenance/completeness such as `reported`, `estimated`, or `unavailable`.
- Show partial totals as partial, including which calls/providers are missing cost.

**Acceptance**

- Unknown cost never becomes `0.0` in a manifest, projection, export, or visual.
- Known zero and unknown render differently.
- Mixed-known/unknown totals are labeled `partial`, not exact.

## P1 — status, navigation, and information hierarchy

### VIS-GEPA-007A: The terminal header repeats status and clips the QA receipt line

**Observed**

At the desktop QA width, the first red receipt line is vertically clipped and the same terminal fact is repeated immediately below as a `TERMINATED` badge, `Run terminated` headline, and `Job: Terminated` metric. This spends the most valuable part of the viewport repeating state while truncating the durable-event identity.

**Required behavior**

- Render the QA/source receipt as a complete, single-line provenance strip or move it behind a disclosure; it must never be cropped by the sticky header.
- Use one dominant terminal status treatment. Remove `Job` from the metric grid when it duplicates the banner.
- Keep the reason, threshold, timestamp, run ID, and durable sequence/event count together as the useful terminal receipt.
- Preserve model, policy, score, cost, and rollout metrics below that receipt without repeating the terminal label.

**Acceptance**

- At 1440, 1024, 768, 680, 480, and 390 px widths, every visible line has sufficient height and remains legible at 100% and 200% zoom.
- `terminated` appears once in the primary header, excluding an intentionally opened technical-details disclosure.
- The full run identifier and durable event count are available without overlapping or clipping.

### VIS-GEPA-007: The terminal state needs a recovery-oriented presentation

**Observed**

The visual now identifies the circuit-breaker termination correctly, but status is repeated in several places while the useful next actions are absent. The stage timeline leaves `Complete` looking pending, and the top-level counters can retain an active-work flavor.

**Required behavior**

- Use one dominant status banner with terminal reason, threshold evidence, timestamp, last durable sequence, and stage where termination occurred.
- Mark the final stage `terminated`/`failed`, not pending.
- Stop active throughput indicators and convert queued/running work to their terminal dispositions.
- Offer evidence actions: view affected rollouts, copy/download termination receipt, and resume/retry only when the backend advertises durable support.

**Acceptance**

- A terminal event overrides stale run metadata everywhere.
- No worker count, queue, throughput, stage, or rollout remains visually active after termination.
- Scrubbing to before the terminal event shows the historical running state; scrubbing past it shows the terminal state.

### VIS-GEPA-008: The sticky header consumes most of a narrow viewport

**Observed**

At 480 px the full sticky header occupies roughly 220 px and obscures the generation controls and trace context during scrolling. There is no horizontal page overflow, but the usable vertical viewport becomes too small.

**Required behavior**

- Collapse the sticky header after scroll or below a width threshold.
- Keep only status, current stage, best score, and a disclosure control in compact mode.
- Move secondary metrics into an expandable tray.
- Preserve the selected candidate/generation context when the header changes size.

**Acceptance**

- At 390/480 px, the compact sticky region consumes no more than about 96 px.
- Generation tabs and the first trace item can be viewed together.
- Test 1440, 1024, 768, 680, 480, and 390 px widths with no horizontal page overflow or covered controls.

### VIS-GEPA-009: Candidate ordering hides the actual search structure

**Observed**

Candidate cards are ordered by latest event sequence, producing a surprising order such as Seed, two Gen 0 siblings, Gen 1 siblings, then the accepted Gen 0 proposal. Live updates can move cards as new events arrive.

**Required behavior**

- Default to stable generation/proposal order or a lineage tree.
- Provide explicit sort choices: generation, score, decision time, and status.
- Do not reorder an open list merely because a candidate receives another event.
- Keep the selected candidate pinned and visible during live updates.

**Acceptance**

- Replaying the same events in another arrival order yields the same default ordering.
- Seed is first, then generations, then proposal index unless the user chooses another sort.
- Selection survives live event insertion.

### VIS-GEPA-010: Generation identity should organize more than color

**Observed**

Generation colors help, and labels prevent a color-only encoding, but candidate points and cards still read as a flat set. The hill-climb especially lacks generation boundaries.

**Required behavior**

- Group candidate cards by generation with collapsible generation headers.
- Add generation bands or lanes to candidate-score views.
- Reuse the generation palette in candidate cards, Pareto rows, rollout groups, trace tabs, and candidate links.
- Reserve the Workshop orange accent for incumbent/frontier/selection meaning rather than using it as another generation color.

**Acceptance**

- A user can identify a candidate's generation and parent without relying on hue.
- Palette contrast passes in light/dark themes and common color-vision simulations.

### VIS-GEPA-011: Proposer traces need durable per-generation navigation state

**Observed**

Tabs and the slider now select one generation trace at a time, which is a substantial improvement. However, remounting a generation resets search and focused/full choices. Long shell commands and paths can still dominate a narrow trace card.

**Required behavior**

- Persist search query, focus/full mode, expansion state, and scroll position per generation during the session.
- Support keyboard tab navigation and direct generation search/jump.
- Add semantic filters for input, thinking, tools, file changes, and output.
- Keep command bodies collapsed by default with copy and expand actions; never render an unstructured content dump.
- Clearly label visible model reasoning summaries; never imply hidden chain-of-thought is available.

**Acceptance**

- Switching generations and returning restores the prior trace view.
- Arrow keys navigate generation tabs according to WAI-ARIA tab behavior.
- A 100-tool-call trace remains usable without expanding the page into an enormous text wall.

### VIS-GEPA-012: The visual needs a clearer top-to-bottom story

The recommended hierarchy is:

```text
Run status + recovery
  └─ Stage timeline + compact operational metrics
      └─ Search overview
          ├─ Authoritative incumbent trajectory
          └─ GEPA per-example Pareto frontier
              └─ Generation-grouped candidates + decision inspector
                  └─ Rollouts and evaluator evidence
                      └─ Proposer Trace V5
```

This makes the common questions answerable in order: Is it healthy? Is it improving? Why did this candidate win or lose? What examples support that? What did the proposer actually do?

## P0/P1 — missing GEPA search observability

The first review concentrated on incorrect or confusing representations already on the page. A second review started from GEPA's actual optimization loop:

```text
declare artifact + objective + budget
             │
             ▼
evaluate seed on the validation/train frontier set
             │
             ▼
select a parent from the instance-wise Pareto frontier
             │
             ▼
sample reflection minibatch + collect trajectories and ASI
             │
             ▼
reflect on wins/failures → mutate, or merge frontier parents
             │
             ▼
paired minibatch gate → full comparable evaluation
             │
             ▼
accept/reject → update frontier and incumbent → repeat
             │
             ▼
heldout evaluation + final selection, or explicit stop condition
```

GEPA's useful signal is not only the scalar reward. It is the causal chain from task evidence, through reflection and a concrete edit, to paired evaluation and frontier change. The visual currently exposes fragments of that chain, but not enough to tell whether the optimizer is learning efficiently or simply spending rollouts.

The official implementation also allows several strategies that the visual must not silently flatten into one story: reflective mutation versus system-aware merge; Pareto, current-best, or epsilon-greedy parent selection; instance, objective, hybrid, or Cartesian frontiers; different batch samplers; multiple candidate admission strategies; component selection; evaluation caching; and budget or callback-based stopping.

### VIS-GEPA-017: The optimization contract is missing

**Question the user cannot answer**

What exactly is being optimized, by which models and evaluator, over which data, under which algorithm and budget?

**Required behavior**

Add a collapsible **Run contract** summary containing:

- mutable artifact type and components/modules
- task model/policy and proposer/reflection model, with provider and immutable pin where available
- evaluator, rubric/objective names, direction, aggregation, score range, and reward semantics
- train/reflection/frontier/heldout split sizes and digests
- seed candidate and program/container versions
- parent-selection, sampling, proposal-admission, component-selection, merge, frontier, and caching strategies
- proposal count, minibatch size, acceptance margin, concurrency limits, and stop conditions
- recipe/config digest and source revision

**Available now**

The reviewed journal declares its objective and frontier type. The recipe declares models, splits, three generations, three proposals per generation, minibatch size 12, rollout/cost limits, per-example frontier, candidate concurrency, and adaptive rollout concurrency. The proposer workspace contains program and task contracts. Most of this is currently hidden.

**Acceptance**

- The visual never labels an arbitrary implementation “GEPA” without exposing the active strategies.
- Two runs with different frontier or sampling strategies are visibly distinguishable before inspecting raw JSON.
- Secrets and private filesystem paths remain redacted while stable digests and pins remain visible.

### VIS-GEPA-018: There is no budget burn-down or definition of remaining search

**Question the user cannot answer**

How much search has actually happened, what resource will stop it, and is there enough budget left for another useful generation?

**Required behavior**

Show a budget panel with separate progress bars for:

- metric/rollout calls spent, reserved, remaining, and maximum
- candidates proposed/admitted versus maximum
- generations completed versus maximum
- train and heldout rollout sub-budgets
- cost known/partial/unknown versus hard ceiling
- elapsed time and configured time limit
- nearest predicted limit, ETA range, and forecast confidence

Also show the expected cost of the next operation: one proposal, its minibatch gate, a full evaluation, and heldout. “47% of rollouts spent” is insufficient if the remaining budget cannot fund another complete candidate.

**Available now**

`optimizer.limit.estimate_updated` already carries spent, reserved, remaining, hard/soft status, prediction intervals, sample count, and nearest limiting resource. The reviewed run had enough information to say which of generations, rollouts, or dollars was forecast to bind first, but the visual did not show it.

**Acceptance**

- A user can tell whether the search stopped because of success, budget, time, plateau, manual cancellation, or infrastructure failure.
- Forecast confidence and incomplete cost data are never presented as exact.
- The UI warns when remaining budget cannot complete the next configured gate.

### VIS-GEPA-019: The iteration pipeline is not visible as a causal unit

**Question the user cannot answer**

Which parent and examples caused this generation, what did reflection conclude, what changed, and did the change survive each gate?

**Required behavior**

Represent each iteration as a compact, expandable pipeline:

```text
parent selected
  → reflection batch sampled
  → traces + ASI assembled
  → reflection thesis
  → mutation/merge proposals
  → paired minibatch results
  → full evaluation
  → decision + frontier delta
```

Each stage should link to its evidence rather than duplicating raw data. Multiple proposals from one reflection call should remain siblings within the iteration. Parallel evaluations should be shown as parallel work, not separate generations.

**Acceptance**

- Generation, iteration, proposer call, candidate, and rollout are distinct nouns.
- A user can start from an accepted candidate and navigate backward to the reflection evidence and forward to the frontier update.
- Missing stages remain explicitly missing; the projector does not infer an event merely from later success.

### VIS-GEPA-020: Parent selection and frontier contribution are unexplained

**Question the user cannot answer**

Why did GEPA choose this parent instead of the aggregate best candidate?

GEPA's instance-wise selection preserves candidates that are best on particular examples. In the classic algorithm, a frontier candidate's sampling weight is related to how often it appears among per-example best sets. A candidate with a lower mean may therefore be a valuable parent because it uniquely solves hard examples.

**Required behavior**

- On every iteration, show parent-selection strategy and the authoritative selection reason.
- For a Pareto-selected parent, show unique wins, shared wins/ties, examples covered, contribution frequency/weight, and sampling probability if emitted.
- Show how frontier membership changed after the iteration: added, removed, retained, and why.
- If the runtime does not emit selection probability or rationale, label it unavailable rather than reverse-engineering a probability.

**Acceptance**

- “Selected because aggregate best” and “sampled because it owns rare frontier examples” render as different explanations.
- Frontier contribution uses the declared coverage semantics, not a hardcoded `reward > 0` rule.

### VIS-GEPA-021: Actionable Side Information is buried inside trace payloads

**Question the user cannot answer**

What did the optimizer learn from the failures?

GEPA's differentiator is Actionable Side Information (ASI): textual diagnostic feedback such as evaluator comments, error messages, tool traces, profiler output, or rendered evidence. A scalar reward chart alone misses the mechanism responsible for sample efficiency.

**Required behavior**

Add a structured **Reflection evidence** view:

- sampled wins and failures, including why each example was sampled
- evaluator feedback/rubric dimensions and system trajectory references
- clustered failure modes with prevalence and representative examples
- guard wins/regressions that the next edit must preserve
- reflection diagnosis, causal hypothesis, and uncertainty
- explicit evidence citations from the diagnosis back to rollout/Trace V5 items

**Available now**

The proposer workspace already contains failure summaries, repair hints, example rows, reflective frames, reflector input, task information, and top failures. The visual currently exposes the proposer tool trace but not a readable synthesis of these artifacts.

**Acceptance**

- Every reflection claim can be traced to concrete evaluation evidence.
- The UI distinguishes evaluator-produced ASI from proposer interpretation.
- Missing or low-information ASI triggers a search-quality warning.

### VIS-GEPA-022: Prompt mutations are not explained as testable hypotheses

**Question the user cannot answer**

What changed, why should it help, and did it fix the intended failures without causing regressions?

**Required behavior**

For each proposal, show:

- word/section diff against its parent, grouped by mutable component
- proposal critique, rationale, intended failure clusters, and expected effect
- mutation versus merge provenance
- novelty/similarity relative to the parent and sibling proposals
- paired minibatch wins, losses, ties, and regressions on the exact sampled examples
- after full evaluation, whether the predicted effect materialized by cluster

Do not make a long prompt dump the primary view. Present semantic edit cards first, with the complete prompt behind disclosure/export.

**Acceptance**

- A user can state the candidate's hypothesis without reading the entire prompt.
- Sibling proposals reveal distinct strategies; near-duplicates are flagged.
- The visual links a claimed fix to before/after evidence on the affected examples.

### VIS-GEPA-023: Search diversity, merge, and exploration/exploitation are invisible

**Question the user cannot answer**

Is GEPA exploring genuinely different solutions, repeatedly polishing one prompt, or combining complementary frontier candidates?

**Required behavior**

- Show the candidate lineage graph with mutation and merge edges.
- Identify strategy per iteration: reflective mutation or system-aware merge.
- For merge candidates, show both parents and component-level provenance.
- Summarize candidate novelty, duplicate rejection, frontier diversity, parent-selection concentration, and generations since a new frontier contribution.
- Show configured versus observed exploration/exploitation behavior.

**Protocol gap**

The reviewed run emits `source=reflector:parent_variation` and parent IDs, but a first-class strategy event should carry selection strategy, sampling weight, mutation/merge type, selected components, merge parents, and component provenance. These must not be guessed from prompt similarity.

**Acceptance**

- A lineage graph can represent one parent, two merge parents, and multi-component candidates.
- Search-collapse alerts are based on declared thresholds and emitted novelty data, not aesthetic judgment.

### VIS-GEPA-024: Mean reward lacks uncertainty and paired evidence

**Question the user cannot answer**

Is the apparent improvement credible, or sampling noise from a small/noisy minibatch or evaluator?

**Required behavior**

- Show score distributions, sample counts, missingness, and uncertainty intervals where statistically valid.
- Prefer paired per-example deltas for minibatch comparisons.
- Show wins/ties/losses and effect-size distribution, not only two means.
- Record evaluator identity/version, stochastic settings, repeats, and aggregation.
- Mark partial cohorts and prevent visual comparison of non-comparable splits/objective schemas.

For judge-based or graded HealthBench evaluation, the UI should expose rubric-dimension evidence if available and distinguish task-model variance from evaluator variance. A narrow numerical improvement without adequate evidence should be labeled uncertain, not successful.

**Acceptance**

- A minibatch candidate cannot appear equivalent to a complete full-train candidate solely because both have a mean.
- Comparisons state cohort, objective, aggregation, `n`, and missingness.
- Confidence intervals are omitted with an explanation when assumptions or data are insufficient.

### VIS-GEPA-025: Progress and success are not defined

**Question the user cannot answer**

Is the optimizer succeeding?

Success needs several independent lenses:

1. **Operational validity:** evaluations and proposer calls are completing without unacceptable data loss.
2. **Search progress:** accepted incumbent and/or authoritative frontier improves on comparable training evidence.
3. **Generalization:** heldout improves without unacceptable regression or leakage.
4. **Efficiency:** improvement is achieved per rollout, dollar, and minute.
5. **Diversity:** the frontier retains complementary candidates instead of collapsing prematurely.
6. **Interpretability:** accepted edits are supported by reflection evidence and observable before/after behavior.

**Required behavior**

Add a progress summary with:

- seed baseline, current incumbent, absolute and relative lift
- target threshold if configured
- best heldout and generalization gap only after legitimate heldout evaluation
- best-found-at rollout/cost/time and marginal improvement since
- accepted/proposed ratio and gate funnel
- generations/metric calls since last incumbent or frontier improvement
- frontier size and unique-example contribution over time
- status such as `improving`, `exploring`, `plateauing`, `budget constrained`, `operationally degraded`, or `generalization unknown`, derived from explicit rules

**Acceptance**

- A large train lift with no heldout is described as promising search evidence, not a successful final result.
- An abnormal termination cannot produce a green “successful” state merely because the last observed train score was high.
- Progress rules and thresholds are inspectable.

### VIS-GEPA-026: Heldout protocol and overfitting risk need first-class treatment

**Question the user cannot answer**

Was heldout protected from search, which candidate was evaluated, and does the improvement generalize?

**Required behavior**

- Declare the purpose and access policy for train/reflection/frontier/selection/heldout splits.
- Before heldout, show only count/digest and `sealed/not yet evaluated`; never leak examples or feedback into reflection.
- At heldout, show candidate selection rule, cohort, score, uncertainty, seed comparison, and generalization gap.
- Record every heldout access and warn on repeated adaptive peeking.
- Distinguish `not run`, `blocked`, `aborted`, `failed`, and a genuine measured score.

**Acceptance**

- The reviewed terminated run says `generalization unknown · heldout not run`.
- A fixture that accesses heldout before final selection raises an integrity warning.

### VIS-GEPA-027: Search pathologies and bottlenecks are not diagnosed

**Question the user cannot answer**

If progress stalls, is the problem the proposer, evaluation capacity, provider reliability, low-information feedback, duplicate candidates, sampling, or the optimization setup itself?

**Required behavior**

Add evidence-backed diagnostics for:

- low-information or missing ASI
- near-duplicate proposals and repeated failure hypotheses
- all candidates failing the same gate
- no frontier/incumbent improvement for a configured budget window
- frontier collapse or one parent dominating selection
- evaluator saturation, high variance, or missing rubric dimensions
- cache hit/miss rate and redundant evaluations
- queue depth, semaphore utilization, latency percentiles, rate limiting, retry/exhaustion causes, and adaptive concurrency changes
- proposer latency, tool failures, invalid manifests, and token/cost concentration

The UI should suggest the next inspection target, not automatically prescribe a configuration change without authority.

**Acceptance**

- Diagnostics are computed from durable events and include their evidence window.
- “No improvement” is distinct from “not enough comparable evaluations yet.”
- Operational degradation cannot masquerade as an algorithmic plateau.

### First-principles screen hierarchy

The earlier hierarchy should be expanded so the visual answers “what is GEPA doing?” before exposing individual event rows:

```text
Run outcome / health / recovery
  ├─ Progress: baseline → incumbent → heldout, with uncertainty
  ├─ Budget: rollouts, candidates, generations, cost, time, next-operation fit
  └─ Run contract: artifact, data, objective, models, strategies, versions

Search loop
  ├─ Iteration pipeline and gate funnel
  ├─ Incumbent trajectory (accepted transitions only)
  ├─ GEPA per-example frontier and contribution ownership
  └─ Lineage / mutation / merge / generation diversity

Why it changed
  ├─ Reflection evidence and failure clusters
  ├─ Candidate hypotheses and parent diffs
  ├─ Paired minibatch evidence
  └─ Full-train decision against decision-time incumbent

Raw evidence
  ├─ Rollouts + evaluator/rubric evidence
  └─ Proposer and policy Trace V5
```

## P2 — scale, accessibility, and replay resilience

### VIS-GEPA-013: Large runs need bounded rendering

The reviewed run is manageable, but the design must survive much larger searches.

**Required behavior**

- Virtualize or paginate large candidate, rollout, and trace lists.
- Keep summaries incremental rather than reprojecting every event on every render.
- For hundreds or thousands of Pareto dimensions, provide aggregation, zoom, and a searchable example matrix instead of an unreadable fixed-width wall.
- Surface dropped, delayed, duplicated, and resumed event counts as diagnostic state only when nonzero.

**Acceptance load fixture**

- 1,000 candidates
- 100,000 rollout events
- 100 proposer traces with 100 items each
- reconnect with duplicate replay and an event gap

The view should remain responsive, preserve selection, and converge idempotently on the same state as a clean replay.

### VIS-GEPA-014: Accessibility needs interaction-level coverage

**Required behavior**

- Keyboard selection for chart points, candidates, generation tabs, trace filters, and disclosure controls.
- Visible focus that is not obscured by the sticky header.
- Programmatic labels for scores, deltas, stage states, generation, and frontier membership.
- Status, decision, and generation must never be encoded by color alone.
- Respect reduced motion for live pulses and chart transitions.
- Announce meaningful live transitions without announcing every rollout event.

**Acceptance**

- Complete the core flow—open run, select candidate, inspect decision, filter rollouts, select generation, inspect tool call—using keyboard only.
- Screen-reader output distinguishes candidate outcome from operational failure.
- Automated contrast and role checks are supplemented by manual VoiceOver QA.

### VIS-GEPA-015: Replay and reconnect must preserve temporal truth

**Required behavior**

- Derive all panels from the same durable cursor/cutoff.
- Reconnect from the last acknowledged sequence and collapse exact duplicates idempotently.
- Do not show a trace artifact, score, frontier update, or terminal reason before its durable event exists at the selected cutoff.
- Detect gaps and show a recoverable degraded state rather than silently skipping data.
- Keep live-follow separate from manual historical scrubbing.

**Acceptance**

- Clean replay, reconnect replay with duplicates, and poll/SSE resume produce identical projections.
- Scrubbing before/after candidate decisions and termination changes every dependent panel consistently.
- A simulated gap blocks authoritative summaries until repaired or explicitly marked incomplete.

### VIS-GEPA-016: Exports should preserve the evidence behind the visual

**Required behavior**

- Candidate export includes prompt/config, lineage, authoritative decision, score provenance, rollout references, and frontier membership at the selected cutoff.
- Termination export includes run ID, durable event/sequence, threshold evidence, stage, outstanding work disposition, and resumability.
- “Copy decision evidence” should be distinct from “Copy prompt.”
- Exports must preserve nulls and completeness metadata.

## Non-negotiable truthfulness rules

```text
null                != 0
pending             != aborted
attempt failure     != scored task failure
high aggregate mean != Pareto membership
scored candidate    != accepted incumbent
parent candidate    != decision-time incumbent
terminal event      > stale run metadata
visible summary     != hidden chain-of-thought
```

When an authoritative optimizer event contains a decision, the visual presents that decision and its evidence. It does not reconstruct a friendlier explanation from secondary fields.

## Recommended implementation order

1. Fix authoritative rejection/acceptance score projection (VIS-GEPA-001).
2. Split incumbent trajectory from candidate scores (VIS-GEPA-002).
3. Correct complete-candidate Pareto classification (VIS-GEPA-003).
4. Normalize example, attempt, retry, abort, and terminal counters (VIS-GEPA-004/005/007).
5. Add the run contract, explicit success definition, and budget/next-operation burn-down (VIS-GEPA-017/018/025/026).
6. Build an iteration projection that joins parent selection → reflection evidence → proposal → gates → frontier delta (VIS-GEPA-019–022).
7. Add lineage, merge, diversity, paired uncertainty, and evidence-backed search diagnostics (VIS-GEPA-023/024/027).
8. Add compact responsive header behavior (VIS-GEPA-008).
9. Stabilize candidate ordering and generation grouping (VIS-GEPA-009/010).
10. Persist Trace V5 navigation state and add semantic filters (VIS-GEPA-011).
11. Add large-run, accessibility, replay, and export gates (VIS-GEPA-013–016).

## Acceptance suite to add

### Semantic fixtures

- Parent differs from decision-time incumbent.
- Two rejected siblings and one accepted sibling arrive out of order.
- Complete dominated vectors remain visible outside the authoritative Pareto set.
- A lower-mean candidate owns unique per-example frontier contributions and is selected as a parent.
- A mutation iteration and a two-parent merge preserve distinct lineage and component provenance.
- Reflection evidence contains evaluator ASI, proposer interpretation, a claimed failure cluster, and cited rollout traces.
- A proposal hypothesis improves its target cluster but regresses a guard-win cluster.
- Train means improve while paired uncertainty remains inconclusive.
- Retries/exhaustion do not alter the number of required examples.
- Known zero cost, unknown cost, and partial cost totals remain distinct.
- Valid negative reward is not an operational failure.

### Terminal fixture

- Circuit breaker fires mid-stage with queued, running, retrying, scored, and never-started examples.
- All counters reconcile; heldout is `not run`; pending becomes aborted; throughput stops.
- Termination receipt and cutoff replay are consistent.

### Responsive CUA matrix

- Widths: 1440, 1024, 768, 680, 480, 390 px.
- Inspect header collapse, chart readability, candidate grouping, inspector, rollout rows, generation tabs, trace tool calls, dialogs, and keyboard focus.
- Require no horizontal document overflow and no control hidden beneath sticky UI.

### Scale and resilience fixture

- 1,000 candidates, 100,000 rollout events, 100 generations/traces.
- Fixed-size rollout semaphore continuously drains its queue.
- Disconnect/reconnect with exact duplicates, delayed events, and a repaired gap.
- Projection is deterministic and the UI remains interactive.

## Definition of done

The GEPA visual is ready for first-class Workshop use when a user can watch it live in the right panel and accurately answer, without reading raw JSON:

1. Is the run healthy, active, completed, or terminated—and why?
2. What artifact, models, data splits, evaluator, strategies, and stop conditions define this run?
3. How much budget remains, what will bind first, and can the next operation finish?
4. Is the accepted incumbent actually improving over time, on comparable evidence and with what uncertainty?
5. Which candidates were explored in each generation, and how are they related by mutation or merge?
6. Why was this parent selected from the frontier?
7. What failure evidence and ASI did reflection use, and what concrete hypothesis did each proposal test?
8. Why was a candidate accepted or rejected, against which incumbent and objective?
9. Which candidates are on GEPA's authoritative per-example Pareto frontier, and which examples does each uniquely contribute?
10. Which rollouts and evaluator/rubric evidence support each decision, including regressions?
11. What input, visible reasoning summary, tool activity, file changes, and output occurred in each proposer call?
12. Has the result generalized to an untouched heldout split, or is generalization still unknown?
13. Are costs, missing data, retries, cached evaluations, and aborted work represented honestly?
14. If progress has stalled, is the bottleneck algorithmic, evaluative, operational, or budgetary?

The page should feel like an operational optimization workspace, not a decorated event log.

## Primary references

- [GEPA paper](https://arxiv.org/abs/2507.19457): reflective prompt evolution, trajectory feedback, and Pareto-based candidate selection.
- [Official GEPA repository](https://github.com/gepa-ai/gepa): select frontier candidate → execute minibatch → reflect → mutate → accept/update frontier; Actionable Side Information and merge support.
- [Official GEPA guide](https://gepa-ai.github.io/gepa/): executor/reflector/curator pipeline, instance-wise frontier behavior, adapters, and reflective evidence.
- [Official GEPA API](https://github.com/gepa-ai/gepa/blob/main/src/gepa/api.py): active strategies, frontier types, sampling, selection, merge, component selection, budgets, stop callbacks, resume, and evaluation caching.
