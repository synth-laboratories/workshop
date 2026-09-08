# Workshop: query eval traces, rewards, and optional annotations

Final implementation plan · 2026-09-07

**Implement typed, reproducible queries over existing eval jobs and their trace substructure. Connect results to optional annotation campaigns and optional visuals. Complete Jesterky result collection for users who choose that runner.**

This plan supersedes the earlier design in this file and incorporates the scope decisions finalized in chat. It specifies proposed implementation and acceptance gates; it does not claim that they have been implemented or passed. No provider-backed runs were launched while writing the plan.

## 1. Final scope decisions

1. **Existing IDs are the selection roots.** Use the existing optimizer/eval job identity, or a list of those identities, to resolve rollout and trace membership. In this document, `eval_job_ids` is illustrative notation for those existing IDs; do not create an alias-ID registry. A subset is job IDs plus a query/filter. There is no new cohort entity or cohort ID.
2. **The core works without enrichment or presentation.** An agent can run an eval, query its traces and rewards, and inspect exact source evidence without annotations, Jesterky, an experiment record, or a visual.
3. **Annotations are optional.** They add separately versioned evidence that queries can join. Deterministic and ordinary model annotators work without Jesterky.
4. **Jesterky is an optional analysis runner.** Its worker/reduction workflow is used only when selected. Querying never implicitly launches model analysis.
5. **Visuals are optional consumers.** Replay, charts, and visual comparisons consume query results. An agent can read grouped or paired results directly without rendering them. Comparison is not a required pipeline stage or new orchestration subsystem.
6. **Reuse current storage and control paths.** Extend Trace V5 projections, typed queries, existing query snapshots, annotation campaigns, visual bindings, and experiment evidence attachments. Save provenance on query/analysis results rather than inventing another object representing the same traces.

```text
 Existing eval job A / B / ...
              |
              v
       Rollouts + sealed traces + objective rewards
              |
              +------------------> Query / read ----------------> Exact evidence
              |                         |
              |                         +--> Optional visual
              |                              replay / chart / comparison
              |
              +--> Optional annotation campaign
                          |
                          +--> deterministic / model / Jesterky
                          |
                          v
                    Evidence revision -----------> Optional query join
```

The requested [Max Bittker post](https://x.com/maxbittker/status/2096986048186229013) remained inaccessible during the earlier review. This plan uses the user's query-and-pipeline brief; it does not attribute unverified details to the post.

## 2. User-facing workflow

> Run two configurations. Using their existing eval job IDs, find low-reward episodes and inspect their failed actions. Optionally annotate selected traces, query reviewed findings, or open a comparison visual. Reopen Workshop later and recover the same results and source evidence.

The minimal path is `job IDs -> query -> read results -> inspect source`. Further processing composes around it:

```text
 Query selected traces
          |
          +--> read results and decide the next experiment
          |
          +--> open a visual
          |
          +--> annotate selection -> query new evidence
                                         |
                                         +--> read results
                                         +--> open a visual
```

A saved sequence references existing queries, campaign jobs, and outputs. A generic workflow/DAG engine is not required. Human or agent-driven iteration uses current eval-launch and experiment controls.

## 3. Query contract

Extend the current typed, read-only query API and `trace_manage` MCP operation. Preserve existing metadata-query behavior. Introduce a versioned extension for new fields and result modes; compile allow-listed operations to parameterized index queries. Arbitrary SQL and unbounded custom query programs are outside this change.

### Inputs

- One or more existing eval/optimizer job identities, resolved through authoritative rollout membership.
- A result grain: episode/trace, event/span/message part, annotation, or aggregate.
- Typed filters, supported relationships, grouping, aggregate functions, and deterministic ordering.
- Evidence selection: latest at query start, or explicitly pinned revisions.
- Bounded page size plus continuation cursor. A page bound limits transfer, not the total set that can be queried.

### Supported query families

| Over | Initial support | Example |
| --- | --- | --- |
| Job/configuration | Job ID, environment/scenario version, task, seed/repeat, lifecycle, prompt/protocol/harness revision, resolved model and effort when recorded | Runs using prompt v3 and medium effort |
| Objective reward | Terminal episode reward, typed reward events, metric/unit/source, numeric range, missing value | Episodes below a reward threshold |
| Events/actions | Kind, actor/session, tool/action name, status, supported payload fields, recorded order/time window | Failed actions followed by a repeat of the same action |
| Messages/coordination | Sender/recipient, message ID, recorded delivery/acknowledgement links, observation membership where captured | Delivered messages lacking a recorded acknowledgement before expiry |
| Annotations | Target identity, annotator/program/taxonomy version, label, typed score, producer, review state, current/superseded state | Accepted `ignores_feedback` findings from annotator v2 |
| Combined evidence | Typed joins through job, rollout, trace, target, and coordination identities | Low-reward episodes with accepted `stale_plan` findings |
| Aggregates | Counts, distinct episode counts, sums/means/min/max, distributions and grouped outcomes | Reward and recovery-failure incidence grouped by prompt/model |
| Cross-job alignment | Match episodes by explicit compatible environment/scenario/task/seed/repeat keys; expose ambiguity and unmatched rows | Baseline and candidate outcomes for the same tasks/seeds |

Support selected, schema-defined payload fields rather than arbitrary JSON traversal. Missing configuration/capture fields remain queryable as unavailable; never infer their values from display names.

### Result semantics

- **Grain is explicit.** Return episode outcomes, matching entities, annotation records, or aggregate rows as requested. Related evidence is attached by selectors; it does not silently change row grain.
- **Do not multiply rewards through joins.** Matching ten annotations on one episode contributes one episode to an episode-level reward mean. Annotation counts and event counts are separate metrics.
- **Preserve reward meaning.** Terminal reward and step rewards are separate; cumulative reward records are not summed as deltas. Annotator scores never replace objective reward. Units and scorer/environment versions accompany results.
- **Preserve missingness.** Distinguish missing reward, missing capture, never annotated, completed inspection with no findings, abstention, rejected output, and partial/failed analysis. Annotation coverage is scoped to the selected annotator/version and inspected targets.
- **Define event order.** Next-decision/repeated-action predicates use recorded actor/session ordering and explicit action equality fields. Cross-agent links require recorded identities. Timestamps alone establish neither delivery nor causation.
- **Separate facts and interpretation.** A missing recorded acknowledgement is an observable query conditional on capture coverage. “Ignored handoff” is an annotation or analyst interpretation.
- **Align comparable units.** Cross-job matching includes environment/scenario version, task, seed and repeat identity. Ambiguous duplicates, incomplete runs, and unmatched rows are explicit. Mixed raw reward scales are not silently averaged.
- **Aggregate the full match set.** Limits and pagination must not turn a mean into a first-page mean. If an execution bound prevents a complete aggregate, return an explicit incomplete/error status rather than a complete-looking number.

### Reproducibility and source resolution

Reuse existing query snapshots/results to record the job IDs, query/version, resolved membership, trace digests, relevant evidence/review revisions, run-status cutoff, result digest, denominators, coverage and exclusions. This is result provenance, not a new cohort object.

A query started with “latest evidence” resolves and pins that evidence for its execution and pages. Refresh creates a new result revision; old saved results do not change. New trace imports and annotation reviews cannot reorder or alter an in-progress paginated result. A saved result remains inspectable if its container stops, provided its imported authority is retained; unavailable payloads get explicit availability errors.

Live run metadata can remain visible, but durable trace analysis defaults to sealed traces. Pending/unsealed or failed-without-trace rollouts remain visible in job accounting. They must not disappear from coverage or silently become zero-reward episodes.

Each entity result provides its job/rollout identity, trace identity/digest, canonical selector, and applicable evidence revision. The same reference supports agent source inspection and optional visual navigation.

## 4. Architecture and code ownership

```text
 EXISTING EXECUTION                         EXISTING AUTHORITY

 Optimizer/eval job -> rollout membership -> Trace V5 + objective results
                                                   |
                            +----------------------+------------------+
                            |                                         |
                            v                                         v
                   Workshop query projections                Optional annotation
                   job / reward / entity facts               evidence revisions
                            |                                         |
                            +----------------------+------------------+
                                                   |
                                                   v
                                        Typed read-only query
                                                   |
                                                   v
                                        Existing query result/snapshot
                                        + exact source provenance
                                                   |
                         +-------------------------+---------------------+
                         |                         |                     |
                         v                         v                     v
                     Agent reads             Optional visual      Optional campaign
                                                                  existing scheduler
```

| Owner / current entry point | Planned change |
| --- | --- |
| Workshop `trace_ingest.rs`, existing job membership and trace projections | Resolve job-to-rollout-to-trace membership; project queryable config, reward and entity facts; preserve missing/partial capture and import idempotency |
| Workshop `trace_query.rs` | Extend typed filters, relationships, aggregate result modes, stable paging, and snapshot provenance |
| Workshop `synth_traces_mcp.rs` and existing trace IPC dispatch | Expose job-scoped queries and result/source reads through existing tools; keep old operations compatible |
| Workshop annotation IPC/projections, human annotation service, optional post-rollout stage | Project queryable annotation/review revisions; pass selected existing trace identities into existing campaigns; report partial analysis separately from eval completion |
| Workshop existing visual bindings and shared `agent_trace.v1` components | Consume query results; resolve selection to exact evidence; support reviewed-evidence refresh and production host transport |
| Containers trace/annotation packages | Preserve canonical selectors; supply bounded indexed access and annotation coverage; complete optional Jesterky worker collection/reduction/accounting |
| Evals adapters/domain definitions | Record environment-specific facts and communication relationships; define annotator semantics and retained correctness examples for DungeonGrid/Craftax/RuneBench |
| Existing experiment/evidence registry | Optionally attach jobs, query results and analysis outputs; extend configuration provenance only where absent |

Containers remains authoritative for trace structure, selector validation and sealed annotation evidence. Evals owns task/reward meaning and annotation taxonomies. Workshop owns queryable projections, orchestration and presentation. Do not introduce a second trace format, independent annotation authority, or replacement execution scheduler.

### Optional annotation and Jesterky integration

A query selection resolves to existing trace identities and, where supported by the annotator, exact target scope. If an annotator is whole-trace only, expose that scope in its estimate instead of pretending it processes only selected events. Campaign estimates and submissions bind to the same pinned selection.

Review writes create evidence revisions. Query projections refresh from the authoritative revision; older snapshots remain valid. Rebuilding projections must preserve review history and produce the same query answers.

For Jesterky, implement:

```text
 Selected sealed traces
        |
        v
 Bounded actor/window partitions + source/context references
        |
        +------> worker A
        +------> worker B
        +------> worker C
                     |
                     v
          Collect EVERY worker result
          Attribute / validate / deduplicate
                     |
                     v
          Explicit cross-window/actor reduction
          Findings + disagreements + coverage + usage
```

Each shard has stable identity and bounded scope. Overlap does not duplicate final findings; source citations survive reduction. Preserve rejected, abstained and failed results in receipts. Retry only eligible failed/unstarted work; cancelled work stays cancelled unless explicitly resumed. Successful work and charges must not be duplicated. Provider-backed execution retains existing reservation/price enforcement and reconciliation.

This corrects the reviewed adapter's first-proposal extraction behavior. It does not make Jesterky a prerequisite for any query, annotation, or visual.

## 5. Environment use cases and scope boundaries

| Environment | Core query use case | Optional enrichment / view |
| --- | --- | --- |
| DungeonGrid | Query actor actions, blocked/repeated movement, reward and captured messages within an exact scenario | Conflicting-plan annotations; map replay and actor lanes |
| Craftax | Query prerequisite feedback, planned versus executed actions, repeated no-ops, achievements and objective rewards | Recovery/grounding annotations; world/inventory and plan-batch inspection |
| RuneBench | Query resource attempts, failed results, actor progress and recorded communication links | Handoff/duplicate-work annotations; synchronized world replay and agent lanes |

The initial RuneBench loop uses baseline and one messaging-protocol revision with other settings fixed. Craftax can use a prompt-only recovery intervention. DungeonGrid supplies a third environment proof through the same contract. Treat each scenario's available fields honestly; shared rendering is not proof of complete capture.

The latest local RuneBench/Craftax handoffs report annotation persistence, replay, and retained-source follow-ons. Those demos do not establish a fresh three-environment configuration matrix or full Craftax multi-agent capture. Use a real supported multi-agent RuneBench scenario for long-trace/coordination acceptance. Creating new Craftax-MA scenarios, new protocols, or a larger RuneBench game mode is outside this implementation unless separately requested.

Prompt × protocol × harness × model × effort remains a motivating use case. Store actual configuration provenance on existing jobs so those dimensions can be filtered/grouped. Automated factorial scheduling, automated finalist selection, held-out study orchestration, automatic next-arm proposal, a new experiment workspace, and a general pipeline engine are deferred. They are not prerequisites for this query-and-analysis delivery.

## 6. Implementation order and exit gates

| Phase | Deliverable | Exit gate |
| --- | --- | --- |
| 1. Job-scoped core | Existing-ID membership, reward/config filters, coverage, source read, stable pagination and result provenance | Tests E1, E2, and core E9 pass without annotations/Jesterky/visuals |
| 2. Trace relationships | Entity queries, bounded event relationships, grouped outcomes and cross-job alignment | E3 and E4 pass with exact source references and no join inflation |
| 3. Optional annotations | Queryable annotation/review projections, selection-to-campaign plumbing, immutable old results | E5 and E6 pass with Jesterky disabled and no visual required |
| 4. Optional consumers | Complete Jesterky runner path and connect existing visuals to query results | E7 and E8 pass independently; neither is required by the core path |
| 5. Integrated acceptance | Small live eval wiring on all three environments, restart/recovery, retained long-trace validation | Full E1–E10 matrix passes with evidence from the tested build |

Extend current stores and tool contracts first. Avoid building a new orchestration framework to demonstrate a sequence the existing agent/tools can already execute.

## 7. End-to-end acceptance suite

Use retained, schema-valid traces with independently specified expected records/answers for precise correctness, plus small live smoke evals for launch/capture/import wiring. Retained fixtures must include multiple actors, reused native IDs across sessions, reward events, missing capture, annotation revisions, partial jobs and more than 200 trace records. Model-generated annotation content is not the deterministic correctness oracle.

Optional features are optional at runtime but their branches must pass acceptance before this full plan is marked complete.

| ID | End-to-end scenario | Required assertions |
| --- | --- | --- |
| **E1** | Launch a small eval, then query/read by its existing job ID with annotations/Jesterky/visuals disabled | Every expected rollout is accounted for; sealed traces and rewards match authoritative records; failed or pending rollouts without traces are visible; no new cohort/experiment/visual creation or paid annotation occurs |
| **E2** | Filter job/configuration/reward/status and aggregate outcomes | Exact expected membership, values and denominators; missing reward stays missing; terminal/cumulative/delta reward semantics hold; multiple matched events/annotations cannot multiply episode-level reward contributions |
| **E3** | Query known failed-action/repeat and message/acknowledgement relationships; inspect sources | Exact actor/session/entity selectors; correct recorded order and target scope; no accidental cross-session join for reused IDs; missing communication capture yields unknown, not a false ignored-message claim |
| **E4** | Query two existing job IDs, group by arm and align episodes | Exact compatible task/seed/repeat matches; duplicate/ambiguous/unmatched/incomplete rows explicit; raw rewards with incompatible units/versions not silently combined; no comparison visual required |
| **E5** | Annotate only a selected subset with a deterministic or ordinary model annotator, then query labels/scores/reviews alongside rewards | Campaign runs over the advertised selected scope; combined queries are correct; unannotated, inspected-no-findings, abstained and partial/failed states differ; Jesterky remains disabled; original rewards/traces unchanged |
| **E6** | Accept/reject/supersede an annotation; refresh query, then reopen the earlier saved result | Refreshed answers reflect authoritative review revision; earlier answers/evidence remain pinned; author/producer provenance correct; projection rebuild/import does not duplicate annotations or change history |
| **E7** | Select Jesterky for a retained long multi-agent trace, with multiple partitions and an injected worker failure | Every shard/result accounted for; all valid proposals considered; overlapping findings deduplicated without erasing disagreement; cross-actor claims cite actual evidence; resume does not rerun successful paid work; usage/reservation and partial coverage remain accurate |
| **E8** | Bind a query result to an optional chart/table/comparison and drill into evidence/replay | Rendered values and denominators equal the bound result; selection resolves correct trace/actor/event; replay seeks only when timing exists; stale/missing evidence explicit; returning preserves job filters and selection; querying still works with visuals disabled |
| **E9** | Page through at least 450 retained traces, introduce an import/review between pages, restart Workshop, reopen results and resume interrupted analysis | No missing/duplicate/reordered rows across pinned pages; saved answers survive restart and stopped source container; complete aggregates cover all matches, not one page; explicit unavailable-data errors; eligible recovery preserves completed work and cancellation |
| **E10** | Repeat the core eval-job → query → source-read path on DungeonGrid, Craftax and RuneBench | Same API/query semantics across environments; domain reward provenance preserved; exact configuration/environment versions recorded; small live smoke proves actual launch/capture/import; unavailable capabilities/capture are explicit; no mandatory annotations or visuals |

For E7, use controlled worker outputs and failure injection to establish exhaustive collection, ordering, deduplication, restart and accounting, then a bounded live runner smoke to verify actual CLI/provider compatibility. Do not treat fake-runner tests as live Jesterky proof. For E8, verify the actual final Workshop build with real retained evidence, not only isolated components or an earlier demo revision.

Include a retained long trace with at least 10,000 events across four actors for query/source/optional-view checks. Record index time, query latency, first usable render and memory on the acceptance host. Evidence loading/rendering must remain bounded and responsive; acceptance must not rely on loading every full payload into a list before interaction. Record failures and measurements in the final report rather than asserting unmeasured performance.

### Required correctness invariants

- Original sealed trace bytes and objective rewards do not change during querying, annotation or review.
- Every result can resolve its source selectors or explicitly explain why evidence is unavailable.
- Missing, failed, partial, invalid and zero-valued outcomes are distinct.
- Episode statistics survive one-to-many joins without denominator inflation.
- “Latest” resolves once per query execution; pagination and saved results use pinned revisions.
- Existing job IDs remain sufficient. No additional grouping object is required to query all traces in a job.
- Reading/filtering/rendering never implicitly launches paid inference.
- Optional branches may fail independently; eval completion and analysis completion remain separate.

## 8. Validation and completion evidence

Implement focused tests beside the existing query/ingest, annotation, and viewer tests; keep broader app acceptance in the repository's established release/E2E harness. Extend the current test structure rather than introducing a second test platform. Follow each repository's applicable test-authoring instructions.

A release report must identify the tested commit/build plus any dirty implementation dependencies, command results, retained fixture/expected-answer digests, live job IDs, query/result revisions, trace/evidence references, campaign/worker receipts, and final UI verification evidence. Do not call a gate passed from component exports, historical handoffs, or fixture-only execution when the gate requires live wiring.

Live eval, annotation and Jesterky smoke runs need an explicit bounded execution authorization before launch. When an experiment is authorized, state aggregate maximum and expected cost once and use existing authorization across its arms/retries within the approved scope. Use already authorized project-local environment credentials or Workshop's secrets proxy; no Keychain-backed credential flow. Finalizing this plan does not itself launch paid work.

**Definition of done:** an agent can query all traces/rewards in one existing eval job, query across jobs and exact trace entities, optionally enrich/query reviewed annotations or use Jesterky, optionally visualize results, and recover the same saved evidence after restart. All ten acceptance scenarios have recorded passing evidence; none of the optional branches is a prerequisite for the core path.

## Source anchors

- [Trace ingestion](/Users/joshuapurtell/GitHub/workshop/apps/synth_desktop/src-tauri/src/trace_ingest.rs)
- [Typed trace queries and current row cap](/Users/joshuapurtell/GitHub/workshop/apps/synth_desktop/src-tauri/src/trace_query.rs)
- [Existing trace MCP operations](/Users/joshuapurtell/GitHub/workshop/apps/synth_desktop/src-tauri/src/bin/synth_traces_mcp.rs)
- [Existing experiment membership models](/Users/joshuapurtell/GitHub/workshop/apps/synth_desktop/src-tauri/src/experiments/models.rs)
- [Annotation IPC](/Users/joshuapurtell/GitHub/workshop/apps/synth_desktop/src-tauri/src/annotations_ipc.rs)
- [Annotation projections](/Users/joshuapurtell/GitHub/workshop/apps/synth_desktop/src-tauri/src/session/annotation_projection.rs)
- [Optional post-rollout annotation stage](/Users/joshuapurtell/GitHub/workshop/apps/synth_desktop/src-tauri/src/optimizers/annotation_stage.rs)
- [Jesterky runner](/Users/joshuapurtell/GitHub/containers/src/synth_containers/tracing/annotation/jesterky_runner.py)
- [Shared trace-view contract and transport limits](/Users/joshuapurtell/GitHub/workshop/visuals/components/agent_trace.v1/README.md)
- [Shared trace local follow-ons](/Users/joshuapurtell/GitHub/workshop/docs/handoffs/2026-09-07-shared-agent-traces.md)
- [RuneBench scope and retained-evidence acceptance](/Users/joshuapurtell/GitHub/evals/workshop/runebench-ma-demo/ma/IMPLEMENTATION-SCOPE.md)
- [Post-hoc annotation architecture](/Users/joshuapurtell/GitHub/workshop/docs/HANDOFF_ANNOTATIONS_POSTHOC_ARCHITECTURE_2026-09-01.md)
- [Environment QA profiles and acceptance limits](/Users/joshuapurtell/GitHub/workshop/services/environment-qa/README.md)

### Independent Jesterky analysis of existing rollouts

Jesterky also operates after evaluation, independently of any dedicated annotated
eval job. A saved analysis-scope setting chooses entire selected rollouts (the
default) or selected events only. Preparation takes immutable snapshot/result
IDs and can combine rollouts from multiple existing eval jobs. It preserves the
source identities and emits explicit per-container trace selections for the
existing annotation campaign API; no policy rerun or active eval protocol is
inherited. Luna/low remain analysis defaults, with separate estimates and budget.

Extend E5/E7 acceptance to select rollouts from two earlier eval jobs without an
active annotated eval, deduplicate shared traces, persist the scope preference,
and verify the resulting campaign contains only those traces. Event-only mode
must carry exact event scope or refuse preparation. Saving a setting or preparing
a selection must never launch evaluation or annotation compute.
