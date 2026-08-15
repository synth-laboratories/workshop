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

- [x] Keep ChatGPT refresh authority in the native host, issue only a bounded
  access credential to the shell-enabled child, delete it on close, and rebind
  a long-running session when the native host refreshes access.
- [x] Bind OpenRouter leases to a source-owned provider origin rather than a
  renderer-selected base URL.
- [x] Add continuous local-memory pressure monitoring during model load,
  prefill, and generation, with a tested emergency unload path.
- [x] Add a parent-liveness contract for the detached Laguna daemon and an
  emergency process exit when active native work cannot release Metal memory.
- [x] Verify provider checksums for every downloaded model shard, not only
  index completeness and declared aggregate size.
- [x] Fix Synth Cloud revocation/cache semantics, enforce a host-side spend
  breaker, and correct cross-turn settled-cost attribution.
- [x] Apply the Report script-termination corpus and private-report auth checks
  to the publication backend (`josh/v0.4-report-auth`, `98f8ab0fd`).
- [x] Remove destructive optimizer status polling, require the live sidecar
  capability handshake, and bound approval/status waits.
- [x] Upgrade the mock app off vulnerable Electron 35; the complete npm audit
  now reports zero vulnerabilities.

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

## Branch evidence

- Workshop: `josh/v0.4-cleanup`, based on `origin/dev` `4c32e751`; Josh's local
  `dev` branch is neither modified nor pushed.
- Publication backend: `josh/v0.4-report-auth`, commit `98f8ab0fd`, based on
  backend `origin/dev` `565e30f8`.
- Rust: complete crate suite, including 490 passing library tests, command binaries,
  the real Intern wire-contract integration test, and generated-protocol drift.
- Laguna: 234 daemon tests pass with 39 hardware/live-provider skips; the
  lifecycle, memory-pressure, parent-ownership, and provider-checksum controls
  are deterministic unit tests.
- Renderer: typecheck, 116 Visual tests, 192 accessibility/source invariants,
  186 passing Playwright end-to-end tests with 2 explicit prerequisite skips,
  production Tauri build, mock Electron build, and zero-vulnerability audit.
- UI fault injection also closed stale bridge fixtures, pre-answer activity
  placement, terminal-layout persistence, semantic Visual affordances, and a
  transient WCAG contrast failure caused by the visual-pane entrance opacity.
- Backend: 14 Report upload/reader tests and Ruff checks pass.

The separate pinned integration task owns installed-app/provider qualification.
It must distinguish a missing credential/model prerequisite from a product
failure, grade exact assistant output, inspect the accessibility/eval-driver
surface, and scan finalized receipts for secret material.
