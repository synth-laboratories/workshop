# Deterministic / integration / fault-injection gate sequence

Run on **clean, pinned revisions**. Copy-pasted pass counts from earlier commits are not receipts.

## Minimum sequence (WP7)

```bash
# 0. Identity
git -C workshop-v0.1 rev-parse HEAD
git -C evals rev-parse HEAD   # must be the isolated workshop evals branch, not intern dirt
git -C backend rev-parse HEAD

# 1–2. Evals harness + negative control
cd evals/workshop
npm test
npm run typecheck
npm run gate:negative-control -- --workshop /path/to/workshop-v0.1

# 3–6. Workshop static + Node + Playwright + Rust
cd /path/to/workshop-v0.1
# typecheck / lint as shipped in the Desktop package
# full Node unit suite (no skip/fixme/todo/expected-fail launch-debt)
# full Playwright suite
# full Rust suite: cargo test -p synth_desktop (or the crate name as shipped)

# 7. Desktop build/verification wrappers (unsigned OK for this step; Gate F still needs signed)

# 8. Backend
cd backend-desktop-account-snapshot   # or the isolated release backend
pytest -m units tests/units/test_fake_autumn_checkout.py \
  tests/units/test_desktop_account_snapshot.py \
  tests/units/test_public_inference_gateway_settlement.py
# then contracts + integration against synth-integration-scratch Postgres
# then test_desktop_account_live_routes.py against the deployed image

# 9. Frontend auth/download/upgrade-deep-link (isolated bfd2d5a3 / 4638f3d7)

# 10. Trace V5 deterministic + live correlation
cd evals/workshop
npm run gate:local -- --workshop /path/to/workshop-v0.1 \
  --instance "$WORKSHOP_INSTANCE" --craftax-url "$WORKSHOP_GATE_CRAFTAX_URL"

# 11. Configured local topology (frontend/auth, MLX bearer, backend, Craftax, eval driver)

# 12. CRAFTAX-LUNA-010 (credentials + live Craftax + installed candidate)
npm run craftax-luna-010 -- --instance "$WORKSHOP_INSTANCE" --craftax-url "$WORKSHOP_GATE_CRAFTAX_URL"
```

Helper: `workshop-v0.1/scripts/run_launch_gates.sh` runs the deterministic subset and fails closed on missing tools.

## Fault injection (must remain truthful)

- App kill mid-turn / mid-checkout return
- Sidecar kill (Laguna daemon)
- Container kill mid-rollout
- Backend / network loss during a paid action
- Duplicate SSE terminal frames
- Stale startup + corrupted prior SQLite
- Upgrade from last shipped schema (migration 8: legacy usage preserved, no silent zeroes)

## Exit

One machine-readable evals gate receipt + backend pytest JUnit + Desktop suite logs, all on the same SHAs that will be signed.

## Channel scope and known annotated exceptions

- These gates bind the **stable** channel. Nightly builds (see UPDATES_AND_CHANNELS.md) are signed/notarized and pass the secret-scan gate, but are exempt from the full manual/CUA matrix by design.
- The Rust suite carries exactly one `#[ignore]`: `trace_v5_e2e::imports_real_bundle_into_trusted_catalog_and_keeps_duplicate_identity` — external-fixture by design (needs `SYNTH_TRACE_V5_REAL_BUNDLE`; runner `scripts/test-trace-v5-real-bundle.sh`). It is not launch debt; run it whenever a real dogfood bundle is on the machine.
- The "no skip/fixme/todo/expected-fail" rule above covers everything else, with zero current exceptions.
