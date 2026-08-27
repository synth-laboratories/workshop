# Workshop visuals and data model

Date: 2026-08-25 (updated 2026-08-26)  
Tied to: [`v08-right-panel-cua-20260825.md`](v08-right-panel-cua-20260825.md)  
IA reference: [`refs/eiso-kant-model-factory-2026-08-18.png`](refs/eiso-kant-model-factory-2026-08-18.png) — Eiso Kant / Poolside Model Factory (index beside lineage canvas)

## Legend — core nouns

```
Workshop                         product (Synth Desktop). Not a domain record.
│
├─ Workshop window               Tauri + WebView chrome. “Workbench” in code is this, not a noun.
│    ├─ Sidebar                  chats, Visuals, Reports, Experiments, Data, Optimizers, Settings
│    ├─ Main column              the page for the current route
│    └─ Right pane               VisualPane / Outputs / inspect
│         └─ ArtifactRef         viewer pointer. Not VisualRecord; status is VisualStatus
│                                (draft|live|saved|failed|archived). Reviews stay on metadata.
│                                Chat + inventory routes share one VisualPane host.
│
├─ Session                       one chat. Kind is exactly one of:
│    ├─ Codex                    app-server → Responses (local or Shoal Laguna)
│    └─ Intern                   Synth Cloud (sync | async)
│
├─ Local store                   sqlite + journal + CAS. Durable authority, not a class.
│    ├─ sqlite / WAL             ids, revisions, edges, indexes
│    ├─ event journal            append-only facts (rebuildable projection)
│    └─ CAS                      traces, seals, checkpoints, TSX/HTML bodies
│
└─ Records                       class (shipped)  →  instance (row in the local store)
     │
     ├─ Visual
     │    ├─ Visual template     class: blank.canvas.v1, shell.tsx under visuals/families
     │    └─ VisualRecord        instance vis_…  (SQLite row + optional CAS body)
     │         └─ VisualRevision numbered snapshot
     │              ├─ Binding   input fill (stream, document, …)
     │              └─ VisualSeal receipt that a rendition is immutable
     │
     ├─ Report                   pointer document, not a copy of the visual
     │    ├─ Report block schema class: report.visual.v1, report.prose.v1, …
     │    └─ ReportRecord        instance rep_…
     │         └─ ReportRevision numbered snapshot
     │              ├─ ReportBlock  pointer at vis_ / digest
     │              ├─ ReportSeal   receipt that this revision is frozen
     │              └─ ExperimentRecord  appendix JSON; optional pointer at ExperimentGroup
     │
     ├─ Experiment               durable row, many per session
     │    ├─ ExperimentGroup     identity + task/model + members
     │    ├─ ExperimentNode      member: optimizer_run | eval_campaign | direct_evaluation
     │    ├─ CandidateRecord     hangs off optimizer_run (producer id, not a member kind)
│    ├─ experiment_edges    intra-group: evaluated | compared_with | promoted_to
│    └─ experiment_lineage  inter-group: follow_up | forked_from | rerun_of
     │
     ├─ Container
     │    ├─ capability          class: live-eval.v1 ops
     │    └─ ContainerDeployment instance in the registered pool
     │
     ├─ Optimizer
     │    ├─ recipe              class: gepa / sft / eval
     │    └─ OptimizerRun        one execution; attaches as optimizer_run when session_ref set
     │
     └─ Plugin                   PluginStatus.phase (14 values) + PluginNotReady receipt
                                 LagunaStatus is a parallel machine, not this registry
                                 CUA is a plugin id the agent cannot install
```

Records persist in the local store. The renderer is a projection. The same visual is one local record, then five independent projections. The CUA contradictions are those projections disagreeing, plus two different “seal” verbs. Rust `CoreRuntime` is the composition root that opens the store; it is not a domain noun.

## Cuts landed 2026-08-26

Experiment is no longer three leftover types. Member DAG and child experiments are local-store rows. The renderer canvas is a projection of those rows, not a second graph store.

- **Intra-experiment.** Members are `optimizer_run` | `eval_campaign` | `direct_evaluation`. The only written member edge is `evaluated` on `experiment_edges` (node FKs inside one group). Historical `baseline` | `variant` | `result` | `run` rows stay readable; nothing new writes them. `optimizer_relationships.started_from` is not an experiment edge.
- **Inter-experiment.** Migration 38 dropped `experiment_groups.session_id` uniqueness. A session owns many groups. Parent → child is `experiment_lineage` with `follow_up` (default), `forked_from`, or `rerun_of`. `create_child.relation` selects which; unknown fails closed. Follow-up cannot live on `experiment_edges` (those FKs are nodes, not groups).
- **Attach target.** `experiment_session_cursor` → `sessions.active_experiment_id` → oldest group for that session. Cursor exists because some `session_id` values are not `sessions` rows. Do not attach an optimizer when `session_ref` is empty.
- **Producers.** Optimizer `experiment_bind`: attach on create/seed/import; settle on terminal event commit, cancel, and stale-run reconcile. Terminal settle writes `cost_usd` and attaches `visual_refs` as evidence kind `visual`. Campaigns already attached themselves.
- **Surfaces.** `experiments_create_child` / `experiments_activate` (Tauri + MCP `experiment_create_child` + HTTP). GET by experiment id returns that child, not the session primary. UI: searchable index | ranked pan/zoom DAG | inspector. Forest when nothing selected; member DAG when selected. Compact stack at 780px. Layout coords are view state, not sqlite.
- **Candidate.** Durable sqlite class on the experiment spine (migration 39). Rows hang off an `optimizer_run` member: producer id, kind, `protocol_id`, parents, metrics. Read DTO is `ExperimentNode.candidates[]` (empty for eval/direct and for SFT/CISPO with no candidate events). Folded from `optimizer_event.v1` already parsed in `optimizers/service.rs`. Container still owns `POST /candidates`.
- **Compare / promote.** Member↔member writes `experiment_edges` `compared_with` | `promoted_to`. Candidate↔candidate stays on the Candidate row (`compared_with_json`, `promoted_to`; migration 40) — not a member kind and not an experiment edge. Mixed kinds fail closed. `experiments_relate` (specta 264). Auto-attach still only writes `evaluated`.
- **Report appendix.** `reports.ExperimentRecord.experimentGroupId` is an optional pointer at `ExperimentGroup`. Unlinked rows stay appendix JSON (`appendix · unlinked`). Unknown group id fails closed. Arms/runs/results stay. `reports_promote` stays off agent visuals MCP.

Plugin was never a boolean. `PluginStatus.phase` has 14 values; `pluginPresentation.ts` is the single presentation owner; recipes gate on `require_plugin_ready`. Laguna is a parallel `LagunaStatus` object, not a plugin registry row. CUA is plugin id `computer-use` (human-only install).

Window chrome: Chat, Visuals, Experiments, Optimizers, Data, and Reports share one pane host so `VisualPane` does not remount across those routes. Settings joins that host only while a pane is open. `ArtifactRef.status` is `VisualStatus`. Chat Outputs ownership does not evict the window pane. CloudDesk still mounts its own pane and stays unmounted. Data/Outputs catalogs show `formatVisualAdmissionIdentity` (title stays the human label).

Projection closeout (2026-08-26): Visuals/Reports filtered-empty states no
longer claim the registry is empty; Data, Chat Outputs, Visuals, the pane, and
Reports preserve the `id + revision + labeled digest` join key. The Templates
tab is named **Template visuals** because it is still a VisualRecord projection,
not the shipped template catalog. The shared pane close path unwinds temporary
pane modes and restores workbench focus; focus visual is review/presentation,
not an editor.

- **list_templates components.** `TemplateMeta.inputs` copies `template.json` `inputs`/`slots` (same vec; `slots` is a one-release copy). `TemplateMeta.components` copies `template.json` `components[]`. MCP `visual_list_templates` / GET `/v1/visuals/templates` echo both. Empty `components` when unadvertised. No `list_components` or `list_inputs` verb. Specta field-on-existing-type (no bump at that cut).
- **Laguna vs Plugin.** Catalog and MCP `plugin_id` remain `optimizers` (catalog) + `computer-use` (human-only, no catalog). `PLUGIN_NAV` has no Laguna row. `require_plugin_ready` is the Optimizers sidecar. `LagunaStatus` stays a parallel object.
- **Admission (visual/diagram).** `admit_visual_evidence` is the shared predicate: pin and report seal of `report.visual.v1` / `report.diagram.v1` require a `VisualSeal` for that `visualId`+revision. Attach still writes a live pointer (`integrity=unresolved`); if a seal exists it copies `receiptDigest` into `sourceDigest` and stays `referenceMode: live` until pin. `validate_revision` errors `unresolved_visual_evidence` so “Ready to seal” is no longer shape-true for a blank canvas. Experiment-records / research-log appendix stay on auto-limitations. `contentDigest` and `receiptDigest` stay labeled, never merged. No `ArtifactRevision` sqlite class.

- **Sourced / compose CUA (packaged).** Instance `sourced-cua` (`com.synth.desktop.v08.dev.sourced-cua`). Compose `vis_4fb0164a8f544ad0a6602fe73662b848` (`CUA-SOURCED-1`); sourced `vis_1a80f20f24c04fef96bc774c881d86c2` (`CUA-SOURCED-2`, authored TSX + host EventStream); fail-closed `vis_bdac981b99604681ad814e64d5d0e948` (`Unknown import "lodash"`, no event log). Captures under that instance’s `visual-review-captures/`. Original `workshop-v08-release` not committed.
- **Compose later kit.** `metrics.v1`, `scrubber.v1`, `candidate_inspector.v1` advertised on `compose.visual.v1`. Product `optimizer.*` overlays stay private.

## Core nouns (how Workshop works)

```
                         Workshop window  (Tauri host + WebView)
  ┌────────────┬──────────────────────────────────────────────┬──────────────┐
  │  Sidebar   │           Main column                        │  Right pane  │
  │            │                                              │              │
  │  chats     │   page: Chat | Visuals | Reports |           │  VisualPane  │
  │  Visuals   │         Experiments | Data | Settings        │  or Outputs  │
  │  Reports   │                                              │  or inspect  │
  │  Experiments                                              │              │
  │  Data      │   transcript / registry / DAG / catalog      │  inventory   │
  │  Optimizers│                                              │  host shared │
  │  Settings  │   composer ──────────────────────────────┐   │              │
  └────────────┴──────────────────────────────────────────┼───┴──────────────┘
                                                          │
                     terminal PTY ────────────────────────┘


  CLASSES (definitions, shipped in the app)          INSTANCES (durable rows)

  Visual template          blank.canvas.v1           VisualRecord      vis_…
    rendererKind           template|systems|…          VisualRevision
    inputs[]               stream, document            bindings
                                                       VisualSeal      receipt  (optional)

  Report block schema      report.visual.v1          ReportRecord      rep_…
                           report.prose.v1             ReportRevision
                                                       ReportBlock     pointer at vis_/digest
                                                       ReportSeal      receipt  (optional)

  Experiment               task / model on the group  ExperimentGroup     exp_…  (many per session)
                           member kinds                 ExperimentNode      optimizer_run | eval_campaign | direct_evaluation
                           follow_up | forked_from |    experiment_lineage  parent → child
                           rerun_of                     experiment_edges    evaluated | compared_with | promoted_to
                                                       CandidateRecord      can:… on optimizer_run
                                                                            compared_with / promoted_to on the row
                                                       reports.ExperimentRecord  appendix; optional ExperimentGroup pointer

  Container capability     live-eval.v1 ops          ContainerDeployment  registered pool
  Optimizer recipe         gepa / sft / eval         OptimizerRun         attached via experiment_bind
  Plugin / service         PluginStatus.phase        Optimizers, CUA      14-phase registry + receipts
                           LagunaStatus.phase        Laguna sidecar       separate status object


  SESSION  (routing law: exactly one runtime)

      Session ──kind──► Codex ──► app-server ──► Responses provider
                    │                              ├─ local Laguna XS (MLX sidecar)
                    │                              └─ configured / Shoal-hosted Laguna
                    └──kind──► Intern ──► Synth Cloud  (sync | async)


  AUTHORITY  (who may be telling the truth)

      local disk     sqlite + journal + CAS                 default, no account
      backend        Synth Cloud intern / publish           explicit opt-in
      Shoal          hosted Laguna desired vs observed      serving
      Modal          NanoHorizon iteration                  not a Workshop route


  WHAT THE LOCAL STORE HOLDS

      local store
        ├── sqlite / WAL     ids, revisions, edges, indexes
        ├── event journal    append-only facts  (rebuildable projection)
        └── CAS              traces, seals, checkpoints, export bodies
              ▲
              │  typed Tauri commands  (renderer never owns authority)
              │
         VisualsPage  ReportsPage  ExperimentsPage  DataPage  Chat Outputs
              │            │              │              │         │
              └────────────┴──────────────┴──────────────┴─────────┘
                           each re-projects the same rows
                           with a different status vocabulary
```

## Experiment zoom (two graphs, one sqlite)

Kind is the load/run contract of a member. Relation is a typed edge. Do not invent a property graph, a second sqlite, or `baseline`/`variant`/`result` nodes.

```
  session  ──many──►  ExperimentGroup (exp_…)
                          │
                          ├─ members     optimizer_run | eval_campaign | direct_evaluation
                          ├─ nodes       one row per member (FK inside this group)
                          ├─ candidates  CandidateRecord[] on optimizer_run nodes (sqlite, not overlay JSON)
                          ├─ edges       evaluated | compared_with | promoted_to  (member → member)
                          └─ lineage     follow_up | forked_from | rerun_of  (this group → child group)

  attach target for the next run:
      experiment_session_cursor.active_experiment_id
        else sessions.active_experiment_id
        else oldest experiment_groups row for that session_id
```

Renderer: list stays mounted. Empty selection shows the forest (`follow_up` / `forked_from` / `rerun_of`). Selecting a group drills into that group's member DAG. `+ child` / `+ fork` / `+ rerun` call `experiments_create_child` with the matching relation. Canvas ranks are computed in the WebView.

GEPA `candidate_id` is a durable `CandidateRecord` on the `optimizer_run` node (`experiments_get` / list). Kind is the load/run contract; `protocol_id` is the mutation dialect. Do not stuff it into member `kind`. Overlay JSON is no longer the authority. Compare / promote: members write `experiment_edges`; candidates write columns on the Candidate row. Mixed kinds fail closed.

## Visual composition (how it works)

The agent already has three escape hatches. None of them is a kit.

```
  shipped template          whole pane class     optimizer.gepa.live.v1, live.eval_stream.v1
  analysis.visual.v1        static block spec    note | metrics | ranked-bars | table | scatter
  blank.canvas.v1           dump HTML/SVG        reinvent chrome every time
```

Ingest already exists and must stay owned by the host:

```
  agent bind input "stream"  (kind live_sse, declared poll_url — never guessed)
       │
       v
  VisualHost  →  ReplayClient   (visual_replay_transport.md)
       │
       v
  useLiveEvalStream / useLiveEvalStreams
       │
       v
  events[] + transport state   idle|declared|replaying|live|terminal|error
```

Templates must not discover URLs. `live.eval_stream.v1` is the shortcut whole pane: same host ingest, advertised `metrics.v1` / `scrubber.v1` / `event_stream.v1` / `detail_modal.v1`, no compose spec required. Optimizer families share `RunChrome` / `GlobalTimeline` / candidate inspector as private TSX. Those overlays stay unadvertised.

**Custom visuals are the point.** The agent authors a pane and Desktop **runs it**. The old rule (“saved TSX is evidence, never executed”) was a scar from unconstrained modules that fetched their own URLs. That is not the product. Two authoring dialects, same host ingest, same advertised components:

```
  compose spec     JSON placements of shipped parts     compose.visual.v1
  sourced_visual   agent TSX that imports those parts   kind sourced_visual
```

`blank.canvas.v1` stays HTML/SVG with no scripts. It is not the TSX path.

### Nouns

```
  Visual template          class: whole pane. Still one per VisualRecord.
  VisualRecord             instance vis_…  (unchanged)
  bindings                 how data enters  (stream, spec, …)
  Visual component         class: shipped renderer + bind dialect
  placement                instance: { id, component, input?, from?, config }
```

Kind is the load/render contract (`event_stream`, `detail_modal`, `sourced_visual`). `protocol_id` is the bind dialect (`event_stream.v1`, `whole_file.v1`). A placement is not a VisualRecord and not a node in a property graph.

### Compose template (agent path)

Do not grow `analysis.visual.v1` a live `stream` input. Static analysis stays static. New template `compose.visual.v1`:

```
  inputs:
    spec            required   inline   synth.visual.compose_spec.v1
    stream          optional   live_sse / fixture / inline     container eval
    optimizer_run   optional   optimizer_run / fixture / inline   optimizer_event.v1
```

`stream` / `optimizer_run` are required at bind time only if a placement consumes that input. Unknown `component` ids fail closed (same as unknown protocol ids). Layout is an ordered vertical list. No xy graph. Product `optimizer.*` and `diagram.*` do not switch.

Compose is the ad-hoc live overlay for **evals and optimizer event streaming**. It does not replace `optimizer.gepa.live.v1` / `optimizer.sft.live.v1` / `optimizer.eval.live.v1`. Those stay product-owned. Two host-owned bind dialects, not one mashed firehose:

```
  stream          live_sse / fixture / inline     container eval (Harbor, Craftax gold, …)
  optimizer_run   optimizer_run / fixture / inline   optimizer_event.v1 (GEPA, SFT, CISPO)
```

`includeKinds` matches envelope `kind` or `type`. Do not flatten child eval traces into optimizer events. Do not invent a generic `rlvr.*` pane: hosted RLVR is **CISPO** (`algorithmId: cispo`, `cispo.*` events). Exact RLVR payloads stay producer-owned; compose filters by those type names.

```json
{
  "schemaVersion": "synth.visual.compose_spec.v1",
  "title": "Harbor smoke · live stream",
  "lede": "Declared SSE only. Heartbeats are not evidence.",
  "placements": [
    { "id": "log", "component": "event_stream.v1", "input": "stream",
      "config": { "includeKinds": ["rollout.finished", "run_finished"] } },
    { "id": "inspect", "component": "detail_modal.v1", "from": "log" }
  ]
}
```

Optimizer dialect (same components; placement `input` is `optimizer_run`):

```json
{
  "schemaVersion": "synth.visual.compose_spec.v1",
  "title": "CISPO clip · optimizer_run",
  "placements": [
    { "id": "log", "component": "event_stream.v1", "input": "optimizer_run",
      "config": { "includeKinds": ["candidate.accepted", "sft.training.metrics", "cispo.clip.identity"] } },
    { "id": "inspect", "component": "detail_modal.v1", "from": "log" }
  ]
}
```

Agent still uses `visual_manage` `create_with_bind` / `bind`. No new MCP verb per component. `list_templates` echoes `components[]` (id, kind, protocolId, consumes, emits) next to `inputs` and `example_binding`. Empty when the template does not advertise parts. There is no `list_components` verb.

### Runtime (one ingest, many placements)

```
  VisualHost
    load shell compose.visual.v1
    ReplayClient from bound stream input        (eval SSE; idle if none)
    optimizerPayload from bound optimizer_run  (subscribeToRun / inline events)
         │
         v
  Compose shell
    useLiveEvalStream once when a placement consumes stream
    optimizerEventsToLiveEval when a placement consumes optimizer_run
    cursor: { placementId, eventId, sequence }     view state, not sqlite
         │
         ├─ event_stream.v1    reads the input it named; writes cursor on select
         └─ detail_modal.v1    reads cursor via `from`; in-pane overlay
```

`detail_modal` is not a second VisualPane and not a second VisualRecord. It is a dialog/drawer inside this visual's chrome. Cursor identity is envelope `event_id` + `sequence`, not array index (replay re-ingests).

Components never call `poll`, never read bindings, never invent SSE URLs. Filter (`includeKinds`) is placement config, not a second stream.

### Advertised compose components

```
  event_stream.v1           kind event_stream
                            consumes stream | optimizer_run
                            emits cursor
                            renderer: visuals/components/event_stream.v1

  detail_modal.v1           kind detail_modal
                            consumes cursor from a named placement
                            renderer: visuals/components/detail_modal.v1

  metrics.v1                kind metrics
                            protocolId metrics.reduce.v1
                            consumes stream | optimizer_run
                            reduce events → strip

  scrubber.v1               kind scrubber
                            protocolId scrubber.v1
                            consumes stream | optimizer_run
                            emits cursor

  candidate_inspector.v1    kind candidate_inspector
                            protocolId candidate_inspector.v1
                            consumes optimizer_run (fail closed on stream)
                            emits cursor on select
                            empty/honest when no candidate.accepted
```

Product `optimizer.*` overlays stay private. `live.eval_stream.v1` is the shortcut whole-pane template on that same advertised kit.

### What the agent does

1. Ground: declared stream URL from the container / create-rollout receipt, or an `optimizer_run` id. Guessed Craftax/Harbor URLs still fail `liveStream.ts`.
2. `create_with_bind` `compose.visual.v1` with `spec` inline, then bind `stream` (`live_sse` + `poll_url`) or `optimizer_run` (`optimizer_run` / inline `optimizer_event.v1`).
3. Host stores one VisualRecord. Compose shell hydrates from ReplayClient and/or host optimizer payload. The two dialects stay separate.
4. `show`. Capture/review still apply to authored families. `optimizer.*` stay product-owned.

`live.eval_stream.v1` remains a shortcut whole-pane template (kit rewrite landed). `blank.canvas.v1` remains last resort.

### Sourced visual TSX (agent custom pane)

Kind `sourced_visual`. Protocol `whole_file.v1`. Register-then-show: host stores the module on the VisualRecord, compiles it, mounts it as the pane Shell. Do not re-compile per seed. Same MCP: `create` / `update` with source, `bind`, `show`. No eval-in-the-renderer of random strings.

Allowlisted imports only:

```
  react / react-dom
  @synth/visuals/chrome
  @synth/visuals/components/<advertised id>
  useLiveEvalStream   (consumes host ReplayClient; does not discover URLs)
```

Unknown import, `fetch`, `EventSource`, `eval`, or a guessed `/events` URL fails closed (visible error, no shell). Host still builds `ReplayClient` / `optimizerPayload` and passes `replay`, `events`, `state`. The agent TSX lays out advertised components; it does not own ingest.

```tsx
import { EventStream } from "@synth/visuals/components/event_stream.v1";
import { DetailModal } from "@synth/visuals/components/detail_modal.v1";
import { VisualChrome } from "@synth/visuals/chrome";

export default function Shell({ title, events, state, replay }) {
  return (
    <VisualChrome title={title} testId="visual-sourced">
      <EventStream events={events} state={state} onSelect={...} />
      <DetailModal event={selected} onClose={...} />
    </VisualChrome>
  );
}
```

`visual_save_tsx` on other templates still writes a frozen wrapper around a registered family shell. `sourced.visual.v1` is the executed path: host stores the module, compiles allowlisted imports, and mounts it as the pane Shell. Compose spec remains the JSON shortcut when the layout is an ordered list of placements.

### Non-goals

- Nested compose / widget trees as a second product
- Components as durable sqlite rows
- Unconstrained TSX (own fetch, own EventSource, npm imports)
- A second ingest path inside agent modules
- Conflating this with `diagram.systems.dynamic.v1` (storyboard + time, not live eval)

## Remaining noun ranking

The ranking items from 2026-08-26 (lineage writers, compare/promote including Candidate, report ExperimentRecord pointer, compose later components, pane-host / catalog identity) are landed. Bind COMPAT **writes** of `slot`/`slots` are dropped: new envelopes are `{ schemaVersion, inputs }` with descriptor field `input` only. Dual-**read** of stored `slot`/`slots` stays; disagree still fails closed. Catalog `template.json` uses `inputs`. `TemplateMeta.slots` still echoes `inputs` for old `list_templates` readers. `LIVE_EVAL_INPUT` is `"stream"`; `LIVE_EVAL_SLOT` is the alias. GPU admission “slot”, MCP `inputSchema`, and optimizer `delta.slot` are different nouns.

What is left is not a new class:

1. CHECK leftovers with no writers: `experiment_edges` `warm_started_from` | `produced` | `reproduced_on` | `rolled_back_to`. Do not invent writers to fill the leftover.
2. `ArtifactRevision` as a sqlite class is still not built — attach/pin/preflight/seal of visual/diagram blocks share `admit_visual_evidence` instead. `reports_promote` exists and must not be advertised on agent visuals MCP. Laguna vs Plugin is already two machines.
3. CloudDesk stays unmounted (v0.1 Intern removal). Do not remount it for a second pane host.
4. Packaged-build capability (Optimizers plugin present vs `Not installed`) and an independent CUA rerun are release gates, not new nouns.

Read the rest of this file as a zoom into the **Visual** noun and how it is (mis)projected into Reports, Data, Chat, and the right pane.

## Canonical records (what actually exists)

```
  template catalog          local visual registry              optional seals
  (kind, not instance)      (durable rows)                     (receipts)

  templateId ──────────┐
  rendererKind         │
                       v
                 ┌─────────────────────────────────────────┐
                 │ VisualRecord                            │
                 │  id              vis_b38ea7d7           │
                 │  title           "New visual"           │  ← human, not unique
                 │  currentRevision 1                      │
                 │  status          draft|live|saved|      │  ← VisualStatus
                 │                  failed|archived        │
                 │  templateId      blank.canvas.v1        │
                 │  rendererKind    template|tsx|...       │
                 │  contentDigest   often null             │  ← drops at handoff
                 │  sessionId?                             │
                 │  runId?  traceId?                       │
                 └──────────────┬──────────────────────────┘
                                │ 1..n
                                v
                 ┌─────────────────────────────────────────┐
                 │ VisualRevision                          │
                 │  visualId + revision                    │
                 │  bindings / bindingsDigest              │
                 │  contentDigest?  previewDigest?         │
                 └──────────────┬──────────────────────────┘
                                │ 0..n
                ┌───────────────┼────────────────┐
                v               v                v
         annotations      VisualSeal        chat ArtifactRef
         (labels)         receiptDigest     (viewer pointer;
                          visualRevision     status = VisualStatus)
                          artifactId
                          index/data/runtime
                          digests
```

`VisualRecord.status` is **authoring persistence**. A `VisualSeal` is **immutability of a rendition**. They are not the same state machine. A draft can exist forever with no seal; a seal can exist while the live record keeps mutating.

`ArtifactRef.status` is the same `VisualStatus` machine (`draft | live | saved | failed | archived`). Review receipts stay on metadata. A fourth evidence enum in `visuals/runtime/visualEvidence.ts` (`ready | reviewed | partial | failed`) still exists; its comment already says pinning, sealing, and sharing are separate facets. None of those drive Reports.

## How the same row is shown

```
                         VisualRecord
                         vis_...  rev 1
                         title "New visual"
                         status draft
                         digest null
                                │
        ┌───────────┬───────────┼───────────┬───────────┐
        v           v           v           v           v
   Visuals     Data→Visuals  Reports     Chat         Right pane
   registry    catalog       block       Outputs      (VisualPane)

   "Draft·rev 1"  vis_ +      live        vis_ +       title · identity
   Open / Label   labeled     available   labeled      vis_ + digest
   Seal / Add     digest      unresolved  digest
```

That is RP-CUA-054 / 060 / 061: **one row, four vocabularies, identity stripped at the edges.**

Attach from Visuals (`VisualsPage.addSelectedToReport`):

```
payload.visualId      = vis_...          // full id
payload.visualRevision= 1
referenceMode         = "live"           // pin flips this
accessState           = "available"
integrityState        = "unresolved"     // honest until resolve/pin
sourceRevision        = "1"
sourceDigest          = VisualSeal.receiptDigest when that revision is already sealed, else omitted
```

A visual/diagram block without `sealedHtml` is a **live pointer** (`vis_… · rev N · receipt|content|digest —`), not “Frozen evidence.” Frozen/pinned copy is only for `sealedHtml` or `referenceMode=pinned` + `integrity=verified`.

## Report is a pointer document, not a copy of the visual

```
ReportRecord (draft|sealed)
  └── ReportRevision
        blocks[]
          kind: report.visual.v1 | report.diagram.v1 | prose | claim | ...
          anchor: "visual-vis_b38ea7d7"     ← truncated id, used as uniqueness
          title:  "New visual"              ← display only
          payload.visualId + visualRevision ← the real join
          referenceMode  live | pinned
          accessState    available | missing | ...
          integrityState unresolved | verified | unknown | digest_mismatch | ...

        claims[].evidence_refs[]  → blockId or sourceId
        sources[]                 → another pointer table
        experiments[] / research log   (sibling stores, frozen only at pin/seal)
```

Joins for attach/pin/seal chrome are **`visualId` + revision + labeled digest**. Title stays the human label. Anchor is `visual-{full id}`.

## Visual/diagram evidence gates share `admit_visual_evidence`

```
                    VisualRecord
                    (draft blank canvas, no VisualSeal)

  ATTACH                 PIN ALL / PREFLIGHT / SEAL
  VisualsPage            admit_visual_evidence
  ─────────────          ──────────────────────────
  live pointer always    VisualSeal for visualId+rev?
  allowed                yes → pin can freeze; sealable
  copy receiptDigest     no  → unresolved_visual_evidence
  when a seal exists     Ready to seal follows sealable
```

Pin and report seal of visual/diagram blocks are **receipt-true** (need a VisualSeal). Experiment-records / research-log appendix still freeze via `freeze_blocks` and do not use this gate. The unused `decideVisualEvidence` parallel verdict was removed; Reports use `admit_visual_evidence` rather than a second admission model.

## Viewer state is another store

The right pane does not hold `VisualRecord`. It holds a reducer over `ArtifactRef`:

```
  select / request / resolve / accept / fail
                    │
                    v
         VisualRevisionState
           id, artifact, acceptedRevision,
           requestedRevision, generation, loading, error

  newestVisualArtifact()  = max(revision) among pointers
  bindingAuthorityKey()   = hash(bindings)  [revision and metadata excluded]
```

Chat, Visuals, Experiments, Data, Optimizers, and Reports share one `VisualPane` host. Settings joins that host only while a pane is open. Closing a pane, Escape, and Back still operate on **route + this reducer**, not on a shared window-layout stack. Reports Back returns to the prior chat/inventory or landing; it does not mint a blank chat. Attach/pin/seal chrome and Data/Outputs catalogs show `visualId` + labeled digest; title stays the human label.

## Intended lifecycle vs what shipped

```
  INTENDED (from facets + v0.8 thesis)

  draft ──author──► resolved ──review──► sealed/frozen
                         │                    │
                         │                    ├── pin into report
                         │                    └── share / transmit (explicit)
                         └── unavailable / superseded


  SHIPPED

  create() ──immediately durable VisualRecord──► registry forever
       │
       ├── status: draft   (Visuals); Live/Sealed/Template visuals stay instance filters
       ├── filtered-empty copy names the active filter + Clear filter (registry and Reports)
       ├── Rename / Archive on Visuals cards (archive is the cleanup verb)
       ├── VisualSeal is a separate button/table
       └── report attach copies live+unresolved (live pointer, not Frozen);
           empty reports and unresolved visual/diagram blocks are not sealable
```

Live / Sealed / Template visuals remain instance projections (`status === "live"`, `rendererKind === "template"`). The tab is labeled **Template visuals** so it is not a catalog. Duplicate titles are distinguished by `formatVisualAdmissionIdentity` on cards, pane, Data, and Outputs.

## Where identity dies

```
  vis_b38ea7d7  +  contentDigest?  +  VisualSeal.receiptDigest
           │
  │  attach uses full visualId; chrome shows vis_ + labeled digest
  │  pane header shows title · identity
  │  Data / Outputs catalogs show title · identity
  │  Reports / Settings / Visuals Back restore origin, not a minted chat
           v
        two drafts still share the human title "New visual"
        vis_ + labeled digest is the join across Visuals, Reports, Data, and the pane
```

`contentDigest` is optional on create; blank canvas leaves it null. Pin wants a **seal receipt digest**, not the visual content digest. Attach/pin/seal chrome labels which space it is showing.

## How this maps to the system findings

| Finding | On this diagram |
|---|---|
| No canonical artifact machine | `VisualStatus` = `ArtifactRef.status`; VisualSeal vs integrity vs VisualEvidence still exist; visual/diagram pin/seal share `admit_visual_evidence` |
| Evidence validity disagrees | attach still writes unresolved; pin and report seal of visual/diagram now fail closed without a VisualSeal |
| IDs disappear | attach/pin/seal chrome and Data/Outputs catalogs show `visualId` + labeled digest; title stays the human label |
| Multiple registries | Visuals, Data, Reports, Outputs are four browsers over the same rows (identity is now the same join) |
| Right panel / nav / layout | chat + inventory + Reports share one `VisualPane` host; Settings joins while a pane is open; `ArtifactRef` is not `VisualRecord` |
| Authority / ops IDs | Pane/registry/Data show `session` / `run` / `trace` via `VisualOpsLine`. Local session/run/trace follow through; missing or non-local ids stay labeled `not a Workshop route`. No Shoal/Modal Workshop routes. |

Visual/diagram attach, pin, preflight, and report seal now project one decision (`admit_visual_evidence`) without an `ArtifactRevision` sqlite class. VisualStatus, VisualSeal, and report integrity remain separate stores; this helper is the join for visual/diagram blocks. Claims still list any attached visual.

A blank `blank.canvas.v1` can still be attached as a live pointer. It is no longer “Ready to seal.”
