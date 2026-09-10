# Handoff: inline evaluation control plane

**For:** the engineer reviewing this cut before it is committed.  
**Date:** 2026-08-27  
**Do not commit unless asked.**

This is a Workshop **control-plane** change. Craftax, NanoHorizon policy, and GameBench gold were not the failure. Do not redesign those. Do not invent a 5×5 in-process Craftax stub. Gold is rust GameBench only (`env:craftax_gold`); if gold is down, fail closed.

Durable authority is the **local store**. Do not introduce `CoreRuntime` as a product noun. File work only under `/Users/joshuapurtell/GitHub`. Do not write under `Documents`.

---

## Tree

| Path | Branch | Git |
| --- | --- | --- |
| `/Users/joshuapurtell/GitHub/workshop-v08-release` | `eval/inline-first-admission` | **Dirty, uncommitted.** ~2.6k insertions on top of `b4067601` (clickable container-replacement approval) and `6b353878` (authoritative inline evaluations). |
| `/Users/joshuapurtell/GitHub/nanohorizon` | (dirty checkout, expected) | Leave policy / eval / GEPA / evidence files alone. Source identity must support a tracked revision **plus** a digest over declared dirty inputs. |

Leave `containers`, `optimizers`, `optimizers-beta`, `synth-mlx-rl` alone unless a producer field is missing — then say so; do not invent a fixture world.

Private dogfood instance: Workshop v0.8 **J**.

---

## Why this exists

A paid NanoHorizon Craftax inline eval was requested:

```text
seeds                         780005..780009
maximum rollouts              5
maximum model calls/rollout   10
maximum steps/rollout         2,000
hard aggregate cost ceiling   $2.45
credential route              Workshop secrets proxy only
rendering                     native canonical Craftax only
```

The container eventually came up. Live protocol probe succeeded. The model was never called. Pre-dispatch failed closed.

Observed terminal records from that attempt (do not treat as current live state):

| Record | ID |
| --- | --- |
| Optimizer run | `opt_eval_craftax_6c38576ddd18` |
| Approval receipt | `approval-982423d1863f4de2844a42c2852f299d` |
| Credential capability | `cap_ad058c19c6084e4cb655e07485503d6d` (revoked) |
| Experiment visual | `vis_479e097a401b4876be2ed18e189ec976` |
| Trace workstation | `vis_54bc7f1e5dc1493997bf0c9a150ef7cc` |
| Agent-reported draft digest | `sha256:d7c0f09e860e0fe94d170639b9bff83b6e68e3fa4e9cd5abf3ee8068dc982c68` |
| Approval / visual digest | `sha256:253caa5bcfd5dd4b13e101271ad4297f1e4049c24fa36bba1c59ac99f3a8ad6a` |

Symptoms at fail-closed:

- launch validation treated NanoHorizon as escaping the J task workspace
- registry showed five cached task instances on an unhealthy runtime
- generic MCP dropped `sessionRef` and did not map `hardTotalCostUsd`
- approved call/step caps materialized as `null`
- `policy_source_unavailable` — identity survived, bytes did not
- experiment `failed` with five rollouts still `queued`
- visual claimed five retained traces with zero relay receipts

Failing closed was correct. Everything before that showed that Workshop reconstructed the specification independently at each layer.

Required invariant:

```text
discovered D = validated D = approved D = executed D = reported D
```

---

## Intended flow

```text
approved repository source
        │
        ▼
container declaration + origin (manifest, root, revision, digest)
        │
        ▼
healthy identity-verified live container
        │
        ▼
canonical inline execution specification D
        │
        ▼
clickable paid-compute approval bound to D and its hard limits
        │
        ▼
optimizer run materialized from D
        │
        ▼
five independently tracked Craftax rollouts
        │
        ▼
sealed evidence + truthful experiment / trace visuals
```

A task workspace and a container source repository are different roots. A declaration must carry its own `source_root`. It must not inherit the chat workspace.

---

## What landed (review this; do not rebuild)

Uncommitted working copy on `eval/inline-first-admission`. Five phases, then a copy/chrome pass to match Workshop style (`WORKSHOP_QUALITY_STYLE_GUIDE.md`, `workshop_style.md`).

### 1. Declaration provenance

`ContainerDeclarationOrigin` is first-class: `manifest_path`, `source_root`, `declaration_id`, `source_revision`, `source_digest`.

- Discovery searches approved roots, not only the session workspace.
- Launch includes, working directory, and policy source resolve with `resolve_repository_path` against **that** root.
- Canonicalization that leaves the approved root is still an escape. An approved external repo is a mount, not a symlink smuggle.
- `container_ensure` / restart / reconcile resolve a declaration handle (`ensure_from_session`, `reconcile_declaration`), not `find_container_spec(session_workspace, spec_id)`.
- Structured `LaunchDeclarationError` (declared path, resolved path, digest/revision mismatch).
- Loop-breaker identity includes source root, manifest path, and declaration digest, so a repaired digest is a new attempt.

### 2. Immutable spec, limits, policy material

- `PolicyMaterialRef` on `PolicyPin`: source root, repository-relative path, tracked revision, content digest.
- `source_code` is `skip_serializing` so in-memory bytes do not fork the canonical digest.
- After admission, call / step / rollout / cost limits are required (`ResourceLimits` with non-zero counts). They flow into the run receipt, secrets-proxy `max_calls`, and rollout start.
- Missing bounds after approval is a construction error, not “unavailable.”
- Pre-dispatch re-reads policy bytes from the approved material and checks digest (`policy_source_unavailable` / `policy_source_drift`).

### 3. MCP and session binding

- Dedicated evaluation tools require `hardTotalCostUsd`.
- Generic `optimizer_manage` evaluation ops reject unknown fields (`costCeilingUsd` fails at schema).
- `bind_caller_session()` injects `SYNTH_SESSION_ID`. A mismatched caller `sessionRef` is refused.
- `authorize_inline_evaluation_start` requires a session. Paid approval must not proceed without an origin.
- Shell policy `approval: never` still cannot open a modal for ordinary commands. `ApprovalKind::ContainerLifecycle` and paid compute remain the native gates. Do not use shell as the replacement approval mechanism.

### 4. State and evidence

- `RunProgress::fail_pre_dispatch()` terminalizes nonterminal children (`cancelled` / `failed`), then fails the parent.
- Trace aggregate: `plannedSlots`, `streamsOpened`, `receiptsRetained`, `sealed`, `evidenceGaps`. `items` are retained receipts only. “no relay receipt was recorded for this seed” is a placeholder, not retained evidence.
- `reconciliationErrors` on the progress projection and experiment bindings.

### 5. Inspector and experiment visual

- Container inspector labels instance/interface counts `N live` / `N cached` / `Not reported`. Cached catalog cannot satisfy live readiness. Status copy: **Ready** vs **Couldn’t reach this container**. Launch failures use `.ws-note.ws-note-danger` with a next step. Restart… is the one primary action and still opens native lifecycle approval.
- Experiment overview: **Couldn’t reconcile this record** on `sv-terminal-receipt` (`--sv-bad-*` tokens). Empty traces: **No traces were retained. Planned slots are not evidence.**
- Run-progress adapter warns in sentence case if the campaign is terminal while trials remain queued.

---

## Invariants to check in review

```text
1. declaration source root == validation root == launch root
2. approved source path cannot escape its approved root
3. validated canonical digest == approval-bound digest
4. approval-bound digest == materialized run-spec digest
5. approved hard limits == stored limits == enforcement limits
6. approved policy revision/digest == bytes supplied to runtime
7. authenticated caller session == approval origin session
8. terminal parent ⇒ every rollout child terminal
9. retained trace count == count of actual retained receipts
10. cached metadata cannot satisfy live readiness
11. unavailable telemetry is never represented as zero
12. safety limits are never represented as unavailable after approval
```

---

## File map

| Area | Path |
| --- | --- |
| Origin, path authority, structured launch errors | `apps/synth_desktop/src-tauri/src/optimizers/workspace_recipe.rs` |
| Ensure / reconcile / replace (no port kill) | `…/optimizers/container_lifecycle.rs` |
| Policy bytes from origin | `…/optimizers/inline_eval.rs` |
| Canonical spec, `PolicyMaterialRef`, required limits | `…/optimizers/admission/spec.rs`, `pipeline.rs` |
| Pre-dispatch terminalize, reconciliation | `…/optimizers/admission/state.rs` |
| Run receipt, traces, experiment bindings | `…/optimizers/container_eval.rs` |
| MCP schema, session bind, unknown-field reject | `…/bin/synth_optimizers_mcp.rs`, `…/bin/synth_containers_mcp.rs` |
| Paid start requires session | `…/lib.rs` (`authorize_inline_evaluation_start`) |
| Loop-breaker identity | `…/ipc/mcp_stdio.rs` |
| Restart / repair IPC | `…/visuals_ipc.rs` |
| Container inspector | `src/renderer/src/components/ContainerPane.tsx` |
| Experiment visual | `visuals/families/experiments/experiment.overview.v1/shell.tsx` |
| Run-progress honesty | `src/renderer/src/runtime/runProgress/adapterEval.ts` |

---

## Tests already run

From `apps/synth_desktop/src-tauri` / `apps/synth_desktop`:

```text
cargo test -p synth-desktop --lib -- workspace_recipe admission mcp_stdio container_lifecycle trace_placeholders pre_dispatch_failure
  → 114 passed (earlier focused lane); later 3 focused reconciliation/trace tests passed

cargo test -p synth-desktop --bin synth-optimizers-mcp
  → 17 passed

npm run typecheck
  → clean

node --test tests/container_pane_cached_counts.test.mjs
  → cached counts labeled cached, never as live readiness
```

Representative unit coverage vs the original 31-test list:

| # | Intent | Status |
| --- | --- | --- |
| 1–7 | Provenance, nested root, symlink escape, dirty digest, loop-breaker digest | Present in `workspace_recipe` / `mcp_stdio` |
| 8–12 | Lifecycle approve-once / reject / never kill by port | **Thin.** `replace_declared` exists and documents no port kill; dedicated reject/approve-once tests are not a full suite |
| 13–18 | Same typed fields, unknown cost field, auto session, digest + limits through admit | Present in admission + MCP bin tests |
| 19–21 | Policy material, digest stability, origin-bound read | Present; live drift-at-dispatch still wants a dogfood receipt |
| 22–26 | Pre-dispatch terminalize, counts total 5, placeholders ≠ retained | Present |
| 27–30 | Cached vs live inspector labels, reconciliation visual | Count-label unit test + visual copy; no Playwright/CUA yet |
| 31 | Real NanoHorizon, unhealthy → approval → five rollouts or typed terminal | **Not run** |

---

## What is not done

1. **Real-system acceptance.** Starting from an unhealthy `nanohorizon-craftax` declaration, the unmodified user request should: discover origin → clickable replacement if needed → live probe → one inline digest D → clickable paid approval of D → materialize that D and its limits → five truthful rollout lifecycles or a typed pre-dispatch failure → revoke credentials → honest traces. That lane spends money. Do not claim it.
2. **Lifecycle approval tests 8–12.** Confirm reject does nothing, approve-once runs only the validated declaration, and an unrelated process on the expected port is never killed.
3. **Packaged J smoke / CUA.** Native approval modals and container restart are not proved in this working copy.
4. **Commit.** Working tree is dirty. Do not mix this with unrelated visual-admission or Candidate work on other branches.

---

## Constraints (fail the review if violated)

- Never access, read, write, import into, recover from, or configure macOS Keychain. Never use Workshop’s Keychain-backed Secrets registry.
- Provider credentials stay behind the Workshop secrets proxy or an already-authorized project-local non-Keychain mechanism. Do not read or print `.env`.
- Do not silently fall back to a catalog recipe, another model, another container, another endpoint, or another run.
- Do not encode `nanohorizon`, port `18091`, or Craftax-specific behavior into generic lifecycle code.
- Preserve `ApprovalKind::ContainerLifecycle`. Do not make shell execution the approval mechanism.
- Missing telemetry stays missing. Do not coerce to zero.
- Style: sentence case, token colors (`.ws-note`, `--sv-bad-*`), one primary action per region. No intern language (“authoritative state error”) on user surfaces.

---

## Suggested review order

1. Read this file, then `workspace_recipe.rs` origin + `resolve_repository_path`, then `container_lifecycle.rs` `ensure_from_session` / `reconcile_declaration` / `replace_declared`.
2. Walk one inline request through `admission/pipeline.rs` → `PolicyMaterialRef` → `authorize_inline_evaluation_start` → `container_eval` materialization. Confirm digest and limits are not rebuilt from a catalog recipe.
3. Confirm MCP: unknown cost field dies at schema; session comes from the host.
4. Confirm `fail_pre_dispatch` and trace counters cannot report “5 retained” for five placeholders.
5. Skim ContainerPane + experiment overview for honesty, not novelty.
6. If approving the product cut: run test 31 against dirty NanoHorizon + live `http://127.0.0.1:18091`, `REPLACE=1 WORKSHOP_PROXY_ONLY=1 PORT=18091 ./scripts/up_craftax_container.sh` only after Workshop’s own lifecycle approval path is the thing under test.

Dogfood request (same as the incident):

```text
seeds 780005..780009 · 5 rollouts · 10 calls · 2,000 steps · $2.45 hard total
openrouter/z-ai/glm-5.3-flash · nanohorizon/glm-5.3-flash
secrets proxy only · native Craftax rendering
```

At every stage the same canonical digest, policy material, model pin, seeds, and hard limits must be inspectable. If they diverge, fail closed and keep the record honest.
