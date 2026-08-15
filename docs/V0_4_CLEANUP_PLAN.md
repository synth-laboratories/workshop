# Workshop v0.4 quality and integrity plan

This branch starts from `origin/dev` at `4c32e751` and is intentionally
independent of Josh's local `dev` branch. Do not push local `dev` as part of
this work.

## Release objective

Make Workshop's user-facing trust claims executable: source-owned routing,
native credential custody, verified evidence, bounded local inference, durable
approval state, and end-to-end tests that exercise the installed application
rather than a renderer stub.

## Tranche A — integrity-critical cleanup

- [x] Ignore mutable backend and gateway overrides outside named debug
  instances; require HTTPS except for loopback development.
- [x] Allow only source-owned Synth and Stripe billing destinations in release
  builds.
- [x] Keep ChatGPT refresh credentials in native custody and delete child auth
  material when a session closes.
- [x] Use one production approval broker and bound plugin approval waits.
- [x] Require revision-bound Report visibility approval; disable legacy direct
  share and promote commands.
- [x] Recompute content-addressed storage digests on read.
- [x] Escape Report JSON so no case variant of a script end tag can become
  markup.
- [x] Resolve Report trace projections only from validated Trace V5 evidence.
- [x] Page restored session history past the previous 2,000-event ceiling.
- [x] Gate MLX loading on reclaimable memory as well as installed capacity;
  reject incomplete shard sets before importing MLX.
- [x] Bound long-context prompt caches to two entries and remove the dead
  process-spawning Laguna runtime/shim modules.
- [x] Terminate the owned Laguna process group with bounded TERM/KILL
  verification.
- [x] Make Optimizers status refresh non-destructive and slow the UI poll.
- [x] Preserve numeric-string live-eval sequences and replay recovery from the
  durable origin so a fast lane cannot skip a slow lane.

## Tranche B — remaining release blockers

- [ ] Move ChatGPT session access behind a native loopback credential lease so
  long-running sessions can refresh without placing a refresh token in the
  shell-enabled child home.
- [ ] Bind OpenRouter leases to a source-owned provider origin rather than a
  renderer-selected base URL.
- [ ] Add continuous local-memory pressure monitoring during model load,
  prefill, and generation, with a tested emergency unload path.
- [ ] Add a parent-liveness contract for the detached Laguna daemon and verify
  crash/force-quit cleanup on an installed build.
- [ ] Verify provider checksums for every downloaded model shard, not only
  index completeness and declared aggregate size.
- [ ] Fix Synth Cloud revocation/cache semantics, enforce a host-side spend
  breaker, and correct cross-turn settled-cost attribution.
- [ ] Apply the Report script-termination corpus and private-report auth checks
  to the publication backend and public reader deployment.
- [ ] Replace remaining polling-driven lifecycle mutation with explicit state
  transitions and health hysteresis.
- [ ] Upgrade or remove the mock app's vulnerable Electron 35 dependency; the
  current npm audit reports a high-severity Electron/extract-zip chain.

## Tranche C — failing-capable evidence gates

The integration harness belongs in the separate `testing` repository. Its
first merge floor should cover:

1. Workshop launches and exposes the accessibility/eval-driver contract.
2. A deterministic local MLX turn reaches a terminal event and produces
   visible assistant output.
3. ChatGPT login state is observable without exposing tokens, and a bounded
   authenticated turn succeeds when the operator account is connected.
4. OpenRouter configuration and a bounded turn succeed without exposing the
   upstream key.
5. Report seal/reopen, approval-bound visibility, Visual creation, and a
   two-lane live-stream reconnect produce durable receipts.
6. Negative controls prove mutable release routing, digest tampering,
   unapproved publication, missing model shards, and low-memory admission fail
   closed.

## Qualification sequence

1. Run focused Rust, Python, visuals, typecheck, and production frontend-build
   gates on this branch.
2. Run the new integration suite against an isolated debug instance with
   deterministic provider fixtures.
3. Install the exact v0.4 candidate ZIP and repeat the core persona suite:
   fresh local-only, Synth Cloud, ChatGPT, OpenRouter, and local MLX.
4. Run the v0.1→v0.4 and v0.2/v0.3→v0.4 state migration matrix.
5. Publish a claim ledger containing artifact hash, test revision, environment,
   negative control, receipt, and verdict for every release claim.

No stable-channel promotion should happen while a tranche B security or
machine-safety blocker remains open.
