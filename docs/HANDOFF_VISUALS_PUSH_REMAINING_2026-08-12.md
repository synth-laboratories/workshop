# Handoff — Aug 12 visuals push, remaining work

Written 2026-08-12 evening for whoever picks this lane up. The visuals rebuild
itself (GEPA workspace, SFT family, Craftax/Harbor/dig.bench live viewers,
shared components, responsive gate) is **done, tested, and committed** on
`josh/aug12-optimizers-workshop-visuals`. What remains is landing work,
one cross-repo contract half, three acceptance gaps (A2/A6/A8), and the
external drills that were always receipts rather than product code.

Authority docs, in order: this file → [`aug_12_update.md`](./aug_12_update.md)
(A1–A8 acceptance + receipts) → [`aug_12_remaining.md`](./aug_12_remaining.md)
(A9–A18 / V / O / W scope disposition and drill specs) →
[`receipts/2026-08-12/README.md`](./receipts/2026-08-12/README.md).

---

## 1. Where things stand

- **Branch:** `josh/aug12-optimizers-workshop-visuals`, tip `7804a3d`
  ("land Aug 12 v0.2 bind, GEPA multiplex, and live visuals"). **46 commits
  ahead of `origin/main` (merge-base `1606284a`), no upstream, nothing
  pushed.** CI is suspended repo-wide (deploy-only policy), so a push will
  not run tests for you — run them locally (§5).
- **The worktree is shared with live agents.** At handoff time ~13 files were
  dirty from a concurrent lane (Composer/ComposerDock/PaneResizeHandle/
  VisualsPage/routes/app.css, `runtime-regressions.spec.ts`, receipt docs)
  with mtimes seconds old. **Check mtimes before committing or attributing
  failures**; never `git add -A` here.
- **Verified green at close of the push** (receipt index numbers, which
  supersede any earlier counts):
  - visuals node tests: **65 pass** (`npm run test:visuals`)
  - Workshop Rust: **392 pass**, 1 paid test ignored
  - desktop renderer: TypeScript + production Vite build pass
  - Playwright: `visuals-registry` + `visual-responsive-gate` (3 specs,
    12 DOM-measured screenshots) + `optimizer-banking77` all pass
  - `cargo check`: 0 errors
  - Containers: 278 pass / 8 platform skips / **1 fail — the Docker daemon on
    this machine cannot start containers** (environment fault, see A2)
- **Acceptance state** (receipts in `docs/receipts/2026-08-12/`):
  A1 PASS (10/10 paid Luna lanes, $0.0311) · A3 PASS (two live GEPA,
  Luna vs Sol, four visual flips, no stall) · A4 PASS (two hosted Tinker SFT)
  · A5 PASS · **A6 PARTIAL** · **A2 NOT DONE** · A7 out of cut ·
  **A8 BLOCKED**.

## 2. What was built — map for navigation

Everything below is committed; paths are the places you'll actually edit.

| Layer | Where |
| --- | --- |
| Shared workspace components (algorithm-agnostic header, stage timeline, rollout browser) | `visuals/templates/optimizer.run.v1/components/workspace/{WorkspaceChrome,RolloutBrowser}.tsx` |
| Shared projection (GEPA state machine, limits, gate decisions, `proposer.delta`/`proposer.transcript.loaded` handlers, goex slice) | `visuals/templates/optimizer.run.v1/components/projectEvents.ts` |
| GEPA workspace (frontier, candidate inspector + word diff, proposer trace incl. streaming + reflection, Luna-vs-Sol comparison) | `visuals/templates/optimizer.run.v1/overlays/gepa/` |
| SFT workspace (stages incl. promotion ≠ ready, curves, checkpoint rail, campaigns, provenance) | `visuals/templates/optimizer.run.v1/overlays/sft/{model.ts,SftWorkspace.tsx}` |
| Thin template shells over the shared pieces | `optimizer.gepa.{live,frontier,candidate,evaluations}.v1`, `optimizer.sft.live.v1` |
| Craftax semantic viewer (step grouping, semantic checkpoints, scope isolation) | `visuals/templates/live.craftax.v1/{projectCraftax.ts,shell.tsx,viewer.css}` |
| Harbor / dig.bench live templates (trial folding, reward fails closed; history + lanes, no fake frames) | `visuals/templates/live.harbor_eval.v1`, `visuals/templates/live.digbench.v1` |
| Long-ID component + workspace CSS tokens | `visuals/chrome/Identifier.tsx`, `visuals/chrome/tokens.css` |
| Desktop host: comparison-sibling loader, pane expand, proposer-transcript backfill | `apps/synth_desktop/src/renderer/src/components/VisualHost.tsx`, `src-tauri/src/optimizers/recipes.rs` (`append_proposer_transcripts`) |
| Tests | `visuals/tests/{gepa_workspace,sft_workspace,craftax_semantic,optimizer_family}.test.mjs`; `apps/synth_desktop/tests/playwright/visual-responsive-gate.spec.ts` |

Real-run fixtures the tests and gate replay (do not rerun paid GEPA to get
these):
`~/.synth-desktop/instances/v02/gepa/runtime/gepa/runs/banking77_gepa_sol_med_45856f25`
and `…/banking77_gepa_luna_med_82f8136b` (`events.optimizer.jsonl` in each; the
Playwright gate skips itself if they're absent).

## 3. Remaining work, in recommended order

### 3.1 Land the branch (gated on Josh)

Push `josh/aug12-optimizers-workshop-visuals` and open the PR. Before any
commit: confirm the concurrent lane has gone quiet (mtimes), fold or drop the
empty placeholder `docs/aug_12_notes.md`, and decide whether the live-lane
dirty files (in-app live receipt + composer/visuals-page edits) ride along or
land separately. No branch protection exists; nothing merges itself.

### 3.2 Producer-side `proposer.delta` — the one cross-repo item

**Repo: synth-optimizers, not workshop.** The consumer side is complete and
tested here; live runs show no proposer text until the producer emits it.

- **Contract:** `optimizer_event.v1` event `type: "proposer.delta"`, delta
  payload `{ generation, channel, text }` — one event per streamed chunk. The
  source is opencode's SSE `item/agentMessage/delta` items (the real Sol run
  log contains 262 of them). On completion the producer may emit
  `proposer.transcript.loaded` itself; today the Desktop backfills it from
  `proposer_workspaces/generation_*/.agent_artifacts/opencode_response.json`
  (see `recipes.rs::append_proposer_transcripts` — idempotent event IDs
  `{run_id}:proposer-transcript:{generation}`, strings length-capped with
  explicit `truncated` flags).
- **Consumer behavior to test against:** chunks extend one open trace row in
  place per generation (never add rows); reconciliation replaces streaming
  text with the structured reflection. Locked by tests in
  `visuals/tests/gepa_workspace.test.mjs`; UI in
  `overlays/gepa/ProposerTracePanel.tsx` (`proposer-streaming`,
  `proposer-reflection` testids).
- **Artifact guidance:** `opencode_response.json` is the human story
  (critique / failed patterns / winning patterns / rationale / proposals);
  `opencode_messages.json` is JSON-RPC transport noise — don't project it.

### 3.3 A2 — Harbor GameBench live

Two blockers, one code, one environment:

1. `harbor_docker.py` has **no pinned-bundle path**, so nothing reaches
   Workshop even when Docker works.
2. The Docker daemon on this machine currently **cannot start containers**
   (the single red Containers test). Fix or move machines before attempting.

The Workshop side is ready: `live.harbor_eval.v1` folds
trial.planned/launched/verifier into trial cards, per-trial `reward.txt`
fails closed ("missing — never defaulted to 0"), ATIF is labeled a
projection. Acceptance needs: in-app register of a Harbor-packaged GameBench
task, two pinned policies, visual open before start, native-vs-wrapped
verifier agreement, sealed Trace V5.

### 3.4 A6 — SFT checkpoint-eval scoring

Structure passed 7/7; **campaign rollouts score `null`** because the
container cannot sample a Tinker checkpoint locally. The fix is a hosted
sampling path for checkpoint-eval campaigns (optimizers-beta side). The
`SftWorkspace` already renders selection-vs-heldout (heldout labeled
measurement-only), aligned curves, and promotion ≠ checkpoint-ready — real
numbers light up with no visual work. **Caution:** the hosted
`sft_banking77_nemo_30k_v2` run may still be training on the replacement
beta server — do not restart or disturb that server to test this.

### 3.5 A8 — dig.bench capstone

Blocked purely on **`DIGBENCH_API_TOKEN`** (api.digbench.ai returns 401;
no token on this machine). `live.digbench.v1` is ready: observation /
legal-actions / lives / level / steps, bounded ordered history, multi-lane,
scope binding, `/reward` pending until terminal status, text evidence only.
Acceptance requires: freeze one public game on the receipt, **both**
harnesses (basic ReAct next-action AND agentic Codex + `digbench-mcp`),
visual connected before `start_session`, reopen after their relay/session is
gone, token never in the log. C8 `digbench_mock` remains the only exercised
path. Do not Harbor-wrap or OpenEnv-wrap it.

### 3.6 External drills — receipts, not product code

`aug_12_remaining.md` is the spec; its scope table marks all of these
"implemented" at the product layer. What's missing is the machine receipt
(the `receipt.json` bundle format is in that doc's *Receipt standard*
section). Run from a clean split tip with isolated data roots; several are
destructive.

| Drill | What's still owed |
| --- | --- |
| A11 reconnect/reopen | A destructive **live** socket + container-kill transcript (in-process recovery is already proven) |
| A12 idempotency | A paid-call-count receipt under retry (deliberately not claimed without a paid run) |
| V5 performance | Browser heap / long-task benchmark under sustained live delivery (the deterministic 100,017-envelope projection test exists and passes) |
| V6 accessibility | Formal axe / screen-reader receipt (names, focus, reduced-motion already in) |
| O1–O4 operations | Parallel-budget exhaustion, cancellation matrix, slow-consumer backpressure, auth-rotation + nested-payload redaction drills |
| O5 rebuild identity | Rebuild + relaunch the named instance repeatedly; verify stable signer and no repeated Keychain prompts. **Deliberately not run while the tree was dirty** — safe to run after landing. |
| W1 | Clean-workspace agent run: "find the Craftax container, register, run ten, visualize" with no guessed URLs |
| W2 | Real CUA-agent visual-iteration gate (create → review → revise → ready → paid start). The Playwright `visual-responsive-gate` is a rendered-DOM **proxy** for this, not the agent acceptance itself. |
| W3 | Tool-failure injection (visual MCP down, 503s, frame 404, pin refusal) — agent must stop with a precise blocker, never fabricate |

### 3.7 GELO visuals — concurrent lane, do not assume

No `optimizer.gelo.*` template exists yet. `goex.sft.v1` is the GELO plugin
(≠ standalone `algorithm_id: "sft"`; do not collapse them — locked decision).
The goex slice inside `projectEvents.ts` belongs to the other agent's lane
and they were active at handoff time. **Sync with that lane before starting
anything GELO-shaped.** The shared workspace components were built for
exactly this reuse: header + stage timeline + `RolloutBrowser` + `Identifier`
should carry a GELO workspace the way they carried SFT.

### 3.8 Known non-blockers (leave alone unless they bite)

- Specta TS export is still disabled (`i64`/`u64` vs JSON) — explicit
  decision: **do not block on the exporter**.
- `visuals/tsconfig.json` emits TS5097 noise — pre-existing, not a gate;
  validate new files with ad-hoc strict `tsc` flags.
- Merged Luna-vs-Sol Pareto overlay and a side-by-side two-board layout are
  **out of this cut** (flip is enough); so are Prime GSM8K, Chess OpenEnv,
  MAPO/RLVR/OHCO, GEPA/SFT on dig.bench.

## 4. Contract invariants — do not regress

- **Missing ≠ 0.** Reward, usage, cost, sequence fail closed. Cost renders
  "unavailable", never a fake zero.
- **Connect-before-run; persist-before-publish.** Visual bound to the
  *declared* stream ID before the first paid call; never a constructed URL.
- **No LIVE/Waiting on completed runs.** Terminal runs say so
  ("At end of run"); queued runs are honestly queued with zero fabricated
  progress.
- **Promotion ≠ checkpoint-ready.** Promotion completes only on an explicit
  event.
- **Heartbeats/control records never advance the evidence cursor.**
- **Raw events stay accessible behind disclosure** — semantic projection is
  the primary surface, never the only one.

## 5. How to verify anything you change

```bash
# from repo root
npm run test:visuals                                   # 65 tests
cd apps/synth_desktop
npm run typecheck
npx playwright test tests/playwright/visuals-registry.spec.ts \
                    tests/playwright/visual-responsive-gate.spec.ts \
                    tests/playwright/optimizer-banking77.spec.ts
(cd src-tauri && cargo check)
```

The responsive gate writes 1440/1024/768/390 screenshots to
`apps/synth_desktop/test-results/visual-gate/` — **look at them**, don't just
trust the overflow assertion. A headless render-check without Tauri:
esbuild-bundle a template shell (`--alias:react=<workshop node_modules>`,
`--loader:.css=empty`, `--format=cjs`) + `renderToString` against real run
events; stub `useLiveEvalStream` with an esbuild `onResolve` plugin for
templates that ingest live.

## 6. Gotchas that cost real time

- Playwright `getByRole(name:)` **substring-matches** — an aria-label
  containing another spec's role name breaks it silently. Grep existing specs
  before naming controls.
- VisualPane expand must hide `> :not(.visual-pane)` siblings; targeting
  `.inventory-page` misses `VisualsPage` (different class) and leaves a
  sliver pane at 390px.
- Comparison payloads need `normalizeOptimizerEvents` before
  `projectAtCursor` — raw snake_case pages project to an empty state with no
  error.
- Templates render in the gallery preview **and** the pane simultaneously —
  scope Playwright locators to `getByTestId("visual-pane")` or hit
  strict-mode violations.
- `node --experimental-strip-types` cannot import `.tsx` — keep pure logic in
  `.ts` siblings (the `overlays/*/model.ts` pattern) so node tests can import.
- Ghostty can drop `~/Documents` TCC access mid-session: `git`/`ls` die with
  "Operation not permitted" repo-wide while network tools keep working.
  Restart Ghostty or re-grant in System Settings.
- This worktree hosts concurrent agents. Check file mtimes before committing
  or attributing failures; prefer new files and tight edit anchors.
