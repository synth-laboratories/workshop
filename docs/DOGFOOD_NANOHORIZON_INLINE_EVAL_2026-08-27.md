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
