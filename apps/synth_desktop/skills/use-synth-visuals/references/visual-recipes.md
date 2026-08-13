# Visual recipes

Use these mappings as starting points. Change the composition when the analytical question changes.

## Task information

Show a compact task card before results when the environment matters:

- task/environment name and objective;
- agent/policy, model, provider, and reasoning effort;
- seed(s), rollout limits, and termination condition;
- representative environment frame or state when available;
- links or IDs for container, run, and trace.

Borrow the information hierarchy of the Craftax eval page: establish the environment and objective visually, then move to results. Do not copy its dark theme mechanically; preserve Desktop chrome and legibility.

## Model or policy comparison

### Two arms, one rollout each

Use:

1. exact head-to-head metric cards or a compact table;
2. achievement observations and their raw 0/1 outcomes;
3. token, call, cost, and latency deltas;
4. a visible “one rollout per arm” caveat.

Do not use a trend line, distribution, confidence interval, or claim of stable frequency. A `frequency-diff` block may still show observed outcomes, but label them as empirical outcomes from `n=1`.

### Two arms, repeated seeds

Use:

- paired differences by seed when seeds match;
- reward distribution or interval when there are enough observations;
- achievement-frequency differences sorted by absolute percentage-point delta;
- exact `n`, mean/median choice, uncertainty method, failures, and seed coverage.

### Three or more independent arms

Use a scatter plot only when both axes are quantitative, such as mean achievements versus cost per rollout. Show points only. Add a Pareto frontier only when it is computed from the points and visually distinguish dominated arms. Use direct labels or collision-aware labels.

## Achievement statistics

For each achievement, preserve:

- numerator and denominator;
- frequency or success probability estimate;
- percentage-point delta between named arms;
- family/category when it helps scanning;
- uncertainty when the sample size supports it.

Sort by absolute delta when diagnosing differences, by progression order when explaining the task ladder, or by baseline frequency when showing coverage. State the chosen ordering.

Use grouped horizontal bars, dot plots, or a signed-delta table. Use a heatmap only for a genuinely large matrix with multiple arms and achievements.

## Rewards

Keep reward concepts separate:

- outcome reward: final session-level score;
- event/step reward: changes over time;
- reward components: verifier or task sub-scores;
- achievement count: unique unlocked achievements;
- pass rate: fraction satisfying a criterion.

Use:

- component bars for additive or comparable reward parts;
- step plots for reward accumulation over ordered steps;
- paired or distribution views for repeated rollouts;
- provenance tables when the reward needs auditability.

Never imply additivity unless the reward definition says components sum to the outcome.

## Trace V5

Start with a trace summary:

- task, model, reward, status, duration, cost, tokens, and event count;
- start/end timestamps and trace/run/container IDs;
- errors, retries, annotations, and asset availability.

Then choose one or more:

- timeline for ordered messages, tool calls, observations, and rewards;
- step scrubber when environment state changes are central;
- latency waterfall for nested or sequential spans;
- token/cost bars by turn, tool, or model;
- error/retry strip for reliability analysis;
- filterable event table for forensic inspection.

Keep the raw event sequence accessible. Summaries must link back to the relevant trace or step.

## Live evals

Show progress, throughput, completed/failed counts, running reward summary, and recent anomalies. Make live state visually distinct, avoid unstable axis rescaling when possible, and freeze the final state when the run completes.

## Precision and formatting

- Costs below one cent: show enough significant digits, e.g. `$0.00134`.
- Rates: include denominators when `n` is small.
- Tokens/calls: use thousands separators.
- Durations: use a consistent unit within a comparison.
- Deltas: use signed percentage points for frequency differences and signed absolute/relative values for continuous metrics; label which one.

## Choosing the blank canvas

Choose `blank.canvas.v1` when the intended result is more like an authored visual explanation than a standard chart: a task-and-results story, a trace topology, a reward-flow diagram, a compact experiment dashboard, or a bespoke environment-state panel. System / UML / flow pictures belong on `diagram.mermaid.v1` via `author-synth-diagrams`, not as HTML dumped into a canvas.

Keep using `analysis.visual.v1` for ordinary metric, bar, frequency-difference, table, and scatter compositions. The blank canvas increases freedom and review burden; inspect it carefully at the actual Desktop pane width.
