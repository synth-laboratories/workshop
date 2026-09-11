# Workshop cloud foundations — September 11, 2026

Owner: Workshop implementation task `01a091c5-9b09-75f3-ad94-2e5fdb6295a3`.
Cloud counterpart: `01a091c5-9b09-75f3-ad94-2e3d7be81763`.
Parent: `01a09132-34bc-7010-b309-26c341127bf3`.

Worktree: `/Users/joshuapurtell/GitHub/worktrees/workshop-cloud-foundations-20260911`.
Branch: `codex/workshop-cloud-foundations-20260911`.
Base: reviewed clean candidate `1e17bf7006a1260df1ae5d1dcd0ec6d4c233b9ad`.
Main checkout and eval-infra candidate were not modified. No slot claimed.

## Implemented

- WI-235: `CoreRuntime::open` defers cloud resolution/transport construction;
  lazy failures cache redacted `CloudUnavailable`. No key and bad config cannot
  escape this boundary as a startup error. Reconfiguration disables the prior
  client before resolving replacement configuration. Unit runtime composition
  does not load personal config/.env sources.
- WI-230: existing Intern transport/models extracted to private
  `crates/synth-api-client`, with independent Cargo lock and tests. Desktop
  reexports preserve its internal call sites. Specta schema mirrors remain at
  the desktop IPC boundary. Caller injection supports explicit HTTP transport;
  default transport disables redirects. SDK has no Tauri, SQLite, Specta or
  credential-store dependency and performs no configuration/credential reads.
- Generation-zero regression, structured denial code/correlation/retry metadata,
  separate 401/403 classification. Backend free-form detail is excluded from
  displayed errors. Transient-error classification does not authorize mutation
  replay. Existing route subset and approval defaults retained; no new route
  or fresh contract pin inferred from the newer review.
- WI-233/234/236/237 preparation: additive SQLite **design fixture**, not a
  registered migration, tests account isolation, legacy rows remaining unbound,
  stale epoch rejection, transaction rollback/checkpoint ordering, replay
  dedupe, immutable outbox identity/body, outcome_unknown and reconciling state.
  Adapter checkpoint types keep Intern sequence/generation, swarm opaque IDs
  and MQ durable sequence separate. They are internal candidate representations,
  not selected wire DTOs or a replacement MQ SDK.
- Bounded SSE framing handles fragmented UTF-8, LF/CRLF/CR, comments, multiline
  data, opaque IDs, null-ID rejection and advisory retry. No live stream wiring.

## Evidence and limits

- `cargo test --manifest-path crates/synth-api-client/Cargo.toml --offline`:
  14 passing tests, none skipped (7 unit + 7 protocol/loopback HTTP tests).
- `python3 -m unittest discover -s tests/cloud-foundations -v`: 4 passing tests.
- Native `cargo check --lib --offline -j 2`: PASS.
- Native targeted `core_runtime::tests::local_journal_opens_with_unavailable_cloud_configuration`:
  PASS (1 passed, 1694 filtered out, none ignored). Hermetic config path and
  TMPDIR were under this worktree; no personal config or Keychain read.
  This proves Local journal composition after cloud initialization failure,
  not a packaged native agent/provider journey.
- Evidence: `artifacts/cloud-foundations/evidence.json`, `sdk-tests.log`,
  `storage-fixtures.log`, `native-startup-result.txt`, `sdk-dependencies.txt`.
  Full native build/test log remains at `artifacts/cloud-foundations/native-startup-test.log`.
- Initial native compile found an extraction orphan-rule issue in the binding
  conversion; converted the local concrete DTO instead of Option<DTO>.
- These results are offline/component evidence. W06/E06 packaged Local journey,
  all live Cloud gates and storage migration admission
  remain unqualified until their respective checks complete.

## Cloud handoff required

Before live binding or registering migration, supply the exact qualified
manifest with source/contract digest and operation subset, plus authenticated
backend-origin/account/org/profile identity, identity authority, expiry and
revocation behavior. A key hash is never an account identity. Specify whether
profile changes preserve identity or intentionally partition persisted state.

Need backend-authored fixtures for generation zero, first-send create+command
partial failure, duplicate command key/body conflict, receipt lookup after
accepted timeout, presence-required approval denial and auth vs entitlement
errors. Need Intern contiguous replay/retention reset, swarm opaque event IDs,
state versions and live-to-archive transcript cursors; MQ wake vs durable
sequence, subscription/grant identity and commit/ack/revocation semantics.
Use the existing MQ Rust SDK after its version and contract are qualified.

## Remaining integration work

The current live provider still has unscoped session discovery/reload, an
in-memory poller cursor ahead of ingestion, and restart receipt failure logic.
The new storage fixtures DO NOT repair those runtime paths. Integrate all scope,
checkpoint, outbox and external binding changes together after identity
qualification; fence writes by auth epoch, stop readers when commits fail, and
resume from the committed checkpoint. Legacy rows must not be adopted by a new
login. Local command receipts must not stand in for remote execution state.
Remote restart should reconcile against authority, not infer remote failure.

No new UI, live contract DTO pin, cloud route remount, migration registration,
paid provider run, Keychain access, push, merge, deployment or publication was
performed. Existing Local hosted-model sessions remain Local; Jesterky optional.
