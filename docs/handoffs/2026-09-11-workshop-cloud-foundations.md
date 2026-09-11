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

The production poller now waits for ingestion's post-commit acknowledgement before
advancing its candidate cursor. Storage failure drops the acknowledgement and stops
the reader; cancellation interrupts pending HTTP/commit waits. Existing transaction
and replay tests plus loopback cursor/ack and failed-consumer tests pass.

Restart no longer terminally fails an accepted Intern receipt. The existing receipt
remains `accepted` with `deliveryState=outcome_unknown` and
`remoteExecutionState=reconciling`; the abandoned *local* run is interrupted with an
explicit remote-reconciling outcome. The original command can still be resolved by
an authoritative response. This is not an implemented remote receipt lookup loop.

Unscoped session discovery/reload, verified account epochs, durable command outbox,
external run bindings and the registered storage migration remain pending. The cloud
handoff at `/Users/joshuapurtell/GitHub/testing/docs/internal/cloud-fundamentals-20260911/HANDOFF.md`
explicitly supplies no qualified backend/account/org/profile tuple or revocation SLA
and requires migration/live binding to remain disabled. Its account snapshot org
fallback is not sufficient identity authority. No new cloud binding is enabled here.

Follow-up native evidence: 13 polling/ingestion tests and 1 restart test passed with
no ignored tests. `artifacts/cloud-foundations/commit-boundary-evidence.json` records
source hashes and concise test results; full logs remain beside that artifact.
The initial evidence above belongs to commit `ce5ea5db` and remains historical.

No new UI, live contract DTO pin, cloud route remount, migration registration,
paid provider run, Keychain access, push, merge, deployment or publication was
performed. Existing Local hosted-model sessions remain Local; Jesterky optional.

## Native repository and Local follow-through

The prior Python SQL prototype is superseded by `cloud/storage`, a native
Database-backed repository with 17 passing tests. Candidate schema remains
unregistered. Scope ownership, boot/account epochs, scoped history, external
execution bindings, atomic journal/checkpoint CAS and immutable exact-byte
outbox now exist behind that gate. Outbox extends shared command receipts;
received/delivered never imply remote execution completion. Async dispatch
runs SQLite work on blocking workers and persists outcome_unknown before send.
Timeout, cancellation and restart preserve original command identity for recovery.

Candidate identity validation pins backend `5f9166a30`: exact schema and fresh
revalidation contract, canonical HTTPS origin bound to the expected transport,
UUID tuple and at-most-60-second observation window. No live caller activates
storage. Identity observations are not authorization leases.

Combined native cloud suite: 56 passed, zero failed/ignored. Native Local actor
fixture completes a turn and reopens completed results with cloud disabled:
1 passed. This is component evidence, not packaged E06 qualification.
Launcher now accepts `SYNTH_DESKTOP_SEED_CREDENTIALS=0` to skip personal provider
and ChatGPT seeding for fresh instances. Existing saved credentials are not
removed. Full desktop instance contract regression passes, including no-seed
preparation and clearing inherited OAuth seed/state paths. Ad-hoc signing remains
the explicit Keychain-free packaging option. Evidence is recorded in
`artifacts/cloud-foundations/scoped-native-evidence.json`.

Still outstanding: packaged Local E06, qualified identity activation and registered
migration, live scoped routing and UI epoch resets, creation/first-send recovery,
and qualified remote reconciliation/stream transport integration. These are not
claimed complete by the native repository or fixture tests.

## Packaged Local fixture journey

Candidate `0def6215a787` builds and ad-hoc-verifies the named
`com.synth.desktop.v09.dev.cloud-local-e06` app. Browser runtime is explicitly
omitted for this text-only fixture. The pinned MLX and optimizer distributions
were copied as immutable inputs and validated by their staging scripts.

The first packaged launch exposed native/launcher provenance disagreement:
build.rs included untracked source, the launcher excluded it. The app refused
EX_CONFIG. The launcher now uses the same untracked-source policy; generated
test outputs are ignored, and a new source-file regression passes.

With the corrected candidate, a fresh signed-out app starts with empty provider
.env and no OAuth/optimizer credential. The native Local turn completes against
the repository FakeBackend through authenticated loopback. A separate request
through the visible composer displays its result. After quit/relaunch, the
conversation and result remain visible. CUA inspected both rendered states.
Evidence: `artifacts/cloud-foundations/packaged-local-evidence.json`.

This is a packaged **fixture** journey, not a real-model performance or full live
E06 qualification. FakeBackend loads no weights and uses explicitly simulated
memory facts. The unchanged real-model memory gate refused insufficient free
memory. No cloud provider run or Keychain operation occurred. Harness requests
now inherit the actual machine permission policy and explicitly select the local
provider for both creation and sending.

## Durable creation and first-send candidate

The native candidate now persists creation and first-send intent before a remote
runtime exists. One authoritative creation result binds the original draft and
admits the original first command atomically. Timeout/cancellation remains
outcome_unknown; retry cannot regenerate a creation key. Restart/account recovery
preserves the original first-command epoch, so an observed creation result does
not authorize automatic sending of an older pending message. Shared command
receipts track creation separately from remote execution completion.

Combined native cloud suite: 60 passed, zero failed/ignored. New tests cover
injected transaction failure, exact request preservation, mutation conflict,
restart/account fencing and timeout after the durable claim. Evidence:
`artifacts/cloud-foundations/creation-recovery-evidence.json`. No live creation
adapter, receipt lookup contract, migration registration or cloud capability was
enabled. The packaged app and deterministic daemon were stopped after verification.
