# Scoped cloud storage

Native SQLite service for WI-233/234/236/237. The SQL migration candidate is
compiled into Workshop but is deliberately absent from the migration registry.
`CloudStore::open` fails when the candidate has not been explicitly installed.
Only isolated native tests install it today; CoreRuntime does not activate it.

The service owns scopes, a global monotonic auth epoch, explicit new conversation
ownership, external stream/run bindings, outbox requests and checkpoints. It reuses
the desktop event journal and shared command-receipt records. Namespaced local
receipt/event IDs prevent identical remote IDs in different accounts colliding.
No credentials are stored, hashed into identity, read from the environment or
loaded from Keychain by this service.

`activate_verified_until` requires an already verified origin/backend/account/org/profile
tuple and a future observation deadline no more than 60 seconds away. It validates
local shape, not remote truth. The unlimited helper exists only in unit tests.
The live authority adapter is still gated. Opening the service, switching identity and signing out invalidate
old leases. Old outbox requests survive; they cannot silently flush under a new
auth epoch. Scoped reads fail for stale leases. Legacy sessions are never adopted:
only explicit conversation creation can allocate a new scope-owned conversation, and all
additional stream bindings must reference such a conversation.

`create_local_mq_conversation` explicitly allocates a fresh Codex conversation
and an MQ thread binding in one transaction. Its inference target may be local,
remote or gateway-backed; session execution stays Local. The thread ID never
becomes a Codex runtime ID. Same-target retries reuse the binding, while a
different target or an Intern-bound thread refuses. Account epochs fence both
creation and inbox reads. This does not adopt existing local/legacy sessions,
issue device grants or start a turn; explicit existing-session connection and
the restricted dispatcher remain required for the complete product flow.

`dispatch_once` persists the exact body, key and generation; atomically changes a
pending request to outcome_unknown before invoking an injected transport; checks
receipt identity; then records the response under the current epoch. Concurrent
claimants have one winner. Timeout/cancellation never invents a new key or replays
an uncertain send. Recovery must use an authoritative receipt lookup; no live
lookup or automatic retry policy is guessed here. Received/delivered are not
completed in the shared receipt store. Applied completes the command, not its run.

`commit_page` compares the expected checkpoint and commits events/checkpoint in one
transaction. Replays require identical content; unknown old event IDs, sequence
gaps and generation regressions fail. Intern generation zero is valid. Swarm
cursors remain opaque. MQ wakes cannot advance a durable checkpoint. All histories
and external-run resolution require an active scope lease. Remounting a view only
reads bindings; it does not mutate execution state. Only explicit authoritative
observations change remote run state; boot/sign-out changes nonterminal cached
states to reconciling while preserving terminal results.

`dispatch_once` runs admission and receipt persistence on blocking database workers
so SQLite waits do not block the async executor. Other synchronous repository reads
and writes must also run on host database workers when wired into UI flows.
The injected dispatch adapter, receipt lookup, account-expiry
handling, global renderer history filtering and epoch-aware view reset still need
host integration after the identity contract is qualified. Do not call the existing
unscoped Intern reload path as a substitute. No new route/DTO selection or live
cloud authorization is implied by this implementation.

MQ page commits also insert `cloud_mq_pending_inputs` in the event/checkpoint
transaction. Pending inputs remain separate from message execution or answered
receipts and survive restart. Reads require the current scope lease and exact
session binding; replay does not duplicate the queue entry. `accept_mq_input`
atomically creates an idempotent `mq.input` command receipt from the persisted
message and records its command ID on the queue entry. A failed transaction
leaves the message pending and creates no command. Repeated acceptance returns
the same command, and the original message remains stored for recovery/audit.
`mq_input_commands` recovers those command receipts in bounded sequence pages
under a freshly verified scope lease after restart. It preserves command status
and performs no acceptance or execution. The dispatcher must reconcile uncertain
execution rather than resubmit simply because a receipt exists.
This is durable handoff, not execution or an answered receipt. The schema remains
an unregistered migration candidate. Turn-boundary dispatch, restricted tool
policy, grant validation and the network adapter are still required before
activating this path in the product.

Run from the repository root:

```sh
TMPDIR="$PWD/.test-tmp" SYNTH_DESKTOP_CONFIG="$PWD/.test-tmp/config.toml" \
  cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml \
  --lib cloud::storage --offline -j 2
```

Fixtures use the real native database and migration code. They exercise interrupted
upgrade rollback, existing rows, account/backend/org/profile separation, old epochs,
concurrent claim races, timeout/restart, late/wrong receipts, transaction failure,
replay identity, cursor adapters, external ownership and native execution location.

Creation recovery now stages the exact creation body/key and first-command
body/key/generation with a fresh scope-owned draft before any network call.
An uncertain create is never automatically replayed. An authoritative creation
result binds that same draft and enqueues its first command in one transaction;
any binding/outbox failure rolls the entire operation back. After restart or
account revalidation, lookup may resolve the original creation, but the first
command retains its original epoch and cannot silently flush under the new one.
Creation delivery uses the same shared receipt store; it does not complete a run.
The injected creation dispatcher uses blocking database workers and rejects
receipt identity drift. Live creation lookup/retention semantics remain gated.

Every database fence also checks observation expiry, independent of UI timers or
worker cancellation. A fresh verified observation for the unchanged tuple may
renew an unexpired epoch with `refresh_verified_until`. An expired observation
cannot revive its epoch. Credential replacement must invalidate the scope before
renewal, even for the same account. Remote operations still require fresh server
revalidation: observation freshness is not an authorization lease.
