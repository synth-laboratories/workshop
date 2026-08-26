# Workshop visuals and data model

Date: 2026-08-25  
Tied to: [`v08-right-panel-cua-20260825.md`](v08-right-panel-cua-20260825.md)  
IA reference: [`refs/eiso-kant-model-factory-2026-08-18.png`](refs/eiso-kant-model-factory-2026-08-18.png) — Eiso Kant / Poolside Model Factory (index beside lineage canvas)

## Legend — core nouns

```
Workshop                         product (Synth Desktop). Not a domain record.
│
├─ Workshop window               Tauri + WebView chrome. “Workbench” in code is this, not a noun.
│    ├─ Sidebar                  chats, Visuals, Reports, Experiments, Data, Optimizers, Settings
│    ├─ Main column              the page for the current route
│    └─ Right pane               VisualPane / Outputs / inspect  (duplicated per route)
│         └─ ArtifactRef         viewer pointer. Not VisualRecord; own status vocab.
│
├─ Session                       one chat. Kind is exactly one of:
│    ├─ Codex                    app-server → Responses (local or Shoal Laguna)
│    └─ Intern                   Synth Cloud (sync | async)
│
├─ CoreRuntime                   sole durable authority
│    ├─ sqlite / WAL             ids, revisions, edges, indexes
│    ├─ event journal            append-only facts (rebuildable projection)
│    └─ CAS store/               traces, seals, checkpoints, TSX/HTML bodies
│
└─ Records                       class (shipped)  →  instance (durable row)
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
     │              └─ ExperimentRecord  appendix JSON (one of three “experiment” types)
     │
     ├─ Experiment               three live types share the word
     │    ├─ ExperimentGroup     chat membership bag
     │    └─ ExperimentNode      DAG node: baseline | variant | result
     │
     ├─ Container
     │    ├─ capability          class: live-eval.v1 ops
     │    └─ ContainerDeployment instance in the registered pool
     │
     ├─ Optimizer
     │    ├─ recipe              class: gepa / sft / eval
     │    └─ OptimizerRun        one execution
     │
     └─ Plugin                   Laguna | Optimizers | CUA     installed | not installed
```

CoreRuntime owns durable records. The renderer is a projection. The same visual is one local record, then five independent projections. The CUA contradictions are those projections disagreeing, plus two different “seal” verbs.

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
  │  Data      │   transcript / registry / DAG / catalog      │  (duplicated │
  │  Optimizers│                                              │   per route) │
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

  Experiment (v0.8)        recipe / task-contract    three live nouns share the word:
                           candidate kind              ExperimentGroup    chat bag of members
                                                       ExperimentNode     baseline|variant|result
                                                       reports.ExperimentRecord  appendix JSON

  Container capability     live-eval.v1 ops          ContainerDeployment  registered pool
  Optimizer recipe         gepa / sft / eval         OptimizerRun
  Plugin / service         Laguna, Optimizers, CUA   installed | not installed


  SESSION  (routing law: exactly one runtime)

      Session ──kind──► Codex ──► app-server ──► Responses provider
                    │                              ├─ local Laguna XS (MLX sidecar)
                    │                              └─ configured / Shoal-hosted Laguna
                    └──kind──► Intern ──► Synth Cloud  (sync | async)


  AUTHORITY  (who may be telling the truth)

      local disk     CoreRuntime sqlite + journal + CAS     default, no account
      backend        Synth Cloud intern / publish           explicit opt-in
      Shoal          hosted Laguna desired vs observed      serving
      Modal          NanoHorizon iteration                  not a Workshop route


  WHAT THE KERNEL ACTUALLY STORES

      CoreRuntime
        ├── sqlite / WAL     ids, revisions, edges, indexes
        ├── event journal    append-only facts  (rebuildable projection)
        └── CAS store/       traces, seals, checkpoints, export bodies
              ▲
              │  typed Tauri commands  (renderer never owns authority)
              │
         VisualsPage  ReportsPage  ExperimentsPage  DataPage  Chat Outputs
              │            │              │              │         │
              └────────────┴──────────────┴──────────────┴─────────┘
                           each re-projects the same rows
                           with a different status vocabulary
```

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
| Right panel / nav / layout | pane is a per-route viewer of `ArtifactRef`, not a host for the record |
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
