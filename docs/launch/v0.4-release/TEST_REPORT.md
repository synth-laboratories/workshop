# v0.4 test report

## Passed

- Desktop frontend typecheck and production build.
- Visual unit/invariant suite: 152 passed.
- Accessibility and design-debt invariants: 251 passed.
- Frozen-head Playwright: 220 passed, 2 intentionally skipped.
- Craftax 100k-envelope performance acceptance: passed in 48.1 seconds after the test entered the new Replay and Raw trace surfaces explicitly.
- Rust workspace tests: 683 library tests plus all binary and integration suites passed. The real local Trace V5 bundle test and one documentation example remain explicitly ignored by contract.
- Protocol binding regeneration: passed with zero generated-file drift.
- Desktop contract drift: 195 commands, 10 event channels, and 2 origins matched.
- Desktop conformance script: passed.
- `cargo fmt --check`: passed after a mechanical formatting commit.
- `cargo clippy --all-targets --all-features`: passed with the repository's existing warning backlog.
- Banking77 producer contract: 5 unit tests passed; Ruff format/check and Python compilation passed.
- Public website: 17 release-catalog tests passed, generated-content typecheck passed, and the production build passed under the documented local auth bypass.
- Artifact signature, CDHash, ZIP checksum, ZIP round trip, and isolated install verification passed.
- Exact installed-artifact CUA passed on Workshop `9fffe8c8b5ede969b734118c04935fe42cc6baf1`; Desktop reported v0.4.0 and optimizer v0.2.14 as ready.
- Paid Banking77 GEPA v2 smoke acceptance passed with 140 scored rollouts, zero failed evaluations, and $0.0101283 actual spend against the $2.45 recipe ceiling and $20 operator authorization.
- The optimizer visual rendered terminal metrics, elapsed time, two candidates, the one-member Pareto frontier, all evaluation groups, and the complete Generation 0 proposer Trace V5.
- The chat advanced-trace panel opened the final Responses API v5 receipt, listed 250 recorded provider events, and rendered a selected event payload.

## Observations

- One full Rust run hit the timing-sensitive `dead_approval_origin_expires_and_drains_pending_state` assertion. The isolated test then passed five consecutive runs, and the final complete Rust suite passed.
- The first final Playwright run revealed that the performance test still assumed Replay was the default surface. The trace had fully replayed, but the transcript-first UI hid the lane controls. The acceptance now navigates the public surface controls before asserting them.

## Promotion-only remainder

- Upload the immutable ZIP, promote through `dev`, merge/tag, switch the production website to v0.4, and verify production route/checksum behavior.
