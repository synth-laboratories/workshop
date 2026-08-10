# Handoff: Better general-purpose Trace V5 viewer

**Date:** 2026-08-09  
**Audience:** Engineer continuing Trace V5 inspection, search, and visualization in Synth Desktop  
**Status:** Proposed follow-on to the implemented Trace V5 vault and rollout inspector  
**Related:** [`HANDOFF_TRACES_V5.md`](./HANDOFF_TRACES_V5.md) · [`TRACES_V5_STORAGE_FORMAT.md`](./TRACES_V5_STORAGE_FORMAT.md)

## Goal

Make any valid Trace V5 useful immediately after import, without adding a parser or viewer for each benchmark. Harbor, Codex, Craftax, HealthBench, Banking77, and future producers should all get a strong default inspection and search experience. Domain-specific visuals should remain optional enhancements.

The acceptance test is:

> Import an unfamiliar valid V5 archive and, without benchmark-specific code, find it by model/container/time, inspect its actors and lanes, search any nested value, understand capture coverage, and render a useful timeline.

## Architectural decision

Do **not** create another canonical trace schema and do **not** mutate sealed Trace V5 archives.

```text
immutable Trace V5 archive (source of truth)
    -> generic versioned projector
    -> disposable TraceViewModel + search documents
    -> reusable viewer modules
    -> default or agent-composed visual
```

`TraceViewModel` is a read model, not a new storage format. It may be cached for speed, but it must always be regenerable from the trusted archive. Improving projection logic should allow old traces to be reindexed without migrating or rewriting them.

The generic projector structurally extracts standard fields, relationships, native event names, nested payloads, evidence, and coverage. It must not claim benchmark-specific meaning based on filenames or brittle heuristics. Producers may add optional semantic hints; unknown native events remain visible and searchable.

## Why this layer exists

The current Craftax dogfood trace proves that valid V5 data can be healthy while the UI loses information:

- The archive contains `gpt-5.6-luna`, but the catalog displays **Unknown model**.
- It contains two logical rollout lanes, but the projection presents one generic `imported react` lane.
- It contains `collect_wood`, but viewer search returns no results for that value.
- It contains 83 application events, but Focus mode selects zero because the classifier recognizes only a narrow set of Codex/tool/message event types.
- A displayed zero may mean **not captured**, not that no model or tool activity occurred.

A shared read model fixes these problems once. Without it, every visual template independently reparses raw payloads and reproduces incompatible metadata, filtering, and coverage behavior.

## Concrete dogfood artifact

The real example was produced by `evals/suites/nonproduct/craftax` using `gpt-5.6-luna:medium`:

| Fact | Value |
| --- | --- |
| Seeds | `7301`, `7302` |
| Rollouts | 2 |
| Steps | 12 each |
| LLM calls | 3 each |
| Rewards | `1.0`, `2.0` |
| Achievements | `collect_sapling`, `collect_wood` |
| Events | 83 |
| Tokens | 21,846 |
| Estimated cost | $0.004431 |
| Trace ID | `trace_f6832ddf5154b1a1` |
| Trace digest | `sha256:34e328b27fab9fbf8a1a2c302fa5be00e5607e006faf2c08ac234d1d1c932c73` |
| Validation | All 14 Trace V5 checks passed |

Portable archive:

```text
/Users/joshuapurtell/Documents/GitHub/evals/artifacts/suites/nonproduct/craftax/bundles/craftax-luna-medium-seeds-7301-7302.tracev5.zip
```

The run used `--allow-drift`: the cached GameBench image resolved to commit `1fd53a15...`, while the suite pin was `ef6bb06...`. Preserve and surface that provenance; do not silently label the run as pinned.

## TraceViewModel responsibilities

### Identity and metadata

Extract or derive, with provenance for each derived value:

- Trace title, digest, run ID, task, suite, environment, source, and timestamps
- Container/session/attempt/rollout relationships
- Model, provider, model version, reasoning effort, and policy configuration
- Score, reward, outcome, terminal status, latency, cost, and token usage
- Capture coverage, redaction, truncation, and evidence availability

Use producer-neutral optional keys where possible:

```text
trace.title
task.id
task.domain
suite.id
run.id
rollout.id
container.id
model.id
model.provider
model.reasoning_effort
evaluation.scorer
evaluation.score
evaluation.outcome
```

The UI should be able to say whether a value was explicit, derived, unavailable, or conflicting.

### Actors, lanes, and hierarchy

Preserve real concurrency and causality:

```text
run
|- rollout 7301
|  |- model call
|  |- environment actions
|  `- evaluation
`- rollout 7302
   |- model call
   |- environment actions
   `- evaluation
```

The same representation must handle Codex primary agents and subagents, Harbor trials and graders, HealthBench conversations and scorers, and Banking77 examples and predictions. Do not collapse unrelated actors merely because they share an import capture.

### Events

Every event retains:

- Stable event identity, time, sequence, actor, lane, parent/span, and causal links
- Original native type and full native payload
- Human-readable summary and searchable text
- Optional normalized category supplied by explicit producer hints or lossless standard mapping
- Status, duration, metrics, evidence, and artifact references when present

Useful broad categories are:

```text
message
reasoning
model.request
model.response
tool.call
tool.result
environment.observation
environment.action
state.snapshot
metric
achievement
evaluation
artifact
lifecycle
error
unknown
```

Unknown is a supported state, not a reason to drop an event.

### Coverage semantics

Keep these states distinct:

- Captured
- Not captured
- Unavailable
- Derived
- Redacted
- Truncated

For example, `tool_events: not_captured` must render as **Tool activity not captured**, not **0 tool calls**. Counts should only be asserted when the relevant stream was captured.

## Search and filtering

Build one flattened search document per trace and per event at ingestion/reprojection time. Include all nested scalar values, not just title and summary fields.

Index:

- Titles, messages, reasoning, observations, event names, and native types
- Nested payload keys and values, including achievements
- Model/provider/configuration
- Task, suite, environment, container, run, rollout, actor, lane, and span
- Tool names, arguments, results, errors, and exit status
- Rewards, scores, evaluator outcomes, usage, cost, and latency
- Evidence and artifact names
- Timestamps, durations, provenance, coverage, and terminal state

Support plain text plus structured facets:

```text
model:luna reward:>1
suite:healthbench scorer:*
container:craftax created:24h
type:tool.call status:error
achievement:collect_wood
task:banking77 outcome:incorrect
```

Catalog filters and in-viewer search should use the same indexed facts. Search results should show why they matched and allow jumping to the corresponding event.

## Viewer composition

The viewer is a common shell composed from reusable visual modules according to available capabilities:

- Metadata and provenance header
- Actor/lane selector and span tree
- Event timeline with virtualized rendering
- Conversation and reasoning transcript
- Model-call inspector
- Tool-call and result inspector
- Environment observation/action view
- State snapshot inspector
- Reward, score, and metric progression
- Achievement timeline and frequency comparison
- Evaluator/rubric results
- Evidence and artifact gallery
- Cost/token/latency summary
- Raw event and raw payload inspector

Domain templates may add Craftax maps, HealthBench rubric layouts, or Banking77 confusion views, but they should consume the shared read model rather than reimplement archive parsing.

The agent should also be able to compose these modules ad hoc on a blank canvas. Templates provide good defaults, not a railroaded chart selection.

## Focus mode

Replace the current hard-coded classifier with explainable semantic presets:

- Narrative
- Decisions/reasoning
- Model calls
- Tools
- Environment
- Evaluation and reward changes
- Errors
- Everything

The default Focus view should include messages, decisions, tool calls, meaningful state/reward changes, achievements, evaluations, errors, and terminal events. The user must be able to see which rule included or omitted an event. Everything always exposes unknown native events.

## Comparison support

Make traces and rollout lanes selectable for comparison using normalized facts:

- Configuration and model differences
- Outcome, score, and reward
- Achievement frequency
- Tool success/failure
- Evaluator agreement/disagreement
- Token use, cost, and latency
- Event/span differences

Choose charts from the data shape. Do not draw a trend line for unordered categories or one sample. Prefer tables, bars, distributions, timelines, or explicit small-sample text when appropriate.

## Implementation sequence

1. Define and version the `TraceViewModel` and per-event search document.
2. Fix generic recursive indexing of nested native fields.
3. Extract model/title/task/container/time metadata with explicit derivation provenance.
4. Reconstruct actors, spans, and rollout lanes from V5 relationships and explicit hints.
5. Implement coverage-aware counts and labels.
6. Replace Focus classification with semantic presets and an unknown-event fallback.
7. Refactor the existing rollout inspector into a shell plus reusable modules.
8. Add cross-trace and cross-lane comparisons.
9. Expose the same query/index interface to visual templates and agent-created visuals.
10. Add reproject/reindex support keyed by projector version, without touching the trusted archive.

## Acceptance corpus

Maintain representative fixtures for:

- Craftax multi-rollout environment trace
- Harbor trial with agent, container, and grader
- Codex task with tools and subagents
- HealthBench conversation with multiple scorers
- Banking77 batch with correct and incorrect predictions
- Partial/legacy trace with missing metadata
- Failed or interrupted rollout
- Unfamiliar trace containing unknown native event types

For every fixture, assert:

- Correct identity/model/task/container extraction
- Correct actor, lane, span, and causal structure
- Nested values are searchable
- Model/container/time/outcome filters work
- Focus presets include expected semantic events
- Coverage language is honest
- Unknown events remain visible
- Projection snapshots are deterministic
- Reprojection does not mutate archive bytes or trace digest

Specific Craftax regression assertions:

- Catalog model is `gpt-5.6-luna`, not Unknown.
- Two rollout lanes are independently selectable.
- `THOUGHT` finds six reasoning records.
- `collect_wood` and `collect_sapling` find their nested records.
- Focus mode includes reasoning, achievement, phase, terminal, and material state-change events.
- The viewer explains that provider/model/tool interception was not captured for this application-only trace.

## Non-goals and guardrails

- Do not replace or rewrite Trace V5.
- Do not create one parser per benchmark.
- Do not make a visual template the authority for trace interpretation.
- Do not discard unfamiliar events.
- Do not infer proof, readiness, or task membership from filenames.
- Do not treat unavailable capture streams as observed zeros.
- Do not require a specialized template for basic inspection and search.

## Definition of done

An engineer can import each acceptance fixture into a clean isolated Synth Desktop instance and use the same catalog, filters, timeline, lane selector, search, coverage UI, and raw inspector successfully. Specialized visuals improve the presentation, but removing them does not make the underlying trace opaque or unsearchable.

