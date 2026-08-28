# Engineering handoff: Craftax GLM 5.3 release-acceptance failures

Date: 2026-08-28  
Workshop instance: `j` (`com.synth.desktop.v08.dev.j`)  
Workshop source revision: `7d7783b66e51`  
Branch: `codex/finish-inline-eval-refactor`  
Run: `opt_eval_craftax_899e1fe95813`  
Session: `6c94668a-cf3a-4f02-81af-8ca195cf7214`

## Objective

Complete a release-acceptance run using exactly:

- container: `nanohorizon-craftax`
- policy: `nanohorizon/glm-5.3-flash`
- provider: `openrouter`
- model: `z-ai/glm-5.3-flash`
- seeds: `780005` through `780009`
- rollouts: 5
- maximum model calls per rollout: 10
- maximum steps per rollout: 2,000
- hard total cost ceiling: $2.45
- credential route: existing file-backed Workshop proxy only; no Keychain

The modal fixes were verified, but the evaluation is **not a release acceptance**. All five rollouts failed evidence integrity and the resulting visual is materially incomplete and remains in the wrong lifecycle state.

## Executive result

The corrected paid-compute modal displayed the intended envelope and fit inside a short Workshop window:

- `openrouter/z-ai/glm-5.3-flash`
- maximum charge `$2.45`
- `5 rollouts · 10 calls each · 2,000 steps each`
- both Reject and Approve visible

After approval, provider execution occurred through the Workshop proxy. The terminal receipt reported:

- 37 actual model calls
- $0.018659 charged (rendered as `$0.02`)
- 5/5 rollouts terminal
- 5/5 failed
- 0 valid evaluator results
- 0 sealed traces
- evidence ledger: 0 complete, 0 partial, 0 aborted, 5 missing

Every seed encountered the same trace-relay journal-integrity error at producer sequence 10. Workshop failed closed, which is correct. The producer/relay digest defect and the downstream state/projection defects remain release blockers.

## Known-good identities

The fresh container was healthy and identity-checked before paid compute:

- container ID: `ctr_c694dbc5a60f45069f82d7f06edd4530`
- runtime URL: `http://127.0.0.1:18091`
- image digest: `sha256:1bcf6309f4a10c6936fc1a563da53dc56d8f7d00560103b1bbf3d4934483a316`
- producer source revision: `04e0a94aa3336fee6dfbaab4942dc1352ab86584`
- execution spec digest: `sha256:8b9dab40c281d662f2669e169bc2cfbb51e6b3ab36930de4ab774173e6135c23`
- primary visual: `vis_7023a1183afb4bc59f7e9b99c1b2766a`
- trace workstation: `vis_e2760320a8c840ad994aeabc624812aa`

The run pinned the correct container digest, policy revision, policy-source digest, evaluator, seeds, call limits, step limits, and cost ceiling. No identity mismatch was reported before execution.

## Release blockers

### 1. All journals fail digest validation at sequence 10

Confirmed behavior:

- Seed `780008` first reported: producer digest at journal sequence 10 did not match Workshop's computed digest.
- The other four seeds later failed at the same sequence and for the same reason.
- Workshop rejected the journals rather than treating them as sealed Trace V5 evidence.

Primary code boundary:

- `apps/synth_desktop/src-tauri/src/optimizers/eval_relay.rs`
  - digest validation near the `journal event digest mismatch at sequence` error
  - producer-event decoding and `producer_digest` handling
- `apps/synth_desktop/src-tauri/src/optimizers/container_eval.rs`
- the Craftax producer implementation at source revision `04e0a94aa3336fee6dfbaab4942dc1352ab86584`

Root-cause questions:

1. What is the exact sequence-10 event kind and payload for each seed?
2. Which canonical byte representation does the producer hash?
3. Which representation does `eval_relay` recompute after decoding?
4. Is the digest intended to cover the full envelope, the payload only, or a pre-normalized carrier?
5. Are omitted/null fields, numeric normalization, map ordering, newline handling, or timestamp rewriting changing the canonical bytes?
6. Is the producer emitting a provenance digest that `eval_relay` is mistakenly treating as a SHA-256 content digest? Note that `eval_relay.rs` separately documents some producer digests as verbatim provenance and “never used as” SHA-256.
7. Why is the mismatch deterministic at sequence 10 across all seeds?

Required proof:

- capture the raw sequence-10 envelope before host transformation
- record the producer's exact preimage and digest
- record Workshop's exact preimage and computed digest
- add a cross-language golden vector shared by producer and relay
- rerun all five seeds and produce five valid sealed traces

Do not weaken or bypass digest validation.

### 2. Evidence identity is internally inconsistent

During reconciliation, Workshop reported that the terminal record for seed `780008` had no rollout ID and safely refused reconciliation. The final state later reported that all five exact rollout IDs were recorded.

This may be a late-arriving projection, a transaction-ordering race, or inconsistent reads between the terminal record and final run projection.

Investigate:

- when rollout IDs are assigned and persisted relative to terminal events
- whether `watch_run`, reconciliation, and the terminal projection read the same authoritative table/cursor
- whether terminal settlement can become visible before the rollout identity row commits
- whether replaying from cursor 0 produces the same final projection deterministically

Required test:

- force a journal-integrity failure and assert that every terminal rollout already has its stable rollout ID before reconciliation is callable

### 3. Visual lifecycle remains `streaming` after terminal failure

The optimizer card correctly rendered `Failed`, `Finished`, and `5 / 5 trials`, while `live.craftax.v1` continued to render:

- green `streaming` status
- `Follow live`
- active-looking replay controls

Primary code boundary:

- `visuals/families/first_class_example_containers/live.craftax.v1/shell.tsx`
  - `visualLive` and the status label near lines 359–360
  - `Follow live` versus `Jump to end` near the replay controls
- the bindings/projection that supplies run lifecycle to the visual

Root-cause questions:

1. Does the visual receive a terminal optimizer-run event when all evidence is rejected?
2. Is `visualLive` inferred only from missing end timestamps or a still-open subscription?
3. Does the failed run close the stream without revising the durable visual binding?
4. Is the visual subscribed to trace events but not optimizer lifecycle events?

Expected terminal UI:

- status `Failed`, not `streaming`
- no `Follow live` action
- explicit `5/5 rollouts terminal`
- prominent evidence-integrity failure summary
- disabled replay controls when there are no trustworthy replay events

### 4. Empty visual does not explain why it is empty

Observed visual state:

- `5 / 5 moments · time unavailable`
- `0 calls · cutoff seq 0`
- “No policy.call has been emitted at this temporal cutoff.”
- empty transcript, metrics, frames, rewards, and useful replay chronology
- visual digest shown as `—`; Seal disabled without a visible explanation

The provider made 37 calls. Saying no `policy.call` was emitted is technically a statement about accepted evidence, but it reads as if no calls occurred.

Required UX distinction:

- “37 provider calls occurred, but zero calls have valid retained trace evidence.”
- “Replay unavailable because all five journals failed integrity validation.”
- show the failure code, failing sequence, affected rollout/seed, and whether evidence is missing or rejected
- explain why the visual cannot be sealed

The “5/5 moments” label is also misleading. These appear to be lifecycle/terminal markers, not five meaningful replay moments. The existing tests explicitly distinguish replay moments from environment steps; add a failed-evidence fixture so terminal markers are not presented as useful replay content.

### 5. Credential lease and execution envelope disagree

Before inline admission, the agent requested and received this proxy policy:

- `maxCalls: 40`
- `maxCostUsd: 0.60`

The requested execution envelope was:

- maximum 50 calls
- maximum $2.45

The later paid-compute modal correctly approved 50 calls / $2.45. The two approval authorities therefore disagreed.

Primary code boundaries:

- `apps/synth_desktop/src-tauri/src/visuals_ipc.rs`
  - `/v1/secrets/request_use` handling and `requestedPolicy`
- `apps/synth_desktop/src-tauri/src/lib.rs`
  - limit extraction and approval payload construction
- `apps/synth_desktop/src-tauri/src/secrets/proxy.rs`
- inline evaluation admission and paid-compute approval construction

Required behavior:

- preflight must compute one effective envelope before requesting a capability
- the credential capability must be at least as restrictive as, and sufficient for, the admitted execution spec
- Workshop must refuse to start if `maximumRollouts × maximumModelCallsPerRollout > capability.maxCalls`
- the two modals must not present conflicting caps without explaining the relationship

### 6. Capability revocation appears to unregister the credential source

The agent intended to revoke the one-time capability. Its final verification instead reported the file-backed source as:

- `loaded: false`
- `registered: false`

Revoking a run-scoped proxy capability should not remove the reusable file-backed locator/source unless the user explicitly requested source removal.

Investigate the distinction among:

- revoke/expire capability
- unload source material from process memory
- unregister/remove locator or source metadata

Required test:

1. Register a file-backed source.
2. Issue a one-run capability.
3. Revoke the capability.
4. Assert the capability is unusable and absent from active leases.
5. Assert the source remains registered and can be reloaded through a future explicit approval.

No test or investigation should read credential values or use Keychain.

### 7. Cost telemetry is contradictory

During execution, the optimizer card reported:

> Cost unavailable · producer emitted no cost telemetry

At the same time, the proxy tracked calls and later produced a provider receipt for `$0.018659`.

Primary code boundary:

- `apps/synth_desktop/src/renderer/src/runtime/runProgress/usage.ts`

Required behavior:

- distinguish producer-reported cost from proxy/provider-reported cost
- prefer the trusted proxy receipt when available
- avoid saying cost is unavailable when Workshop is already displaying a live `$x / $2.45` proxy total

## Agent and transcript quality issues

The run transcript is operationally correct in several important ways—it preserved missing evidence as missing, refused unsafe reconciliation, did not bypass digest validation, and did not rerun without authorization. It is nevertheless too noisy and contains misleading transitions.

Observed problems:

1. “The exact preflight is green” was premature because the requested credential lease was later found insufficient.
2. Skill selection and workflow bookkeeping dominate the transcript.
3. “I’m updating the current conversation’s scoped title…” is internal process noise.
4. The execution path changed from a 40-call lease to inline admission without a concise explanation of the two authorities.
5. The missing-rollout-ID report conflicts with the later claim that all five exact IDs were recorded.
6. The final cleanup reports source unregistration when only capability revocation was required.
7. The task consumed roughly 174.7K agent tokens for a five-rollout acceptance, indicating excessive discovery/tool overhead.
8. There was no compact terminal handoff containing run status, cause, calls, cost, evidence state, and credential state in one place.

Recommended transcript contract:

- preflight: one compact identity/budget summary
- approval: one sentence stating the exact cap being requested
- progress: only material state changes
- failure: causal failure plus affected evidence
- final: run ID, status, calls, cost, evidence ledger, credential capability state, next blocker

## Earlier defects found during this acceptance cycle

These were repaired before the terminal run above but should remain covered by regression tests.

### Container manifest/build context

The Craftax image manifest pointed `gamebench` at `$GAMEBENCH_CRAFTAX_ROOT`, while the Dockerfile expected the `tasks/craftax-singleplayer` subtree. It was corrected to:

```toml
gamebench = "$GAMEBENCH_CRAFTAX_ROOT/tasks/craftax-singleplayer"
```

The correction currently exists in the detached evals checkout:

`/Users/joshuapurtell/GitHub/evals-craftax-live-context/containers/images/craftax-gamebench-rust/image.toml`

Verify that this change is committed in the authoritative evals repository before release.

### Stale runtime identity

An earlier container `/info` response omitted `imageDigest`. Workshop correctly failed closed with `container_image_digest_missing`. The rebuilt container now reports the verified digest listed above.

### Unhelpful manifest error

`container_ensure` initially returned only `parse workshop.containers.toml`. Parse failures should include the manifest path, field/location, and underlying TOML diagnostic without exposing secrets.

## Modal defects fixed and verified

The following fixes are present in Workshop commits `743380f8` and `7d7783b6`:

- explicit readable foreground colors for modal headings and values
- credential modal shows provider, operation, call cap, cost cap, and run instead of opaque locator details
- paid-compute modal reduced to model, maximum charge, and limits; technical details are collapsed
- approval label shortened to `Approve`
- modal is fixed to the viewport and compact enough for a short window
- both Reject and Approve are visible without scrolling the page

Checks completed:

- TypeScript typecheck
- app CSS lint
- focused Playwright approval tests
- live rebuild of Workshop J at `7d7783b66e51`
- live visual confirmation and successful approval click

## Reproduction

Use a fresh Workshop instance built from the target revision. Do not reuse this failed run and do not use Keychain.

1. Verify `/info.imageDigest` matches the local Docker image digest.
2. Verify producer source revision and exact policy/model identity.
3. Confirm zero active credential capabilities.
4. Request a file-backed OpenRouter proxy capability for the full 50-call / $2.45 envelope.
5. Admit exactly seeds `780005..780009`, five concurrent rollouts, ten calls and 2,000 steps per rollout.
6. Confirm both approval surfaces show mutually consistent bounds.
7. Start the run and retain the raw producer journal before host canonicalization.
8. On sequence 10, compare the producer and Workshop digest preimages byte-for-byte.
9. Allow the run to terminalize and verify optimizer and visual lifecycle states agree.
10. Revoke only the run capability and verify the file-backed source remains registered.

## Acceptance criteria for closure

- five exact seeds complete under the pinned runtime/policy/model identities
- no journal digest mismatches
- five stable rollout IDs exist before terminal settlement
- five sealed Trace V5 traces
- rewards, steps, calls, tokens, frames, achievements, and cost are retained or explicitly unsupported by contract
- visual terminal state matches optimizer terminal state
- replay shows meaningful chronological evidence, not terminal placeholders
- transcript reports the 37-or-fewer actual calls from accepted `policy.call` evidence and the proxy receipt
- cost source is labelled accurately
- run capability is revoked while the file-backed source remains registered
- no Keychain access occurs

## Local evidence locations

Workshop J transcript:

`/Users/joshuapurtell/.synth-desktop/instances/v08/j/data/codex/homes/6c94668a-cf3a-4f02-81af-8ca195cf7214/sessions/2026/08/28/rollout-2026-08-28T10-05-19-01a048b0-75a4-7c53-b469-2c0c09570ebe.jsonl`

Workshop J data root:

`/Users/joshuapurtell/.synth-desktop/instances/v08/j/data`

Treat these paths as local diagnostic evidence. Do not copy credential material into issues, tests, or logs.
