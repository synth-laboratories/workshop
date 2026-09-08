# Workshop experiment and trace research: consolidated context and handoff

Date: 2026-09-08
Originating chat: **Review eval trace scope**
Status: local implementation and recorded acceptance completed; official Jesterky distribution deferred.

## Purpose and source context

Build a fast agent-driven loop for running experiments, investigating their traces,
and using the evidence to choose the next change. The motivating dimensions are
**prompt × messaging protocol × harness × model × reasoning effort**, applied to
DungeonGrid, Craftax and RuneBench.

The user supplied [Max Bittker’s tweet thread](https://x.com/maxbittker/status/2096986048186229013)
and described the desired data-query/pipeline workflow in this chat. The original
review could not retrieve the post. This document consolidates the scope agreed
with the user; it is not a verified transcript or independent interpretation of
every post in Max’s thread.

The intended outcome: an agent launches evals, gets durable traces and rewards,
queries and inspects evidence, optionally annotates or visualizes selected results,
and rapidly iterates. This supports both evaluation research and diagnosis of
environment or harness problems.

## Agreed feature set

- **Rapid experiment loops:** vary prompts, protocols, harnesses, models and effort
  using existing eval-launch controls. Preserve the actual configuration provenance.
- **Launch and collect:** native Optimizers jobs produce retained rollouts, sealed
  Trace V5 bundles, objective rewards and configuration/environment identities.
- **Query results:** filter rewards, annotations, configuration, actors, actions and
  recorded coordination facts; group outcomes and save reproducible result sets.
- **Inspect evidence:** query rows resolve to exact trace entities, with optional
  data, environment and rollout views where capture supports them.
- **Compare experiments:** grouped results and compatible paired rewards, with
  optional comparison visuals and explicit incompatible, ambiguous or missing data.
- **Annotate selected rollouts:** ordinary deterministic/model or human annotation
  can happen after an eval, independently of a dedicated annotated eval job.
- **Optional Jesterky:** downloadable/local-installable analysis runner, skills,
  MCP and visuals; Luna/low defaults; bounded long-trace and multi-agent analysis,
  result collection, reduction and resumable processing.
- **Close the loop:** use findings to diagnose failures, revise the experiment and
  rerun while retaining source provenance, annotation revisions and review history.

## Decisions that must survive future changes

1. **Existing eval job IDs are enough.** Select one job, multiple jobs, or a filtered
   subset. There is no extra trace-cohort entity or alias-ID registry.
2. **Annotations are optional.** Queries and objective reward analysis work without
   them. Annotation scores never overwrite objective rewards.
3. **Jesterky is optional.** Ordinary annotators work independently. Reading a query,
   saving a setting or preparing a selection never implicitly starts paid analysis.
4. **Visuals are optional consumers.** Agents can read comparisons directly through
   tools. Rendering is not a required pipeline stage.
5. **Reuse existing authority and orchestration.** Keep Trace V5, current eval jobs,
   query snapshots, annotation campaigns and visual bindings. Do not introduce a
   replacement trace store, scheduler or general workflow engine.
6. **Evidence must remain honest.** Missing capture is unknown; no recorded
   acknowledgement does not prove an agent ignored a message. Failed, missing,
   partial, invalid and zero-valued outcomes are distinct.

## End-to-end target workflow

Example research question: does an explicit acknowledgement protocol reduce
coordination failures without lowering task reward?

1. Choose an environment/task and baseline configuration. Record prompt, protocol,
   harness, model, effort, environment version and seed/repeat when available.
2. Launch baseline and candidate using existing Optimizers eval jobs. Initially
   change one axis and hold the others fixed for an interpretable comparison.
3. Query both job IDs. Check rollout accounting, completion, missing rewards and
   capture availability before interpreting averages.
4. Compare compatible task/seed/repeat pairs and grouped rewards. Inspect exact
   actions or recorded message relationships associated with poor outcomes.
5. Optionally open replay, actor lanes or a query/comparison visual. Domain views
   depend on the facts actually captured by that environment.
6. Select interesting rollouts across either job. Optionally estimate and start an
   ordinary annotation campaign or Jesterky analysis without rerunning evaluation.
7. Query findings alongside rewards. Review, accept/reject or supersede annotations;
   distinguish measured facts from labels such as “stale plan” or “ignored handoff.”
8. Save the query/evidence, revise the next configuration and launch another arm.
   Reopening Workshop must preserve earlier answers and exact source evidence.

```text
 prompt / protocol / harness / model / effort
                       |
                       v
            Existing Optimizers eval jobs
                       |
                       v
     Rollouts + sealed V5 traces + typed rewards
                       |
               +-------+--------------------------+
               |                                  |
               v                                  v
        Query / source read              Optional selected-rollout
               |                         annotation campaign
               |                                  |
               |                         ordinary / human / Jesterky
               |                                  |
               +<---- evidence / review revisions--+
               |
        Saved, pinned query result
               |
       +-------+----------------------+
       |                              |
       v                              v
 Agent reads / next iteration   Optional visual / comparison
                                      |
                                      v
                              Exact trace evidence / replay
```

This loop is composable through existing tools. Automatic factorial scheduling,
next-arm proposal and automatic finalist selection are not part of this delivery.

## How jobs, containers, traces, queries and annotations relate

```text
 Existing eval job ID
       |
       +-- rollout membership + execution/configuration metadata
                    |
                    +-- container/runtime identity + environment version
                    |
                    +-- sealed trace ID + digest
                    |       |
                    |       +-- actors / sessions / ordered events
                    |       +-- actions / spans / message parts
                    |       +-- recorded coordination links
                    |
                    +-- objective reward + definition + units + provenance
                    |
                    +-- optional annotation evidence
                            +-- canonical trace/entity selector
                            +-- producer / version / label / score
                            +-- coverage / status / review revisions

 Workshop projections index these existing facts.
 Query snapshots pin membership, evidence revisions and source references.
 Visuals and analysis selections consume those results.
```

Containers owns trace structure, selector validation and sealed evidence. Evals
owns task and reward meaning. Workshop orchestrates existing jobs/campaigns,
projects queryable facts, preserves query snapshots and presents results. Jesterky
is an optional producer of separately attributed annotation evidence.

## Query capabilities and correctness rules

| Query family | Intended supported use |
| --- | --- |
| Job/configuration | Select job IDs, environment/task, recorded version, seed/repeat, lifecycle, prompt/protocol/harness and resolved model/effort where recorded |
| Rewards | Numeric filters, missingness, metric/units/definition, grouped outcomes and compatible paired deltas |
| Trace entities | Actor/session, kind, action/tool, status, supported fields and recorded ordering |
| Coordination | Exact recorded delivery/acknowledgement relationships; missing evidence remains unknown |
| Annotations | Labels, typed scores, producer/version, target, review state and supersession |
| Combined queries | Low-reward episodes with selected findings, without duplicating episode reward contributions |
| Saved results | Bounded pagination, pinned evidence, stable membership, exact source reads and restart persistence |

Use the versioned typed query contract, not arbitrary SQL or arbitrary JSON
programs. Available fields depend on capture. Illustrative questions are not a
promise that every environment captures every dimension.

Reward comparisons require compatible definitions, units and environment versions.
Seed or explicit repeat identity is needed for pairing; ambiguous and unmatched
rows are explicit. Legacy traces without typed reward definitions or version facts
remain unknown. Joining multiple findings must not multiply episode denominators.

“Latest” evidence is resolved once for a query snapshot. New imports or reviews
produce refreshed results; they do not mutate earlier saved answers or active pages.
Every result must resolve its source or explain why that source is unavailable.

## Independent Jesterky use

Users can select rollouts from earlier eval jobs with no current annotated eval.
Preparation deduplicates sealed trace identities and returns explicit per-container
campaign selections. Whole selected rollouts are the default saved scope; event-only
mode must retain exact event scope or refuse unsupported preparation.

Luna/low are analysis defaults, independent of the policy model/effort used during
evaluation. Existing estimates, budgets and campaign execution remain authoritative.
Long traces are partitioned into bounded actor/window work, then worker results are
collected, validated, deduplicated and reduced with citations, coverage and usage.
Interrupted work must preserve successful results rather than charge for them again.

## Implementation by repository

| Repository | Completed local work |
| --- | --- |
| Workshop | Native eval-to-query wiring, typed research queries/source reads, saved results, selected campaigns, optional query/comparison visuals, independent annotation replay overlay, Jesterky plugin/MCP/settings integration |
| Containers | Typed terminal rewards, persisted environment versions, trace/annotation projections and source contracts, relationship correctness, optional Jesterky collection/recovery integration |
| Evals | Permanent RuneBench reconstruction and launch integration; Craftax/DungeonGrid reward definitions; isolated native acceptance engines and permanent DungeonGrid build |
| Jesterky | Platform packages and analysis/recovery paths; receipt-verified local Linux artifact installation alongside the official-download path |

Core Workshop entry points include `trace_research.rs`, `trace_query.rs`,
`trace_ingest.rs`, `annotations_ipc.rs`, `data.rs`, existing Optimizers integration
and `synth_traces_mcp.rs` under `apps/synth_desktop/src-tauri/src`.
The optional views use `visuals/families/analysis/trace.catalog.v1` and
`trace.rollout_inspector.v1`.

The reconstructed RuneBench source lives permanently in
`/Users/joshuapurtell/GitHub/evals/containers/images/runebench-harbor-codex`.
The owned DungeonGrid acceptance build lives in
`/Users/joshuapurtell/GitHub/evals/workshop/runtime-builds/dungeongrid`.
Required runtime sources must not be placed in temporary directories.

## Recorded acceptance and its limits

- **Native RuneBench:** actual Optimizers job `opt_eval_runebench_676598085674`
  completed with 23 provider calls and reward 212 normalized XP/min. Job-scoped
  trace/configuration/source queries and store reopen passed. Seed is explicitly
  null; task/repeat and runtime image identity are preserved.
- **Native Craftax/DungeonGrid:** four jobs, eight rollouts and four matched pairs
  passed typed/scalar reward agreement, denominator, environment-version, source
  and optional visual checks, including after actual app restart.
- **Independent campaigns:** two selected rollouts annotated, two unselected;
  findings reconciled into queries and rewards stayed unchanged. Native replay
  separately passed display and inspection of an independent finding.
- **Review history:** recorded review/query tests preserve prior saved answers and
  source evidence while refreshed results reflect new evidence revisions.
- **Coordination:** exact acknowledgement selectors and actor/session/order guards
  passed focused tests. Missing links explicitly remain unknown.
- **Scale:** pinned pages over 450 results survive a new import and restart; refresh
  sees 451. Retained 10,002-event/four-actor trace checks cover bounded viewing and
  Jesterky partition/recovery paths. Controlled recovery tests complement a separate
  bounded live Luna/low smoke; they are not themselves live multi-agent evaluation.
- **Jesterky platforms:** 92 selected tests passed per platform; two live tests per
  suite were ignored and separate live evidence is retained. Native macOS plugin
  install/MCP/enable-disable/restart passed. Linux ARM64 and emulated x86_64 local
  installation, version and trace validation passed.
- **Latest focused changes:** 21 query/portable-bundle tests passed; diff checks passed.

Memory measurements varied: final observed peak process-RSS sum was 1,017,600 KiB,
with an earlier 1,788,608 KiB measurement. No fixed performance threshold is claimed.
These checks establish local implementation evidence, not a published-release gate,
a full factorial study or complete capture for every environment/scenario.

## Remaining work

1. Review and commit the relevant changes across dirty local repositories without
   disturbing unrelated work. Source fingerprints and receipts identify the tested
   local state; there is not one clean released commit covering everything.
2. Publish verified Jesterky artifacts using authorized release-write credentials or
   a functioning release workflow, and complete macOS signing/notarization.
3. Populate the official Workshop catalog only after published URLs serve the
   matching verified bytes. The official catalog remains empty.
4. Run official-distribution acceptance: catalog download, hash verification,
   installation, enable/disable and supported remote delivery. Local installations
   have passed; official downloads have not.

Deferred product expansion, rather than unfinished implementation in this delivery:
full factorial scheduling, automated next-arm/finalist selection, held-out study
orchestration, a generic DAG engine, new Craftax multi-agent scenarios and expanded
RuneBench game modes. Do not quietly treat these as already delivered.

## Operational handoff

- Jesterky is installed in the isolated **Workshop Trace Acceptance** app; this did
  not modify the other running Workshop installation.
- Owned test servers are stopped. The original DungeonGrid server and original
  RuneBench container were preserved. Provider capabilities were revoked.
- No Keychain access occurred. Use authorized project-local `.env` credentials and
  Workshop’s secrets proxy. Do not use the Keychain-backed Secrets registry or its
  import flow without explicit authorization for that operation.
- The announced provider maximum was $20, expected actual below $3. The final native
  RuneBench run reported $0.014922 proxy usage; all four native attempts reported
  $0.064384, distinct from earlier direct-facade usage and actual account billing.
- New provider runs must respect the user’s existing scope/budget rules. This
  document does not request another run or authorize a larger experiment.

## Detailed plans and evidence

- [Original finalized scope and E1–E10 acceptance specification](EXPERIMENT_TRACE_RESEARCH_WORKFLOW_2026-09-07.md)
- [Implementation notes and detailed qualifications](TRACE_RESEARCH_IMPLEMENTATION_2026-09-07.md)
- [Final acceptance status](../../artifacts/trace-research-e2e/final-acceptance-status.json)
- [Source fingerprints and retained evidence hashes](../../artifacts/trace-research-e2e/validation-manifest.json)
- [Typed native paired-comparison receipt](../../artifacts/trace-research-e2e/native-launch/typed-pair-acceptance.json)
- [Native RuneBench acceptance](../../artifacts/trace-research-e2e/native-runebench/native-launch-acceptance.json)
- [Local Jesterky plugin installation](../../artifacts/trace-research-e2e/local-jesterky-install.json)
- [Jesterky platform acceptance](../../artifacts/trace-research-e2e/jesterky-platforms/acceptance.json)

The original plan describes desired behavior; the implementation notes and receipts
qualify what was actually tested. Preserve that distinction when making release claims.
