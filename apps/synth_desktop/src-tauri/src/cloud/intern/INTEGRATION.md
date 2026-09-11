# Rust Intern integration

Intern is integrated into CoreRuntime, desktop commands, ingestion and the
journal. The transport and wire models live in the private path dependency
`crates/synth-api-client`; this directory reexports them and owns pollers,
normalization, persistence and runtime lifecycle.

CoreRuntime opens Local storage without requiring cloud configuration. Its lazy
Intern runtime resolves caller-owned configuration on first cloud use, caches a
redacted CloudUnavailable state after failure, and can be reconfigured explicitly.
Reconfiguration disables the old client before resolving replacement settings.

Run transport fixtures independently of Tauri:

```sh
cargo test --manifest-path crates/synth-api-client/Cargo.toml --offline
```

From repo root, native integration tests still live at
`apps/synth_desktop/src-tauri/tests/intern_protocol.rs`. These compile the app.

The poller's in-memory cursor is not yet a commit acknowledgement. Scope/auth
epoch fencing, durable outbox, receipt reconciliation and external bindings must
be integrated together before qualifying live Cloud. Do not replay legacy rows
under a new account or interpret a lost local connection as remote failure.
See `docs/handoffs/2026-09-11-workshop-cloud-foundations.md` for design fixtures,
required cloud contracts and evidence limits. No live contract pin is implied
by the current transport subset or fixture passes.
