# MQ delivery security contract — first implementation slice

Companion backend contract: cloud-fundamentals-backend-20260911/notes/specifications/tanha/current/systems/platform/mq_delivery_security.md.

Source changes only; no deployed security or E01 qualification.

## Principal token thread restrictions

The Rust SDK exposes `set_participant_role`, including `Role::Revoked`, using
the authenticated PATCH route. Participant identifiers are encoded as single
URL path segments; empty and dot-segment identities refuse locally. HTTP errors
are preserved without automatic retry. An HTTP contract test checks reserved
characters, revoked serialization and a single request on forbidden response.
Workshop grant-editor wiring remains required.

Participant role `revoked` retains identity with zero capabilities. Existing
serialized role mutation authorizes revocation/restoration; owners cannot be
modified and revoked callers cannot restore themselves. Subsequent reads and
publishes refuse, and active SSE connections observe revocation on their next
authorization check (at most five seconds while idle). The HTTP/memory journey
verifies close and access refusal. Postgres uses its existing text role column
and derived capability storage; live Postgres round-trip remains unqualified.
This is membership revocation, not grant generations: restoring membership can
make an otherwise valid old credential usable again. Queued deliveries and
in-flight delivery still requires recipient-side grant fencing. Atomic message
acceptance already rechecks publisher and recipient capabilities while serialized
with role mutation. Align clients
with the expanded role enum before deploying.

Revocation now dead-letters all pending jobs for that exact thread/principal in
the role-change transaction, including leased jobs, and clears lease/retry times.
Late settlement fails its existing pending-status fence; explicit membership
restoration does not revive cancelled jobs. HTTP/memory tests cover one leased
and one queued job. Managed slot6 Postgres qualification also passes, including
a fresh store reload, revoked read/publish refusal, late settlement rejection
and no job resurrection after restoration. The disposable database was dropped.
This cannot recall a bridge request already sent
by a worker; native recipient grant-generation enforcement remains required.

A signed principal JWT may carry `thread_scope` with one UUID `thread_id` and
one or both unique `operations`: `read`, `publish`. The scope is an additional
restriction; existing organization and persisted membership checks still apply.
Unknown operations/fields, empty operations and duplicates refuse. A restricted
token cannot use global listing, create/ensure, invitation or role-edit routes,
even if its principal is an owner. Read covers thread metadata, messages and SSE;
SSE rechecks the scope with expiry and membership before wakes and while idle.
The principal-only resolver refuses restricted tokens so callers cannot discard
their restrictions. Existing unscoped credentials keep their existing authority.

This does not implement the full device-grant contract: issuer integration,
device/session/incarnation binding, grant IDs, persisted revocation generations,
renewal and queued-delivery fencing remain required before local participant
credentials can be qualified. No credential issuer was enabled by this change.

`backend_scope_contract` exercises Python `mint_mq_thread_bearer` against the
actual Rust HTTP router with a fixture key. It checks read-only/publish-only
tokens, wrong-thread/global refusal and exactly one accepted publish. Run with
`MQ_TEST_BACKEND_ROOT` pointing at the backend checkout and its `.venv`, using
`cargo test --locked --offline -p mq-server --test backend_scope_contract -- --ignored`.
It passes against backend b68f9ac36. This is in-process cross-language
conformance, not deployed authentication, device enrollment or revocation proof.

The sole worker ingress is `POST /internal/mq/v1/delivery`; `/v1/delivery` is removed.
Both main and local app compositions use this authenticated router. Authentication runs
before the database dependency. Missing/short configuration returns 503; absent or
invalid worker credentials return 401. This route is excluded from public OpenAPI.

The dedicated `MQ_DELIVERY_JWT_SECRET` must contain at least 32 bytes and must not be
an organization API key, provider key, general MQ principal signing key, or sandbox key.
Only worker and backend ingress receive it. Provisioning/rotation and transport/network
configuration require a separately admitted deployment. No credentials were provisioned.

Worker signs exact UTF-8 JSON request bytes with HS256: issuer `manderqueue-worker`,
audience `synth-mq-delivery`, subject `mq-worker`, random `jti`, integer `iat`/`exp`,
maximum lifetime 60 seconds, method `POST`, exact path, and lowercase hex SHA256 in
`body_sha256`. Ingress requires all identity/time claims and verifies the body digest.
Retries obtain new short-lived tokens but retain message/job identities. Backend checks
recipient organization against the run-thread binding, including non-actor recipients.

`dispatched` means the runtime returned matching runtime/message IDs, event ID and
acceptance time. It does not mean consumed, acted, or answered. `awaiting_pull` applies
to non-actor recipients. `not_routable` means no run binding. Worker stores these states
separately; unknown or legacy success bodies retry, eventually dead-letter. No stub
settlement is available. HTTP has a 10-second deadline and does not follow redirects.

The existing SQL status column is TEXT; new states require coordinated worker/readers.
No migration files or deployed checksums were changed. Rollback to older readers after
new states are written is not qualified; drain/coordinate versions before deployment.

`MQ_PROFILE` defaults to `deployed`; only `local` and `deployed` are accepted. JWT is
the default auth mode. Explicit `MQ_AUTH=dev` requires `MQ_PROFILE=local`; off/empty/unknown
modes fail. Deployed MQ requires Postgres and `MQ_WRITE_BUFFER=off`. Principal JWTs require
issuer/audience/expiry/token ID and nonempty principal/org identities. Extra backend JWT
claims cannot override issuer, identity, lifetime or token ID. HS256 asymmetric rotation
(WI-202) is still unresolved and this patch does not waive it.

## Remaining gates

A token may be replayed during its 60-second validity window. Stable runtime message IDs
and `client_request_id=job_id` now survive retries, but durable one-action consumer dedupe
is NOT qualified: the Horizons SQL fallback still appends controls on repeated ingress.
No claim of one-time token consumption or exactly-once runtime action is made.

WI-200 remains partial until replay/durable dedupe and authorized deployed exposure/wiring
checks pass. WI-201/203 remain partial: ready/migration/image verification, actual deployment
configuration and rotation are not qualified. WI-204 fixes owner/peer role mutation and
peer invitations at the Fabric boundary, but transaction-time revocation/races and the
full lifecycle remain open. WI-214/218 only gain truthful states and stable request identity;
full receipts, leases/backoff, replay fencing and dead-letter management remain open.

WI-104 remains a critical blocker: agent sandbox materialization still injects broad org
and worker credentials. No real-credential research profile is admitted by these changes.
Atomic MQ append/outbox, concurrent sequencing/fingerprints, Intern attribution, descendant
stop, lease-safe pool cleanup, cancellation usage and E01 are still implementation work.

Atomic acceptance, replay, leases, and the unexecuted forward migration are specified in [DELIVERY_DURABILITY.md](DELIVERY_DURABILITY.md).

Bridge replies must match the signed request's job_id, message_id, thread_id,
recipient and attempts before the worker accepts their transport disposition.
Status-only, missing-identity and wrong-attempt replies are unverified and use
the bounded retry/dead-letter path. Deploy the identity-bearing backend first.
This correlation does not turn dispatched into runtime consumption or answered.
