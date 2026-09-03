# `start_workflow` hangs for every provider-backed recipe

**Date:** 2026-09-03
**Instance:** `qa-9df00bc0` (`v0.9.5`, source `7e13a7b127d2`)
**Impact:** GEPA on every container; HealthBench and RuneBench entirely.

## Symptom

`POST /v1/optimizers/workflows/start` never returns for a recipe whose
`provider` is anything other than `none`. No run is created, no error is
raised, and the caller waits until it gives up. Container-local recipes start
in under a second.

Observed identically on three separate recipes:

| Recipe | Provider | Result |
|---|---|---|
| `eval.banking77.annotated.v1` | `none` | starts in <1s, completes |
| `eval.craftax.gold.annotated.v1` | `none` | starts in <1s, completes |
| `eval.healthbench.annotated.v1` | `openrouter` | hangs |
| `gepa.banking77.qa.v1` | `openrouter` | hangs |
| `gepa.banking77.local.v1` | `local-laguna` | hangs |

The local-provider case is the important one: it removes credit, network, and
external latency from the repro. The hang is not the provider being slow or
unpaid.

## Cause

`authorize_optimizer_recipe_start` (`src/lib.rs`, the `PaidCompute` block)
awaits a host approval:

```rust
let paid_approval_id = codex
    .approvals
    .authorize_host(app, session_id, paid)
    .await
```

followed by one `CredentialAccess` approval per entry in `credential_names`.

A recipe with `provider = "none"` produces an empty `credential_names` and
never enters that path, which is exactly the split in the table above.

The await is for a decision. A session created through the eval driver has no
running agent turn, so nothing is listening to answer it, and the await never
resolves. It is not a deadlock or a slow call — it is a question asked into an
empty room.

`/v1/preflight` reports `approvalPolicy: never` and
`paidCompute.requiresBoundedCap: true`, and auto-approval demonstrably works
for other kinds: installing the optimizer plugin returned
`approvalReceiptId: approval-auto-eb6288a0d5a1438e968bae1dde8eeb2f` with no
human present. So the machinery exists; it is not reaching `PaidCompute` and
`CredentialAccess` for a driver-created session.

## Reproduce

`gepa.banking77.local.v1` is committed for this purpose. It targets
`poolside/Laguna-XS-2.1-NVFP4-mlx`, the model Workshop already serves locally,
so the repro needs no credential and no network:

```bash
# a session with no agent turn, purely for workspace context
curl -sX POST "$URL/v1/sessions" -H "authorization: Bearer $TOKEN" \
  -H "x-synth-eval-driver: synth.eval-driver.v1" \
  -H 'content-type: application/json' -d '{"sessionId":"repro"}'

# hangs; no run appears in GET /v1/optimizers/runs
POST /v1/optimizers/workflows/start
  {"recipeId":"gepa.banking77.local.v1","containerId":"<banking77>","sessionRef":"repro"}
```

Swap the recipe for `eval.banking77.annotated.v1` and it completes, which
isolates the provider branch as the only difference.

## What a fix has to preserve

The approval is not ceremonial: it is the bound on paid compute, and
`requiresBoundedCap` is what stops an unattended run from spending without a
ceiling. The fix is to make the unattended profile answer the question — the
way it already answers plugin lifecycle — not to remove it. A driver session
that cannot be asked should still be refused when the cap is unbounded; it
simply must be refused *quickly and in writing* rather than by never replying.

An approval route already exists at
`POST /v1/sessions/{id}/approvals/{callId}`, so granting out of band is
plausible if pending approvals can be enumerated; there is currently no route
that lists them.

## Note on ownership

At the time of writing, `lib.rs`, `credential_broker.rs`,
`secrets/capability.rs`, `session/paid_compute_budget.rs`, and
`optimizers/service.rs` were all dirty in the working tree under another
agent's in-flight change. This defect was diagnosed but deliberately not
patched, because the fix belongs in files being rewritten and would have been
aimed at a moving target.
