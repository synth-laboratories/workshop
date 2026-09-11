# Workshop cloud foundations — September 11, 2026

Workshop task: `01a091c5-9b09-75f3-ad94-2e5fdb6295a3`.
Cloud counterpart: `01a091c5-9b09-75f3-ad94-2e3d7be81763`.
Parent: `01a09132-34bc-7010-b309-26c341127bf3`.

Worktree: `/Users/joshuapurtell/GitHub/worktrees/workshop-cloud-foundations-20260911`.
Branch: `codex/workshop-cloud-foundations-20260911`.
Base: reviewed Workshop candidate `1e17bf7006a1260df1ae5d1dcd0ec6d4c233b9ad`.
The main checkout was not modified. All changes are local, unmerged candidates.

## Implemented foundations

- Local startup defers cloud configuration and transport construction. Failures
  become cached, redacted `CloudUnavailable`; replacing configuration disables
  the old client. Unit composition does not load personal credential sources.
- Private `crates/synth-api-client` owns the existing Intern client/models, typed
  errors and separate protocol checkpoints. It has no desktop framework, SQLite
  or credential-store dependency. The default transport refuses redirects.
  Generation zero is supported. Bounded SSE framing is available; live stream
  adapters remain subject to qualification.
- The existing Intern poller advances only after ingestion acknowledges the
  journal/checkpoint transaction. Cancellation interrupts HTTP and commit waits.
  Restart leaves uncertain remote command receipts accepted/reconciling while
  interrupting the abandoned local execution record.
- Native `cloud/storage` replaces the original Python SQL prototype. Its additive
  schema is compiled but **not registered**. Scope keys use verified backend
  origin/backend/account/org/profile identity, never a credential hash. Global
  epochs fence scoped reads/writes, new conversation ownership, external run
  bindings, histories, outbox and checkpoint transactions. Legacy rows are not
  adopted. Intern numeric, swarm opaque and MQ durable cursors remain distinct.
- The outbox extends shared command receipts and preserves exact request bytes,
  IDs, keys and generation. It commits uncertainty before an injected send,
  rejects duplicate claimants and stale/wrong receipts, and does not automatically
  retry unknown outcomes. Receipt application does not complete remote execution.
- Creation stores its original request and first-message intent before a remote
  runtime exists. An authoritative creation result binds the same draft and
  admits the first command atomically. Restart/account recovery preserves the
  original command epoch; resolving creation cannot silently re-authorize an old
  pending message. Async dispatch runs database work on blocking workers.
- Production scope activation requires a finite observation deadline, at most
  60 seconds away. Database fences enforce expiry independently of UI timers.
  Fresh verification of an unchanged, unexpired tuple can preserve the epoch;
  expiry cannot revive old workers. Credential replacement must invalidate first.
- The candidate identity client fetches `/api/v1/desktop/cloud-identity` afresh,
  requires `no-store`, bounds responses to 16 KiB and preserves denial status.
  Host validation checks exact schema/revalidation semantics, expected canonical
  HTTPS origin, UUID tuple and timestamps. Source contract: backend `5f9166a30`
  and compatible follow-ups. No live authority caller is wired.
- Explicit session kind determines execution location. A hosted model used by a
  native Codex session remains Local.

## Verified evidence

- Native combined cloud suite: **62 passed**, zero failed or ignored.
- Independent SDK suite: **16 passed**, zero failed or ignored.
- Native Local actor completion/reopen with cloud disabled: **1 passed**.
- Desktop instance contract suite passes, including credential-free preparation
  and an untracked-source provenance regression.
- Final non-test library check and source hashes are recorded in
  `artifacts/cloud-foundations/final-authority-evidence.json`.
- Earlier evidence: `evidence.json`, `commit-boundary-evidence.json`,
  `scoped-native-evidence.json`, `creation-recovery-evidence.json` in the same
  artifact directory. Those records describe their own earlier source slices.

## Packaged Local fixture

App candidate `0def6215a787`, bundle `com.synth.desktop.v09.dev.cloud-local-e06`,
was built and verified with ad-hoc signing. Existing MLX/optimizer distributions
were copied as immutable inputs and hash-verified by their staging scripts.
Browser runtime was explicitly omitted for this text-only fixture.

The first launch exposed native/launcher dirty-policy disagreement and correctly
refused EX_CONFIG. The launcher now includes untracked source just as build.rs
does, with generated test state/logs ignored. Credential seeding can explicitly
be disabled for a fresh instance, including provider, OAuth and optimizer paths.
Existing saved credentials are not erased by that option.

The corrected signed-out app started with empty provider .env and no saved OAuth
or optimizer credential. An API-driven local turn completed with a loopback
provider and no cloud fallback. A separate visible-composer request displayed its
result. After quit/relaunch, CUA verified the conversation and result again.
Evidence: `artifacts/cloud-foundations/packaged-local-evidence.json`.

This was a deterministic **FakeBackend fixture**, with explicit simulated memory
facts and no model weights. It is not real-model performance or full live E06
qualification. The unchanged real-model memory gate refused insufficient free
memory. The fixture app and daemon were stopped after verification. No paid
provider run or Keychain operation occurred.

## Remaining integration gates

A deployment owner must qualify the stable live backend/profile tuple, served
revision, fresh identity authority, revocation/account switching, and failure
behavior. No stable IDs or live profile were assigned by this task. An account
snapshot or source fixture is not authority. See the cloud counterpart packet:
`/Users/joshuapurtell/GitHub/testing/docs/internal/cloud-fundamentals-20260911/HANDOFF.md`.

After qualification, register the additive migration and integrate the scoped
repository/authority lifecycle into CoreRuntime, global discovery/history, UI
reset and worker cancellation. Existing legacy Intern live paths remain unscoped;
they must not substitute for that integration or enable new Cloud capability.

Remote creation/receipt lookup, replay retention/reset, stream reconnect and MQ
subscription/grant renewal require qualified operation contracts and live checks.
Use the existing MQ Rust SDK for MQ transport. Real-model packaged E06 and enabled
Cloud journeys remain unqualified. Jesterky remains optional.
