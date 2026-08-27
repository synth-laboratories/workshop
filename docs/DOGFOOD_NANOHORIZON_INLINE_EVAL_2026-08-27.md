# NanoHorizon inline-eval dogfood log — 2026-08-27

This is a factual incident log from Workshop v0.8 instance J. Missing evidence is
recorded as unavailable; no fixture or synthetic runtime result is accepted.

## Build under test

- Branch: `eval/inline-first-admission`
- Initial commit: `93e76f31073c`
- Credential route: file-backed ChatGPT OAuth plus Workshop secrets proxy
- Keychain: prohibited and unused

## Incident 1 — stale unique experiment/session index

- Stage: fresh experiment creation, before inline-spec drafting or spend
- Symptom: `experiment_create` reused the session's existing experiment;
  `experiment_create_child` then failed with
  `UNIQUE constraint failed: experiment_groups.session_id`.
- Runtime evidence: J's `synth.sqlite3` still had
  `CREATE UNIQUE INDEX experiment_groups_session ON experiment_groups(session_id)`
  even though schema migration version 47 was recorded.
- Root cause: the prerelease database had consumed an earlier migration number.
  The later repair used `CREATE INDEX IF NOT EXISTS`, which cannot replace an
  existing unique index with a non-unique index.
- Fix: migration 48 explicitly drops and recreates
  `experiment_groups_session` as non-unique on
  `(session_id, created_at, id)`.
- Regression: a v47-shaped database with the legacy unique index must accept
  two experiment rows for one session after migration.

## Earlier evidence retained from this task

- A prior admitted run returned null per-rollout call and step caps and a
  mismatched inline digest. The agent cancelled before dispatch.
- A prior run failed `policy_source_unavailable` before any rollout executed;
  its credential capability was revoked and cost remained unavailable.
- Those identities and results must not be reused by the fresh run.

## Incident 2 — contradictory live summary telemetry

- Stage: run `opt_eval_craftax_9b4825781ecd`, all five rollouts running
- The chat run card reported `20s elapsed`, while the attached experiment
  visual reported `ELAPSED 0s` for the same authoritative run.
- The chat run card rendered `$0.00 / $2.45` while its own next line said
  `Cost unavailable · producer emitted no cost telemetry`; the experiment
  visual correctly said `awaiting telemetry / $2.45`.
- Required fix: one typed aggregate projection must drive both surfaces.
  Missing cost is `unavailable`, never numeric zero. Elapsed time must derive
  from the same admitted-run start timestamp or explicitly identify a
  different clock/phase.

## Successful state transitions observed after migration 48

- Child experiment: `exp_965561c61739435b8a982a5924335437`
- Inline spec: `sha256:4dfbcede52b9b4bb992fde685d2948ece4d3becb86697f1f254471a4ff36f392`
- Run: `opt_eval_craftax_9b4825781ecd`
- Overview visual: `vis_df48327dfde54d97b8ee71e4bba9e5a6`
- Trace visual: `vis_238ecc4dd8104dafa5d2ad7a93f96d6e`
- Five rollouts moved from queued to running with distinct rollout identities.

## Incident 3 — registered trace launcher referenced a deleted interpreter

- Stage: terminal evidence import after all five rollouts completed
- Symptom: the registered `synth-trace` launcher existed, but executing it
  failed because its generated wrapper referenced a removed virtualenv Python.
- Root cause: trace authority resolution checked only `Path::is_file`; a stale
  launcher therefore passed readiness without being executable as a runtime.
- Repair: re-register synth-containers `0.4.1.dev20260817`; pin Desktop to that
  exact version and require a successful, exact-version `synth-trace version`
  probe before accepting the authority.

## Incident 4 — canonical Trace V5 payload read through the wrong field

- Stage: importing the five closed, self-contained Trace V5 bundles
- Symptom: each bundle indexed its immutable trace record but frame extraction
  failed with `sealed frame event omitted step for unique artifact`.
- Root cause: the canonical producer wrote frame `step` and
  `source_event_digest` under `event.payload`; Workshop incorrectly read the
  non-canonical `event.detail` field.
- Fix: frame extraction and maximum-step calculation now read the canonical
  `payload` object only. No inferred or legacy fallback was added.
- Regression: the portable Trace V5 PNG test uses the canonical event shape and
  verifies step, producer digest, dimensions, bytes, and aggregate maximum.

## Terminal evidence before Incident 4 repair

- All five rollouts completed with 50 total model calls and 98,167 total
  tokens. Rewards, environment-step totals, and cost were unavailable.
- The run was classified `degraded`; no replacement run was started.
- Capability `cap_138605d58394419ea6af80d59964bf34` was confirmed revoked.
- Evidence-only reconciliation registered five trusted `tracev5_*` records.
- The run card incorrectly said evidence did not persist while the visual and
  trace inventory showed retained frames and five indexed traces. This is
  another projection split and must be eliminated by the shared aggregate
  projection described in Incident 2.

## Consolidated gap inventory

This inventory distinguishes missing producer facts from host bugs. Workshop
must never manufacture one category to conceal the other.

### P0 — authoritative state is internally contradictory

1. **Terminal run contains running policy calls.** The workstation header says
   `5/5 terminal`, while call rows 6–9 still say `deciding… / running`. A
   terminal rollout must settle every open call to a typed terminal outcome
   (`completed`, `invalid_response`, `timed_out`, `cancelled`, or `aborted`).
   There must be no terminal-run projection containing an open call.
2. **Run card denies persisted evidence that inventory proves exists.** The chat
   card says `Evidence unavailable` and `the run finished but its evidence did
   not persist`, while five trusted, inspectable `tracev5_*` records and retained
   frames exist. Both surfaces must consume one authoritative aggregate.
3. **Terminal elapsed time continues increasing.** The workstation showed more
   than 30 minutes after a run whose recorded wall time was 1m38s. Terminal
   elapsed time must freeze at `ended_at - started_at`; viewer-open duration is
   a different metric and must not replace it.
4. **Completion and degradation are conflated.** All five rollout execution
   states are completed, but the campaign is degraded for missing evidence.
   Execution state and evidence-quality state need separate enums and displays;
   neither should overwrite the other.

### P0 — required evaluation outputs were never produced

1. **Rewards are absent for all five seeds.** The declared evaluator cannot be
   considered successfully executed without a terminal reward fact or a typed
   evaluator failure per rollout. `completed + reward absent` is insufficient.
2. **Policy responses frequently failed tool-call parsing.** The environment
   applied no action for affected calls, yet the call remained visually
   `running`. Invalid responses must become durable typed call outcomes and feed
   rollout/evaluator status deterministically.
3. **Environment-step totals are absent for four seeds.** Retained frame steps
   are evidence locations, not execution-step telemetry. Workshop must not
   substitute frame counts or maximum frame indices for actual environment
   steps.
4. **Achievements are not evidenced.** An empty authoritative achievement list
   and missing achievement telemetry are different states. The UI currently
   says `No achievements reported yet`, which is ambiguous after termination.

### P1 — usage, cost, and approval accounting are incomplete

1. **Token split-brain.** The terminal response records 98,167 tokens, while the
   workstation says `tokens unavailable / no limit`. The approved spec had no
   token ceiling, but observed usage still exists and must be projected.
2. **Cost is unavailable.** This is honest, but the chat card previously rendered
   `$0.00` and `$0.01` beside an unavailable-cost warning. Numeric cost must be
   impossible unless an authoritative cost observation exists.
3. **Paid-approval receipt ID is lost from terminal records.** The approval
   occurred and was digest-bound, but the terminal result could not report its
   receipt ID. Admission receipt identity must remain attached through every run
   state and evidence reconciliation.
4. **Limit presentation lacks semantic status.** `50 / 50` model calls is shown
   in red without saying whether the exact bound was cleanly reached or violated.
   Limit state needs a typed enum such as `within`, `at_limit`, `exceeded`, or
   `unavailable`.

### P1 — trace workstation projects stale or incomplete per-seed state

1. **Seed chips show dashes after terminal settlement.** Each chip should show
   execution terminal state and evidence quality independently.
2. **Call timeline is not reconciled from sealed Trace V5.** Live relay rows
   survive unchanged after sealed evidence is imported. Terminal projection must
   replace or explicitly supersede provisional live rows.
3. **Selected image can be symbolic ASCII despite retained native RGB frames.**
   A native-required run must either display a verified embedded RGB artifact or
   raise `native_frame_unavailable`; symbolic rendering cannot silently replace
   it.
4. **Frame coverage is too sparse and unexplained.** Four seeds retained only
   step 0 and one retained steps 0–5. The UI must show the declared retention
   policy, observed coverage, and any capture gaps rather than implying a full
   replay.
5. **Reward distribution renders an empty outlined bar.** With zero reported
   rewards, no numeric histogram or zero-valued bucket should render. Show a
   typed unavailable state and the missing denominator.

### P1 — reconciliation semantics are unsafe or misleading

1. **Import could persist trace records and still return an extraction error.**
   This partial-success contract caused the agent to first report failure and
   then discover indexed records. Import needs a typed transaction result that
   distinguishes `trace_indexed`, `projection_failed`, and `fully_reconciled`,
   or must be atomic where appropriate.
2. **Reconciliation reports success while required facts remain absent.** A
   successful command means the reconciliation procedure completed; it must not
   imply the evaluation is evidentially complete. Return procedure status and
   evidence-quality status separately.
3. **Visual revisions do not clearly expose run binding.** The trace workstation
   header showed `run — · trace —` even while rendering a specific run and
   imported traces. Binding IDs must be first-class, non-null, and inspectable.

### P2 — runtime and development operability

1. **A stale trace launcher passed readiness.** Fixed in `61cb243a`: Desktop now
   probes the executable and exact version instead of checking file existence.
2. **Canonical event fields were read through the wrong schema object.** Fixed
   in `61cb243a`: frame fields come from `event.payload`, with no legacy fallback.
3. **Local rebuild launch can lose ChatGPT authentication.** `rebuild-run` opened
   J without the file-backed subscription context; a subsequent `cua-run`
   restored it. The instance launcher must have one canonical launch path that
   always injects the authorized file-backed OAuth context and verifies auth
   before declaring readiness.
4. **Required workflow skill was absent from the session home.** The trace tool
   told the agent to load `use-synth-traces`, but that skill was not installed.
   Tool availability, skill installation, and generated instructions must be
   validated together at session creation.

## Recommended ownership boundaries

- **Container/runtime:** terminal call outcomes, environment steps, evaluator
  reward facts, achievement facts, sealed Trace V5 completeness.
- **Optimizer control plane:** run/rollout state transitions, admission receipt
  retention, capability revocation, evidence-quality state, reconciliation.
- **Trace ingestion:** exact runtime readiness, atomic/typed import outcomes,
  canonical schema validation, immutable projections.
- **Shared projection layer:** the sole aggregate consumed by chat cards,
  experiment visual, and trace workstation.
- **Renderer:** faithful presentation of typed states only; no inference,
  numeric defaults, or symbolic/native substitution.
- **Instance launcher:** deterministic file-backed ChatGPT authentication and
  end-to-end readiness verification without Keychain.

## Exit criteria for the next dogfood run

1. Five rollouts traverse planned → queued → starting → running → one terminal
   execution state, with no skipped or lingering open child state.
2. Every rollout has a terminal evaluator result: numeric reward or typed
   evaluator failure.
3. Calls, steps, tokens, cost, achievements, and frames each report value,
   unavailable reason, and source independently.
4. Chat card, experiment visual, and trace workstation agree byte-for-byte on
   aggregate state from the same projection revision.
5. All five Trace V5 bundles import without partial errors and expose explicit
   frame coverage.
6. Native-required rendering never shows a symbolic fallback.
7. Approval receipt and revoked credential capability remain queryable at the
   terminal record.
8. Restarting J preserves ChatGPT authentication and can resume the same task
   without creating replacement execution identities.

## CUA follow-up after FailureRuntime integration

This pass reviewed the rebuilt J app at commit `8634ddbfd02a` with Computer
Use, then corroborated the visible behavior against J's durable SQLite state.
J remained authenticated through the authorized file-backed ChatGPT route. No
container launch, credential operation, paid approval, rollout, or synthetic
data was created.

### P0 — the Errors surface is present but not operable

1. **The outer `Errors` tab is clipped out of the workbench header.** At both
   the normal J window size and macOS full-screen size, the visible header ends
   at `Diagnostics` followed by the close button. Accessibility exposes a fourth
   `Errors` tab, but a user cannot see or click it.
2. **Accessibility activation does not recover the hidden tab.** Activating the
   AX `Errors` element closed the side panel. Tabbing from `Diagnostics` moved
   to the close control, and arrow-key navigation did not select `Errors`.
   This is both a discoverability defect and a keyboard-access defect.
3. **The naming is nested and ambiguous.** The hidden outer tab is `Errors`;
   inside it, the first inner tab is also `Errors` beside `Logs`. The hierarchy
   should be one visible top-level `Failures` surface with explicit
   `Occurrences` and `Logs` children, or an equivalently unambiguous structure.

### P0 — selected objects and adjacent evidence can belong to different runs

1. **Experiments selected the current degraded campaign while showing an old
   failed visual.** The registry selected `NanoHorizon Craftax baseline · seeds
   780015–780019` with status/result `degraded`, but the adjacent visual was
   `vis_479e097a401b4876be2ed18e189ec976`, an older failed run for seeds
   780005–780009 and digest
   `sha256:253caa5bcfd5dd4b13e101271ad4297f1e4049c24fa36bba1c59ac99f3a8ad6a`.
   A globally open visual may remain open, but the UI must either bind it to the
   selection or visibly label it as unrelated. Side-by-side unlabeled identity
   divergence is unsafe.
2. **Navigating Back from Optimizers resurrected that stale visual.** The user
   did not select the old run during the review. Back navigation returned to
   the chat and opened the historical failed artifact alongside the current
   task, preserving neither the current selected run nor a clear independent
   visual context.
3. **The current experiment inspector said `Evidence —`.** The selected current
   campaign has five trusted, inspectable Trace V5 identities, yet the node
   inspector exposed no evidence. This is a registry/projection gap, not a
   producer gap.
4. **Registry rows concatenate incompatible state fields without labels.** The
   same table showed `degraded degraded` for the current campaign and `running
   failed` for another. If these are experiment state and latest-run result,
   label both fields; if they describe one lifecycle, the latter combination is
   invalid and must be reconciled.

### P0 — historical failed visual violates its own lifecycle

The stale seeds-780005–780009 visual simultaneously rendered:

- `EXPERIMENT · FAILED` and terminal ETA;
- progress `0/5` with **five queued** rollouts;
- `Traces 5 retained`, while every trace row said no relay receipt existed;
- five trace buttons that all targeted the same visual ID instead of distinct
  trace identities; and
- the sentence `The evaluation failed: 0 of 5 rollouts did not complete
  successfully`, a double negative that states zero rollouts failed.

A failed parent must settle queued children to a typed terminal reason. Trace
retention must distinguish trace identity, visual identity, relay receipt, and
missing evidence. Generated assessment text must be derived from the same typed
counts as the table and must pass semantic tests, not only snapshot tests.

### P1 — FailureRuntime does not yet account for the active incident

J's durable failure tables contained six occurrences after migration:

- five `historical_failure_unclassified` rows; and
- one `diagnostics_index_degraded` row.

All six had `session_id = NULL`, all six were immediately `terminalized`, and
there were zero failure relationships. Consequences:

1. **The panel queries failures by the active chat session, so migrated failures
   are invisible there.** A reachable Errors panel would appear empty for this
   task even though Diagnostics shows many related failures.
2. **The current run has no failure occurrence.** There is no occurrence bound
   to `opt_eval_craftax_9b4825781ecd`, even though its authoritative evidence
   state is degraded by `reward_missing` for all five rollouts.
3. **Typed historical causes were discarded.** One migrated cause still
   contains `policy_source_unavailable`, while others contain exact failed-
   rollout counts, but every row is projected as
   `historical_failure_unclassified`. Migration should preserve any recognized
   closed code and represent only genuinely unknown shapes as unclassified.
4. **Repaired incidents have no resolution lineage.** The launcher-path and
   sealed-frame-step failures are still visible in Diagnostics, while the new
   store has no `supersedes`, `repair_of`, or resolution transition tying them
   to their successful repairs.

### P1 — the log store is noisy, misclassified, and disconnected

At review time `log_records` contained 28 rows, all with level `error`, all with
`failure_id = NULL`, and only three distinct messages.

1. **A persistent condition is emitted every 15 seconds.** Twenty-six rows were
   identical `diagnostics/index_degraded: binary_missing` messages. The matching
   failure occurrence was terminalized once at startup even though the ongoing
   log spam proves the condition remained active. A durable occurrence should
   stay open and accumulate observations, then resolve once; polling should not
   create unbounded duplicate error logs.
2. **Normal service startup is classified as error.** `Visuals IPC listening`
   and `Eval driver listening` entered the durable store at error severity only
   because they came through `eprintln`. Stream choice is not severity. These
   are typed `info/runtime_ready` events.
3. **Logs are not correlated to failures.** Even the repeated
   `diagnostics_index_degraded` rows do not reference the corresponding
   occurrence. `operation_id`, `failure_id`, and safe context must be populated
   at the emission boundary.
4. **Failures and logs use different scope.** The Errors query is session-bound;
   the Logs query is global and has no visible scope label. Switching inner tabs
   would silently change the population being inspected.
5. **The UI loses core log facts.** The Logs list omits timestamp, operation ID,
   failure ID, safe structured fields, truncation status, and pagination. It
   returns at most 100 records with no indication that additional rows exist.

### P1 — legacy Diagnostics and durable Failures remain split-brain

1. Diagnostics showed 246 journal events and generic
   `WARN mcp_request_failed` entries for typed launch and Trace V5 failures. The
   fixed sealed-frame-step incidents remain indistinguishable from live
   failures; there is no resolved or superseded presentation.
2. Diagnostics' overall banner remained `DEGRADED binary_missing`, while the
   durable occurrence for that same condition was already `terminalized`.
3. Changing the open visual changed the task-filtered Diagnostics population to
   two `visual_bindings_invalid` entries. The surface does not make the active
   session/visual scope or the reason for the population change sufficiently
   explicit.
4. The new failure detail loader catches timeline-query failures and replaces
   them with an empty timeline. That is another silent fallback. A timeline
   retrieval failure must render a typed error distinct from a legitimately
   empty timeline.
5. The renderer exposes only approval remediation for a container. Other typed
   remediation kinds, category, disposition, diagnostic reference, and safe
   context are stored but not presented, so the UI cannot yet guide most
   failures to resolution.

### P1 — Optimizers lifecycle language is internally inconsistent

1. The page says plugin `Installed v0.2.19` and `stopped`, while every local eval
   card says `the local Optimizers runtime is not installed`. Installed,
   provisioned, stopped, and unavailable must be separate states with one
   readiness projection.
2. The current run is terminal `degraded`, but the inspector's `SELECTION` is
   `failed` even while `WHY` says `baseline-only evaluation; no promotion
   decision`. Baseline evaluation outcome and candidate-selection outcome are
   different domains; absence of a selection is not a failed selection.
3. A `Cancel` button remains enabled for the terminal degraded run. Terminal
   runs cannot transition to cancelled; the action must be absent or disabled
   with an exact reason.
4. The sidebar labels all of Optimizers `Needs attention: stopped`, even though
   historical evaluation inspection remains available. Capability readiness
   should identify which operation is blocked instead of degrading the whole
   surface.
5. `eval.fixture.policy-smoke.v1` is exposed in the production recipe list.
   Product runtime catalogs must never publish fixture identities. Tests must
   construct typed in-memory or temporary data through test-only boundaries,
   never product-visible fixture recipes or fixture fallbacks.

### P2 — additional persistence and presentation defects

1. After restart, the final historical response changed from `Worked 1m 16s`
   to `Worked 0s`; other messages retained their durations. Persisted timing
   must be immutable and hydration must not substitute a zero default.
2. Restart did not restore the open visual or workbench pane state. If restoring
   those is intentional, bind the restored visual to its owning run; if not,
   clearly reopen to a neutral state rather than later resurrecting a stale
   artifact through Back navigation.
3. Outputs listed duplicate run and visual names without enough identity or
   revision information to distinguish them. Revisions of one durable object
   and separate objects with the same title need different presentation.
4. Visual headers still render `digest — · run — · trace —` for artifacts that
   contain specific execution data. Missing binding is a typed visual-quality
   failure and should explain why `Seal` is disabled; `Pass the E1 visual
   quality gate` does not provide an actionable route to the failing checks.

## Additional exit criteria from the CUA follow-up

9. The Failures surface is visible and keyboard-operable at every supported
   workbench width; activating it never closes the pane.
10. Failure occurrences and logs share explicit scope, correlation IDs, and
    lifecycle. Ongoing conditions deduplicate; resolved conditions settle once.
11. Normal startup messages cannot enter the error store solely because they
    were written to stderr.
12. The active degraded run has a typed, run-bound evidence failure occurrence,
    and repaired historical failures retain repair/resolution lineage.
13. Experiments, Optimizers, Outputs, and visual panes cannot present unrelated
    object identities as if they were one selection.
14. A parent terminal state deterministically terminalizes every child; no
    failed experiment can retain queued rollouts or open calls.
15. Production catalogs contain no fixture identities or fixture fallback
    paths.
16. Terminal run actions, assessment prose, selection semantics, and evidence
    counts are generated from closed enums and validated against the same
    aggregate revision.
