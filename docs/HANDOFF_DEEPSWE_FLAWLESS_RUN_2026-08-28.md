# DeepSWE flawless-run blockers: what is now implemented

Branch: `codex/openrouter-scope-runtime-fix`
Worktree: `/Users/joshuapurtell/GitHub/workshop-v08-release-k-scope-build`
Harness: `/Users/joshuapurtell/GitHub/evals/temp/deepswe-harbor-codex`

This closes the seven blockers from the 2026-08-28 engineering handoff. Every
change below is host- or harness-side; none of it starts paid compute. The run
itself still needs an operator at the approval sheet, and that is the only
remaining step.

## 1. The approval gate is no longer bound to the agent's turn

The sheet used to live and die with the turn that raised it: `turn/completed`
swept every pending approval, so the operator got the seconds between the model
finishing its sentence and the turn closing. That is the 15–20 seconds.

- Workshop-owned sheets that require a human (paid compute, credential access,
  container lifecycle) now own their own clock — `limits::HOST_APPROVAL_LIFETIME`,
  900 seconds, with a contractual floor of 60 asserted in `limits.rs`.
- A turn boundary no longer closes a live sheet. Every other sweep still does:
  an interrupt, a dead provider process, and a restart all mean nothing can
  receive the decision. `sweep_is_forced` owns that distinction.
- `authorize_host` waits on the sheet's own clock and, when it elapses, raises
  the typed `approval_expired` failure — with the digest, the window, and the
  control identifiers — instead of hanging until an unrelated sweep notices.
- That typed failure now survives the loopback hop. `AppError` carries the
  `StructuredFailure` it was classified from, and the evaluation-start route
  re-raises it rather than rebuilding the error from `to_string()`. `K` reads
  `code: "approval_expired"`, not a sentence.
- Stable control identifiers: `approval-control-paid_compute-approve` /
  `-reject` / `approval-dialog-paid_compute`, derived from the kind alone, so
  they are identical on every render and every restart. They are stamped as DOM
  ids and published on the `approval.requested` payload.
- The sheet is queryable and authoritative: `GET /v1/approvals/pending`
  (read-only, agent-visible, MCP `optimizer_manage { operation: "approval_state" }`)
  and the `approvals_pending` command. Both report the digest, the controls,
  and the seconds remaining. Chat prose is never the authority.
- Idempotent approve-by-digest: the `approvals_approve_digest` command settles
  the open sheet bound to exactly that specification digest. A repeat for a
  digest that already settled reports the standing outcome (`alreadySettled`)
  rather than granting twice. It is human-only — deliberately *not* on the
  agent's MCP surface.
- The modal renders a live countdown and shows source revision, loaded runtime,
  and reachable-call arithmetic.

## 2. Call ceiling and lifetime are now consistent by construction

`1 + floor(5400 / 370)` is fifteen; the sheet said eighty. Both halves are
fixed:

- Pacing is provider-driven, not a constant guessed from one transcript.
  `CREDENTIAL_UPSTREAM_MIN_INTERVAL` is a **floor** (6s) that each capability
  starts at. A 429 raises that capability's interval to what OpenRouter
  actually reports — `Retry-After`, or `X-RateLimit-Reset` in epoch
  milliseconds, epoch seconds, or as a plain delta — bounded by
  `CREDENTIAL_UPSTREAM_MAX_INTERVAL` (600s). Each admitted request decays it
  back toward the floor by three quarters.
- Admission refuses a call ceiling the approved lifetime cannot reach, naming
  the declared count, the realizable count, the pacing floor, and the lifetime
  the declared count would need.
- The approval disclosure states the same arithmetic the pacer enforces, under
  `pacing`.
- The capability now gets the lifetime that was approved. `container_proxy_policy`
  used to overwrite the approved scope with a task-specific constant, so the
  disclosure and the issued capability disagreed about the same number.

At the floor, 80 calls inside 5400 seconds is reachable; if the route pushes
back, the pacer follows the route rather than a guess.

## 3. Harbor and Workshop agree about cancellation

- `RunProgress` records `cancel_requested` (migration 50), written in the same
  transaction as the run-mirror status, because the evidence worker settles
  from the progress record and could not otherwise tell a deliberate stop from
  an evaluator that produced nothing.
- `settle()` maps a requested stop — or producer-reported `cancelled` rollouts
  with no genuine failure — to `RunState::Cancelled`. A real failure alongside
  a cancellation is still a failure; cancellation launders nothing.
- The dispatch loop raises a typed `EvaluationCancelled` instead of a prose
  bail, and `settle_worker_failure` settles it as `cancelled` with
  `cancel_pre_dispatch`, never through the compute-failure path.
- A rollout Harbor reports as `cancelled` keeps that word instead of being
  folded into `failed`.
- A cancellation's reason is filed as `optimizer.run.warning` at `info`, not as
  `optimizer.run.error`.

## 4. Live telemetry reads the trusted capability ledger

- `CapabilityLedger` sums one run's capabilities — calls, input/output tokens,
  billed cost, worst status — including revoked and exhausted rows, which is
  exactly the state a run is in at terminal.
- Progress projections prefer the evaluator's per-rollout numbers, fall back to
  the ledger, and **name the source**: `usageSource` / `costSource` are
  `evaluator`, `capability_ledger`, or `awaiting_evaluator_telemetry`. The last
  is a different statement from "unavailable".
- The run summary carries `capabilityLedger`, and the terminal manifest carries
  `usage.providerLedger` plus `usage.costSource`, so Trace V5 reconciles the
  two accountings instead of one silently standing in for the other.
- The "cost is unavailable, not zero" caveat is dropped when the ledger holds a
  billed figure.

## 5. The terminal success path is proven, not assumed

`tests/test_container.py::test_terminal_success_path_completes_with_checkable_evidence`
drives one rollout from prepare, through a bridged capability and a Harbor job,
to a sealed bundle, and asserts each thing the operator is asked to believe:
non-empty committed patch, reward 1 resting on stated F2P (104/104) and P2P
(250/250) splits, sealed Trace V5 with a content digest, usage the host can
reconcile, revocation recorded, no evidence gap, and no rollout worker left
alive.

Supporting changes: `verifier_test_splits` projects the splits a reward rests
on (absent splits report as `None`, never zero) and they now appear on the
rollout evidence, the `verifier.completed` event, and `reward.get`. A
`delegated` revocation — the declared outcome for a Workshop-bridged route — is
no longer recorded as an evidence gap.

## 6. Pause means cancel, and says so

An inline evaluation has no checkpoint contract: its work lives in an ephemeral
container workspace bound to one approved digest, and held dispatch would leave
a paid capability alive with nothing running and no way to prove on resume that
the continued work is the approved work. `pause` on such a run is refused with
`pause_without_checkpoint_contract`, which states that cancelling revokes the
capability and discards the ephemeral work. The `cancel_run` and `pause_run`
tool descriptions say the same thing.

## 7. Container freshness is three facts, not one

- Workshop stamps the declared revision into the launched container's
  environment (`SYNTH_CONTAINER_SOURCE_REVISION`, `limits::CONTAINER_SOURCE_REVISION_ENV`),
  and the harness echoes it on `/health` and `/info` as `source_revision` /
  `runtime_revision`. A container left running from an earlier launch still
  carries that earlier declaration's revision, which is exactly what the
  declaration digest could never reveal.
- An unstamped process reports `None`, not a locally-invented identifier. A
  self-generated content digest can never equal a declaration's revision, so
  reporting one would refuse every honest container; it is published
  separately as `source_content_digest`, as evidence rather than identity.
- `ContainerPin` gains `runtime_revision`, read only from `/info/...`. Reading
  `metadata.gitRevision` or `metadata.capabilities.revision` would compare the
  declaration to itself and report every container fresh.
- Admission refuses a stale runtime with `container_runtime_stale`, mapped to
  the existing `ContainerFailure::SourceRevisionMismatch` so it shares an
  identity with probe-time health and reaches restart remediation.
- Because the runtime revision is inside the pinned specification, a runtime
  that changes between admission and dispatch is caught by the existing drift
  check. A runtime that reports nothing is `runtimeFresh: null` — an
  unanswered question, not a claim of freshness.
- The sheet shows source revision, loaded runtime, and freshness beside the
  declaration digest.

## Test status

Workshop `cargo test -p synth-desktop --lib`: 1367 pass, 4 fail. All four fail
identically at `a0e6b761` (verified in a detached worktree) and are unrelated to
this work:

- `optimizers::eval_recipes::…::a_mutable_tag_is_refused_before_the_run_is_created`
- `optimizers::manager::tests::installed_service_has_offline_runtime`
- `optimizers::service::tests::absent_capabilities_refuse_paid_start_instead_of_skipping_the_pin`
- `plugins::policy::tests::never_auto_authorizes_risky_actions`

This branch also **fixes** one that failed at `a0e6b761`:
`optimizers::container_eval::tests::container_proxy_policy_uses_the_approved_credential_scope`
(the approved-lifetime overwrite in §2).

Desktop node tests: 517 pass, 8 fail — the same eight fail at `a0e6b761`.
Harness `pytest`: 27 pass.
`scripts/check-failure-runtime.sh` and `scripts/conform-desktop.sh` pass.

Known pre-existing red gate, untouched here: `npm run lint:app-css` reports
`bare font-size debt increased: 520 -> 521` against an unmodified `app.css`.

## What is left

The run. Everything it needs is in place; nothing here can start it, because
the paid-compute sheet requires a human. The bounds from the handoff are
unchanged — seed 780019, one rollout, one step, 80 calls, 400,000 input tokens,
5,400 seconds, $0.60, `openai/gpt-5.6-luna` at High through the project-local
configured-env secret and Workshop's ephemeral proxy, policy configuration
`{"operation": "responses.create", "context_token_budget": 5000}`, and no reuse
of the cancelled run's solution artifacts.

Two operational notes for that run:

- The sheet now stays open for 15 minutes and publishes stable control ids, so
  approval no longer has to race an accessibility refresh. If it does lapse,
  `evaluation_start` returns `approval_expired` and the same call raises a
  fresh sheet.
- The container must be replaced before the run so its loaded runtime revision
  matches the declaration; admission will now refuse a stale one rather than
  running under it.
