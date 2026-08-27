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
