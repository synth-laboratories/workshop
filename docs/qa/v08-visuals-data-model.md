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
│         └─ ArtifactRef         viewer pointer. Not VisualRecord; status draft|review|ready|failed
│                                Inventory routes share one VisualPane host.
│                                Chat still remounts its own pane.
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
     │              ├─ Binding   slot fill (stream, document, …)
     │              └─ VisualSeal receipt that a rendition is immutable
     │
     ├─ Report                   pointer document, not a copy of the visual
     │    ├─ Report block schema class: report.visual.v1, report.prose.v1, …
     │    └─ ReportRecord        instance rep_…
     │         └─ ReportRevision numbered snapshot
     │              ├─ ReportBlock  pointer at vis_ / digest
     │              ├─ ReportSeal   receipt that this revision is frozen
     │              └─ ExperimentRecord  leftover appendix JSON; not an ExperimentGroup
     │
     ├─ Experiment               durable row, many per session
     │    ├─ ExperimentGroup     identity + task/model + members
     │    ├─ ExperimentNode      member: optimizer_run | eval_campaign | direct_evaluation
     │    ├─ experiment_edges    intra-group: evaluated
     │    └─ experiment_lineage  inter-group: follow_up  (forked_from/rerun_of CHECK only)
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
- **Inter-experiment.** Migration 38 dropped `experiment_groups.session_id` uniqueness. A session owns many groups. Parent → child is `experiment_lineage.follow_up`. `forked_from` and `rerun_of` exist in the CHECK only; no writers. Follow-up cannot live on `experiment_edges` (those FKs are nodes, not groups).
- **Attach target.** `experiment_session_cursor` → `sessions.active_experiment_id` → oldest group for that session. Cursor exists because some `session_id` values are not `sessions` rows. Do not attach an optimizer when `session_ref` is empty.
- **Producers.** Optimizer `experiment_bind`: attach on create/seed/import; settle on terminal event commit, cancel, and stale-run reconcile. Terminal settle writes `cost_usd` and attaches `visual_refs` as evidence kind `visual`. Campaigns already attached themselves.
- **Surfaces.** `experiments_create_child` / `experiments_activate` (Tauri + MCP `experiment_create_child` + HTTP). GET by experiment id returns that child, not the session primary. UI: searchable index | ranked pan/zoom DAG | inspector. Forest when nothing selected; member DAG when selected. Compact stack at 780px. Layout coords are view state, not sqlite.
- **Still leftover on this noun.** `reports.ExperimentRecord` appendix JSON. No durable `Candidate` row. No compare / promote. CHECK leftovers on `experiment_edges` (`compared_with`, …) and `experiment_lineage` (`forked_from`, `rerun_of`) have no producers.

Plugin was never a boolean. `PluginStatus.phase` has 14 values; `pluginPresentation.ts` is the single presentation owner; recipes gate on `require_plugin_ready`. Laguna is a parallel `LagunaStatus` object, not a plugin registry row. CUA is plugin id `computer-use` (human-only install).

Window chrome first cut: Visuals, Experiments, Optimizers, and Data share one `inventory-workbench` so `VisualPane` does not remount when switching those routes. Chat and CloudDesk still mount their own panes. `ArtifactRef.status` is still a separate vocab from `VisualStatus`.

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
    slots[]                stream, document            bindings
                                                       VisualSeal      receipt  (optional)

  Report block schema      report.visual.v1          ReportRecord      rep_…
                           report.prose.v1             ReportRevision
                                                       ReportBlock     pointer at vis_/digest
                                                       ReportSeal      receipt  (optional)

  Experiment               task / model on the group  ExperimentGroup     exp_…  (many per session)
                           member kinds                 ExperimentNode      optimizer_run | eval_campaign | direct_evaluation
                           follow_up (CHECK also has    experiment_lineage  parent → child
                           forked_from, rerun_of)       experiment_edges    evaluated (member → member)
                                                       reports.ExperimentRecord  leftover appendix JSON
                                                       Candidate            still missing (GEPA overlay JSON)

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
                          ├─ edges       evaluated  (member → member)
                          └─ lineage     follow_up  (this group → child group)

  attach target for the next run:
      experiment_session_cursor.active_experiment_id
        else sessions.active_experiment_id
        else oldest experiment_groups row for that session_id
```

Renderer: list stays mounted. Empty selection shows the forest (`follow_up` dashed). Selecting a group drills into that group's member DAG. `+ child` calls `experiments_create_child`. Canvas ranks are computed in the WebView.

GEPA `candidate_id` still lives in optimizer event JSON and visual overlays. It is not a durable class. Do not stuff it into member `kind`.

## Visual composition (how it works)

The agent already has three escape hatches. None of them is a kit.

```
  shipped template          whole pane class     optimizer.gepa.live.v1, live.eval_stream.v1
  analysis.visual.v1        static block spec    note | metrics | ranked-bars | table | scatter
  blank.canvas.v1           dump HTML/SVG        reinvent chrome every time
```

Ingest already exists and must stay owned by the host:

```
  agent bind slot "stream"  (kind live_sse, declared poll_url — never guessed)
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

Templates must not discover URLs. `live.eval_stream.v1` already follows this and then hardcodes MetricStrip + a JSON event log. Optimizer families share `RunChrome` / `GlobalTimeline` / candidate inspector as private TSX. None of that is advertised.

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
  placement                instance: { id, component, slot?, from?, config }
```

Kind is the load/render contract (`event_stream`, `detail_modal`, `sourced_visual`). `protocol_id` is the bind dialect (`event_stream.v1`, `whole_file.v1`). A placement is not a VisualRecord and not a node in a property graph.

### Compose template (agent path)

Do not grow `analysis.visual.v1` a live `stream` slot. Static analysis stays static. New template `compose.visual.v1`:

```
  slots:
    spec     required   inline   synth.visual.compose_spec.v1
    stream   optional   live_sse / fixture / inline
```

`stream` is required at bind time only if a placement consumes it. Unknown `component` ids fail closed (same as unknown protocol ids). Layout is an ordered vertical list. No xy graph. Product `optimizer.*` and `diagram.*` do not switch.

Compose is the ad-hoc live overlay for **evals and optimizer event streaming**. It does not replace `optimizer.gepa.live.v1` / `optimizer.sft.live.v1` / `optimizer.eval.live.v1`. Those stay product-owned. Two host-owned bind dialects, not one mashed firehose:

```
  stream          live_sse / fixture / inline     container eval (Harbor, Craftax gold, …)
  optimizer_run   optimizer_run / fixture / inline   optimizer_event.v1 (GEPA, SFT, CISPO)
```

`optimizer_run` is not on the first-cut template yet. Adding it is the next compose cut. Do not flatten child eval traces into optimizer events. Do not invent a generic `rlvr.*` pane: hosted RLVR is **CISPO** (`algorithmId: cispo`, `cispo.*` events). Exact RLVR payloads stay producer-owned; compose filters by those type names. `includeKinds` matches envelope `kind` or `type`.

```json
{
  "schemaVersion": "synth.visual.compose_spec.v1",
  "title": "Harbor smoke · live stream",
  "lede": "Declared SSE only. Heartbeats are not evidence.",
  "placements": [
    { "id": "log", "component": "event_stream.v1", "slot": "stream",
      "config": { "includeKinds": ["rollout.finished", "run_finished"] } },
    { "id": "inspect", "component": "detail_modal.v1", "from": "log" }
  ]
}
```

Agent still uses `visual_manage` `create_with_bind` / `bind`. No new MCP verb per component. `list_templates` on `compose.visual.v1` advertises `components[]` (id, kind, protocol_id, consumes, emits) next to `slots` and `example_binding`. First cut does not add `list_components`.

### Runtime (one ingest, many placements)

```
  VisualHost
    load shell compose.visual.v1
    ReplayClient from bound stream slot   (or idle if none)
         │
         v
  Compose shell
    useLiveEvalStream once
    cursor: { placementId, eventId, sequence }     view state, not sqlite
         │
         ├─ event_stream.v1    reads events + state; writes cursor on select
         └─ detail_modal.v1    reads cursor via `from`; in-pane overlay
```

`detail_modal` is not a second VisualPane and not a second VisualRecord. It is a dialog/drawer inside this visual's chrome. Cursor identity is envelope `event_id` + `sequence`, not array index (replay re-ingests).

Components never call `poll`, never read bindings, never invent SSE URLs. Filter (`includeKinds`) is placement config, not a second stream.

### First two components

```
  event_stream.v1     kind event_stream
                      consumes stream
                      emits cursor
                      renderer: visuals/components/event_stream.v1

  detail_modal.v1     kind detail_modal
                      consumes cursor from a named placement
                      renderer: visuals/components/detail_modal.v1
```

Later components (not first cut): `metrics` (reduce events → strip), `scrubber`, candidate inspector. Those stay unadvertised in optimizer overlays until they have a protocol.

### What the agent does

1. Ground: declared stream URL from the container / create-rollout receipt. Guessed Craftax/Harbor URLs still fail `liveStream.ts`.
2. `create_with_bind` `compose.visual.v1` with `spec` inline and `stream` bound (`live_sse` + `poll_url`).
3. Host stores one VisualRecord. Compose shell hydrates from ReplayClient.
4. `show`. Capture/review still apply to authored families. `optimizer.*` stay product-owned.

`live.eval_stream.v1` remains a shortcut whole-pane template. Do not rewrite it in the first cut. `blank.canvas.v1` remains last resort.

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

1. **Candidate** — missing durable class on the experiment spine. Not a fourth member kind.
2. **Compose `optimizer_run` slot** — same event_stream + detail_modal over GEPA / SFT / CISPO `optimizer_event.v1`. Product `optimizer.*` chrome stays.
3. **Window / `ArtifactRef`** — inventory routes share one pane host. Chat still remounts. Separate status vocab.
4. Later: Laguna vs Plugin; one admission object for seal/pin/attach; `rerun_of` / `forked_from`; compare/promote. `list_templates` does not yet echo `components[]` (advertised on `template.json` + skill).

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
         (labels)         receiptDigest     (viewer pointer,
                          visualRevision     different status
                          artifactId         vocab)
                          index/data/runtime
                          digests
```

`VisualRecord.status` is **authoring persistence**. A `VisualSeal` is **immutability of a rendition**. They are not the same state machine. A draft can exist forever with no seal; a seal can exist while the live record keeps mutating.

There is also a third status enum used only by the chat pane (`ArtifactRef.status`: `draft | review | ready | failed`) and a fourth evidence enum in `visuals/runtime/visualEvidence.ts` (`ready | reviewed | partial | failed`) whose comment already says pinning, sealing, and sharing are separate facets. None of those drive Reports.

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

   "Draft·rev 1"  title +     live        Saved        title only
   Open / Label   templateId  available   reports      no vis_ id
   Seal / Add     no lifecycle unresolved  (draft!)    no digest
                  actions     "Frozen     Back → new
                              evidence"   blank chat
```

That is RP-CUA-054 / 060 / 061: **one row, four vocabularies, identity stripped at the edges.**

Attach from Visuals does this on purpose (`VisualsPage.addSelectedToReport`):

```
payload.visualId      = vis_...
payload.visualRevision= 1
referenceMode         = "live"         // not pinned
accessState           = "available"    // not proven
integrityState        = "unresolved"   // honest, then ignored
sourceRevision        = "1"
sourceDigest          = (omitted)
```

The report renderer then falls through to hardcoded copy: *“Frozen evidence attached to this revision.”* That string is not computed from pin/seal. Any visual block without `sealedHtml` gets it.

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

Joins are **title + truncated anchor**, not `id + contentDigest`. Duplicate drafts look identical. Second `Add to report` fails as `duplicate_block_anchor` (schema uniqueness), not “already attached.”

## The five evidence gates (they do not share a predicate)

```
                    VisualRecord
                    (draft blank canvas, no VisualSeal)

  (1) ATTACH          (2) CLAIMS         (3) PIN ALL
  VisualsPage         claim picker       reports.pin_all
  ─────────────       ────────────       ────────────────
  enabled if a        lists any          resolve_evidence_state:
  visual is selected  attached visual    need VisualSeal for that
  writes live +       including blank    revision
  unresolved          draft              no seal → integrity unknown
  ALWAYS succeeds                        → bail "cannot pin unresolved"
                                         (internal)

  (4) PREFLIGHT / UI  (5) SEAL
  validate_revision   reports.seal
  ─────────────────   ────────────
  errors only:        freeze experiment/log payloads
  duplicate id/anchor then resolve_evidence_state again
  pinned-without-     then SAME validate_revision
  digest              unknown/unresolved is NOT an error
  digest_mismatch     empty prose is NOT an error
  bad claim shape     blank visual is NOT an error
                      sealable = !any(severity==error)
                      → "Ready to seal"
```

Pin is **content-true** (needs a visual seal receipt + digest). Seal validation is **document-shape-true** (ids unique, pinned rows complete if already pinned). The UI then labels shape-true as “Ready to seal.”

```
  pin_all                      validate_revision / sealable
  ───────                      ────────────────────────────
  visual has VisualSeal?  ──►  integrity unresolved?  ignored
  digest present?         ──►  live referenceMode?    ignored
  else FAIL                    empty findings/methods ignored
                               blank canvas             ignored
                               → Ready to seal = YES
```

That is RP-CUA-009 / 014 / 024 / 025 / 057, grounded in code, not copy.

`decideVisualEvidence` in the visuals package **explicitly does not block completion**. Reports never call it.

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

Chat, Visuals, Experiments, Data, and Optimizers each mount their own `VisualPane` + splitter. Closing a pane, Escape, and Back operate on **route + this reducer**, not on a shared window-layout stack. That is why IDs/digests vanish in the pane header and why Back from a report mints a blank chat: the origin was never part of the record.

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
       ├── status: draft   (Visuals)
       ├── filter Live looks for status==="live"  → empty copy "no visuals yet"
       ├── archive() exists on the bridge; UI only exposes Open
       ├── VisualSeal is a separate button/table
       └── report attach copies live+unresolved and calls it Frozen
```

RP-CUA-001 / 015 / 050 / 051 / 052: filters and labels are **instance projections** (`status === "live"`, `rendererKind === "template"`) pretending to be product concepts (Live feed, template library, empty registry).

## Where identity dies

```
  vis_b38ea7d7  +  contentDigest?  +  VisualSeal.receiptDigest
           │
           │  attach uses visualId, truncates to anchor visual-vis_b38ea7d7
           │  pane header shows title · rev N
           │  Data shows title · templateId
           │  Outputs shows report title
           v
        "New visual" × 2 drafts, indistinguishable
        cannot prove Visuals row === Reports block === sealed receipt
```

`contentDigest` is optional on create; blank canvas leaves it null. Pin wants a **seal receipt digest**, not the visual content digest. Two digest spaces, neither shown at the handoff.

## How this maps to the system findings

| Finding | On this diagram |
|---|---|
| No canonical artifact machine | `VisualStatus` ≠ `VisualSeal` ≠ `ReportBlock.integrity` ≠ `ArtifactRef.status` ≠ `VisualEvidence` |
| Evidence validity disagrees | attach writes unresolved; pin requires seal; `validate_revision` ignores unresolved |
| IDs disappear | join key is title / truncated anchor; pane omits `id`+digest |
| Multiple registries | Visuals, Data, Reports, Outputs are four browsers over the same rows |
| Right panel / nav / layout | inventory routes share one `VisualPane` host; chat still remounts; `ArtifactRef` is not `VisualRecord` |
| Authority / ops IDs | `sessionId`/`runId`/`traceId` exist on the visual but are not the operation the UI follows into Shoal/Modal |

The kernel that is missing is not “better Visuals copy.” It is one admission object every surface must project:

```
  ArtifactRevision
    id + revision + contentDigest
    lifecycle: draft | resolved | sealed | unavailable | superseded
    authority: local | backend | shoal | modal
    admission: { ok | blocked, reasons[], remediation }

  ReportBlock.payload  ──must──►  that revision
  Pin / claim / preflight / seal  ──must──►  that admission
  UI chrome  ──must──►  id+digest whenever attach/pin/seal is offered
```

Until attach, pin, and seal consume that one decision, a blank `blank.canvas.v1` will keep showing up as draft, live, frozen, and ready-to-seal at the same time — because that is what the current types allow.
