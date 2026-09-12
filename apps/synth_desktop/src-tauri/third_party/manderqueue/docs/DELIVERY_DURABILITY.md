# Atomic publish acceptance and leased delivery

This source slice is locally fixture-tested, not PostgreSQL or deployed-profile qualification.

`Fabric.publish` calls `Store.append_with_delivery`. Memory acceptance holds one mutex; PostgreSQL acceptance holds a thread row lock and commits the message, frozen recipients, and delivery jobs in one transaction. Per-thread sequence allocation is serialized. Publisher and recipient permissions are checked again inside that transaction. This does not make all participant/role lifecycle operations transactional.

Idempotency is scoped to organization, thread, sender kind, sender ID, and caller key. Acceptance stores canonical request semantics (including body, kind, payload, explicit recipients, and correlation/causation/parent IDs); changed reuse conflicts. An unchanged replay repairs missing jobs for the frozen recipient set without resetting settled jobs. Historical rows without an acceptance fingerprint refuse replay with `legacy_publish_unverifiable`. No historical payload is guessed or backfilled.

Workers claim one job at a time with a 30-second lease, use a 10-second HTTP timeout, and settle only the matching attempt generation while its lease remains valid. Pending retries use bounded exponential backoff. Crash or timeout can repeat bridge delivery, so backend control idempotency is still required. `dispatched` records runtime acceptance only; it does not prove actor consumption. `awaiting_pull` and `not_routable` retain the meanings in DELIVERY_SECURITY.md.

Product publication bypasses `BatchingStore`'s volatile buffer, including in local profiles. The legacy explicit append/enqueue/flush APIs and batching benchmarks remain experimental; they are not a durability or throughput qualification for this product path. Deployed profiles already reject buffered operation.

## Forward migration and rollout constraints

`20260911230000_delivery_acceptance.sql` is a new, unexecuted forward migration. Previous files/checksums are unchanged. It replaces the organization-global message idempotency index with the publisher/thread-scoped index, adds acceptance metadata, and adds lease/backoff fields. New source compiles against these fields; it must not be run against the old schema.

Mixed old/new writers or workers are unsupported: old writers omit acceptance metadata, and old workers ignore leases and cannot parse new statuses. A future authorized rollout must stop old publishers/workers, validate existing migration checksums and data/index assumptions on a disposable clone, apply the forward migration, and start coordinated binaries. Qualification must cover concurrent publishers, transaction failure, worker crash/reclaim, and bridge retries against PostgreSQL. The opt-in `postgres_atomic_acceptance_replay_and_worker_lease` regression must run only on an isolated disposable database.

Rollback cannot simply recreate the old unique index after different publishers reuse a key. Retain coordinated new binaries or stop traffic and prepare a reviewed data-aware rollback. No destructive downgrade is supplied. Memory checkpoints now emit version 2 with acceptance metadata; version 1 can be read but its old messages cannot qualify idempotent replay. Old binaries do not support version 2 checkpoints. Retain a pre-upgrade checkpoint separately when local recovery requires it.

No migration, database service, network delivery, deployment, or paid evaluation was executed for this slice.

## Membership mutation boundary

Product invites and role changes now call `Store.mutate_participant`: caller capability,
target organization, owner/peer protection, and mutation share the memory mutex or PostgreSQL
thread-row transaction lock. This serializes role changes with product publishes. A repeated
invite for the same existing role succeeds without changing it; a different role conflicts.
This makes recovery/timebox sender invitation retryable without converting invite into role
mutation. A demoted caller must pass the current membership check on its next operation.
Low-level storage setters remain trusted adapter APIs; do not expose them as product routes.
No owner transfer/removal API is introduced; the single owner remains protected.
