# Rust Intern integration

This directory is intentionally compile-tested without runtime wiring. Integrate it after the
current `CoreRuntime` convergence changes settle:

1. Add `mod cloud;` beside the other modules in `src/lib.rs`.
2. Add `intern: Arc<cloud::intern::InternRuntime>` to `CoreRuntime`. In `CoreRuntime::open`, call
   `synth_config::resolve()` and construct `InternRuntime::configured(...)` when `api_key` exists;
   otherwise use `InternRuntime::unconfigured()`. Expose `pub fn intern(&self)`.
3. When backend settings change, call `InternRuntime::reconfigure` (or `disable`). This shuts down
   old pollers before replacing the credential and endpoint.
4. Build the shared Session/Run service above `InternRuntime`; renderer commands must call that
   service, never `InternClient` directly. Start pollers with the journal's persisted remote cursor.
5. Consume `PollUpdate::Events` in order. Convert each `NormalizedInternEvent` to
   `storage::EventAppend` (`EventSource::Intern`) and call `CoreRuntime::append_and_broadcast`.
   Persist the session cursor/state generation in the same database transaction before advancing
   the durable cursor. Treat the update's `next_sequence` only as a candidate until commit.
6. Apply `PollUpdate::Projection` through the same Session service. Project statuses/checkpoints to
   SQLite, then append their journal event; do not create a second renderer-side state machine.
7. On `PollUpdate::Stopped`, project `authentication_failed` as configuration-required and stop.
   Retry updates are diagnostics, not synthetic mailbox events.
8. Call `InternRuntime::disable` during app shutdown. There must be one `InternPoller` owned by the
   core, not one per Tauri command.

Run the isolated contract suite with:

```bash
cd apps/synth_desktop/src-tauri
cargo test --test intern_protocol
```
