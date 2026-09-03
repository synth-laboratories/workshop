# Optimizer Visuals v0.6

Status: proposed redesign plan  
Scope: GEPA first; shared primitives must support GELO, SFT, and eval runs  
Reference run: `banking77_gepa_luna_med_59687ffa`

The broader interactive optimizer workbench is deferred to v0.7 and specified in
[`optimizer_ux_v07.md`](optimizer_ux_v07.md). v0.6 is limited to trustworthy
inspection, durable evidence, and prerequisite contracts; pause/checkpoint/restart,
agent interventions, branching, and DAG composition are not v0.6 deliverables.

## Screenshot references

All paths are relative to this plan.

1. [`optimizer_visuals_v06_references/01-run-header-contract-and-budgets.png`](optimizer_visuals_v06_references/01-run-header-contract-and-budgets.png) — clipped header, duplicated chrome, contract hierarchy, raw budget presentation.
2. [`optimizer_visuals_v06_references/02-budget-bars-and-incumbent-chart.png`](optimizer_visuals_v06_references/02-budget-bars-and-incumbent-chart.png) — misleading terminal ETAs/raw cost and train-only incumbent chart.
3. [`optimizer_visuals_v06_references/03-frontier-and-candidate-inspector-overlap.png`](optimizer_visuals_v06_references/03-frontier-and-candidate-inspector-overlap.png) — cramped split pane, overlapping candidate decision and controls.
4. [`optimizer_visuals_v06_references/04-pareto-list-and-prompt-diff.png`](optimizer_visuals_v06_references/04-pareto-list-and-prompt-diff.png) — dense frontier matrix, candidate inspector, unreadable inline prompt diff.
5. [`optimizer_visuals_v06_references/05-rejected-candidate-card-list.png`](optimizer_visuals_v06_references/05-rejected-candidate-card-list.png) — repetitive rejected-candidate cards and excessive vertical length.
6. [`optimizer_visuals_v06_references/06-candidate-summary-and-evaluations.png`](optimizer_visuals_v06_references/06-candidate-summary-and-evaluations.png) — ambiguous accepted state and unscannable evaluation rows.
7. [`optimizer_visuals_v06_references/07-proposer-trace.png`](optimizer_visuals_v06_references/07-proposer-trace.png) — oversized proposer trace and raw-detail-first presentation.
8. [`optimizer_visuals_v06_references/08-banking77-eval-missing-per-seed-results.png`](optimizer_visuals_v06_references/08-banking77-eval-missing-per-seed-results.png) — baseline eval collapses ten valid seed-level trials into a mean and incorrectly implies that no evidence exists.
9. [`optimizer_visuals_v06_references/09-eval-transcript-card.png`](optimizer_visuals_v06_references/09-eval-transcript-card.png) — completed eval transcript card overemphasizes orchestration telemetry and underemphasizes the scored result.

## Outcome

Make an optimizer run understandable in ten seconds and inspectable in depth without turning the primary view into a raw event dump.

The default view must answer, in this order:

1. Did the run improve the heldout objective?
2. What is happening now, or why did it stop?
3. What did it cost and how much bounded work was consumed?
4. Which candidate won each gate, and why?
5. What changed in the candidate?
6. What evidence supports the decision?

## Problems observed

### Truth and hierarchy

- The completed run visually celebrates `BEST TRAIN 0.80` while the authoritative outcome is a heldout regression from `0.60` to `0.56`.
- `ACCEPTED` means accepted at the full-train gate, but reads like the candidate won the run.
- The run consumes 710 budgeted rollouts while the evidence section shows 599 evaluation rollouts. Both can be correct, but the distinction is not explained.
- Completed budget rows still show estimates such as `~6m 10s left` and `~22h 23m left`.
- Raw floating-point cost (`0.05061499999999999`) leaks into the UI.
- Empty or unavailable contract fields occupy prime space and look broken.
- The run title, terminal result, and model labels are repeated without establishing a single dominant verdict.

### Layout and density

- The visual opens underneath surrounding Workshop chrome, artifact controls, and a terminal strip.
- The title is clipped at the top.
- The page is one very long vertical document with no persistent navigation or sense of position.
- Dense two-column regions collapse badly: frontier rows, candidate inspector, rejection copy, and action buttons collide.
- Monospace metadata is overused, reducing legibility and making ordinary values look like debug output.
- Cards, borders, pills, and nested panels compete equally for attention.
- Candidate and evaluation lists repeat large blocks instead of supporting scan, sort, and drill-down.
- The proposer trace renders every item at full width and full height by default.

### Visualization quality

- The incumbent chart encodes scored order but does not label candidates directly or show heldout performance.
- The orange trajectory is visually dominant even though it represents train-only acceptance.
- The Pareto cell matrix is difficult to decode and consumes substantial space before it yields insight.
- Budget bars mix limits, actual work, and ETA projections; terminal rows preserve stale projections.
- The final heldout comparison is not the main chart.
- The eval overview receives per-trial `records` but does not render them, so task/seed outcomes disappear.
- `Evidence: 0 items` means no attached artifacts, but reads as if the ten scored trials have no supporting evidence.

## Banking77 eval target

Reference: `08-banking77-eval-missing-per-seed-results.png`.

The current visual is a useful summary header followed by a missing analysis body. The redesigned eval visual should look like this:

### Above the fold

- Compact title and terminal state.
- Primary result: `10/10 correct · 100% accuracy` rather than three redundant mean cards.
- Policy, split, sample count, elapsed time, and cost/usage coverage.
- Baseline-only limitation as a small contextual note, not the largest terminal panel.

### Per-task/seed results

Immediately below the summary, render the authoritative trial records:

| Seed | Example | Expected | Predicted | Result | Reward | Duration |
|---:|---|---|---|---|---:|---:|
| 0 | Why was my cash withdrawal declined? | `declined_cash_withdrawal` | `declined_cash_withdrawal` | Pass | 1.00 | — |

- One row per task/seed from the visual binding’s `records` field.
- Show seed and stable example/task ID even when prompt text is unavailable.
- Include pool/split, expected label, predicted label, reward, status, latency, and failure/error details when present.
- Default sort by seed; allow sorting and filtering by pass/fail, label, split, and reward.
- Expand a row for full input, policy output, grader/verifier result, rollout reference, and declared artifacts.
- For ten rows, render all rows. For large evals, virtualize and paginate without changing the schema.

### Outcome distribution

- Add a compact pass/fail strip or confusion matrix when expected and predicted labels exist.
- Show `10 passed · 0 failed`; do not rely on a mean of `1` to communicate correctness.
- If all rewards are binary, label the aggregate `Accuracy 100%` rather than `Mean 1`.
- Preserve missing rewards as missing and list them separately from failures.

### Evidence semantics

Rename the current section:

- `Trial evidence` — the scored task/seed records; always present when trials completed.
- `Attached artifacts` — traces, screenshots, verifier files, and downloadable outputs; may legitimately be empty.

Never display `No evidence has been attached` directly beneath a completed `10/10` eval without clarifying that it refers only to artifacts.

### Responsive behavior

- Desktop: results table with a right-side or modal row inspector.
- Narrow pane: stacked compact rows showing seed, expected/predicted, result, and reward; details expand in place.
- Keep the result summary and filter bar sticky while scrolling the trial list.

### Eval acceptance criteria

- The captured Banking77 run displays ten seed-level rows without another network request.
- A user can identify which seed failed and why in two interactions or fewer.
- `Evidence` never reports zero when valid trial records exist.
- Binary rewards render as accuracy/pass counts; continuous rewards render as distributions and means.
- Trial evidence and attached artifacts are visibly separate concepts.

### Eval transcript card

Reference: `09-eval-transcript-card.png`.

The in-chat card should be a compact outcome summary, not a miniature operations dashboard.

For a completed Banking77 baseline eval, it should read approximately:

> **Banking77 baseline eval** · Completed  
> **Accuracy 100%** · 10/10 examples correct  
> Baseline only · no promotion decision  
> 2s · Cost not reported  
> `Inspect results`  `Open visual`

Required changes:

- Replace `Campaign finished` with the primary scored outcome when terminal.
- Render binary reward as `Accuracy 100%` and `10/10 correct`, not just `10/10 trials`.
- Remove terminal concurrency, queue depth, throughput, and parallelism from the default card; retain them in the expanded progress dialog.
- Shorten `Cost unavailable · producer emitted no cost telemetry` to `Cost not reported`, with the diagnostic in a tooltip/details view.
- Replace the long selection sentence with a small neutral badge: `Baseline only` and secondary text `No promotion decision`.
- Rename `View progress` to `Inspect results` after termination.
- Keep one primary action and one quiet secondary action.
- Reduce card height and visual gaps by roughly one third.
- If any trial failed, replace the success headline with the failure count and expose `Review failures` as the primary action.
- Preserve detailed operational telemetry during live execution; collapse it when the run becomes terminal.

### Complete eval visual and modal audit

The eval experience currently fails at three connected layers: data semantics, the full visual, and the in-chat progress card/expanded modal.

#### Data and semantics

- The backend emits seed, split/pool, task/scenario, reward, status, policy reference, rollout reference, and error information for each trial.
- The experiment binding includes those rows under `records`, but the overview projection ignores that field and renders no trial-level view.
- Ten separate evaluations are therefore collapsed into a single mean and cannot be audited.
- `Evidence · 0 items` refers only to attached artifacts, but reads as if 10/10 valid trials have no evidence.
- Trial evidence and attached artifacts must be separate first-class concepts.

#### Full visual

- It answers whether orchestration finished but not what happened on each task.
- It repeats the research question and completion state while omitting the expected-versus-predicted results.
- `Train mean 1` and `Overall mean 1` redundantly encode what should be `Accuracy 100% · 10/10 correct`.
- Heldout receives a large card despite being absent.
- Progress dominates after completion, when results and evidence should replace it.
- Missing elapsed, ETA, usage, cost, and heldout values consume prime space as dashes.
- A baseline-only policy gets a full Variants section even though there is no comparison.
- The expected `Baseline only` design constraint is presented as a large warning-like limitation.
- There is no pass/fail distribution, confusion matrix, missing-reward analysis, or failure diagnosis.
- There is no evidence path from aggregate → failed seed → expected/predicted → grader → rollout/artifact.
- The design does not scale: aggregate-only is unauditable, but rendering hundreds of stacked cards would be unusable.

#### Transcript card and expanded modal

- The completed card emphasizes active workers, queue depth, trials/minute, and parallelism instead of accuracy.
- Terminal throughput such as `640 trials/min · 10 parallel` is operational noise.
- `producer emitted no cost telemetry` exposes an internal diagnostic where `Cost not reported` is sufficient.
- `baseline-only evaluation; no promotion decision` should be a neutral badge plus short secondary text.
- `10 valid of 10 finished trials` should become `10/10 correct` for binary classification.
- `View progress` is the wrong terminal action; use `Inspect results`.
- Excessive padding and spacing make the card roughly one third taller than necessary.
- The generic expanded modal appears to preserve the live orchestration abstraction after termination.
- The modal must switch from live mode (workers, queue, ETA, throughput) to result mode (accuracy, trials, failures, selection, artifacts).

#### Required terminal hierarchy

1. Outcome: accuracy or primary score, pass count, split, and sample count.
2. Trial results: per-seed/task table with expected, predicted, result, reward, and duration.
3. Analysis: distribution/confusion, missing evidence, and failure clusters.
4. Details: policy metadata, rollout traces, attached artifacts, raw events, and operational telemetry.

The current implementation shows fragments of outcome and details while omitting the evidence and analysis layers that make an evaluation useful.

## Information architecture

### 1. Sticky run header

One compact header, below Workshop chrome:

- Run name and algorithm.
- State badge: `Running`, `Completed`, `Failed`, or `Cancelled`.
- Primary verdict badge:
  - `Improved +x.xx heldout`
  - `No measured improvement`
  - `Heldout unavailable`
- Current phase for live runs, including proposer heartbeat and phase elapsed time.
- Compact facts: elapsed, cost, rollouts, proposer, policy.
- Actions: pause/cancel while live; export/share after completion.

Do not show private artifact URL controls or terminal chrome inside the visual body. Move developer-only controls to Debug.

### 2. Outcome overview

The first content block after the header:

- Side-by-side seed and selected-candidate heldout scores.
- Absolute delta with confidence/sample context when available.
- Train score as secondary diagnostic evidence.
- One-sentence decision explanation.
- Explicit gate semantics: `Promoted on train`, `Rejected by heldout`, or `Selected for deployment`.

For the reference run:

> No measured improvement. Train increased 0.72 → 0.80, but heldout decreased 0.60 → 0.56 across 100 heldout samples.

### 3. Run timeline

A compact horizontal or stepped timeline:

`Seed evaluation → Proposal → Minibatch gates → Full train → Heldout → Decision`

Each step shows state, duration, and one relevant count. The active proposer step includes:

- pulsing heartbeat;
- generation and model;
- elapsed time since `proposer.started`;
- last durable trace activity time;
- `Still working` versus `No events for N minutes` warning.

### 4. Candidate funnel

Replace the long candidate-card list with a funnel/table:

| Candidate | Minibatch | Full train | Heldout | Decision |
|---|---:|---:|---:|---|
| Seed | — | 0.72 | 0.60 | Baseline |
| Proposal 1 | 0.90 | 0.80 | 0.56 | Heldout regression |

- Sort by decision, heldout, train, generation, or cost.
- Default to meaningful candidates: seed, incumbents, heldout-evaluated candidates, and failures needing attention.
- Put early-gate rejections behind `Show 6 rejected at minibatch`.
- Selecting a row opens a stable inspector drawer; it must not reflow the table.

### 5. Candidate inspector

Use tabs:

- Summary: lineage, gate decisions, score deltas, changed levers.
- Prompt diff: readable word/line diff with synchronized old/new modes.
- Evidence: won/lost examples and confusion clusters.
- Raw: JSON and artifact downloads.

Decision labels must be scoped:

- `Passed minibatch`
- `Promoted on train`
- `Rejected on heldout`
- `Final selection`

Never use unqualified `Accepted` for an intermediate gate.

### 6. Evaluation explorer

- Virtualized table rather than full-width stacked rows.
- Columns: candidate, split, completed, accuracy, failures, duration, cost.
- Filters: split, candidate, pass/fail, label/confusion, rollout ID.
- Expand a row only to inspect individual examples.
- Clearly distinguish budget accounting from unique evaluation evidence.

### 7. Proposer trace

- Default to a concise trace summary: model, duration, tools, token/cost usage, proposals emitted.
- Show a live heartbeat while active.
- Collapse raw tool calls and command output.
- Promote reasoning summaries, proposal manifest creation, validation, and terminal output.
- Keep `Full trace` as an explicit secondary mode.

### 8. Contract and diagnostics

Move the search contract below the outcome and candidate funnel.

- Render only populated fields by default.
- Group mutable surface, objective, splits, limits, and models.
- Put missing fields and schema diagnostics behind `Diagnostics`.
- Put evidence integrity, raw events, artifacts, usage coverage, private URLs, and terminal access in Debug.

## Visual system

- One neutral page surface; use cards only for semantic grouping.
- Limit borders to container boundaries and selected rows.
- Use sans-serif for UI values; reserve monospace for IDs, code, prompts, and raw telemetry.
- Use green only for measured success, red for measured regression/failure, amber for uncertainty or active gates, and blue for neutral selection/navigation.
- Use a consistent 8 px spacing system and a readable maximum content width.
- At widths below 1100 px, switch all split panes to a table plus drawer; never squeeze two analytical panes side by side.
- Preserve keyboard navigation, visible focus, screen-reader phase announcements, and reduced-motion alternatives.

## Data and semantic fixes

1. Add an explicit final-verdict projection with baseline, selected candidate, split, sample count, and absolute delta.
2. Separate `budgetRolloutsSpent` from `evaluationRolloutsObserved` and label both.
3. Freeze terminal elapsed time and remove all terminal ETAs.
4. Format currency through one shared formatter; never render raw floats.
5. Scope every candidate decision to its gate.
6. Prefer heldout evidence over train evidence in completed-run headlines.
7. Track proposer liveness from durable trace events:
   - started at;
   - last activity at;
   - completed/failed at;
   - heartbeat state;
   - stale threshold.
8. Do not render unavailable contract fields in the overview.

## Eval candidate, task, seed, and environment contract

Normal optimizer evals are matrix experiments, not a single aggregate score. The staged candidate set owns immutable policy variants; the pinned recipe/container contract owns the task or scenario variants, environment implementation, split, and seed ledger. The run plan must preserve both sides and the visual must join them as one row per `candidate × task/scenario × seed` trial.

The visual payload should pass through and render:

- candidate ID, label, kind, digest, entrypoint/model reference, and baseline marker;
- environment/container ID, image digest, runtime (`native Rust` where applicable), task/scenario ID, and contract version;
- split, seed, trial ID, status, reward/verifier reward, latency, usage/cost, and attached Trace V5/evidence links;
- per-candidate scorecards plus paired deltas computed on identical seed/scenario cells;
- explicit missing, failed, retried, and unpaired cells rather than silently dropping them;
- selection status separately from orchestration/run status.

The primary eval view should therefore show a compact candidate comparison first, then a filterable trial matrix/ledger. Selecting a candidate, task, seed, or failed cell should open one shared inspector with reward details, environment metadata, trace, logs, and artifacts. Aggregate means are summaries of that ledger, not replacements for it.

The Banking77 baseline recipe is a deliberate measurement-only exception: it evaluates the container-advertised baseline and makes no promotion decision. It should not invite staging multiple candidates, and the UI must say before launch that a supplied candidate set will not be used. The current flow did the opposite: it asked for policy staging, accepted a frozen two-candidate set, then displayed one container-advertised variant. That is a contract/UI bug, not evidence that evals cannot represent variants.

### Craftax Rust 10× ReAct local-source finding

The v0.6 catalog exposes these Craftax eval contracts:

- `eval.craftax.code-policy.smoke.v1`: report-only, seeds 101 and 102, parallelism 2, 10 trials per candidate in the recipe contract;
- `eval.craftax.llm-policy.smoke.v1`: report-only ReAct/LLM-policy path, seeds 101 and 102, parallelism 4;
- `eval.gamebench.craftax-code-policy.confirm.v1`: promotable native GameBench confirmation path;
- `eval.gamebench.llm-policy.confirm.v1`: promotable LLM-policy confirmation path.

The runner does **not** require the target to be published in a registry. Its supported development path accepts an immutable local OCI image ID when that ID matches the recipe pin. A local source-overlay target was built with pulls and build networking disabled from `tasks/craftax-singleplayer/Dockerfile.eval-target-local`. Its immutable ID is `sha256:1150dde7018395f46fa62db72357562307abd5dd69b5d923fafa57fb6f160e24`; it retains the `eval.target.v1` contract and GameBench source provenance. Both Craftax smoke recipes are pinned to that local ID and pass `eval doctor`; no Docker Hub/GHCR pull or publication is needed to execute them on this workstation.

This distinction matters: the ordinary GameBench Rust Dockerfile launches a long-running HTTP service and is **not** an eval target. The eval runner needs a one-shot wrapper that reads `/input/trial.json` and `/input/policy`, evaluates the checked-in Craftax engine, and writes `/output/result.json`, `/output/events.jsonl`, the verifier report, and the required trace. The locally cached target implements that contract via `/app/target.py`. Future source-only setup should build this wrapper plus the GameBench task tree, label the resulting image with its source commit and `eval.target.v1` contract, then pin the resulting local image ID.

The code-policy smoke path is locally runnable now. The LLM/ReAct path additionally requires `OPENAI_API_KEY` in the eval home's `secrets.toml`; the current eval home contains no configured secret. That credential requirement is independent of image publication and must be shown separately in preflight rather than collapsing both conditions into “unavailable.” The two confirmation recipes remain unpinned.

The cached smoke target currently invokes GameBench with `--lane python`; its provenance is the Craftax source tree, but it is not the requested native Rust execution lane. The checked-out Rust service builds and runs successfully, yet its HTTP-server interface cannot be substituted for `eval.target.v1`. A truthful native-Rust run still needs a one-shot target wrapper that starts or invokes `craftax_gold`, drives the same pinned seed/scenario, and translates its evidence into the eval result/trace contract. Until then the UI must label this target `Craftax symbolic Python`, not `Native Rust Craftax`.

### Craftax setup integration findings

- The Craftax code-policy card became available after local pinning and correctly advertised seeds 101/102, `reward`, `report_only`, and parallelism 2.
- Its generated agent prompt was stale and incompatible with the recipe: it requested kind `python-code.v1` and entrypoint `policy:Policy`; the target requires `python-code.craftax-choose-actions.v1` and a `choose_actions` entrypoint.
- The agent successfully staged two intended project variants (`craftax_action0` baseline and `craftax_action1`) as `policy_set_b45353a44f1c4d04818b2cff2733c36c`.
- Start then failed before run creation with `target_not_digest_pinned`, even though the same eval home's `pins.toml` contains the local immutable image ID and `eval doctor` reports both Craftax smoke recipes `ok`. This indicates the Workshop MCP/start path and CLI doctor were reading cached or different runtime state.
- Restarting during a concurrent v0.6 app rebuild reopened a different local profile where the Optimizers sidecar was stopped and Craftax reported that the local runtime was not installed. Do not treat that profile's catalog as evidence about the original `b77gepa` instance; stabilize the app/data-root binding before retrying.

Once published, the Craftax visual is the acceptance test for this eval design. It should make the following visible above the fold:

- `Native Rust Craftax` target, exact image digest, recipe ID, and report-only/promotable decision mode;
- policy variants, including code-policy versus LLM/ReAct kind where relevant;
- the exact planned trial count and concurrency;
- seed/scenario coverage and live active/queued/completed cells;
- per-policy reward/verifier-reward, paired lift, confidence/coverage, and selection outcome;
- direct access from every trial cell to its Trace V5 trajectory and environment evidence.

An unavailable recipe card should identify the precise missing prerequisite and remediation—local/published target pin, candidate, or credential—rather than merely disable `Set up run`.

## Implementation sequence

### Parallel dependency — local secrets broker

The Workshop Local Secrets Broker is being implemented in parallel by another owner. Its design is captured in `outputs/workshop-local-secrets-broker-v06.md`. This optimizer-visual track should consume its stable contract rather than duplicate the vault or proxy implementation.

Until that work lands:

- continue visual testing with optimizer/eval recipes that do not require provider credentials;
- do not restore plaintext `secrets.toml` or inject provider keys into agent/container environments;
- represent credential/proxy readiness as an explicit run dependency;
- resume Craftax ReAct only after the broker exposes a scoped provider-use capability.

### Phase A — correctness and hierarchy

- Build the final-verdict summary.
- Fix terminal ETA and currency formatting.
- Disambiguate rollout budget versus observed evaluations.
- Rename intermediate candidate decisions.
- Remove unavailable fields from the overview.

Acceptance:

- The reference run’s first screen says `No measured improvement` and shows `0.60 → 0.56` before `0.72 → 0.80`.
- No terminal view contains `left`, an active spinner, or a raw floating-point currency value.
- No intermediate candidate displays an unqualified `Accepted` badge.

### Phase B — responsive structure

- Implement sticky header and section navigation.
- Replace split-pane candidate layout with table plus inspector drawer.
- Remove visual-body terminal/artifact chrome.
- Establish responsive breakpoints and overflow tests.

Acceptance:

- No clipping or horizontal overlap at 900, 1100, 1440, and 1920 px.
- The run verdict, current phase, and primary action remain visible without scrolling.

### Phase C — candidate and evidence workflows

- Candidate funnel/table and scoped decisions.
- Inspector tabs and prompt diff.
- Virtualized evaluation explorer.
- Compact Pareto/explore-exploit view as an optional analytical tab.

Acceptance:

- A user can identify the winning train candidate, its heldout result, and its rejection reason in three interactions or fewer.
- A 10-candidate/850-rollout run does not render hundreds of rows until requested.

### Phase D — proposer observability

- Live proposer heartbeat in header and timeline.
- Staleness detection based on durable event timestamps.
- Concise trace mode with explicit expansion to raw details.

Acceptance:

- During a multi-minute proposer call, the UI continuously shows model, generation, phase elapsed, and last activity.
- A quiet-but-active proposer is distinguishable from a stalled proposer.

### Phase E — cross-optimizer primitives

- Extract shared verdict, timeline, budget, candidate-table, inspector, and trace primitives.
- Apply them to GELO, SFT, and eval without flattening algorithm-specific semantics.

## Verification

- Projection tests for final verdict, terminal ETA, cost formatting, gate-scoped decisions, and proposer liveness.
- Component tests for hierarchy and collapsed/default states.
- Screenshot tests at 900, 1100, 1440, and 1920 px.
- Fixtures for running proposer, stale proposer, no-heldout, heldout regression, measured improvement, partial evidence, and failed run.
- Replay the captured Banking77 run and compare every displayed total to its durable manifest/events.
- Keyboard and screen-reader pass for table, drawer, tabs, timeline, and live phase announcements.

## OAuth credential isolation: never show the macOS Keychain prompt

The named `b77gepa` development app displayed a macOS password dialog for the
legacy `synth-desktop` Keychain item after LaunchServices reopened the `.app`
outside `scripts/desktop-instance.sh`. The wrapper-provided
`SYNTH_DESKTOP_INSTANCE` environment was absent, so environment-derived instance
identity was not a safe boundary for selecting a credential store.

The v0.6 invariant is now stronger: Workshop does not read or write Codex OAuth
credentials through macOS Keychain. Every build uses an instance-state-scoped,
owner-private credential file. A named debug/QA instance may copy from an
explicit seed only when both its seed and private state paths are configured;
normal and release-like launches never implicitly import credentials.

Required guarantees:

- No Workshop process may access the legacy Keychain service `synth-desktop`.
- Credential state lives below the resolved instance state root, not a shared
  global namespace.
- Missing launcher environment fails closed to an unseeded private store.
- Direct Finder/Dock launch, `open`, macOS restoration, Computer Use attachment,
  and wrapper launch must select the same isolated state root or remain signed
  out; none may fall back to shared credentials.
- Credential files are created with owner-only permissions and logs, errors,
  exports, traces, and agent-visible tool results redact bearer tokens.
- Agents receive scoped use capabilities from the secrets proxy; they do not
  receive the underlying OAuth refresh/access token.

Regression matrix:

1. Launch a named instance through `desktop-instance.sh`.
2. Quit and reopen its `.app` directly with all launcher variables cleared.
3. Reopen through Finder/Dock and macOS window restoration.
4. Attach through Computer Use and other automation entry points.
5. Rebuild and re-sign the same bundle identity, then repeat direct launch.
6. Launch canonical, debug, named QA, and release-like configurations.

For every case, assert that no Keychain API is called, no password dialog is
shown, and the resolved credential path stays inside that instance's state root.
The current unit contract lives in `codex_oauth.rs` as
`every_build_uses_a_private_file_and_only_explicit_qa_may_seed_it` and
`canonical_store_path_is_scoped_to_the_instance_state_root`.

### Craftax ReAct local-run retry — 2026-08-18

The exact `eval.craftax.llm-policy.smoke.v1` workflow was retried with two
frozen `llm-policy.v1` candidates (`luna-low` baseline and `luna-medium`), the
recipe-owned seeds 101/102, report-only selection, parallelism 4, and the
locally built `eval.target.v1` image ID
`sha256:d1b3eaccfd833f0f67eaf682be0ea162e93ddacb71db944be9b3e03c82cd09bd`.

Two separate admission defects were confirmed:

1. The installed runtime's doctor/catalog accepted the operator-pinned local
   OCI image, while Workshop start rejected the registry-less catalog name as
   `target_not_digest_pinned`. A loopback-qualified local tag
   (`localhost:5000/craftax-eval-target`) proved that no push or Docker Hub is
   technically required: the same local image ID passed admission and the run
   was created.
2. The run then failed before dispatch because the eval worker could not resolve
   `OPENAI_API_KEY`. Workshop already has a configured provider secret in its
   host-managed source, but the new secrets broker is not yet wired into the
   eval home's `secrets.toml`/worker capability boundary. Do not copy the value
   into agent-visible files as a workaround; the broker must materialize or
   proxy a run-scoped capability for the worker.

Created evidence:

- run: `opt_eval_d2ea1c28916b`
- visual: `vis_4581a9f35d5c4bef86b268da4be1d765`
- template: `optimizer.eval.live.v1`
- terminal status: `failed`, zero trials dispatched

The failure visual is open and exposes another presentation problem: it shows
`TRIALS 0/0`, no candidates, all stages skipped, and “selection pending,” even
though two candidates and four recipe-owned candidate×seed trials were planned.
A pre-dispatch failure should preserve the declared matrix (`0/4`), list both
staged policies, identify the failed dependency (`OPENAI_API_KEY capability
unavailable`), and replace “selection pending” with “not attempted.”

The direct-launch lifecycle bug also remains broader than OAuth: after an app
exit, LaunchServices/automation can reopen the named bundle without the wrapper
and hydrate the canonical data profile. Bundle-owned instance identity and data
root must be resolved intrinsically before any subsystem starts; removing
Keychain access fixes the password dialog but does not fix profile switching.

## Definition of done

- The default screen communicates the final heldout verdict without scrolling.
- Live runs communicate motion without fabricating progress.
- Dense evidence is available but not dumped into the primary reading path.
- Candidate decisions are unambiguous about which gate made them.
- Terminal values are frozen, formatted, and internally consistent.
- The layout remains usable across supported window sizes.
- The same primitives can represent GEPA, GELO, SFT, and eval runs.
