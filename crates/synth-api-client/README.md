# synth-api-client (private)

Extracted Workshop Intern transport. `publish = false`. Caller supplies keys,
endpoint and optionally a configured reqwest client; this crate never reads
configuration, environment variables, credentials stores or the filesystem.

Run `cargo test --manifest-path crates/synth-api-client/Cargo.toml --offline`.
Existing fixture routes reflect the Workshop base, not a newly qualified live
contract. See `docs/handoffs/2026-09-11-workshop-cloud-foundations.md` at repo root.

`is_retryable` classifies transient failures for polling. It is not permission
to replay a mutation. Durable outbox/reconciliation remain desktop-owned.
Checkpoint structs are adapter-specific internal persistence representations,
not generated backend DTOs. MQ transport will reuse the existing MQ Rust SDK.

The candidate `identity_observation` request fetches
`/api/v1/desktop/cloud-identity` afresh, sends and requires `Cache-Control: no-store`,
rejects redirected successful responses and bounds identity documents to 16 KiB.
It preserves 401/403/503 classification without exposing backend response text.
Wire timestamps and IDs are validated by the desktop authority layer. Receiving
this document never registers a migration or grants a storage scope; a live,
revision-bound profile must first be qualified. The method does not load credentials.
