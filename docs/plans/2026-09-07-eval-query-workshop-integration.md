# Existing eval jobs → query → evidence → optional analysis and visuals

Review and implementation plan, 2026-09-07. This is a code-grounded integration plan, not a claim of production readiness. No provider runs or product code changes were made for this review.


## Coordination update — other implementation takes precedence

The user supplied the other implementer’s latest progress report after this review. Treat their shared query/annotation/Jesterky implementation and their existing engineering documents as the integration baseline. Do not interpret the review below as a request to rebuild those capabilities or as a competing backend plan.

Their update additionally reports independent retained-rollout annotation, saved whole-rollout versus exact-event scope, cross-job selection, owning-container grouping, strict scope rejection, and expanded validation (58 Containers tests plus native/browser checks). These are reported progress, not checks rerun by this task. Earlier statements below about preparation gaps must be reassessed against that newer code before proposing changes.

Our complementary lane is RuneBench consumer integration and acceptance: exercise the existing query/snapshot/source interfaces with the retained runs, bind the comparison visual to them, verify metric/outcome mappings with exact source evidence, and improve the investigation UI. Preserve their backend, plugin, campaign and scheduler work. Treat the retained-data observations below as acceptance cases to check on their final build, not authorization for parallel rewrites.

Leave their final binding/typecheck/MCP/browser validation, contract-drift reconciliation, plugin release/provisioning, and shared recovery/performance work coordinated with their existing plan. Do not launch paid acceptance or message another task without the user’s authorization. No new cohort IDs, trace format, query service or scheduler.

## Decision

Proceed, but integrate the existing `synth.trace-query.v2` implementation before building another viewer or SQL service. The RuneBench comparison is a useful acceptance fixture and first visual consumer. It is not the shared query backend.

Keep existing job IDs, V5 authority, query snapshots, annotation campaigns, optimizer read models, and visual components. A cohort is a selection over existing jobs/results. Do not introduce cohort IDs, a trace format, an execution scheduler, or a mandatory processing pipeline.

The next deliverable is **a trustworthy query-backed RuneBench comparison with exact evidence drilldown**. A small distribution viewer follows once its numbers and source links are correct. Annotation and Jesterky are independently optional consumers; release packaging for Jesterky must not block reading traces.

This plan supplements the existing [workflow scope](../engineering/EXPERIMENT_TRACE_RESEARCH_WORKFLOW_2026-09-07.md) and [implementation account](../engineering/TRACE_RESEARCH_IMPLEMENTATION_2026-09-07.md). Those documents already describe much of the requested architecture. The sequencing below addresses concrete integration gaps found in this review rather than restarting that implementation.

## What I inspected and verified

- Workshop: `trace_query.rs`, `trace_research.rs`, `data.rs`, trace IPC/MCP dispatch, optimizer read-model entry points, `trace.catalog.v1`, and shared inspector contracts.
- Containers: `research.py`, catalog projection/store, RuneBench adapter, coordination models, annotation selection/store interfaces, and Jesterky partition/collection/retry code.
- Evals: results metadata/index, `research_context`, RuneBench runner, custom query index and viewer; searched the existing DungeonGrid/Craftax capture paths.
- Existing focused tests: `python -m pytest tests/test_trace_research.py tests/test_trace_v5_annotation_jesterky.py -q` in Containers: **17 passed**. These include controlled/fake Jesterky execution, not a live provider/runtime proof.
- Real retained RuneBench probe: indexed sealed Luna-low trace and its evidence with the shared `ResearchIndex`. All **267 canonical events** were returned. The index exposed **zero error-status events**, while **24 native event payloads contained result.success=false**. Those 24 include tool and engine records and are not 24 distinct failed decisions. The current repeated-failed-action relation returned zero. Source: [probe receipt](../../../containers/artifacts/query-review-2026-09-07/retained-probe.json).
- Prior visual review remains relevant: transcripts begin below the first viewport, and exiting/reopening the custom comparison resets investigation state. Revision 28 proves exact event selection and paired annotation persistence, not query-backed distributions.
- Existing Rust test logs were inspected but not rerun in this planning pass. No production deployment, three-environment live run, large-archive benchmark, or installed Jesterky smoke is claimed.

The reviewed implementation contains uncommitted/untracked work across repositories. Coordinate by scoped hunks and preserve existing work; do not rewrite its plans or stage generated recordings wholesale.

## Existing foundation versus remaining work

| Area | Present in code | Gap that matters for this delivery |
|---|---|---|
| Query API | V1 metadata queries; V2 typed job-scoped episodes/entities/rewards/annotations; existence filters, count/reward aggregates and paired results | Not arbitrary SQL; V2 filters currently scan decoded cached rows in Python. No distribution/cost/latency query metrics yet. |
| Membership | Workshop resolves optimizer runs, evaluation rows, rollout refs, candidate/checkpoint/stage fields; optional GoEx child traversal | Resolver starts from `optimizer_runs`. It does not directly resolve an arbitrary Evals filesystem run ID. RuneBench demo currently bypasses this route. Multiple trace authorities on a trial are rejected; multiplicity needs an explicit membership contract. |
| Capture | Sealed V5 actors, sessions, events, message parts, selectors and provenance; RuneBench tool results preserve `is_error` | Research projection does not expose RuneBench unsuccessful outcomes consistently. Physical events duplicate one logical call across tool/engine phases. |
| Rewards | Typed reward definitions/records and grain-aware aggregation exist | RuneBench V5 terminal rewards are actor-scoped. The viewer's episode XP sum is a domain reduction, not a universal reward. Episode research aggregation currently uses the host trial score; matching reward evidence does not select/reduce a metric. |
| Snapshots/source | Full result snapshots, deterministic IDs, persisted results, 1–200-row pages; exact trace selector source read with digest | Source reopening looks up the archive by current trace registry path. Full historical annotation evidence readback/retention needs proof. Aggregate rows lack a direct trace selector and need snapshot-preserving drilldown. |
| Annotations | Canonical evidence, human reviews, preparation from explicit result IDs, existing campaigns | Demo notes use a separate local store. Analysis status is currently collapsed across all jobs for a trace, not scoped by annotator/version/selected targets. Preparation requires a source container. |
| Jesterky | Luna/low defaults, actor/session windows, proposal union, exact duplicate removal, shard retry receipts and proxy-enforcement requirement | Live enforcing-proxy acceptance and remote provisioning remain unverified. Review cross-actor context, per-shard terminal validation and usage across retries; optional synthesis is not established by unioning proposals. |
| Visuals | Shared readable trace inspector; generic V2 result table; custom RuneBench comparison | Generic table locally pages an already-loaded snapshot and opens the trace root. It does not open an exact result selector or render cohort distributions. RuneBench uses inline data and port 8118 rather than production host transport. |

## Correctness issues to resolve before charting

### 1. Define a logical action outcome independently of record status

A successfully captured `tool.result` can report an unsuccessful action. Preserve those two facts. Do not relabel every captured event as an infrastructure error to make a query work.

Normalize a queryable outcome from existing typed message parts (`is_error`), native result fields and call/result links. Keep capture status, execution/transport status and environment outcome separately named. Preserve unknown outcome when absent.

Expose one logical call/action fact with references to its request, result, engine events and observation. This is a rebuildable projection, not a new V5 trace schema. Use actor/session plus recorded call identity; never join by nearest timestamp. Keep all physical events queryable, but count logical calls for failure rates.

The current `repeated_failed_action` relation checks action-name equality on adjacent action-bearing events with status `error`. It does not compare target/arguments and does not reliably join results back to calls. Define repetition over consecutive logical decisions by the same actor/session, with a versioned action-equality policy. For RuneBench, `chop(x,z)` on a different tree must not count as repeating the same target. Return both selectors and the equality basis.

Likewise, `recorded_link` currently means that some relationship exists, including message-reference links. Add relation type/direction constraints before presenting it as delivery, acknowledgement, or coordination analysis. “No acknowledgement recorded” needs capture coverage; it is not “ignored message.”

### 2. Make the measured unit and metric explicit

A trial may contain one shared multi-agent episode, multiple independent episodes, or more than one trace. Audit actual producers before asserting trial=episode=trace. Keep existing identities and explicit membership/cardinality; do not synthesize new job IDs to fit optimizer tables.

For RuneBench, define a named episode metric as the sum of current valid terminal Woodcutting XP records for the expected actors at the authoritative cutoff. Store the reward-definition digest, actor scope, reducer version and input selectors. Actor XP remains separately selectable. Missing actors make the aggregate incomplete; do not silently sum a partial team.

Keep three denominators visible: selected episodes, episodes with the required metric, and episodes with usable traces. Distinguish invalid, infrastructure failure, timeout, partial, missing, and valid zero. Define how a timeout receives a valid task reward in each environment. Do not derive validity only from lifecycle status.

Add model-call count, response duration, tokens and cost only from recorded usage. Label total versus output/reasoning tokens and reported versus unavailable cost. Multiple annotations/events must not multiply episode scores or costs. Cross-environment raw rewards remain separate by definition/version/unit.

### 3. Pin the investigation, not just its first page

The current API has useful immutable snapshots. Extend that contract to aggregate/plot drilldown: selecting a bucket must refer to the original snapshot's resolved input membership and revisions, not rerun the filter against latest data.

Expose member result references through existing snapshots and paging; aggregate rows need a supported member-resolution operation. A derived selection is still a query/result, not a cohort object. Keep limits honest: current bounds are 100 explicit job IDs, 1,000 expanded jobs, 100,000 matched result rows and 64 MiB serialized snapshots.

Retain archive and evidence revisions needed by saved results. Trace identity alone cannot pin the body of a historical annotation. Preserve both the annotation record/revision and its trace target/citations. Source reads must distinguish reading the finding itself from reading the event it annotates. Verify visibility/redaction policy at the host boundary, rather than trusting raw preview payloads by default.

### 4. Do not confuse storage in SQLite with indexed query execution

`ResearchIndex` stores JSON per entity and decodes all entity, annotation and reward rows for each episode before filtering. `read_source` extracts/loads a bundle before slicing the response. The catalog visual slices full bound rows client-side. Transfer pagination alone does not bound parsing, memory or render work.

Extend existing disposable tables/indexes with typed hot fields and push supported predicates/aggregates into parameterized SQLite operations. Preserve verified cache invalidation and authoritative readback. Use bounded source/trace-window loading and virtualized event rendering. Measure cold and warm performance before considering a different database.

## Target user/API workflow

1. Select existing eval job IDs, with child eval inclusion explicitly off by default. Inspect resolved membership, recorded configuration and unavailable fields.
2. Run a typed query without opening a visual or launching analysis. Read results or exact evidence directly.
3. Optionally open a comparison: configuration groups and a selected metric, individual runs/distributions, sample counts and coverage.
4. Select points/bins or a predicate. Open members in the existing trace inspector with exact event identity and saved comparison state.
5. Optionally prepare annotations against selected pinned results. Estimate actual target scope and launch through the existing campaign/reservation route. Plain deterministic/model annotators work with Jesterky absent.
6. Optionally select Jesterky, defaulting analysis workers to Luna low independently of evaluated model/effort. Show per-shard coverage and usage.
7. Refresh after review to obtain a new snapshot. Earlier results, claims and sources remain accessible.

### SQL-esque, without a second query engine

Keep the current typed API as the executable contract. Example supported query shape (membership must first be wired for the selected existing IDs):

```json
{
  "schemaVersion": "synth.trace-query.v2",
  "evalJobIds": ["existing-job-id"],
  "grain": "entities",
  "where": [
    {"field": "eventType", "op": "eq", "value": "tool.result"}
  ],
  "limit": 100
}
```

Outcome predicates, metric selection, distributions and snapshot-member drilldown are planned additions, not fields this example already supports. Extend the versioned typed contract compatibly and expose discoverable field/metric/operator capabilities. A SQL-like editor may later compile a bounded SELECT/WHERE/GROUP BY subset to the same AST; do not add arbitrary SQL or another backend now. The viewer and agent must issue equivalent queries and receive the same values and result references.

## Concrete implementation sequence

### A. Connect existing RuneBench job membership and canonical facts

**Owners:** Workshop `trace_research.rs`, existing eval/optimizer ingest/read model; Evals `core/results` and RuneBench capture/import; Containers research/catalog projections.

- Reconcile the existing research implementation and record its tested build/dependencies.
- Resolve retained Evals jobs through the established import/read-model boundary, preserving original job/trial identifiers. Use a discriminated existing job source if required; do not fabricate optimizer execution history.
- Preserve actual candidate/checkpoint, evaluation stage, model, effort and harness/environment/prompt/protocol revisions. Null remains null. Requested model and resolved provider model must not be silently conflated.
- Normalize call/result identity, outcome and metric reductions. Add queryable actor-terminal rewards and usage facts with source linkage.
- Ingest current RuneBench annotations into the production evidence route without rewriting sealed trace bytes or assigning existing notes to a human.

**Exit:** Query the four retained arms by existing IDs, account for all attempts, reproduce 575/650/500/475 XP and 40/40/40/39 calls with explicit Terra policy error. Retrieve the exact `mab-1` results in low and medium. No demo server, annotation execution or visual is required to answer.

### B. Finish reproducible query/drilldown semantics

**Owners:** Containers `research.py` / catalog; Workshop `data.rs`, trace IPC/MCP and snapshot persistence.

- Add metric selection and distribution summaries (initially reward, cost, response duration, call outcome rate). Preserve individual episode rows and full denominators.
- Make aggregate members navigable within pinned snapshots. Support snapshot-bound follow-up selection, defined ordering and page validation.
- Add typed time/order filters and logical repeated-action/recorded-link relationships only with supported evidence.
- Preserve pinned evidence readback and explicit unavailable/redacted states. Scope annotation coverage by program/version and inspected targets.
- Push predicates into indexed projections where measurements justify it; cap work, not merely returned text.

**Exit:** A 450-result query survives insertion/review between pages and restart with no duplicates/omissions. Grouped values match an independent full-data oracle. Same query without visuals or annotations yields the same core results. Result references resolve after source containers stop.

### C. Make Workshop's comparison a query consumer

**Owners:** `trace.catalog.v1`, host visual transport/bindings and shared `agent_trace.v1`; RuneBench extension owns only game semantics/replay.

- Replace custom inline-data comparison filters with query-backed membership and metrics.
- Provide one compact configuration/metric control area; prioritize charts and evidence instead of duplicated trace toolbars.
- Show individual points at low N; enable distributions when data warrants them. Display N, missing coverage and differing configurations. Do not imply that one run estimates model strength.
- Point/bin selection opens exact members; selecting an event opens the correct actor and canonical selector. Keep game replay optional and timing availability explicit.
- Persist job filters, snapshot, selected runs, actor, event, cursor and alignment through navigation/restart. Use host transport instead of a localhost capability embedded in a visual.
- Reuse the shared inspector's external exact-selection contract. Do not make another transcript renderer.

**Exit:** A reader follows configuration difference → episode subset → `mab-1` contrast → original source → note and back without losing context. The first viewport presents useful data/evidence. Charts exactly match query results. No arbitrary network permission or running port 8118 is required for ordinary retained-trace inspection.

### D. Complete optional annotation/Jesterky branches

**Owners:** existing campaign/annotation APIs, Containers runner and Workshop plugin integration.

- Preparation binds explicit result IDs and their pinned sources. Whole-trace annotators disclose broader scope; selected event IDs do not falsely promise event-only billing/inspection.
- Support archived-source campaigns using existing registered source/workspace mechanisms, or report a precise unavailable-source capability; do not create a fake container identity.
- Keep absent, inspected-no-findings, abstained, failed, cancelled and partial states separate per annotator/version/scope. Refresh only after authoritative revisions are ingested.
- Verify Jesterky all-worker collection, per-shard terminal identity, proposal validation, deduplication and disagreement retention. Exact-byte deduplication is already present; semantic deduplication/synthesis is optional, versioned analysis with its own citations and cost.
- Give cross-actor analysis explicit context or a separately requested reduction. Actor-only partitions cannot establish a team-level claim without the other actors' evidence.
- Test retry/cancel/crash accounting, stale manifest rejection and no repeated successful paid work. Require the existing enforcing proxy for live paid workers.

**Exit:** Deterministic annotation round trip passes with Jesterky disabled. Controlled multi-shard failure/retry passes independently. A separately authorized bounded live Jesterky run proves installed runtime/proxy compatibility. Plugin publication/provisioning remains its own release gate, not a prerequisite for query delivery.

### E. Prove portability, performance and live wiring

**Owners:** environment adapters/Evals QA plus Workshop/Containers acceptance suites.

- Reuse retained Craftax model/tool traces and locate the authoritative DungeonGrid capture route (existing gold/gamebench paths) before deciding an adapter is missing. Do not infer unsupported MA coverage from a renderer.
- Specify independent expected answers for all three environments. Include reused native call IDs across actors/sessions, duplicate physical phases, missing rewards/capture, reviewed evidence and incompatible metric versions.
- Exercise at least 10,000 events/four actors plus 450-result pagination. Measure cold import, warm query, source-page retrieval, first usable render and peak memory. Proposed interactive target: warm first page/source reads under one second on the acceptance host, first usable visual under two seconds; record actuals and adjust explicitly rather than claiming an unmeasured guarantee.
- Run small separately authorized live evals through the existing launcher to prove launch → capture → import → query → source, after retained correctness passes. No new scheduler or factorial experiment orchestration.

**Exit:** Same core API and source semantics across RuneBench, Craftax and DungeonGrid; optional branches fail independently; restart/source shutdown retain saved evidence. Native verification is on the final tested build.

## Prioritization and boundaries

Start A, then B and C as one RuneBench vertical slice. Build D against those same pinned selections; verify E progressively rather than waiting until the end. Do not launch a large model/effort matrix to compensate for incorrect indexing. The four existing configurations suffice for workflow correctness, not distribution inference.

After the first vertical slice works, a balanced repeated-run experiment can populate useful distributions using the existing runner. It is a separately authorized experiment; distinguish unseeded RuneBench repetitions from paired-seed trials. Harness strength comparisons require real harness variants with recorded revisions, not labels.

Defer arbitrary SQL, automated next-experiment selection, a generalized DAG engine, new cohort identities, automatic synthesis, invented cooperation scores and new MA scenario design. None is needed to demonstrate the user's requested path.

## Validation matrix and implementation receipts

| Check | Independent oracle / assertion |
|---|---|
| Membership | Existing job results and authoritative trial/trace refs; no missing failed/pending rows; child inclusion explicit |
| Outcomes | Native call/result records; unsuccessful gameplay distinct from capture error; one logical call despite multiple events |
| Metrics | Terminal reward definitions/actor records; unit/scope/reducer pinned; no one-to-many join inflation |
| Order/links | Known actor/session ordering, exact call IDs/targets and recorded delivery links; unknown remains unknown |
| Annotation revision | Create/review/supersede; old query and annotation body stay readable; coverage specific to selected program |
| Snapshots | 450 rows, intervening import/review, restart, stopped source; pinned pages and drilldown retain membership |
| Visual | Plot values equal query oracle; click resolves exact evidence; saved navigation state; compact/wide/native |
| Jesterky | Controlled all-shard outputs/failure/retry/usage, then separate live enforcing-proxy smoke |
| Portability | Retained expected answers and small live wiring on three environments; no inference in correctness tests |

Write tests in established Workshop/Containers locations. Evals currently prohibits new/extended tests unless explicitly requested; use its existing checks and add cross-environment retained integration coverage in Containers/Workshop. Record build/dirty dependencies, commands, fixture digests, actual job IDs, snapshot/result IDs, exact evidence references and native screenshots in the delivery receipt. Historical test logs and component exports are not substitutes for the acceptance path.

## Code anchors

- [Workshop job resolution and preparation](../../apps/synth_desktop/src-tauri/src/trace_research.rs)
- [Workshop snapshot/source operations](../../apps/synth_desktop/src-tauri/src/data.rs)
- [Trace tool contract](../../apps/synth_desktop/src-tauri/src/bin/synth_traces_mcp.rs)
- [Containers V2 research implementation](../../../containers/src/synth_containers/tracing/research.py)
- [Existing catalog](../../../containers/src/synth_containers/tracing/store/sqlite_catalog.py)
- [RuneBench V5 adapter](../../../containers/src/synth_containers/tracing/adapters/runebench.py)
- [Jesterky runner](../../../containers/src/synth_containers/tracing/annotation/jesterky_runner.py)
- [Evals metadata axes](../../../evals/core/results/research.py)
- [Generic query visual](../../visuals/families/analysis/trace.catalog.v1/shell.tsx)
- [Current RuneBench custom viewer](../../../evals/workshop/runebench-ma-demo/ma/viewer.tsx)
