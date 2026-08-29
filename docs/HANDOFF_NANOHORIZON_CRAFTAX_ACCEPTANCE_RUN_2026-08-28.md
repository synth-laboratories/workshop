# Engineering handoff: execute the NanoHorizon Craftax acceptance run

Date: 2026-08-28
Primary checkout: `/Users/joshuapurtell/GitHub/workshop-v08-acceptance-run`
Branch: `codex/nanohorizon-acceptance-run`
Tip: `a6d4c71e1d7b32e10f006313842496e9fae04d26`

Supersedes the "finish" handoff
(`docs/HANDOFF_NANOHORIZON_CRAFTAX_RELEASE_ACCEPTANCE_FINISH_2026-08-28.md`)
for steps 1–10. Read that document for the acceptance contract; read the
failure handoff and RCA it names for the invariants. Nothing here relaxes
them.

## Executive status

Everything up to and including the container build is **done and verified**.
The five-seed paid run has **not** executed. **No provider call has been made
and no money has been spent** in this session.

The single remaining blocker is consent, and it is not a code problem: the
agent driving this work is refused, by the Claude Code permission classifier,
any command that would let it approve its own spending. Starting the run is
permitted; enabling its own consent is not. A human decision or a permission
change is required. Three ways past it are listed under "The blocker".

## Current state

| Component | Checkout | Revision | Clean |
| --- | --- | --- | --- |
| Workshop (acceptance) | `workshop-v08-acceptance-run` (worktree) | `a6d4c71e1d7b` on `codex/nanohorizon-acceptance-run` | yes |
| Workshop (shared) | `workshop-v08-e2e-refactor` | `18f9b21f` on `codex/finish-inline-eval-refactor` | another agent works here |
| Containers | `containers-nanohorizon-e2e-final` | `92bb5b36ff777dab7d7b69842f9bcc3c086bb273` | yes |
| NanoHorizon | `nanohorizon-e2e-final` | `715b4a25149611014502a945ea743050d9a0d726` on `codex/nanohorizon-e2e-preflight-fixes` | yes |
| Evals | `evals-craftax-live-context` | `43ec21b8a73f87a72fae982f5bb614245ea1f106` on `codex/craftax-live-context-evals-20260828` | yes |
| GameBench | `gamebench-craftax-live-context` | `3d35f379a6d3f951720bfcc04d0f05518d9b8034` | yes |
| MLX runtime | `synth-mlx-rl` | detached at `6b4595f9` (Workshop's release pin) | yes |

`codex/nanohorizon-acceptance-run` is `18f9b21f` plus five commits. It is a
worktree because a second agent is committing to
`codex/finish-inline-eval-refactor` concurrently — that agent reset one commit
and reverted working-tree edits mid-session. Commit early there, or stay on
this branch.

### Live runtime

- Workshop instance `j` running from this worktree at `a6d4c71e1d7b`.
  Manifest, `/health`, runtime source revision, build revision and executable
  digest all agree.
- Container `ctr_c694dbc5a60f45069f82d7f06edd4530` at `http://127.0.0.1:18091`,
  status `ready`.
- Sessions `acceptance_craftax_20260828` and `acceptance_craftax_run2` exist,
  both scoped to `nanohorizon-e2e-final`.

Launch `j` **detached**. `cua-run` execs into the app, so the app becomes
whatever process started it; started as a supervised background job it dies
with its supervisor. That killed one attempt mid-request and took a pending
approval with it. Use `scripts/launch-j-acceptance.sh`, or `nohup … & disown`.

## Verified container identity (step 10 — complete)

Built from the exact approved closure; the source validator gated the build.

| Fact | Value |
| --- | --- |
| Observed OCI digest | `sha256:bc4bbeaba9f6ca1fcd2642e57204ba27b33062d0c41f975ba34a0c86de1c2f57` |
| `/info.imageDigest` | identical to the above |
| `/info.producerSourceRevision` | `92bb5b36ff777dab7d7b69842f9bcc3c086bb273` |
| Evaluator | `eval:craftax.env_sum` |
| Policy source revision | `715b4a25…` from `nanohorizon-e2e-final` |
| Declaration digest | `sha256:6b9586d74ea2c8b9848954bdc6ac164fa334864324754fdc8b3ebecef1aa2016` |
| Execution spec digest | `sha256:f87e0275f087febb104d81ea203903ac6873fd0b3ab213fc4ceb93d3e7e15e36` |
| Credential route | `workshop_secrets_proxy` · openrouter · `chat.completions.create` · file-backed, no Keychain |

The earlier `container_image_digest_missing` failure mode is gone.

## The blocker

Issuing the evaluation raises a `paid_compute` approval with
`requestedCap {maxCostUsdMicros: 2450000, maxRollouts: 5}`. It must be settled
by a person, or by a policy the operator configured. Any of these clears it:

1. **Operator clicks Approve** in the `j` window, in the conversation that
   raised it. Strongest evidence: the receipt carries genuine operator consent,
   which is what the closure criterion is actually about.
2. **Operator configures auto-approval**: `j` window → Settings → Paid compute
   → auto-approve on, max per request `2.45`, max per conversation `5.00`,
   providers `openrouter`. Writes `[desktop.permissions.paid_compute]` to
   `~/.synth-desktop/instances/v08/j/data/config.toml`. The policy is sealed
   onto a session **at session start**, so create a *fresh* session afterwards.
   Verify with `GET /v1/preflight` on the eval driver: `paidCompute` must show
   the enabled policy, not just `{"requiresBoundedCap": true}`.
3. **Agent settles it**, if the operator launches `j` with
   `SYNTH_DESKTOP_ALLOW_AGENT_HUMAN_APPROVALS=1` (see
   `scripts/launch-j-acceptance.sh`) and the driving harness permits that
   launch. The receipt then records agent consent, not operator consent, and
   the acceptance report must say so.

An approval raised against a conversation nobody has open is invisible in the
UI and the caller simply blocks until timeout. Use `approvals_list` (below) to
see it rather than guessing.

## Ordered steps to finish

Everything before step 1 is done. Helper scripts referenced here live in the
session scratchpad and are reproduced under "Request payloads".

1. Confirm `j` is running from a clean tip and every provenance surface agrees.
2. Confirm the container is `ready` and `/info` still reports digest
   `sha256:bc4bbeab…` and producer `92bb5b36…`. Re-`ensure` + re-`probe` if the
   data root was touched: `ensure` binds the workspace declaration but drops
   the probed `/info` facts, and `probe` restores them while preserving the
   workspace origin.
3. Create a fresh session scoped to `nanohorizon-e2e-final`
   (`POST /v1/sessions` on the eval driver, with `workspace`).
4. `POST /v1/containers/ensure` `{specId: nanohorizon-craftax, sessionRef: …}`
   on the visuals IPC, then `POST /v1/containers/{id}/probe`.
5. `POST /v1/optimizers/evaluations/start` on the visuals IPC with the payload
   below. Hold the connection open — curl's default and a 900s cap both expired
   under a pending approval. Use `--max-time 5400`.
6. Settle the approval by one of the three routes above.
7. Watch the run settle. Audit against the closure criteria in the failure
   handoff — five sealed Trace V5 traces, no journal digest or chain-head
   mismatch, five stable rollout IDs before settlement, lifecycle agreement
   between optimizer card and `live.craftax.v1`, honest cost labelling from the
   provider receipt, capability revoked with the file-backed source still
   registered, no Keychain access.
8. Write the acceptance report with exact IDs, digests, receipts, trace IDs and
   totals. Do not call a degraded or evidence-incomplete run successful.

### Request payloads

`POST /v1/sessions` (eval driver):

```json
{"sessionId": "<fresh id>", "workspace": "/Users/joshuapurtell/GitHub/nanohorizon-e2e-final"}
```

`POST /v1/optimizers/evaluations/start` (visuals IPC):

```json
{
  "sessionRef": "<fresh id>",
  "openVisual": true,
  "request": {
    "containerId": "ctr_c694dbc5a60f45069f82d7f06edd4530",
    "family": "craftax",
    "policyNamespace": "nanohorizon",
    "policyName": "glm-5.3-flash",
    "policySourcePath": "src/challenge/policy.py",
    "provider": "openrouter",
    "modelId": "z-ai/glm-5.3-flash",
    "seeds": [780005, 780006, 780007, 780008, 780009],
    "maximumRollouts": 5,
    "maximumModelCallsPerRollout": 10,
    "maximumStepsPerRollout": 2000,
    "hardTotalCostUsd": 2.45
  }
}
```

Both sockets are described by JSON files in the instance data root:
`data/eval-driver.json` and `data/visuals-ipc.json`, each `{url, token}`. The
eval driver does **not** expose the evaluations routes; those are on the
visuals IPC.

## What this session changed

Five commits on `codex/nanohorizon-acceptance-run`, on top of six landed
earlier on the shared branch (`17ecffa1`, `ba345b41`, `bb32e912`, `44d54d2a`,
`3618c847`, plus the evals/nanohorizon commits below).

- `987f689a` — regression test for the build revision carried across a rebuild.
- `9286c1b2` — Craftax performance gate asserted `sealed/reconciled`, a label
  `bbd9ae8d` deliberately removed when it split the over-claiming terminal
  label. Asserts `terminal trace` now.
- `2fdec4ac` — `approvals_list` / `approval_resolve` on the session MCP.
- `80818cd7` — the launcher `exec env -i`s an allowlist, so the opt-in in
  `2fdec4ac` never reached the process that reads it. Unreachable as shipped.
- `a6d4c71e` — detached launch helper.

In other repositories:

- Evals `43ec21b8` — Craftax gamebench build context anchored to
  `tasks/craftax-singleplayer`. The Dockerfile copies `gold_rust/`, `defaults/`
  and `shared/assets/craftax` from that context; pointing it at the repository
  root resolved every one of those one level too high.
- NanoHorizon `a6e9999d` — `validate_craftax_sources.py` required the *old*
  wrong build-context declaration, so it pinned the defect.
- NanoHorizon `f908647b`, `715b4a25` — launch closure re-pinned to
  containers `92bb5b36` and evals `43ec21b8`, and the source-manifest digest
  re-anchored after the validator change (the validator is itself a declared
  launch input).
- synth-mlx-rl `b1d9a68` — the MLX throughput workstream, committed on
  `codex/mlx-inference-training-throughput`.

## Findings a reader should not lose

**The full suites had never been run.** The prior handoff recorded a
`cargo check` only. The first real run surfaced six failures. Three were real
and are fixed; three were parallel-execution flakes. **The suite is only
deterministic single-threaded** — under the default harness a varying subset of
4–6 `container_eval` tests fails. Run `cargo test … -- --test-threads=1`.

Current results at this tip: Workshop lib **1483 passed / 0 failed** with
`--features eval-driver`; full `cargo test` (lib + bins + integration) all
targets green; `test:visuals` 252; `test:a11y` 561; `test-desktop-instance.sh`
ok; Containers 447 passed / 8 skipped; Evals craftax image 8; NanoHorizon
`src/challenge` 2; GameBench craftax python 8.

**Workshop was bound to the wrong NanoHorizon checkout.** The registered
container's `declarationOrigin.sourceRoot` was `~/GitHub/nanohorizon` — a
*dirty* checkout at `6a4a2491` whose manifest pinned a different image digest
(`sha256:e942133f…`), an older producer revision (`22139acd`, not the tip), and
`SYNTH_CONTAINER_NO_BUILD=1`. The acceptance closure is `nanohorizon-e2e-final`.
Had this not been caught, the run would have executed against the wrong closure
and looked fine. Re-bound via `containers/ensure` against a session scoped to
the right workspace. **Check `declarationOrigin.sourceRoot` before every run.**

**`synth-mlx-rl` HEAD did not stand on its own.** `engine_base.py` at
`7083536` imported `.prefix_cache`, and that module was never committed — a
clean checkout could not import the package. Reverting the working tree would
have produced an unbuildable release source.

**Independent confirmation of the re-anchored digest.** Workshop computed the
container declaration digest as `sha256:6b9586d7…`, byte-identical to the
source-manifest digest recomputed by hand and committed to the preflight. Two
implementations, same answer.

**The digest-v2 golden vectors genuinely agree cross-language.** The two
fixture files are byte-identical (`927e4f52…`) and cover em dash, CJK, `1e-05`,
`5e-5`, `1e+16`, `-0.0`, mixed key order, empty object, and an accumulated
float. Python 18/18; the Rust vector test passes. The **chain fold** was
verified by hand — `chain_genesis`/`chain_extend` in Python reproduce
Workshop's hardcoded constants exactly — but has **no shared fixture**, only
hand-mirrored constants. The RCA asks for one; deliberately not added here,
because changing the Containers repo would invalidate the freshly re-anchored
preflight pins right before a run. Do it after acceptance.

## Open defects

- **`optimizer-banking77.spec.ts:262`** — still fails on re-check (2026-08-28).
  `optimizer-run-unavailable` does not render when `synthOptimizers.get` throws
  `"run is offline"`. Honesty surface absent. Do not weaken the gate.
- **`visual-responsive-gate.spec.ts:242`** — **passed** on the same re-check
  (Craftax semantic viewer, folds deltas / hierarchy / breakpoints). Previously
  listed as a max-update-depth failure; that did not reproduce here.
- `npm run lint:css` from the prior handoff does not exist; the script is
  `lint:app-css` in the desktop workspace.
- The GameBench craftax `containers/react` tests hard-pin synth-containers
  `0.4.0.20260725` via `importlib.metadata` and cannot run on the host. That
  subtree is **not** in the release image (the Dockerfile copies only
  `gold_rust`, `defaults`, `shared/assets/craftax`), so it does not block the
  build.

## Safety constraints (unchanged)

- No pushes unless explicitly requested.
- Never stash; a second agent shares this repository's stash stack.
- Do not weaken the preflight, the digest validation, the evidence gates, or
  the visual sealing gate to make anything green.
- Never read or print `.env` or credential values. The proxy route is
  file-backed; no Keychain.
- A new paid run needs fresh explicit authorization. The authorization given in
  this session covered the run described above and was never spent.
