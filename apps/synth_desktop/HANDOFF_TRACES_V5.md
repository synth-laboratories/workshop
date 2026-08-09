# Handoff: First-class Trace V5 — local store + view

**Date:** 2026-08-09  
**Audience:** Engineer making Trace V5 storage and inspection first-class in `apps/synth_desktop`  
**Status:** Implemented and dogfooded in isolated Desktop instance `gamma` (badge 3)  
**Related:** [`synth_desktop_research_eng.md`](../../synth_desktop_research_eng.md) §4 · [`containers.md`](./containers.md) · [`HANDOFF_CONTAINERS_CRAFTAX.md`](./HANDOFF_CONTAINERS_CRAFTAX.md) · evals `docs/eval-standard.md` (sibling repo)

**Normative storage contract:** [`TRACES_V5_STORAGE_FORMAT.md`](./TRACES_V5_STORAGE_FORMAT.md)

---

## 0. One-liner

> **Ingest → CAS digest → Inventory vault → Open as visual** (PostTrain-style scrub + optional annotations). Desktop is a **viewer/consumer** of sealed Trace V5; it does not invent readiness or rewrite Harbor/native trajectories.

UX bar: Poolside’s [trajectories.poolside.ai](https://trajectories.poolside.ai) / [Laguna S 2.1 blog](https://poolside.ai/blog/introducing-laguna-s-2-1) — every score has an inspectable trajectory. Local Desktop should feel like that for runs this machine produced (and imports).

## Implementation update — 2026-08-09

The first-class native path is complete:

```text
synth-trace inspect-input/project
  → quarantined raw import + trusted deterministic archive
  → bundle / member / asset / thin trace index in SQLite
  → rollout-inspector projection resolver
  → deterministic visual id per sealed trace digest
  → trace.rollout_inspector.v1 (filter, scrub, play, evidence/tools)
```

Key behavior:

- `synth-containers` owns validation, deterministic archives, old inline-manifest
  compatibility, and `synth.trace-projection.rollout-inspector.v1` generation.
- Desktop publishes only trusted, self-contained inputs into the trace vault;
  partial, opaque, and invalid inputs remain quarantined.
- The sealed trace, semantic bundle, and archive byte digests remain separate.
- Re-importing the same archive is idempotent. Reopening a trace reuses its
  deterministic `tracevis_<digest>` visual and creates a revision only when the
  projection digest changes.
- The inspector recognizes Laguna/Codex `codex.command_started` and
  `codex.command_finished` as tool events and has working play/pause playback.

The dogfood trace is the real Laguna/Harbor `edit-json` run:

| Fact | Value |
| --- | --- |
| Title | `Laguna XS 2.1 · edit-json · Harbor` |
| Model | `poolside/laguna-xs-2.1` |
| Trace digest | `sha256:4291ac502d61e932f2e1071db039008b4771fcf7d993630cca5d10bed1223164` |
| Projection digest | `sha256:4bcc4f928621d424952ec7d21837fd0626cfaa5b020610fb0d685b6c0ce83e25` |
| Contents | 31 sealed events, 7 spans, evidence present; 49 inspector items |
| Gamma visual | `tracevis_4291ac502d61e932f2e1071db039008b4771fcf7d993630cca5d10bed1223164` |

The example was imported twice into gamma with the second result reporting
`duplicate: true`, then materialized as revision 1 of
`trace.rollout_inspector.v1` and sent through gamma's authenticated `visual.show`
path. The source Harbor directory was never modified; projection and packaging
used a temporary/copy workspace.

Repeat the native real-bundle integration test with:

```bash
cd /Users/joshuapurtell/Documents/GitHub/workshop
scripts/test-trace-v5-real-bundle.sh
```

---

## 1. Planned evidence stack

```text
sealed Trace V5 (content-addressed bytes, digest = identity)
        → catalog facts (reward, metrics, cost, harness/model/env versions)
        → TraceEvidenceBundle / annotations (append-only; never mutate digest)
        → visuals bound to digest (posttrain scrub, annotation overlay, Craftax scrub, …)
```

### Authorities (do not blur)

| Surface | Authority |
| --- | --- |
| Trace V5 sealing + provenance | **synth-containers** |
| Native Harbor trajectory + verifier output | **Harbor** (`agent/trajectory.json`, etc.) |
| Dock package materialization | **dock** |
| Execution lifecycle | SMR / backend |
| Cross-source attempt indexing | **evals** |
| Presentation / filtering | **Desktop viewers** (consumers only) |

Viewers must **not** infer task membership, readiness, or proof from filenames or “a JSON appeared.”

Product principles (HANDOFF): rollouts are first-class; metrics belong next to trajectory steps; visuals emerge from structured data; Electron is a client.

---

## 2. What Desktop has today (honest)

| Piece | Status |
| --- | --- |
| Inventory · Traces tab | Lists title, digest prefix, reward — **no Open / scrub** |
| Protocol `TraceV5Record` | id, digest, title, source, container/session/run links, reward, metrics, path, metadata |
| Rust `InventoryStore` | `list_traces` / `get_trace` — **no ingest** |
| Python `inventory.ingest_trace` | sha256 → `cas/traces/{digest}.json` + SQLite row (migration reference) |
| Visual binding kinds | `trace_v5`, `local_cas`, `fixture`, … in protocol / `visuals/runtime` |
| Bind loaders | Often stub / fixture-only — not wired to Rust CAS |
| Templates | See §4 |
| Chat `trace_ref` chip | Missing |

Empty Inventory · Traces matches the same vault emptiness as Containers before Craftax attach.

---

## 3. Harbor philosophy (keep separate)

Harbor is a **benchmark harness** over containerized agent envs (Codex-in-container + verifiers), not the GameBench gold HTTP frame stream.

| Idea | Meaning for Desktop |
| --- | --- |
| Pipeline | `run → score → save evidence → index` — folders authority; indexes rebuildable |
| Native artifacts | Harbor owns trajectory + verifier results; Desktop **references** or imports into Trace V5 / CAS with provenance |
| Dual products | Gold Craftax (`:8098` frames/NEV) ≠ Harbor DEO (agent + verifier). Different event sources |
| Live Harbor jobs | Template `live.dock_harbor.v1` — job status + rollout stream (SSE/fixtures), not gold `render.png` |

Do **not** force Harbor trajectories through the Craftax gold container contract. Import path: Harbor JSON → seal/cite as Trace V5 (or bind with explicit `source: harbor` metadata) → same viewer.

Eval standard: [evals/docs/eval-standard.md](https://github.com/synth-laboratories/evals/blob/main/docs/eval-standard.md) (local checkout under `~/Documents/GitHub/evals`).

---

## 4. Viewer references (reuse, don’t reinvent)

### PostTrainBench-style (generic inspector)

`visuals/templates/posttrain.rollout_viewer.v1/`

- Timeline scrubber + play/pause  
- Step list: action + reward  
- Cumulative reward sparkline  
- Observation text  
- Slot `trajectory` accepts `fixture` | `local_cas` | `trace_v5`

This remains a compatibility/reference template. The implemented default Open
target is `visuals/templates/trace.rollout_inspector.v1/`, which consumes the
canonical versioned projection instead of interpreting sealed trace internals.

### Annotations (overlay, never mutate seal)

`visuals/templates/annotation.overlay.v1/` — markers (`note` | `bug` | `highlight` | `reward` | `acceptance`) on a sealed digest.

### Env specialization

- `craftax.rollout_scrub.v1` — frames + text + HUD (bind live container or sealed steps)  
- `craftax.eval_matrix.v1` — cohort / pareto  

### Harbor live

- `live.dock_harbor.v1` — Dock/Harbor job cards + rollout events  

### Planned generic eval family (from Rust/visuals handoff)

Prefer shared primitives over one-off Craftax: especially `eval.rollout_inspector.v1` (trajectory, tools, rewards, observations, annotations) as the long-term PostTrain successor.

### Laguna S 2.1 / Poolside trajectories UX

- Full trial trajectories published for every benchmark score  
- Inspect steps, tools, shell, reasoning — not score chips alone  
- Commentary / “seeing the model work” ≈ our annotation overlay  

Desktop goal: **local trajectories vault** with the same inspectability.

---

## 5. Target product flow

```text
Producer (session seal / Craftax rollout / Harbor import / file drop)
  → traces.ingest({ bytes | path, title?, containerId?, sessionId?, runId?, metadata? })
  → CAS put + SQLite TraceRecord (digest unique)
  → journal optional: trace.ingested
  → Inventory · Traces row
  → Open → visuals.create({
        templateId: "posttrain.rollout_viewer.v1",
        bindings: { trajectory: { kind: "trace_v5", source: digest } }
      }) → show in Visual pane
  → optional chat chip / agent MCP traces_show
```

Same IA as containers/visuals: **vault list + chat/pane loop**. Inventory is the library; Visual pane is the inspector.

---

## 6. Implementation plan

### Phase A — Rust ingest + CAS (storage authority)

1. `inventory_traces_ingest` (and/or CoreRuntime helper): accept a bundle directory/archive, standalone sealed V5, or explicitly identified legacy/native input.  
2. Validate through the `synth-containers` format library; Desktop must not establish V5 identity by independently canonicalizing arbitrary JSON.  
3. Store the deterministic bundle archive in the existing Rust `ContentStore` `traces` kind; retain bundle semantic digest, archive byte digest, and sealed trace digest separately.  
4. Add bundle/membership/assets tables while preserving the existing `traces` table and legacy digest values.  
5. Upsert the trace summary and rebuildable filter index from a versioned standard projection.  
6. Idempotent on bundle/archive/trace digests; repeated imports add evidence or membership rather than fork trace identity.  
7. Bridge: `window.synthInventory.ingestTrace(...)`; types in `env.d.ts`.  
8. Legacy inputs are byte-preserved and migrated append-only with aliases, provenance, loss reporting, and a migration receipt.

### Phase B — Resolve + Open in UI

1. Implement `trace_v5` / `local_cas` loaders in the visual bind path (sealed digest → versioned rollout-inspector projection).  
2. Inventory Traces row: **Open** → create/show PostTrain visual bound to digest.  
3. `data-testid`: `open-trace-{id}`, keep `inventory-trace-{id}`.  
4. Detail strip: full digest, source, linked container/session if any.

### Phase C — Producers (dogfood)

Priority order:

1. **Import file** — Attach/drop `.json` Trace V5 or trajectory-shaped payload → ingest → Open.  
2. **Craftax** — After container dogfood: seal last rollout / event_log (+ optional frames manifest refs) into Trace V5 → Inventory.  
3. **Session seal** — Codex/local run → Trace V5 from journal (later).  
4. **Harbor import** — Map `agent/trajectory.json` (+ verifier refs) with `metadata.producer = harbor`; do not rewrite Harbor files in place.

### Phase D — Annotations + chat (follow-up)

- Store overlay markers keyed by digest (separate table or evidence bundle).  
- Chat `trace_ref` chip; MCP `traces_ingest` / `traces_show`.  
- Promote toward `eval.rollout_inspector.v1` when ready.

### Out of scope for first PR

- Replacing Harbor native trajectory authority  
- Full cloud Artifact Platform sync  
- Claiming readiness/proof from Inventory alone  
- Live gold-frame streaming (that’s containers + Craftax scrub)

---

## 7. Suggested file touch list

| Area | Paths |
| --- | --- |
| Storage / ingest | `src-tauri/src/inventory.rs`, `storage/content_store.rs`, `lib.rs` commands |
| Bridge / types | `desktopBridge.ts`, `env.d.ts`, `packages/runtime-protocol` if needed |
| UI | `InventoryPage.tsx` — Open; optional import control |
| Bind / view | `visuals/runtime/bind.ts`, VisualHost / visualsLoader, PostTrain template |
| Tests | Playwright: ingest fixture → list → Open → PostTrain `data-testid` |
| Fixtures | `visuals/fixtures/rollout_steps.json`, Craftax fixtures already in tree |

---

## 8. Dogfood / CUA script

1. Start Desktop; Inventory → **Traces** (expect empty or prior seeds).  
2. Import / ingest `visuals/fixtures/rollout_steps.json` (or a sealed Trace V5 fixture) via Attach/Import.  
3. Assert row: title, digest prefix, reward if present.  
4. **Open** → Visual pane shows PostTrain scrubber; scrub steps; observation text visible.  
5. Re-ingest same bytes → same digest / single row (idempotent).  
6. (Optional) With Craftax `:8098` up and container registered: seal a short rollout → second Trace row → Open.  
7. Screenshot vault + open pane for PR.

### Pass / fail

| Check | Pass |
| --- | --- |
| Store | Digest on disk + SQLite row |
| View | PostTrain (or successor) bound to `trace_v5`, not only inline fixture props |
| Idempotent | Duplicate ingest does not fork identity |
| Boundaries | No fake “verified/proven” from viewer alone |

---

## 9. Acceptance checklist

- [x] Rust bundle ingest → quarantine/trusted CAS + trace catalog/index  
- [x] Inventory Open → canonical rollout-inspector visual for that digest  
- [x] Projection resolver reads a verified standard projection from the stored bundle  
- [x] Import path dogfooded in isolated gamma with a real Laguna/Harbor trace  
- [x] Browser test covers Open, render, tool/evidence metrics, playback, and idempotency  
- [x] Native ignored E2E covers real import, projection resolution, and duplicate identity  
- [x] Storage and backward-compatibility contracts documented  
- [ ] Follow-ups: more legacy migration fixtures, annotation UX, chat chip, session sealing  

---

## 10. Related paths

| Doc / code | Why |
| --- | --- |
| `synth_desktop_research_eng.md` §4–6 | Evidence stack + Harbor/Dock/evals roles |
| `visuals/templates/posttrain.rollout_viewer.v1/` | Default inspector |
| `visuals/templates/annotation.overlay.v1/` | Overlay layer |
| `visuals/templates/live.dock_harbor.v1/` | Harbor job stream (live, not sealed) |
| `HANDOFF_RUST_CORE_VISUALS_AND_INTERN.md` | `eval.rollout_inspector.v1` family |
| `services/local-runtime/.../inventory.py` | Reference ingest/CAS behavior |
| Poolside trajectories + Laguna S 2.1 blog | External UX bar |
| evals `docs/eval-standard.md` | Authority matrix |

---

## 11. Suggested first PR slice

1. Rust ingest + CAS + bridge.  
2. Inventory Open → PostTrain with `trace_v5` binding.  
3. Playwright: ingest fixture → open viewer.  
4. Stop. Harbor import + annotations + Craftax seal = next PRs.
