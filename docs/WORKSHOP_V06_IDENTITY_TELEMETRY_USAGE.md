# Workshop v0.6 identity, telemetry, and usage operations

This document freezes the v1 ownership and operating decisions used by the
Desktop implementation. Shared wire types live in
`contracts/workshop-v06.ts`; Desktop-generated IPC types remain subordinate to
that contract.

## Authorities

| Concern | Authority | Desktop responsibility |
| --- | --- | --- |
| Signup and verification | Clerk through Workshop web | Open the system browser; never implement passwords. |
| Device session | Workshop `/api/auth/device/*` | Poll the single-use exchange and keep approved material host-only. |
| Account and entitlements | Synth Cloud desktop account snapshot | Cache briefly, show freshness, and fail closed for hosted actions. |
| Hosted usage and quota | Synth Cloud account snapshot/usage service | Present returned totals; never rebuild them from local telemetry. |
| Local usage | Desktop `usage_records` ledger | Label as this-device data and never merge it into hosted allowance. |
| Optional analytics | Desktop product-telemetry dictionary | Enforce consent, allowlist, retention, and prohibited-data rules. |
| Release identity | Release provenance plus ZIP SHA-256 | Emit an immutable `releaseId`; never put credentials in URLs. |

Authentication uses the established system-browser device flow for v0.6.
OAuth 2.1 with PKCE may replace its transport later without changing renderer
states. A fresh install starts in explicit local-only mode. Local workflows do
not call identity or entitlement services.

## Credential and failure rules

- The Rust host is the sole credential custodian. Renderer IPC exposes only
  display-safe account state.
- Provider/session secrets use the OS credential store through one adapter.
  SQLite contains metadata and opaque references only.
- Sign-out removes Desktop-managed session material, clears cached private
  account data, and reloads hosted runtimes fail-closed.
- `auth`, `entitlement`, `quota`, `outage`, and `malformed` are separate stable
  failure classes. None may include response bodies, credentials, prompts, or
  paths.
- Revocation becomes signed out for hosted capabilities while local data and
  local workflows remain available.

## Telemetry policy

The event dictionary, policy version, retention, and sync eligibility live in
`contracts/telemetry-v1.toml` — the single source of truth. Desktop embeds it;
the backend registry (`app/api/v1/routes_product.py`, product `workshop`)
mirrors its sync-eligible names and must never be broader.

Consent is three-state and honest: never-asked is distinct from a recorded
choice, every choice pins the collection-policy version it was made under, and
bumping `collection_policy_version` re-asks on every install. The first run
asks once ("Share anonymous usage stats?"); until answered, optional events
record locally and nothing syncs. Allowing enables sync; declining disables
optional analytics and deletes queued events. The same choice is changeable
under Settings → Privacy, which also shows the recorded choice with its date,
last sync time, and a "View collected events" transparency list.

Sync ships only consent-granted, sync-eligible optional events, in idempotent
batches (`pte_` client ids) to `POST /api/v1/product/usage-events` on the
profile's backend, via a background flusher that re-checks consent on every
pass. Essential recovery events are local-only regardless of consent and are
retained for 365 days; optional events for 90. Sign-out deletes optional
events. Account-deletion requests are routed to the Synth privacy owner;
financial/audit usage records follow their separate legal retention policy.

Feature code cannot send arbitrary renderer dictionaries: event creation is
host-owned and every name, field, scalar type, and outcome is validated before
storage. Unknown or nested properties are rejected.

Never collect prompts, outputs, datasets, report text, reasoning, local paths,
filenames, tokens, authorization codes, credentials, cookies, arbitrary tool
arguments, or response bodies. Crash reports, logs, and diagnostic exports use
the same prohibition. Diagnostic export is a separate, explicit user action.

## Hosted usage semantics

Hosted runtimes reuse an idempotency key across retries. The usage authority
stores decimal quantities and currency as strings. Pending records do not enter
finalized cost. Finalization and correction preserve lineage; a correction
references the prior record and is applied once. Late records update the period
containing `occurredAt`, not the receipt period.

Every aggregate includes source period, timezone, generation time, freshness,
pending-record count, totals, and limits. Aggregation failure returns the
explicit `unavailable` variant. Desktop must never render that state as a fresh
zero. Account and workspace authorization applies to aggregates and run-level
evidence.

## Support runbooks

### Identity outage

Confirm `failureKind=outage`, preserve the last safe snapshot as stale, keep
local mode available, and offer retry. Do not ask for research content or an
authorization header.

### Expired or revoked session

Confirm `failureKind=auth`. Clear the rejected session, stop hosted launches,
and offer browser sign-in. Verify local runs remain available.

### Incorrect entitlement or exhausted quota

For `entitlement`, capture account/workspace fixture IDs and the snapshot
timestamp. For `quota`, capture period, limit, remaining value, and reset time.
Do not infer either condition from analytics or local run history.

### Usage delay or dispute

Capture aggregate generation time, period, freshness, and permitted usage
record IDs. Replay the same idempotency keys in the backend test environment.
For a dispute, preserve correction lineage and price version; never edit a
finalized record in place or request prompts/outputs.

## Migration and rollback

Contract changes are additive within v1. Breaking changes require v2 types and
dual-read/dual-write acceptance before switching consumers. Rollback restores
the prior reader while leaving newer immutable usage records intact. Telemetry
collectors reject unsupported versions rather than best-effort parsing them.
Release receipts pin the Workshop commit, tree, executable, frontend, ZIP
digest, CDHash, contract version, and acceptance timestamp.

## Release acceptance

Before release, run `npm run desktop:verify`, build from a clean committed tree,
produce provenance through `scripts/release-artifact.sh`, install only the
verified archive, and execute `scripts/auth-pre-release.sh` with the approved
live Clerk driver. The live driver must cover new/existing users, expiry,
revocation, offline startup, sign-out, one metered hosted operation, and a local
operation after every hosted failure. Attach screenshots and machine-readable
receipts to the exact release ID.
