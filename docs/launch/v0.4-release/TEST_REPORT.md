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
- Exact installed-artifact provider smoke passed for all three supported paths: ChatGPT subscription (`V04_CHATGPT_PROVIDER_OK`), OpenRouter (`V04_OPENROUTER_PROVIDER_OK`), and managed local Laguna XS 2.1 (`V04_LOCAL_LAGUNA_OK`).
- Live local-sidecar fault injection passed. After the managed Laguna process was forcibly terminated, the next local request relaunched the sidecar and returned `V04_LAGUNA_RECOVERY_OK`.
- Live app-kill/restart acceptance passed. The exact installed v0.4.0 application relaunched after a forced process kill, and the paid Banking77 visual reopened with 140/140 rollouts, heldout/TPS/elapsed metrics, candidate/frontier state, and the complete 11-item/5-tool proposer trace intact.
- Focused deterministic fault coverage passed at the released product source (the test checkout differs from `v0.4.0` only by the v0.5 setup document): duplicate terminal usage settles once, duplicate completion/usage remains idempotent, a partially applied migration 8 recovers, and replayed optimizer events deduplicate.
- Production verification passed: the stable manifest reported `0.4.0`; the public ZIP returned HTTP 200 with `application/zip`, 19,360,702 bytes, and SHA-256 `a1f2e882ccc7ac4eeab31ce55b1548a11114cd6b3c10f5290a4e94cecaa114ec`.

## Observations

- One full Rust run hit the timing-sensitive `dead_approval_origin_expires_and_drains_pending_state` assertion. The isolated test then passed five consecutive runs, and the final complete Rust suite passed.
- The first final Playwright run revealed that the performance test still assumed Replay was the default surface. The trace had fully replayed, but the transcript-first UI hid the lane controls. The acceptance now navigates the public surface controls before asserting them.

## External acceptance boundaries

- A clean-account signup/payment rehearsal was not performed because it requires a separate customer identity and payment instrument. The paid product path itself was exercised through an existing test account and stayed within the authorized budget.
- Independent fresh-machine Gatekeeper review remains a human/device acceptance activity. The isolated installed copy, ad-hoc signature, ZIP round trip, and forced-restart checks are complete on the release Mac.
- The friends release is already promoted, tagged, published, and production-verified. These external checks do not change the recorded v0.4 product bytes.
