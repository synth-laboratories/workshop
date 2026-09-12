# Workshop grant contract (v0.11, WP5 items 3–6, WI-202)

**Contract version: 2** (2026-09-12).
- v1: enrollment, grants, grant credentials, history cursor and EdDSA/kid signing.
- v2: enrollment-wide revocation (device sign-out, §5.1); the enrollment
  object gains `revoked_at`; new code `enrollment_revoked`; the backend
  delivery bridge verifies the envelope `grant` field against live MQ state
  (§6.1); MQ reloads a changed `MQ_JWT_JWKS_FILE` without restart (§9).

Status: implemented in MQ branch `claude/workshop-v011-grants-mq` and backend
branch `claude/workshop-v011-grants`. This is the contract the Workshop desktop
consumes. Where this document and the code disagree, the code is wrong or this
file was not updated. Report the mismatch; do not code around it.

## 1. Roles and trust

| Party | Holds | May do |
| --- | --- | --- |
| Backend (Synth control service) | The only Ed25519 **private** signing key, from configuration. | Authenticates the Synth user, calls MQ as that user, and mints short-lived MQ credentials from live MQ state. |
| MQ (manderqueue) | Only a configured **public** JWKS. It never signs client credentials. | Persists enrollments and grants, and enforces them on every operation. |
| Workshop desktop | The user's Synth API key and short-lived MQ grant credentials. | Enrolls its device/session, asks the backend for credentials, and talks to MQ with a grant credential. It never sees a signing key. |
| Child agents / tool proxies | Nothing | Go through the Workshop proxy. They never hold a Synth key or an MQ signing key. |

Workshop never calls MQ administration endpoints directly. Every enrollment and
grant mutation goes through the backend (§3). Workshop calls MQ directly only for
thread operations, and only with a grant credential (§6).

## 2. Identities

- **Owner**: the Synth user, `{"kind":"human","id":<account_id>,"org_id":<org_id>}`.
  `account_id` and `org_id` come from a fresh database check of the API key
  (`services/desktop_cloud_identity.verify_desktop_cloud_identity`), never from
  the request body.
- **Enrollment**: one record per (org, owner, `device_id`, `session_id`). Its
  participant principal is **server-derived** as
  `{"kind":"actor","id":"enrollment:<enrollment_id>","org_id":<org_id>}`.
  The `enrollment:` id prefix is reserved. MQ refuses any credential for a
  reserved principal unless it is an asymmetric grant credential.
- **Incarnation**: a positive integer on the enrollment. Each enroll call for
  the same (owner, device, session) increments it. Only the current
  incarnation is valid: a new incarnation immediately fences every credential
  and issuance request carrying an older one.

## 3. Backend endpoints (what Workshop calls)

All endpoints require `Authorization: Bearer <Synth API key>`. They reply with
`Cache-Control: no-store`. Every one of them reverifies the key and org
membership against the database. The bodies are JSON.

| Method and path | Body | Result |
| --- | --- | --- |
| `POST /api/v1/mq/enrollments` | `{"device_id","session_id","label"?}` | `201 {"enrollment", "mq_endpoint", "identity"}` (new incarnation) |
| `GET /api/v1/mq/enrollments` | – | `{"enrollments":[...]}` (the caller's own) |
| `GET /api/v1/mq/enrollments/{enrollment_id}` | – | `{"enrollment"}` |
| `POST /api/v1/mq/enrollments/{enrollment_id}/revoke` | `{}` | `{"enrollment"}` with `revoked_at` set (device sign-out, idempotent) |
| `POST /api/v1/mq/grants` | `{"thread_id","enrollment_id","operations","ttl_seconds","history_after_seq"?}` | `201 {"grant"}` |
| `GET /api/v1/mq/grants?enrollment_id=&thread_id=` | – | `{"grants":[...]}` |
| `GET /api/v1/mq/grants/{grant_id}` | – | `{"grant"}` |
| `POST /api/v1/mq/grants/{grant_id}/revoke` | `{}` | `{"grant"}` |
| `POST /api/v1/mq/grants/{grant_id}/restore` | `{}` | `{"grant"}` |
| `POST /api/v1/mq/grants/{grant_id}/renew` | `{"ttl_seconds"}` | `{"grant"}` (extends the grant's `expires_at`) |
| `POST /api/v1/mq/grants/{grant_id}/credential` | `{"enrollment_id","incarnation","ttl_seconds"?}` | `{"mq_endpoint","token","token_type":"Bearer","expires_at","kid","grant"}` |
| `GET /api/v1/mq/jwks.json` | – (public, no auth) | JWKS: `{"keys":[{"kty":"OKP","crv":"Ed25519","x","kid","alg":"EdDSA","use":"sig"}]}` |

- `identity` is the verified desktop identity document:
  `backend_origin`, `backend_id`, `profile_id`, `account_id` and `org_id`.
  Bind the local enrollment cache to exactly these values. On account or org
  change, discard enrollments, grants and credentials.
- `mq_endpoint` is an MQ HTTP origin with no path, query, fragment or userinfo.
  It comes from backend configuration (`MANDERQUEUE_PUBLIC_URL`, falling back
  to `MANDERQUEUE_HTTP_URL`) and is never taken from the caller. Construct the
  MQ client only from this value (`mq_sdk::MqClient::try_new`).
- `ttl_seconds` on grant create or renew is the grant's lifetime, from 60 s to
  30 days (2,592,000 s). On `credential` it is the token lifetime, from 60 s
  to 300 s, default 300. The token's `exp` is `min(now + ttl, grant.expires_at)`.
  A grant close to expiry therefore yields a shorter token.
- `operations` is a nonempty, duplicate-free subset of `["read","publish"]`.
- `history_after_seq` is the history lower bound: the grantee sees only
  messages with `seq > history_after_seq`. It must be ≤ the current head
  sequence. **Default is the current head**, so the grant covers future
  messages only. Pass `0` to grant the whole history.

### Who may do what (enforced by MQ from live storage)

| Operation | Requirement |
| --- | --- |
| enroll / list / get / revoke enrollment | Caller is the enrollment owner (human). Another account gets `404`. |
| create grant | Caller owns the enrollment **and** currently holds the `invite` capability on the thread (owner/moderator). The thread and enrollment are in the caller's org. One grant per (thread, enrollment); a duplicate gets `409 grant_exists`. If the enrollment principal is not yet a participant, MQ adds it atomically as `member` (with publish) or `observer` (read-only). If it is a **revoked** participant, MQ refuses with `403 grant_membership_required`. |
| get / list grant | Caller owns the enrollment, or holds `invite` on the grant's thread. |
| revoke | Caller owns the enrollment, or holds `invite` on the thread. |
| restore / renew | Caller holds `invite` on the thread **now**. |
| credential issuance | Caller owns the enrollment; `incarnation` is current; the grant is active and unexpired; grantee membership is not revoked. |

## 4. The grant object

```json
{
  "grant_id": "uuid",
  "org_id": "string",
  "thread_id": "uuid",
  "enrollment_id": "uuid",
  "principal": {"kind": "actor", "id": "enrollment:<uuid>", "org_id": "string"},
  "operations": ["read", "publish"],
  "history_after_seq": 0,
  "expires_at": "RFC3339",
  "incarnation": 3,
  "generation": 0,
  "status": "active | revoked",
  "state": "active | revoked | expired",
  "granted_by": {"kind": "human", "id": "...", "org_id": "..."},
  "created_at": "RFC3339",
  "updated_at": "RFC3339"
}
```

- `incarnation` is the enrollment's **current** incarnation, read live.
- `generation` is server-owned. Revoke increments it; restore and renew never
  change it. So a credential minted before a revoke **stays dead after
  restore**; only a credential minted after the restore works.
- `state` is computed at read time: `revoked` if status is revoked, otherwise
  `expired` once `expires_at` ≤ now, otherwise `active`.

The enrollment object:
`{"enrollment_id","org_id","owner","device_id","session_id","label","principal","incarnation","revoked_at","created_at","updated_at"}`.
`revoked_at` is `null` until device sign-out. After that, every grant of the
enrollment has `state: "revoked"`.

## 5. Revoke, restore and renew semantics

- **Revoke**: sets status to `revoked` and increments generation. In the same
  transaction MQ dead-letters the grantee's pending delivery jobs on that
  thread. Every existing credential fails on its next operation, and open SSE
  streams send `event: revoked` at their next recheck (≤ 5 s) and close.
  Revoking twice is a no-op; generation increments once.
- **Restore**: sets status back to `active` with generation unchanged, so old
  credentials stay refused. An expired grant cannot be restored
  (`403 grant_expired`); renew it first. A revoked participant membership also
  blocks it.
- **Renew (grant)**: sets `expires_at = now + ttl_seconds`. It is refused on a
  revoked grant (`403 grant_revoked`), so a device that was offline when its
  grant was revoked cannot bring it back through renew. It is allowed on an
  expired, unrevoked grant, because it is the grantor re-authorizing.
- **Renew (credential)**: call `/credential` again before `expires_at`. It
  reads live state. After a revoke, expiry or new incarnation it fails with the
  matching code (§7). **Do not retry mutations automatically.** Revoke,
  restore, renew and create are not idempotent from the client's view. On
  transport uncertainty, re-read the grant (`GET`) and decide.
- Revocation cannot recall bytes already read. It stops future access,
  renewal and queued delivery.

### 5.1 Enrollment revocation (device sign-out)

`POST /api/v1/mq/enrollments/{id}/revoke` (MQ: `POST /v1/enrollments/{id}/revoke`)
is available to the enrollment owner only. In **one transaction** it:
- sets `revoked_at` on the enrollment;
- revokes every active grant of the enrollment on every thread
  (status `revoked`, generation +1);
- dead-letters every pending or leased delivery job for the enrollment
  principal, so a late worker settle is refused.

Afterwards every credential of every incarnation fails with
`403 enrollment_revoked`, and so do issuance, grant create/restore/renew and
re-enrolling the same (owner, `device_id`, `session_id`). Open SSE streams get
`event: revoked`. Revoking again is a no-op that returns the same `revoked_at`.
Sign-out is permanent. To sign back in, enroll with a **new `session_id`**,
which creates a new enrollment with a new principal, and ask the user for new
grants. Other enrollments of the same owner are unaffected.

## 6. Talking to MQ with a grant credential

Use `Authorization: Bearer <token>` against `mq_endpoint`. The credential
covers exactly one thread and the listed operations:

| MQ route | Operation |
| --- | --- |
| `GET /v1/threads/{thread_id}` | read |
| `GET /v1/threads/{thread_id}/history?after_seq=&limit=` | read (**use this for catch-up**) |
| `GET /v1/threads/{thread_id}/messages?after_seq=&limit=` | read (legacy array; see §8) |
| `GET /v1/threads/{thread_id}/events` (SSE) | read |
| `POST /v1/threads/{thread_id}/messages` | publish |

Every other route refuses grant credentials (`401`). That includes global
listing, thread creation, participant management and the grant/enrollment
administration routes.

On **every** request, and on every SSE recheck, MQ verifies these against live
storage, atomically with the data read or the publish commit:

signature and `kid`; `exp`; the principal matches the grant; the thread
matches; the operation is in both the token and the grant; the grant is
`active` and `expires_at` > now; the token generation equals the grant
generation; the token incarnation equals the enrollment's current incarnation;
the grantee participant exists, is not revoked and has the matching capability.

Queued delivery (MQ worker to the backend bridge) to an `enrollment:`
principal is checked again right before each dispatch. It needs an active,
unexpired grant with `read`, `message.seq > history_after_seq` and live read
membership, or the job is dead-lettered without dispatch. Envelopes for such
recipients carry `"grant": {"grant_id","generation","incarnation"}` so that
native acceptance can fence on them.

### 6.1 Bridge verification before acceptance

Authority can change between the worker's dispatch and the bridge's
acceptance, so the backend delivery bridge re-verifies every envelope for an
`enrollment:` recipient:

- It calls `POST /v1/grants/{grant_id}/delivery-check` on MQ with
  `{"generation","incarnation","recipient","message_seq"}`. It authenticates as
  `system:mq-delivery-bridge` in the recipient's org, using an **EdDSA**
  credential that only the backend issuer can mint. MQ refuses HS256, dev,
  owner and grant credentials on this route. MQ applies the §6 operation checks
  plus `message_seq > history_after_seq`, and answers
  `{"grant_id","generation","incarnation"}` or a §7 refusal code.
- The bridge requires the answer to echo the envelope's triple exactly.
  - A **live** grant gets receipt `status: "awaiting_pull"` (reason
    `grant_verified`). The Workshop device pulls through `/history`; the bridge
    never pushes into a device.
  - A **stale** grant (`grant_revoked`, `grant_generation_stale`,
    `grant_incarnation_fenced`, `grant_expired`, `enrollment_revoked`,
    `grant_operation_denied`, `not_found`, `grant_mismatch`) gets terminal
    `status: "not_routable"` with that `reason`. This is **not** an
    acceptance.
  - A missing grant on an `enrollment:` recipient gives reason
    `grant_required`; a grant on any other recipient gives `grant_unexpected`.
  - Verification uncertainty (MQ unreachable, issuer unconfigured) is a
    retryable `503`. It never accepts.
- Receipt-identity binding is unchanged: the receipt echoes `job_id`,
  `message_id`, `thread_id`, `recipient` and `attempts`. When the envelope
  carries a `grant`, the receipt must also echo that exact `grant`, otherwise
  the worker does not settle on it.

## 7. Error codes

Backend endpoints return `{"detail":{"code":...}}`. MQ returns `{"error":...}`.

| HTTP | Code | Meaning / client action |
| --- | --- | --- |
| 401 | `unauthenticated` (MQ) | Bad, expired or unknown-`kid` credential, or a wrong route for a grant credential. Request one fresh credential; if that also fails, stop. |
| 401 | `desktop_cloud_identity_revoked_or_unavailable` (backend) | Synth key invalid or membership removed. Sign out. |
| 403 | `grant_revoked` | Stop; show the grant as revoked. Do not flush the outbox. |
| 403 | `grant_expired` | Ask the user to renew the grant. |
| 403 | `grant_generation_stale` | The credential predates a revoke. Request a new credential; if that fails, treat as revoked. |
| 403 | `grant_incarnation_fenced` | Another process owns this device/session now. Stop this process. |
| 403 | `grant_operation_denied` | The operation is not in the grant. |
| 403 | `grant_membership_required` | The grantee participant is revoked or missing. |
| 403 | `invite_required` | The caller lacks `invite` on the thread (create/restore/renew). |
| 403 | `enrollment_owner_must_be_human` | Enrollment was attempted by a non-human principal. |
| 403 | `enrollment_revoked` | The device session was signed out. Stop, discard its credentials and outbox authority, and enroll a new `session_id` if the user signs in again. |
| 404 | `not_found` | A missing object, or one owned by another account or org. These look the same, so existence is not leaked. |
| 409 | `grant_exists` | A grant already exists for (thread, enrollment). List it. |
| 409 | `history_cursor_before_floor` | Legacy `/messages` read below the grant floor. Use `/history`. |
| 400 | `invalid_operations`, `invalid_ttl`, `invalid_history_bound`, `invalid_device_identity`, `invalid_incarnation` | Fix the request (MQ-side validation, passed through). |
| 400 | `recipient_grant_inactive` (MQ) | Directed publish to an `enrollment:` principal without a live read grant. |
| 422 | FastAPI validation (backend) | Malformed body, or an unknown field such as `principal`, `mq_endpoint` or `generation`. Every backend body forbids extra fields. |
| 502/503 | `mq_unavailable`, `mq_rejected_issuer`, `mq_issuer_unconfigured`, `mq_endpoint_unconfigured`, `mq_issuance_mismatch`, `mq_invalid_response`, `mq_error` | Server-side problem. Back off; never fall back to another endpoint. `mq_unavailable` on a mutation means the outcome is unknown, so re-read before acting. |

## 8. Granted-history cursor semantics

`GET /v1/threads/{thread_id}/history?after_seq=N&limit=L` (L from 1 to 200,
default 50) returns:

```json
{
  "thread_id": "uuid",
  "requested_after_seq": 3,
  "history_after_seq": 10,
  "effective_after_seq": 10,
  "skipped": {"after_seq": 3, "through_seq": 10, "reason": "before_grant_history"},
  "messages": [{"seq": 11, "...": "..."}, {"seq": 12, "...": "..."}],
  "next_after_seq": 12,
  "has_more": false
}
```

- `effective_after_seq = max(requested_after_seq, history_after_seq)`.
  `history_after_seq` is `0` for credentials without a grant.
- `skipped` is non-null **only** when the request asked for sequence numbers
  hidden by the grant. It names the exact hidden range
  `(after_seq, through_seq]`. The client records it as an intentional,
  authorized gap. The server never skips records silently.
- `messages` are contiguous: the first has `seq = effective_after_seq + 1`,
  and each next one is +1. Any other discontinuity is a real gap. Resync, do
  not paper over it.
- Store `next_after_seq` as the durable cursor, committed atomically with the
  inbox rows. It equals the last returned `seq`, or `effective_after_seq`
  when the page is empty. `has_more` means another page is available now.
- The legacy array endpoint `/messages` also honours the floor. With a grant
  credential and `after_seq < history_after_seq` it refuses with
  `409 history_cursor_before_floor` rather than returning a silently filtered
  array.
- SSE (`/events`) carries wake events only (`thread_wake`, `resync`,
  `revoked`). After a wake, fetch through `/history` from the stored cursor.
- Topics remain routing labels, not a privacy boundary, until every
  read/export/replay path enforces them. Use separate threads for separate
  confidentiality boundaries.

## 9. Credential format and signing keys (WI-202)

Grant credential (JWS compact form):

```text
header: {"alg":"EdDSA","typ":"JWT","kid":"<kid>"}
claims: {
  "iss":"synth-backend", "aud":"manderqueue", "iat", "exp" (exp - iat <= 300), "jti",
  "principal":{"kind":"actor","id":"enrollment:<uuid>","org_id"},
  "grant":{"grant_id","thread_id","operations":[...],"generation","incarnation","enrollment_id"}
}
```

Treat the token as opaque; Workshop needs only `expires_at` from the response.
MQ also accepts EdDSA **owner** credentials (the same header and issuer,
`principal` of kind human, no `grant`). The backend uses these for its own
administration calls; they are never handed to Workshop.

Verification in MQ (`crates/mq-server/src/auth.rs`):
- `MQ_JWT_JWKS` (inline JSON) or `MQ_JWT_JWKS_FILE` configures the public
  keyset. Only `OKP`/`Ed25519` keys with a unique `kid` are accepted;
  anything else fails boot.
- With `MQ_JWT_JWKS_FILE`, MQ (server and worker) re-reads the file every
  `MQ_JWT_JWKS_RELOAD_SECS` (default 30; `0` disables). It applies the file
  only when the content changes (SHA-256), so rotation needs no restart. A
  missing, unreadable, invalid or empty file is refused and the current keys
  stay active. Inline `MQ_JWT_JWKS` never reloads.
- The `kid` is required. An unknown `kid` refuses (`401`). Only
  `alg=EdDSA`, `iss=synth-backend` and `aud=manderqueue` are accepted on this
  path, with zero leeway.
- **Legacy HS256** (`iss=manderqueue`) still verifies **only if**
  `MQ_JWT_SECRET` is configured, as before. HS256 is never a fallback for
  grant credentials: an HS256 token carrying a `grant` claim is always refused,
  and so is an HS256 token for a reserved `enrollment:` principal.

**Rotation** (no downtime, and old tokens live at most 300 s):
1. Generate the new key. Add its public JWK to MQ's `MQ_JWT_JWKS` alongside
   the old one. With `MQ_JWT_JWKS_FILE`, atomically replace the file and MQ
   picks it up within `MQ_JWT_JWKS_RELOAD_SECS`; with inline `MQ_JWT_JWKS`,
   roll out MQ. Both kids now verify.
2. Point the backend at the new private key and `kid`. Keep publishing the old
   public key through `MQ_ISSUER_ADDITIONAL_JWKS` during the overlap.
   `/api/v1/mq/jwks.json` lists both.
3. Wait at least 300 s after the last old-`kid` token. Then remove the old
   kid from MQ and from the backend's additional JWKS. From then on, old-kid
   tokens refuse with `401`.

**Security review point**: while `MQ_JWT_SECRET` stays configured, anyone
holding it (backend or MQ) can mint legacy HS256 principal and `thread_scope`
tokens. The new scoped grant credentials do not depend on it. Removing
`MQ_JWT_SECRET` from MQ, once no legacy HS256 consumers remain, is the step
that makes MQ verification-only for every credential. This is a tracked
decision, not a silent waiver.

## 10. Deployment order

1. Deploy **MQ** with the migration `20260913000000_enrollments_and_grants`
   and `MQ_JWT_JWKS` configured, keeping `MQ_JWT_SECRET` for legacy callers.
   Older MQ builds do not understand `grant` credentials.
2. Deploy the **backend** with `MQ_ISSUER_SIGNING_KEY` (PKCS#8 PEM, Ed25519),
   `MQ_ISSUER_SIGNING_KID` and `MANDERQUEUE_PUBLIC_URL`. Until these are
   configured, the grant endpoints return `503 mq_issuer_unconfigured`.
3. Enable the Workshop consumer only after both are up.
