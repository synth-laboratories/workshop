# Engineering handoff: finish NanoHorizon Craftax release acceptance

Date: 2026-08-28  
Primary repo: `/Users/joshuapurtell/GitHub/workshop-v08-e2e-refactor`  
Branch: `codex/finish-inline-eval-refactor`  
Workshop tip at handoff: `f6247c0b1cbac0ae6ecb58c4a676cf2c3f007093`

## Executive status

The implementation is substantially repaired, but release acceptance is **not
complete**. One paid five-seed run occurred and correctly failed closed because
all five producer journals failed digest verification. That run is diagnostic
evidence only and must not be reused or relabelled as acceptance.

Since that run, 32 Workshop commits and six Containers commits have addressed
the journal contract, terminal identity and lifecycle, credential/cost
reconciliation, retained evidence, visual projections, replay, aggregate UI,
and package provenance. The latest Workshop tree is clean and compiles with
the eval-driver feature. A fresh five-seed provider-backed run has **not** yet
proved the repaired end-to-end path.

Read these companion documents before changing code:

- `docs/HANDOFF_CRAFTAX_GLM53_RELEASE_ACCEPTANCE_FAILURES_2026-08-28.md`
- `docs/RCA_CRAFTAX_GLM53_RELEASE_ACCEPTANCE_2026-08-28.md`
- `docs/DOGFOOD_NANOHORIZON_INLINE_EVAL_2026-08-27.md`

The failure handoff is the acceptance contract. The RCA gives byte-level root
causes. This document records the newer repository state and the shortest safe
route to closure.

## Current repository state

Do not use temporary agent worktrees or dirty primary checkouts in the final
run. Do not stash. Preserve all existing changes.

| Component | Checkout | Revision / state |
| --- | --- | --- |
| Workshop | `/Users/joshuapurtell/GitHub/workshop-v08-e2e-refactor` | `f6247c0b1cbac0ae6ecb58c4a676cf2c3f007093`, branch `codex/finish-inline-eval-refactor`, clean at handoff |
| Containers | `/Users/joshuapurtell/GitHub/containers-nanohorizon-e2e-final` | `92bb5b36ff777dab7d7b69842f9bcc3c086bb273`, branch `codex/craftax-envelope-digest-v2`, clean |
| NanoHorizon | `/Users/joshuapurtell/GitHub/nanohorizon-e2e-final` | `574ace4b5161c6c3f03d737160375f2e4b4dd56a`, detached, clean |
| Evals | `/Users/joshuapurtell/GitHub/evals-craftax-live-context` | `4726e2bd332b853731dd3b05f49c33935c5c3c0f`, detached, **dirty** |
| GameBench | `/Users/joshuapurtell/GitHub/gamebench-craftax-live-context` | `3d35f379a6d3f951720bfcc04d0f05518d9b8034`, detached, clean |

The Evals dirt is intentional WIP and must be reviewed, tested, and committed
or otherwise deliberately resolved; never discard it blindly:

- `containers/images/craftax-gamebench-rust/image.toml` changes the GameBench
  build context from the repository root to
  `tasks/craftax-singleplayer`.
- `containers/images/craftax-gamebench-rust/tests/test_craftax_gold_environment.py`
  updates the build-context assertion accordingly.

The current `scripts/preflight-nanohorizon-e2e.sh` is stale: it still pins
Containers `04e0a94aa333...` and source manifest
`sha256:6481652f3b3f...`. Do not make it green by weakening checks. First commit
and anchor the Evals fix, then recompute the complete source closure, update
the exact revisions/digest, and rerun the three-repository validator.

## The failed live run

Run: `opt_eval_craftax_899e1fe95813`  
Session: `6c94668a-cf3a-4f02-81af-8ca195cf7214`  
Workshop at execution: `7d7783b66e51`  
Producer at execution: `04e0a94aa3336fee6dfbaab4942dc1352ab86584`

Requested contract:

- container `nanohorizon-craftax`
- policy `nanohorizon/glm-5.3-flash`
- provider/model `openrouter/z-ai/glm-5.3-flash`
- seeds `780005..780009`
- five rollouts
- ten model calls and 2,000 steps per rollout
- hard total cost ceiling `$2.45`
- file-backed Workshop proxy only; no Keychain

Observed terminal facts:

- 37 provider calls
- `$0.018659` charged
- 5/5 rollouts terminal and failed
- zero valid evaluator results
- zero sealed traces
- evidence ledger: five missing
- deterministic digest mismatch at producer sequence 10 for every seed

The container identity itself was correct:

- image digest
  `sha256:1bcf6309f4a10c6936fc1a563da53dc56d8f7d00560103b1bbf3d4934483a316`
- execution spec digest
  `sha256:8b9dab40c281d662f2669e169bc2cfbb51e6b3ab36930de4ab774173e6135c23`
- container ID `ctr_c694dbc5a60f45069f82d7f06edd4530`
- primary visual `vis_7023a1183afb4bc59f7e9b99c1b2766a`
- trace workstation `vis_e2760320a8c840ad994aeabc624812aa`

Local diagnostic transcript (contains operational history; do not copy
credential material):

`/Users/joshuapurtell/.synth-desktop/instances/v08/j/data/codex/homes/6c94668a-cf3a-4f02-81af-8ca195cf7214/sessions/2026/08/28/rollout-2026-08-28T10-05-19-01a048b0-75a4-7c53-b469-2c0c09570ebe.jsonl`

## Repairs already landed

The most important commits after the failed run are:

- `685a6228` — journal digest v2, stable failed-rollout identity, credential
  revoke surface, proxy cost source, terminal lifecycle and failed-evidence UI.
- `d6ed0bd4` — use verified terminal facts rather than provisional relay state.
- `53a40c44` — package/build provenance verification.
- `3fc450eb` through `df75144a` — complete journal/transcript/frame replay and
  retained-media projection.
- `b6e3e9c2` — reconcile missing OpenRouter usage receipts.
- `b44808d7` — preserve workspace policy origin through container hydration.
- `6d58dd09`, `11844d1c`, `b13619d2`, and `4ba69b7e` — authoritative
  terminal summaries, bounded rollout waits, aggregate distributions, and
  identity-safe container ensure.
- `cd7a1489`, `bbd9ae8d`, and `89a3e686` — visual sealing requires complete,
  authoritative evidence.
- `bacd9b10` through `352a3ae5` — release UI, compact side-pane layout,
  aggregate timeline, and prominent outcomes.
- `8b268e1e` — preserve producer float bits during digest verification and add
  a float golden vector.
- `8c9bb9be` — project authoritative provider receipt cost into the terminal
  visual.
- `f6247c0b` — refresh a stale terminal visual projection when reopening the
  owning optimizer run.

Containers `codex/craftax-envelope-digest-v2` contains:

- `7c11b8c` versioned journal envelope digest contract
- `bdc7540` complete terminal rollout records
- `0b3c5fe` retained OpenRouter generation identity
- `0e4c9f0` bounded provider-call exhaustion
- `22139ac` local-call-limit terminalization
- `92bb5b3` accumulated-reward float encoding coverage

Do not replace the digest fix with prompt sanitization. The original mismatch
was caused by divergent JSON encoding of non-ASCII text, first exposed by an
em dash at sequence 10; future model output can contain the same class of data.

## Current Workshop J state

Instance `j` is running. At handoff its authenticated health response reports:

- instance/app `j`, version `0.8.0`
- source and build revision `f6247c0b1cba`
- executable digest
  `sha256:be8e5a11aa967df88a72e00f52f8d87f67e4a2e68565ae73605f7dafaf2afc70`
- process ID `4175`

There is one provenance inconsistency to resolve before release evidence:
`/Users/joshuapurtell/.synth-desktop/instances/v08/j/instance.json` has the
correct top-level/provenance source revision and the health endpoint proves the
compiled binary is `f6247c0b1cba`, but `.runtime.buildRevision` still contains
`8c9bb9be1c45`. Determine whether `mark_runtime` is retaining the previous
field or another launch path is writing stale runtime metadata. The final
manifest, health response, compiled build revision, executable digest, and Git
tip must agree.

The current app is certificate signed. Future code-related Keychain access is
not authorized by default. Unless the user explicitly requests the specific
Keychain-backed signing operation, rebuild with the launcher's supported
ad-hoc mode:

```bash
SYNTH_DESKTOP_USE_DEV_SIGNER=0 \
CONTAINERS_ROOT=/Users/joshuapurtell/GitHub/containers-nanohorizon-e2e-final \
scripts/desktop-instance.sh cua-build j
```

Then launch separately with `cua-run j`. Ad-hoc rebuilding can reset macOS UI
permissions; API/health validation does not require those grants.

## Verification state

Current-tip check performed during this handoff:

```bash
cargo check \
  --manifest-path apps/synth_desktop/src-tauri/Cargo.toml \
  --features eval-driver
```

Result: passed at `f6247c0b`; the repository still emits its existing unused
code/import warnings. This is a compile check only.

Do **not** reuse the earlier full-suite totals as proof for the current tip.
Those totals predate the 32 post-acceptance commits. Before a paid rerun, run
the current full Workshop, Containers, NanoHorizon, Evals, and GameBench gates.
At minimum:

```bash
cd /Users/joshuapurtell/GitHub/workshop-v08-e2e-refactor
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib
npm test
npm run typecheck
npm run lint:css
npm run build
bash scripts/test-desktop-instance.sh

cd /Users/joshuapurtell/GitHub/containers-nanohorizon-e2e-final
python -m pytest

cd /Users/joshuapurtell/GitHub/nanohorizon-e2e-final
python -m pytest

cd /Users/joshuapurtell/GitHub/evals-craftax-live-context
python -m pytest

cd /Users/joshuapurtell/GitHub/gamebench-craftax-live-context
cargo test
python -m pytest
```

Use the repository-specific focused commands if aggregate `npm test` or
`pytest` includes documented live/Docker/provider suites. Do not make provider
calls during preflight. Record exact pass/skip/fail totals and distinguish
known pre-existing failures from new regressions.

Add focused regression coverage for `f6247c0b`: reopening a terminal inline
eval whose visual has stale/missing provider cost must republish exactly once,
show the authoritative receipt value, preserve immutable Trace V5 evidence,
and become a no-op on the second open. The current commit compiles but has no
direct test named for `refresh_terminal_visual_projection_if_stale`.

## Ordered route to completion

1. Re-read the failure handoff and RCA; keep every acceptance invariant.
2. Resolve and commit the two Evals build-context changes on an explicit branch.
3. Verify the cross-language digest-v2 and chain-fold golden vectors in both
   Workshop and Containers, including non-ASCII and floating-point payloads.
4. Add the missing focused test for terminal visual refresh on reopen.
5. Fix or explain the stale `.runtime.buildRevision`; make every provenance
   surface agree on one clean revision.
6. Run the complete current-tip non-provider suite across all five repos.
7. Update `scripts/preflight-nanohorizon-e2e.sh` to the final exact external
   refs and newly computed source-manifest digest. Run it from a clean Workshop
   tree and do not weaken its dirt/ref checks.
8. Rebuild and launch Workshop `j` from the clean final tip using only an
   already-authorized credential/signing mechanism. Never read or print `.env`
   values.
9. Obtain fresh explicit authorization before Docker or paid provider work.
   Prior approval/run history is not authorization for a new paid run.
10. Build the image from the exact clean Containers/NanoHorizon/Evals/GameBench
    closure. Record the observed immutable OCI digest and verify `/info` reports
    the same digest and producer revision.
11. Run a **fresh** evaluation with the exact five seeds and `$2.45` hard cap.
    Do not reuse or mutate `opt_eval_craftax_899e1fe95813`.
12. Audit the terminal aggregate, evidence, usage, capability state, UI, and
    restart/reopen behavior against the closure criteria below.
13. Write the final acceptance report with exact IDs, revisions, digests,
    receipts, trace IDs, totals, and any typed failures. Do not call a degraded
    or evidence-incomplete run successful.

## Closure criteria

All of these must be true in the same fresh run:

- exact seeds `780005..780009` under the pinned runtime, policy, provider, and
  model identities
- no journal digest or chain-head mismatch
- five stable rollout IDs exist before settlement/reconciliation
- five rollouts terminalize without hanging at local call exhaustion
- five sealed Trace V5 traces
- numeric evaluator reward or an honest typed evaluator failure per rollout
- calls, tokens, cost, steps, frames, achievements, and source coverage are
  retained or explicitly unsupported by contract
- provider receipt, run usage, visual usage, and terminal seal agree
- paid approval receipt and its `$2.45`/50-call envelope survive reload
- run capability is revoked while the reusable file-backed source remains
  registered
- optimizer, chat card, experiment, Failures surface, Craftax viewer, and
  trace workstation show one terminal aggregate and lifecycle
- replay contains meaningful chronological evidence rather than terminal
  placeholders
- sealed visuals cannot be produced from missing or rejected evidence
- reopening after restart preserves the authoritative cost and does not mutate
  immutable evidence
- manifest, health, compiled build revision, executable digest, Git tip,
  container digest, and producer revision all agree
- no Keychain access unless the user explicitly authorized that exact operation

## Safety and operating constraints

- No pushes unless explicitly requested.
- Never stash; preserve unrelated changes.
- No Docker rebuild/run or paid/provider call without fresh explicit approval.
- Never read or print credential values or `.env` contents.
- Prefer project-local `.env` plus Workshop's secrets proxy.
- Do not use Workshop's Keychain-backed Secrets registry or macOS Keychain for
  credentials/signing unless the user explicitly requests that specific
  operation.
- Do not bypass journal validation, evidence completeness, immutable target
  admission, or visual sealing gates to make the acceptance appear green.
