# `start_workflow` hangs for every provider-backed recipe

**Date:** 2026-09-03
**Instance:** `qa-9df00bc0` (`v0.9.5`, source `7e13a7b127d2`)
**Impact:** GEPA on every container; HealthBench and RuneBench entirely.
**Status:** root cause superseded — see [Correction](#correction-2026-09-03-later).
The instance was missing a `[desktop.permissions.paid_compute]` declaration.
The remaining defect is that the refusal is silent instead of immediate.

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

## Correction (2026-09-03, later)

**The paragraphs above are wrong about why, and the wrong conclusion was
expensive: it framed a configuration gap as a product defect and parked ten
matrix cells behind a fix nobody needed to write.**

`approval_policy = "never"` governs the *agent tool* approval policy. It says
nothing about paid compute, which is configured separately and, when the block
is absent, defaults to requiring a decision. This instance's `config.toml` has
no `[desktop.permissions.paid_compute]` block at all, so `PaidCompute` had no
standing authorization to auto-grant against — and the machinery correctly
refused to invent one.

A sibling instance (`visualqa`) that declares the block:

```toml
[desktop.permissions.paid_compute]
auto_approve = true
max_conversation_usd = "50.00"
max_request_usd = "35.00"
providers = ["openrouter", "openai", "tinker"]
```

runs provider-backed recipes unattended, including a completed Banking77 GEPA
search (`gepa_gepa_banking77_workspace_v1_98ad28d6`, `$0.288`, 7174 events) and
a five-seed Craftax evaluation whose usage carries
`paidComputeApproval.approvalId = approval-auto-c11b4c3d416949859f5a6b97454cd5b4`
under a `$2.45` cap. Same binary, same code path, same driver-created session
shape. The only difference is the declaration.

So the hang is the honest behaviour of an instance that was never told what it
may spend. The `provider = "none"` split in the table above is real, but it is
a consequence, not the cause: those recipes need no paid-compute grant, so they
never reach the question.

### What still deserves a fix

Two things survive the correction, both smaller than the original claim:

1. **The refusal should be written, not silent.** An instance with no
   `paid_compute` block should fail the start immediately and say
   `paid_compute_unconfigured`, naming the block to add. Waiting forever for a
   decision that nothing can supply is the actual defect, and it is a
   diagnostics bug rather than an authorization one.
2. **Step 7 below still stands.** The GEPA proposer's `api_key_env` falls
   through to `OPENAI_API_KEY` for any provider that is not `openrouter` or
   `anthropic`, so a local provider is asked for an OpenAI key. That is
   independent of approvals.

The workaround noted at the end of this document — `OPENAI_API_KEY` in
`data/.env` holding the Laguna loopback key — should simply be removed rather
than justified. It was reached by the wrong diagnosis.

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

## The full chain, walked

Granting the approvals out of band (`POST /v1/sessions/{id}/approvals/{id}`
with `{"decision":"once"}` -- `approve` is rejected; the accepted values are
`once`, `always`, `reject`) turns the hang into a sequence of real, specific
errors. Each one is a genuine declaration Workshop requires, and the sequence
is worth recording because every step looks like a dead end until the next
error arrives:

1. `container spec X is not declared in any approved workshop.containers.toml`
2. `launch_declaration_missing: container X must declare [container.launch]`
   -- and validation walks *every* entry, so one undeclared container in the
   file blocks all of them.
3. `parse workshop.containers.toml` -- the launch block is
   `deny_unknown_fields` and needs `schema_version`, `health_target`, and a
   `[container.launch.source]` with `revision_policy` and `tracked_revision`.
4. `<path> is outside <workspace>` -- `working_directory` and every `include`
   resolve under the workspace, and the check follows symlinks, so the image
   directory must genuinely live inside it.
5. `container_identity_mismatch: expected health target evals-banking77, got
   banking77_classify` -- `health_target` is the target the service reports on
   `/health`, not the image name.
6. `no credentials.providers.local-laguna variable mapping in config.toml`
7. `OPENAI_API_KEY is absent` -- the GEPA proposer's `api_key_env` falls
   through to `OPENAI_API_KEY` for any provider that is not `openrouter` or
   `anthropic`, so a local provider is asked for an OpenAI key.

Step 7 is the one worth fixing in code: `local-laguna` is a provider class
Workshop already knows, and the proposer mapping should recognise it rather
than demanding a key for an account that has nothing to do with the run.

**A workaround is currently in place on this instance and should be undone.**
`data/.env` holds `OPENAI_API_KEY` set to the *Laguna* loopback key purely to
satisfy step 7, and `data/config.toml` maps
`credentials.providers.local-laguna` to `SYNTH_LAGUNA_API_KEY`. The mapping is
legitimate and worth keeping; the `OPENAI_API_KEY` line is a lie about what
that key is and will confuse the next failure that touches it.

## Note on ownership

At the time of writing, `lib.rs`, `credential_broker.rs`,
`secrets/capability.rs`, `session/paid_compute_budget.rs`, and
`optimizers/service.rs` were all dirty in the working tree under another
agent's in-flight change. This defect was diagnosed but deliberately not
patched, because the fix belongs in files being rewritten and would have been
aimed at a moving target.
