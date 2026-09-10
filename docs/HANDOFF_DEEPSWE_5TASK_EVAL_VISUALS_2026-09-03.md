# Handoff: DeepSWE five-task evaluation and visual failures

Date: 2026-09-03
Workshop branch: `codex/capture-review-pipeline`
Observed Workshop HEAD: `66de8e71b0a02e795071c4f01e087099a4203d26`
Primary run: `opt_eval_deepswe_348bdbe13925`

## Executive summary

The first fully admitted five-task DeepSWE sample reached a terminal state, but it did not measure model quality. All five tasks failed because Workshop's recipe-level policy configuration replaced the container's correct inner Codex sandbox setting with `workspace-write`. Every shell command then failed inside the already-isolated Docker task container because `bwrap` could not create another Linux user namespace.

The run also exposed two visual/telemetry defects:

1. The designated `live.harbor_eval.v1` visual remains permanently at `connecting`, even after the run is terminal and revision 14 contains a complete terminal snapshot.
2. The generic `trace.workbench.v1` visual renders the aggregate failure, but hides the actionable `bwrap` cause inside raw producer JSON. It also reports `36 billed calls` while the usage ledger has no token or dollar values.

Do not use this run's 0/5 result as a DeepSWE performance measurement.

## User-visible evidence

Primary Harbor visual, reopened after the run was terminal:

![Primary Harbor visual stuck connecting](/Users/joshuapurtell/.synth-desktop/instances/v09/codex/data/visual-review-captures/deepswe-5task-primary-stuck-2026-09-03.jpg)

Generic trace workbench for the same run:

![Trace workbench terminal overview](/Users/joshuapurtell/.synth-desktop/instances/v09/codex/data/visual-review-captures/deepswe-5task-trace-overview-2026-09-03.jpg)

## Terminal run facts

| Field | Value |
|---|---|
| Run | `opt_eval_deepswe_348bdbe13925` |
| Recipe | `eval.deepswe.annotated.v1` |
| Provider/model | OpenRouter / `openai/gpt-5.6-luna` |
| Tasks | 5 |
| Concurrency | 5 |
| Result | 0 completed, 5 failed |
| Duration | approximately 1m 21s |
| Cursor | 125 |
| Provider requests recorded | 36 |
| Prompt/completion tokens | 0 / 0 in Workshop's run record |
| Actual cost | missing; approved ceiling was $16 |
| Trace evidence | 5 sealed Trace V5 references, all partial |
| Grades/rewards | none |

The five tasks were:

- `deepswe/anko-default-function-arguments`
- `deepswe/happy-dom-abort-pending-body-reads`
- `deepswe/oxvg-structural-selector-preservation`
- `deepswe/numba-stencil-boundary-modes`
- `deepswe/helm-unified-manifest-stream`

## Confirmed execution root cause

Each `span.policy.opened` event records:

```json
{
  "harness": "codex_agentic",
  "config": "agentic_codex",
  "metadata": {
    "model": "openai/gpt-5.6-luna",
    "sandbox": "workspace-write",
    "wire_api": "responses"
  }
}
```

The task trace then repeatedly records commands failing with:

```text
bwrap: No permissions to create a new namespace, likely because the kernel
does not allow non-privileged user namespaces.
```

The DeepSWE target itself correctly declares `sandbox = "danger-full-access"`. That is appropriate here because the agent already runs inside an ephemeral, network-restricted task container. The bundled Workshop recipe omitted `sandbox`, and the generic policy defaults therefore supplied `workspace-write`, overriding the target seed.

The agent retried observation reads several times, turning one infrastructure error into 36 provider requests across five tasks before terminal failure.

### Source fix already present but uncommitted

The following edit has been made:

```toml
# apps/synth_desktop/src-tauri/recipes/annotation_eval/eval.deepswe.annotated.v1.toml
[policy]
effort = "medium"
sandbox = "danger-full-access"
max_calls = 50
```

A regression assertion was added in:

`apps/synth_desktop/src-tauri/src/optimizers/workspace_recipe.rs`

Focused verification passed:

```text
cargo test --lib five_domain_eval_recipes_opt_into_post_rollout_annotation
test result: ok. 1 passed; 0 failed
```

The running app has not been rebuilt and no second paid run was launched after this edit.

## Earlier admission/runtime failures fixed during setup

Three preceding attempts made zero provider calls and cost $0:

| Run | Failure | Resolution |
|---|---|---|
| `opt_eval_deepswe_f39abe240119` | Container retained placeholder `OPENAI_BASE_URL=http://...:1`; connection refused | Always register a fresh scoped proxy route for provider-backed workspace recipes |
| `opt_eval_deepswe_1bcb300c06a6` | Proxy allowed only `chat.completions.create`; Codex uses `responses.create` | Select proxy operations by harness; `codex_agentic` receives `responses.create` |
| `opt_eval_deepswe_f94d916520ae` | Five nested agent/verifier images missing; HTTP 424 `harbor_environment_prewarm_required` | Pull agent images, bake verifier images, refresh host-local verifier pins, rebuild platform image |

The proxy fixes and their tests are currently uncommitted in:

`apps/synth_desktop/src-tauri/src/optimizers/container_eval.rs`

Focused tests previously passed:

- `codex_agentic_workspace_eval_uses_responses_capability`
- `non_codex_workspace_eval_uses_chat_completions_capability`
- provider-backed scoped-registration coverage

## Runtime and reproducibility state

The DeepSWE runtime was last healthy at `http://127.0.0.1:8105` with:

```json
{
  "curated_tasks": 5,
  "runnable_tasks": 5,
  "unresolved_tasks": null
}
```

Container registry ID:

```text
ctr_dbbec0062713451abacdc9e80be3f7e2
```

The local verifier images were rebuilt, so their host-local digests changed. The generated pin file was refreshed in the Evals checkout:

`/Users/joshuapurtell/GitHub/evals/containers/images/harbor-deepswe/harbor_deepswe/pins.toml`

The source attestations in `/Users/joshuapurtell/GitHub/evals/workshop.containers.toml` were also refreshed. That file is currently untracked in the Evals checkout; inspect ownership and intended version control before committing it.

The Evals catalog build wrapper also has a separate bug: `synth-containers build harbor-deepswe` converts an already tagged `image_name = "evals-harbor-deepswe:local"` into invalid `evals-harbor-deepswe:local:local`. The platform image was rebuilt directly with Docker as a workaround.

## Primary visual defect

Visual ID:

```text
vis_f49a8942f1b745e4a1eedb4fc78735cb
```

Template: `live.harbor_eval.v1`
Persisted state: `failed`, revision 14

The persisted binding contains an `eval.aggregate.v1` terminal snapshot with:

- `projectionRevision: 124`
- five terminal failed trials
- five Trace V5 evidence references
- `lifecycle: terminal`

Nevertheless, reopening the visual shows:

- Status: `connecting`
- Trials: `—`
- Event replay: `0/0`
- `Waiting for trial.planned…`

Probable contract mismatch: `live.harbor_eval.v1` was designed around a connected live SSE input, while the optimizer projection persists terminal snapshot data into the binding. Reopening a historical or terminal visual does not replay the closed stream and the template does not fall back to the snapshot.

### Required behavior

On initial live open, the template may subscribe to the declared stream. On reopen or after terminal settlement, it must render from persisted canonical snapshot/evidence without requiring the producer to still be streaming.

At minimum it should display:

- terminal status and failure count;
- task names, not only seeds;
- selected task's terminal reason;
- retained trace/verifier/patch availability;
- incomplete usage as explicitly unknown, never zero;
- a link/action to open the trace workbench.

## Trace workbench defects

Trace workbench visual IDs encountered:

```text
vis_35b5728e0164450ebac49b26f558fbce
vis_8f4eed6350774c0e9778d9ec41c83029
```

What works:

- Aggregate terminal/running/queued/failed summary
- Duration and concurrency
- Five selectable rollouts
- Retained Trace V5 identity
- Search and raw producer-event access

What does not:

1. Rollouts are labelled `seed 0` through `seed 4`; the task identity and language are missing.
2. The selected policy-call projection says only `this call recorded no answer` and `trace closed before policy close`.
3. Searching for `bwrap` does not surface a structured failure card; the actionable error is buried in raw JSON.
4. There are no first-class tabs for trajectory, verifier output, model patch, or metrics.
5. No per-task step, duration, token, cost, verifier, or patch summary is visible.
6. `36 billed calls` is presented as authoritative even though cost and token reconciliation are incomplete.

## Usage/accounting defect

`secret_audit` contains 36 allowed `responses.create` entries with provider response IDs. Every entry has:

```json
{
  "calls": 1,
  "cost_complete": false,
  "cost_reconciled": false,
  "cost_usd": null,
  "input_tokens": 0,
  "output_tokens": 0
}
```

`optimizer_usage_ledger` has no rows for this run, while `optimizer_runs.usage_json` reports 36 calls and zero tokens. This must not render as a complete billed-usage statement.

Investigate whether the OpenRouter Responses compatibility path drops usage fields, whether response-ID reconciliation is missing, and whether run settlement should stay `usage incomplete` until reconciliation succeeds.

## Useful diagnostic queries

Run summary:

```sh
sqlite3 -header -column \
  /Users/joshuapurtell/.synth-desktop/instances/v09/codex/data/synth.sqlite3 \
  "select status,cursor_seq,
          json_extract(summary_json,'$.progress.completed') completed,
          json_extract(summary_json,'$.progress.failed') failed,
          json_extract(usage_json,'$.calls') calls,
          json_extract(usage_json,'$.promptTokens') prompt_tokens,
          json_extract(usage_json,'$.completionTokens') completion_tokens,
          json_extract(usage_json,'$.costUsd') cost_usd,
          finished_at
   from optimizer_runs
   where id='opt_eval_deepswe_348bdbe13925';"
```

Policy sandbox actually used:

```sh
sqlite3 -json \
  /Users/joshuapurtell/.synth-desktop/instances/v09/codex/data/synth.sqlite3 \
  "select payload_json from optimizer_events
   where optimizer_run_id='opt_eval_deepswe_348bdbe13925'
     and event_type='eval.trial.event';" \
| jq -r '.[] | .payload_json | fromjson
  | select(.delta.container_event.kind=="span.policy.opened")
  | .delta.container_event.payload.metadata.sandbox'
```

Provider audit completeness:

```sh
sqlite3 -header -column \
  /Users/joshuapurtell/.synth-desktop/instances/v09/codex/data/synth.sqlite3 \
  "select at,operation,model,decision,usage_json
   from secret_audit
   where at >= '2026-09-03T20:59:00'
     and action='provider.use'
   order by at;"
```

Visual persisted state:

```sh
sqlite3 -header -column \
  /Users/joshuapurtell/.synth-desktop/instances/v09/codex/data/synth.sqlite3 \
  "select id,template_id,status,current_revision,session_id,bindings_json
   from visuals
   where id='vis_f49a8942f1b745e4a1eedb4fc78735cb';"
```

## Recommended engineering order

1. Preserve `danger-full-access` through recipe resolution and add an admission-level assertion that the effective policy matches the selected container policy seed.
2. Add a fail-fast detector for repeated command failures before allowing dozens of provider retries. A first `bwrap`/namespace failure should terminate the rollout as infrastructure failure, not ask the model again.
3. Make `live.harbor_eval.v1` render persisted terminal snapshots on reopen and test closed-stream restoration.
4. Project command/tool failures into a structured task failure summary in `trace.workbench.v1`.
5. Reconcile OpenRouter Responses usage and distinguish `unknown/incomplete` from zero.
6. Fix the catalog image-name normalization that produces `:local:local`.
7. Only then run another five-task paid sample.

## Acceptance criteria

### Execution

- The effective `span.policy.opened.metadata.sandbox` is `danger-full-access` for all five DeepSWE tasks.
- A trivial `pwd` and repository read succeeds inside every nested task container.
- Five tasks may run concurrently without an admission, proxy, prewarm, or namespace error.
- Infrastructure failures terminate promptly and do not consume repeated model calls.

### Visuals

- The primary visual restores correctly after app restart or explicit close/reopen.
- It never shows `connecting` for a terminal run.
- Each rollout is labelled with task name and language.
- The dominant terminal error is visible without opening raw JSON.
- Task detail exposes trajectory, verifier output, model patch, metrics, annotations, and evidence completeness.
- Unknown tokens/cost render as `unavailable` or `reconciliation pending`, not `0`.

### Comparison quality

The result should approach the information architecture of the official DeepSWE trial viewer: compact task metrics plus separate Trajectory, Verifier output, Model patch, and Metrics views. Reference:

<https://deepswe.datacurve.ai/data/v1/trials/gql-incremental-graphql-delivery__2tZkavD>

### Verification

- Unit tests cover effective sandbox configuration and proxy operation selection.
- A fixture test restores a terminal Harbor visual from persisted snapshot data with no live stream.
- A fixture trace containing a `bwrap` command failure produces an actionable failure summary.
- A usage fixture with response IDs but no reconciled tokens/cost renders incomplete usage truthfully.
- A bounded one-task smoke run succeeds before authorizing another five-task sample.

## Safety and credentials

No macOS Keychain access was used. Provider access used Workshop's one-time, run-scoped OpenRouter proxy capability; the task containers never received the plaintext credential.

Do not delete or rebuild persistent evaluation containers casually. The DeepSWE agent/verifier images are large and host-local verifier digests must remain aligned with `pins.toml`.
