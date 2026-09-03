# Handoff: Workshop v0.8 post-refactor ship

**Date:** 2026-08-26  
**Repo:** `/Users/joshuapurtell/GitHub/workshop-v08-release`  
**Branch:** `codex/v08-release-integration`  
**Committed tip:** `f30d6b8e` — `Record v0.8 CUA diagnosis and experiment/lineage data model.`  
**WIP:** huge uncommitted pile on top of that tip. `git status --porcelain` is not empty.  
**Do not commit unless the operator asks.** `desktop.sh install` and `desktop-instance.sh cua-build` both refuse a dirty tree.

**Plan of record**

- [`docs/V02_REFACTOR_NOTES_2026-08-11.md`](./V02_REFACTOR_NOTES_2026-08-11.md)
- [`docs/HANDOFF_V02_REFACTOR_FINISH_2026-08-11.md`](./HANDOFF_V02_REFACTOR_FINISH_2026-08-11.md)
- Living noun map: [`docs/qa/v08-visuals-data-model.md`](./qa/v08-visuals-data-model.md)
- Right-panel CUA findings: [`docs/qa/v08-right-panel-cua-20260825.md`](./qa/v08-right-panel-cua-20260825.md)
- Packaged CUA runbook: [`docs/HANDOFF_CUA_REFACTOR_VS_PROD_QA_2026-08-11.md`](./HANDOFF_CUA_REFACTOR_VS_PROD_QA_2026-08-11.md)

**Your job:** this WIP is the post-refactor product. Ranking nouns and COMPAT write-drop are in the tree. Ship is **not** proven. Finish commit → clean install → independent packaged CUA (Gate P) + sidecar in that candidate. Do not invent ranking work to look busy.

---

## Status in one paragraph

Workshop v0.8 ranking is implemented in this dirty worktree: experiment DAG + CandidateRecord, compose/sourced kit, pane host, bind envelopes write `{schemaVersion, inputs}` only, Digbench is gone as a live-eval family (Craftax + Harbor only), Wave 6 production error-path substring classification is 0. Locked non-goals were left alone. **Gate P is unproven.** Engineer unit tests are not a tester receipt. Playwright is not Gate P.

---

## Do not reverse (product lock)

- Agents author custom visuals that actually run in the pane. Shared components + host-owned ingest. Compose spec vs sourced TSX. `blank.canvas.v1` stays sandboxed HTML/SVG (no scripts, not a drawing editor). Register-then-show.
- Hosted RLVR is **CISPO**. Do not invent `rlvr.*`. Do not flatten Harbor/Craftax eval traces into optimizer events.
- Names `stream` / `optimizer_run` / `spec` stay. Canonical bind-point noun is **`input`**. Dual-**read** of stored `slot`/`slots`; disagree fails closed. New **writes** never emit `slot`/`slots`.
- `LIVE_EVAL_INPUT` value stays `"stream"`; `LIVE_EVAL_SLOT` is an alias export. GPU “admission slot”, MCP `inputSchema`, optimizer `delta.slot`, container POST `/rollouts` `slot: "stream"` are **different nouns** — leave them.
- Laguna vs Plugin are two machines. Catalog plugin id for recipes is `optimizers`. CUA is plugin id `computer-use` (human-only).
- Craftax is rust GameBench gold only (`env:craftax_gold`). Fail closed if gold is down. No fixture worlds.

### Locked non-goals (do not “finish” these)

1. CHECK leftovers with no writers: `warm_started_from` | `produced` | `reproduced_on` | `rolled_back_to`. Do not invent writers. `optimizer_relationships.started_from` is not an experiment edge.
2. Do not invent `ArtifactRevision` as a sqlite class. Admission is `admit_visual_evidence`.
3. Do not advertise `reports_promote` on agent visuals MCP (`VISUAL_OPERATIONS` has no promote). Desktop command `reports_promote` stays.
4. Do not remount CloudDesk / Intern (v0.1: Intern unmounted; `CloudDesk.tsx` dormant).
5. No `list_components` / `list_inputs`. Components stay on `list_templates`.
6. Do not restore Digbench. Do not delete Containers `DigbenchRuntime` unless asked.
7. Do not drive the remaining 15 process `OnceLock`s to 0 as a fake ship claim (instance lock, boot epoch, MLX supervisor, secrets/telemetry `LIVE`, etc.). Whisper + credential broker are already injected.
8. Do not enable `dangerously_cast_bigints_to_number()`. Do not `xattr -cr` the default cargo target.
9. Do not delete `packages/runtime-client` while `apps/_ref_first_pass` imports it.
10. Do not merge `VisualRecord.status` with report `live|available|unresolved`. Authoring persistence ≠ seal immutability ≠ report pointer facets. Identity chrome + honest “Live pointer” copy is the cut.

---

## What is already in this WIP (uncommitted)

Treat the working tree as authority, not `f30d6b8e`.

### Ranking / product

- Experiment C1 attach/settle, C2 `follow_up`, W1 canvas. Durable `CandidateRecord` (migrations 39–40). `experiments_relate` (specta **264**).
- Laguna vs Plugin two machines. `list_templates` echoes `components[]` + `inputs` (`slots` copy for old readers).
- Bind COMPAT **write** drop: `canonical_envelope` writes `{schemaVersion, inputs}` only; `stamp_binding_input` stamps `input` and strips `slot`. Dual-read stays. MCP `visual_update` no longer advertises a `slots` array. `freeze_bindings` canonicalizes to `inputs` after freezing `live_sse` → `inline`.
- `admit_visual_evidence` + identity chrome (`formatVisualAdmissionIdentity`) on Visuals cards, pane, Data, Outputs, Reports.
- Compose + sourced first kit, later kit: `metrics.v1`, `scrubber.v1`, `candidate_inspector.v1`.
- `live.eval_stream.v1` is a whole-pane shortcut (Metrics → Scrubber → EventStream → DetailModal). Bind-point required input `stream`.
- Pane host: Chat / Visuals / Experiments / Optimizers / Data / Reports share `key="window-visual-host"`. Settings joins only while a pane is open.
- Digbench removed from Workshop live-eval: no `LiveEvalFamily::Digbench`, no family dir, no Playwright dig.bench spec. Classifier `digbench_mock` is `None`. Living docs updated. Archives in `docs/receipts/` stay archives.

### CUA code (not packaged-clicked)

RP-CUA items implemented in renderer: experiment DAG a11y, Usage null spend = Unavailable, dark theme P0, empty report blocks seal, visuals Rename/Archive, Escape + Back origin stack (`originStackRef`), shared URL / label / close focus, inference splitters, compact workbench (`<1100px` / `<860px html.compact-workbench`), capability manifest, focus visual (not Open canvas), outputs/terminal focus restore, `html.visual-expanded` hides sidebar without persisting `sidebar-hidden`, app-owned zoom (`runtime/appZoom.ts`), Diagnostics tab actually keeps the side panel, Outputs+terminal height cap.

RP-CUA-054/057 as a *single* artifact state machine was **rejected** in favor of the noun map (two machines + `admit_visual_evidence`). Do not reopen that.

### V02 Wave 5/6

- All `reqwest::Client::new()` → `crate::http::http_client()`.
- Codex missing-thread: typed `MissingThreadRollout` in `session/codex/proto.rs`; resume uses `error_is`.
- MLX missing adapter: typed `PolicySnapshotMissing` classified from FastAPI `error_code` `policy_snapshot_not_found` | `policy_snapshot_evicted` (404/410). `sidecar_training` load-and-retry uses `error_is`, not `.to_string().contains("policy_snapshot")`.
- `terminal_session_status` uses `RunStatus::parse`.
- Specta: `lib.rs` uses `specta.invoke_handler()`. Adding a command means appending to `collect_commands!`. Field-on-existing-type = ignored regen, **no** count bump. New command = bump 264.
- `export_specta_protocol_bindings` **runs** (64MiB stack + `OpaqueInteger`). Only `regenerate_protocol_bindings` is ignored because it writes the repo.

### Test isolation

`instance.rs` (cfg(test)): `lock_data_root_for_test()` + `IsolatedDataRoot`. All DATA_ROOT mutators must use this or `--test-threads=8` flakes.

### Catalog

Builtin picker is ChatGPT Luna / Synth Cloud Muse Spark / local Laguna — not `openrouter-luna`. OpenRouter entries in `modelCatalog.ts` are Playwright harness, not the builtin picker.

---

## Last proven engineer gates (this WIP, 2026-08-26)

```text
npx turbo run typecheck --filter=@synth/synth-desktop     # green (~5s)
CARGO_TARGET_DIR=/tmp/synth-desktop-candidate-target \
  cargo test --lib -- --test-threads=8                    # 1191 passed, 6 ignored
./scripts/desktop.sh conform
  map_err_to_string 0
  to_string_contains 0          # Wave 6 error-path grep
  status_magic_codex 0
  client_new 0
  window_synth 0
  invoke_string 0
  static_once_lock 15           # process caches; not a ranking leftover
  env_d_ts_lines 90
NODE_PATH="$(pwd)/node_modules" node --test apps/synth_desktop/tests/*.test.mjs
  # previously 514; re-run after you touch renderer
visuals node tests                                             # previously 48
python3 -m unittest scripts.tests.test_modern_stack_dogfood    # 4 passed
cargo test --bin synth-visuals-mcp create_with_bind            # 4 passed
npx playwright test v02-live-eval-visuals                      # 5 passed; NOT Gate P
```

`npm run desktop:check` / `desktop.sh check` can **OOM-kill turbo typecheck** when cargo check runs in parallel. Run typecheck alone.

Default `apps/synth_desktop/src-tauri/target` was poisoned by `xattr -cr target/debug/deps`. **Do not xattr.** Prefer `CARGO_TARGET_DIR=/tmp/synth-desktop-candidate-target`.

---

## What you do next (ordered)

### 1. Commit the WIP when the operator asks

One (or a few) reviewable commits. Do not push unless asked. After commit, `git status --porcelain` must be empty.

Suggested split if they want small PRs: ranking/experiment + visuals bind/compose/sourced + Digbench removal + CUA chrome + Wave 5/6 conform. Operator may prefer one integration commit on this branch.

### 2. Packaged candidate (this is Gate P prep)

```bash
cd /Users/joshuapurtell/GitHub/workshop-v08-release
git status --porcelain    # must be empty
git rev-parse HEAD        # record in the CUA receipt

./scripts/desktop.sh conform
npx turbo run typecheck --filter=@synth/synth-desktop
(cd apps/synth_desktop/src-tauri && \
  CARGO_TARGET_DIR=/tmp/synth-desktop-candidate-target \
  cargo test --lib -- --test-threads=8)
NODE_PATH="$(pwd)/node_modules" node --test apps/synth_desktop/tests/*.test.mjs

# Isolated CUA debug .app (LaunchServices identity). Requires clean tree.
./scripts/desktop-instance.sh cua-build candidate
./scripts/desktop-instance.sh cua candidate
# data → ~/.synth-desktop/instances/v02/candidate/data

# Canonical /Applications install also refuses dirty trees:
# npm run desktop:install
```

**Never** share `~/.synth-desktop` between prod friends ZIP and this candidate.

### 3. Independent packaged CUA (Gate P)

You are allowed to *build* the candidate. **You are not Gate P** if you implemented this WIP. Hand [`HANDOFF_CUA_REFACTOR_VS_PROD_QA_2026-08-11.md`](./HANDOFF_CUA_REFACTOR_VS_PROD_QA_2026-08-11.md) to a tester who was not the refactor implementer. Tier A–C. Screenshot ≠ pass.

CUA findings already closed **in code** still need packaged clicks: RP-CUA-033 geometry, RP-CUA-046 label pin, RP-CUA-047/049 Escape vs label, RP-CUA-051 duplicate “New visual” titles (identity span exists; titles still collide), RP-CUA-063 capability manifest vs sidecar `not_installed`.

### 4. Sidecar in the acceptance candidate

`v08CapabilityRows` is honest: Optimizers **visual families are bundled**; GEPA/SFT recipe runner may be `Not installed`. If the ship claim includes GEPA/SFT, the packaged candidate must actually contain the recipe sidecar. Visual families shipping without the sidecar is allowed if the manifest says so.

---

## Follow-ons (not Gate P, not ranking)

Leave these unless they block CUA:

| Item | Why leftover | Do / don’t |
| --- | --- | --- |
| `packages/runtime-protocol` handwritten `LocalRuntimeTarget` / `normalizeRuntimeTarget` | `RuntimeTarget` has custom serde; generated `protocol.ts` has no `RuntimeTarget` export | Don’t force specta Type without matching the wire format |
| `packages/runtime-client` | `_ref_first_pass` still depends | Don’t delete |
| 15 `OnceLock`s | process caches / LIVE singletons | Don’t inject for a grep of 0 |
| Test fixtures still using `"slots"` | dual-read coverage | Keep |
| `TemplateMeta.slots` echo | old `list_templates` readers | Keep one release |
| HANDOFF “export ignored due to i64/u64” | superseded | `export_specta_protocol_bindings` runs |
| `reconcile_terminal_before_run_start` | terminal before durable run id | Legitimate; Wave 1 `reconcile_failed_turn_start` is gone |
| EvalDriver `TcpListener` | same as `ipc/loopback_server.rs` | Don’t reinvent framing |
| RP-CUA-054 “one state machine” | contradicts noun map | Don’t merge VisualStatus with report pointer facets |

---

## Key paths

```text
docs/qa/v08-visuals-data-model.md          living noun map
docs/qa/v08-right-panel-cua-20260825.md    CUA findings
apps/synth_desktop/src-tauri/src/visuals/models.rs          bind envelope
apps/synth_desktop/src-tauri/src/visuals/artifacts.rs       freeze_bindings → inputs
apps/synth_desktop/src-tauri/src/visuals/live_eval.rs       Craftax|Harbor only
apps/synth_desktop/src-tauri/src/experiments/               DAG + CandidateRecord
apps/synth_desktop/src-tauri/src/optimizers/experiment_bind.rs
apps/synth_desktop/src-tauri/src/optimizers/mlx_runtime.rs  PolicySnapshotMissing
apps/synth_desktop/src-tauri/src/session/codex/proto.rs     MissingThreadRollout
apps/synth_desktop/src-tauri/src/instance.rs                IsolatedDataRoot
apps/synth_desktop/src-tauri/src/contract/specta.rs         264 commands
apps/synth_desktop/src/renderer/src/runtime/capabilityManifest.ts
apps/synth_desktop/src/renderer/src/routes.tsx              origin stack, pane host
visuals/families/.../live.eval_stream.v1/
visuals/families/analysis/compose.visual.v1/
visuals/families/analysis/sourced.visual.v1/
```

File work only under `/Users/joshuapurtell/GitHub`. Do not edit `containers`, `optimizers`, `optimizers-beta`, `synth-mlx-rl` unless a producer field is missing. Do not change optimizer event payload fields such as `delta.slot`.

---

## Done when

1. WIP committed; worktree clean.  
2. Named-instance or `/Applications` candidate built from that SHA, isolated data root.  
3. Independent CUA receipt for Gate P (Tier A–C) against **this** candidate — not Playwright, not engineer clicks on `tauri dev`.  
4. Capability manifest on that candidate matches what is actually bundled (sidecar present or honestly `not_installed`).  
5. Conform counters have not increased; `to_string_contains` stays 0; specta stays 264 unless you add a real command.

Until (3) exists, do not call v0.8 “ready to ship post-refactor.”
