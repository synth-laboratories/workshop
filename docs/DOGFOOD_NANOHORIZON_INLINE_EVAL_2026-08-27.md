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
