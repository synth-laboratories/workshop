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

`activate_verified` requires an already verified origin/backend/account/org/profile
tuple. It validates local shape, not remote truth. The cloud authority adapter is
still missing. Opening the service, switching identity and signing out invalidate
old leases. Old outbox requests survive; they cannot silently flush under a new
auth epoch. Scoped reads fail for stale leases. Legacy sessions are never adopted:
only `create_conversation` can allocate a new scope-owned conversation, and all
additional stream bindings must reference such a conversation.

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
