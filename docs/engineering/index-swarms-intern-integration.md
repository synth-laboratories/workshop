# Workshop: Index, Swarms, and Intern integration

**Status date:** 2026-09-16  
**Target baseline:** local `dev` / `v0.11.1` at `bf09f473`  
**Document status:** canonical consolidation plan; it records existing work but does not claim that unmerged branches ship in the current release

## Executive summary

Workshop already has meaningful implementation work for Synth Index, Swarms,
and Intern, but it is split across divergent branches. The current local
`dev` and `v0.11.1` contain the live Harbor workspace/tool-stream work and are
clean, but they do not contain the product work described below.

| Product | Existing implementation | Current conclusion |
| --- | --- | --- |
| **Index** | Rust contribution client/export pipeline and a Reports “Prepare Contribution” panel on `codex/synth-index-fast-mvp` | A credible contribution MVP exists, but it is not on current `dev` and needs reconciliation and release QA. |
| **Intern** | Rust sync/async client, pollers, ingestion, scoped runtime, durable mailbox, and Workshop panel on `codex/swarms-intern-index-consolidated-20260914` | The Desktop transport is the most complete of the three. Real worker-produced staging output remains the principal external qualification gate. |
| **Swarms** | Backend contract pin, durable swarm checkpoint primitives, rollout listing contract, and shared benchmark observations on the consolidated branch | Foundations and read-side visualization exist. A first-class launch/control/recovery workspace does not yet exist. |

The branch named `codex/swarms-intern-index-consolidated-20260914` does **not**
contain the separate Index contribution MVP commit. “Index” in parts of that
line also refers to the packaged VictoriaLogs diagnostics index. Synth Index
must therefore be integrated explicitly from `codex/synth-index-fast-mvp`.

## Product boundary

The three products are related, but they must keep distinct authorities:

```text
Intern                         Swarms                         Index
cloud agent mailbox           multi-rollout execution       contribution publication
sync + background work        pool/run orchestration        draft/review/release flow
          \                         |                         /
           \                        |                        /
            +---- Workshop Rust CoreRuntime + journal -----+
                         |
                         +-- chat/session projections
                         +-- right-panel live visuals
                         +-- reports and retained evidence
                         +-- scoped cloud identity
```

- Rust/Tauri is the authority for credentials, network calls, durable state,
  idempotency, account scope, and journal commits.
- React projects typed state. It never calls Synth cloud endpoints directly
  and never owns a second mailbox, swarm ledger, or publication state machine.
- Codex, Intern, Swarms, evals, and Harbor may all produce evidence, but they
  converge on the same session/run journal and visual binding contracts.
- Index publishes selected, reviewed contribution assets. It must never infer
  that all project files are eligible for upload.

## Current baseline

Current local release branches:

- `dev` → `bf09f473` (`Stream Harbor tool evidence into live visuals`)
- `v0.11.1` → `bf09f473`
- `codex/v0111-live-workspace-streaming` → `bf09f473`

This baseline contains:

- live Harbor/Codex tool metadata in the right-panel visual;
- cursor replay and automatic verifier sealing;
- read-only live workspace projection;
- retained inline evidence after the producer exits;
- coherent native pixel capture of live-stream evidence.

It does not yet contain the feature branches catalogued below.

## Existing Index work

### Source

- Branch: `codex/synth-index-fast-mvp`
- Merge commit: `037af1eb` (`Merge Index SDK/MCP/agents lane`)
- Branch tip: `8281089c` (right-panel visual open fix layered on the MVP)

### Implemented scope

The MVP adds a Workshop-native **Prepare Contribution** flow under Reports:

1. Select an explicit set of files and assign contribution roles.
2. Build a local preview containing manifest, provenance, claims, evidence,
   reproduction metadata, license, rights, sensitivity, and audience.
3. Create or resume a private Synth Index draft.
4. Upload only the selected bytes through server-issued upload targets.
5. Finalize the revision without silently publishing it.
6. Submit through a separate explicit owner action.
7. Read the revision and independent-review status.

The typed commands are:

- `index_export_preview`
- `index_export_upload`
- `index_export_submit`
- `index_revision_status`

Important safeguards already present in the MVP:

- the renderer never receives the Synth API key;
- preview is local-only and makes no network request;
- retry identities are stable;
- upload target paths and SHA-256 digests are checked;
- public audience requires explicit confirmation and still means “request
  public release,” not automatic publication;
- rights attestation is required;
- runnable reproduction requires an entrypoint and environment assets;
- missing evidence requires an explicit reason;
- only selected files are collected and uploaded.

### Remaining Index work

- Rebase/port the MVP onto the current CoreRuntime and generated command
  contract instead of merging its old generated dispatch wholesale.
- Reconcile Reports UI changes with the current report/visual layout.
- Add durable local export receipts so restart and uncertain finalization are
  visible and resumable.
- Fence status reads and retries by current cloud identity/account generation.
- Validate backend schema compatibility against the current Index API.
- Add fixture, browser, installed-app, and real private-draft acceptance.
- Decide whether browsing/importing existing Index contributions belongs in
  this release; it is not part of the contribution MVP.

## Existing Intern work

### Source

- Consolidated branch: `codex/swarms-intern-index-consolidated-20260914`
- Intern contract merge: `d08a6c46`
- Primary alignment commit: `5cf361af`

### Implemented scope

The consolidated line includes:

- a Rust Research Intern client for Workshop’s sync/live and async/background
  mailbox subset;
- typed wire models and recorded transport fixtures;
- lazy cloud configuration so local Workshop startup does not require a cloud
  account;
- pollers with replay deduplication and commit acknowledgement;
- generation/account-scope fencing;
- semantic command receipts and uncertain-outcome reconciliation;
- atomic durable cloud creation and first-send intent;
- scoped history and settlement reads;
- a durable mailbox/outbox and turn-boundary delivery host;
- a confined Codex executor for mailbox work requests;
- a minimal Workshop mailbox panel;
- sync/async lifecycle controls and restart reconciliation;
- removal of the old Python product-runtime dependency in the intended final
  architecture, with Python retained only for Laguna/MLX inference.

The governing invariant is **one Intern mailbox**. Sync and background modes
are two execution modes of the same remote authority, not separate products.
Workshop may mirror and control that mailbox through typed commands, but must
not invent another commander queue.

### Qualification status

The prior installed-app dogfood reportedly proved create/send/control/poll,
cursor persistence, restart reconciliation, async singleton reuse, and local
database integrity. The open operational gate was external: staging accepted
instructions but emitted no worker `agent_message`, checkpoint, or result.

That claim must be re-proved after integration. Passing transport fixtures is
not sufficient to claim live Intern support.

### Remaining Intern work

- Reconcile the consolidated runtime with current `dev` and current cloud
  contracts without regressing local signed-out startup.
- Verify no legacy rows can cross account generations after sign-out/sign-in.
- Prove sync worker output, async checkpoint/result output, pause/resume,
  intervention, cancellation, and restart from a real staging environment.
- Project Intern-produced visuals through the same right-panel host used by
  Codex and Harbor.
- Preserve credential boundaries: project-local configuration or the existing
  authorized secrets proxy; no renderer secrets and no implicit Keychain use.
- Remove stale/dead transitional surfaces only after installed-app parity.

## Existing Swarms work

### Source

- Consolidated branch: `codex/swarms-intern-index-consolidated-20260914`
- Contract update: `2c9e2ff6` (backend revision with swarm rollouts)
- Shared visual projection: `5bc6f7a1`

### Implemented foundations

- The pinned research contract includes
  `GET /smr/runs/{run_id}/rollouts` and typed `SwarmRolloutPage` /
  `SwarmRolloutSummary` objects.
- Cloud storage has a distinct `Adapter::Swarm` and opaque swarm checkpoint
  shape. Opaque cursors are compared for equality rather than treated as
  sequence numbers.
- Storage tests cover account fencing and reject empty heartbeat commits that
  would advance a durable swarm checkpoint.
- Live eval projections can carry producer-authored benchmark observations.
- A shared Benchmark Status visual renders lane phase, work/verify clocks,
  calls and steps, score, token/cost usage, spend enforcement, stop reason,
  observation time, and retained evidence location.
- Craftax and Harbor visuals can use those observations without inventing
  local billing or lifecycle truth.

### Missing first-class Swarms product surface

The current work is not a complete Swarms integration. It still needs:

- a typed Swarms client/manager rather than storage primitives alone;
- swarm creation/launch with explicit budget, provider, model, task set, seed,
  and concurrency bounds;
- a run list and run-detail workspace;
- live worker/rollout topology and per-lane progress;
- pause/cancel/stop semantics with generation fencing;
- restart and cursor recovery;
- intervention and failed-worker handling;
- budget/refusal/settlement truth;
- selection of a rollout into the right-panel trace/workspace visual;
- terminal retained evidence and Index handoff.

## Target Workshop experience

### Intern

- Start **Intern Live** or **Intern Background** from the same execution-target
  model used by Workshop sessions.
- Show mailbox-authoritative state, receipts, interventions, checkpoints, and
  results in the normal chat/activity composition.
- Let Intern open/update a right-panel visual using the same visual identity
  and evidence rules as local Codex.
- Leaving the conversation must not stop background work.

### Swarms

- Start a bounded swarm from a recipe/eval/task set with one aggregate approval
  and one declared maximum spend.
- Show workers and rollouts as live lanes, not a wall of transcript text.
- Cycle among live rollout traces/workspaces in the right panel.
- Keep aggregate state and individual rollout evidence distinct.
- On completion, retain a sealed run summary and every referenced rollout cut.

### Index

- From Reports or a sealed run, choose **Prepare Contribution**.
- Select the report, traces, visual captures, environment/reproduction files,
  and limitations explicitly.
- Preview exactly what leaves the machine.
- Upload a private draft, then submit through a separate explicit action.
- Show reviewer/QA status without implying approval or publication.

### Combined journey

```text
Intern or local Codex defines the experiment
  → Swarms executes bounded parallel rollouts
  → Workshop streams lane/tool/workspace evidence live
  → verifier and usage settle
  → report/visuals retain exact evidence cuts
  → user prepares a selected Index contribution
  → private draft → explicit submit → independent review status
```

## Integration strategy

Do not directly merge both historical feature branches and accept generated
file conflict resolution as architecture. Their histories overlap older
CoreRuntime, cloud, visual, and generated-contract work.

### Phase 0 — Freeze and inventory

1. Tag the current `bf09f473` baseline.
2. Record all relevant source commits and generate semantic diffs by subsystem.
3. Separate Synth Index changes from diagnostics-index/VictoriaLogs changes.
4. Keep current Harbor streaming and pixel-capture tests as non-regression
   gates throughout the work.

### Phase 1 — Shared cloud foundations

Port the minimal common substrate first:

- account/cloud identity epochs;
- scoped runtime and durable cloud storage;
- mailbox/outbox transaction boundaries;
- typed private API client boundary;
- journal append/broadcast after commit;
- restart and uncertain-receipt reconciliation.

Exit gate: signed-out local Workshop remains fully usable; switching accounts
cannot reveal or replay prior scoped state.

### Phase 2 — Intern

Port the Intern client, pollers, ingestion, mailbox host, commands, and panel.
Retain one mailbox authority and use shared session/run services.

Exit gate: real staging sync and background runs emit worker-produced output,
survive restart, accept intervention, and stop/cancel without duplicate events.

### Phase 3 — Swarms

Add the Swarms client/manager on the common cloud foundation. Implement bounded
launch/control and live rollout projection before adding decorative topology.
Reuse Harbor/Craftax live visual transport for tool and workspace evidence.

Exit gate: a multi-rollout CyberneticsBench run streams multiple lanes, cycles
their right-panel evidence live, respects budget/concurrency bounds, recovers
after restart, and seals exact verifier/usage results.

### Phase 4 — Index

Port the contribution MVP by hand onto the current generated command and
Reports architecture. Add durable receipts and account-scope fencing.

Exit gate: preview makes zero network calls; private upload is resumable and
byte/digest exact; submit is separate; status reflects independent review.

### Phase 5 — Cross-product polish

- Consistent nouns and status colors across mailbox, swarm, eval, report, and
  contribution surfaces.
- One right-panel visual host and one retained evidence grammar.
- Deep links from swarm → rollout → trace/workspace → report → contribution.
- Accessibility, keyboard navigation, narrow-panel behavior, and native pixel
  capture for every live surface.
- Remove obsolete transitional adapters only after evidence-backed parity.

## Test and acceptance matrix

| Area | Deterministic tests | Installed-app / live proof |
| --- | --- | --- |
| Shared cloud | identity generation, transaction rollback, outbox retry, cursor fencing, redaction | signed-out boot; sign-in/out/in with no cross-account rows |
| Intern | fixture client, dedupe, receipts, restart, singleton background binding | real sync response; background checkpoint/result; intervene; pause/resume/cancel; restart |
| Swarms | contract decode, bounded launch, cursor recovery, worker failure, usage/settlement, terminal seal | multi-rollout CyberneticsBench run with live lane cycling and exact final scores |
| Index | path containment, selected-file only, digest checks, preview offline, idempotent retry, audience/rights validation | private draft upload, restart/resume, explicit submit, reviewer status |
| Visuals | replay gaps/conflicts, no invented missing values, capture freeze, retained evidence | right panel updates during execution; native pixel capture contains live rows; reopen works without producer |
| Security | log redaction, bounded errors, no renderer credentials, account scope | inspect process args/logs/storage; confirm no keys or raw tool content leaked |

### Required non-regressions

- Existing Harbor bridge tests remain green.
- `v02-live-eval-visuals.spec.ts` remains green.
- Visual evidence retention and native pixel-capture tests remain green.
- TypeScript typecheck and frontend production build remain green.
- Rust contract and storage suites remain green.
- A full packaged-app smoke test is required; web-only Playwright is not the
  release oracle for native cloud commands.

## Cost and approval behavior

- An explicit experiment request authorizes its related provider-backed arms
  under the user-approved aggregate cap; do not prompt once per rollout.
- Workshop must present the aggregate maximum before execution and preserve the
  actual settled cost afterward.
- Product-enforced paid-compute gates remain authoritative.
- Evals and Swarms should support a first-class bounded QA mode so programmatic
  acceptance does not require repetitive source/recipe prompts.
- QA mode must narrow capabilities; it must not bypass credentials, account
  scope, spend limits, or destructive-operation confirmation.

## Security and privacy invariants

- Never place provider or Synth API keys in renderer state, command arguments,
  visual events, traces, or uploaded Index assets.
- Do not use macOS Keychain implicitly. Use only an already-authorized
  project-local environment or Workshop secrets proxy.
- Live visual tool feeds expose sanitized tool name/status/duration/exit code,
  never raw commands, stdout, instructions, reasoning, or credential values.
- Index sends only explicitly selected files and displays an exact preview.
- Public Index release is always a request requiring explicit user confirmation
  and independent review.
- Account generation and scope fence every remote read, retry, receipt, cursor,
  and persisted binding.

## Definition of done

This integration is complete only when:

1. Index, Swarms, and Intern live on the same current release branch.
2. Local/signed-out Workshop remains usable with no cloud configuration.
3. Intern Live and Background produce real worker output and survive restart.
4. Swarms can launch and control a bounded multi-rollout run and stream each
   rollout into the right panel while it is running.
5. Final scores, usage, spend, stop reasons, and evidence are retained exactly;
   missing data is never rendered as zero.
6. A sealed run/report can become a selected private Index draft without
   leaking unselected project data.
7. Submission is explicit and review/publication status is truthful.
8. Identity switching, cancellation, retry, crash/restart, and partial network
   failure have deterministic tests and installed-app evidence.
9. No credential or raw sensitive tool content appears in UI evidence, logs,
   traces, or contribution uploads.
10. The current Harbor/live-visual and native capture behavior remains intact.

## Branch and commit ledger

| Purpose | Branch / commit |
| --- | --- |
| Current live-visual baseline | `dev`, `v0.11.1` → `bf09f473` |
| Intern/Swarms consolidation | `codex/swarms-intern-index-consolidated-20260914` → `5bc6f7a1` |
| Intern contract consolidation | `d08a6c46`; alignment at `5cf361af` |
| Swarm rollout contract pin | `2c9e2ff6` |
| Shared benchmark visual projection | `5bc6f7a1` |
| Synth Index contribution MVP | `codex/synth-index-fast-mvp`; merge `037af1eb`; tip `8281089c` |

These references are inputs to a controlled port. They are not, by themselves,
proof that the features are present or release-qualified on current Workshop.
