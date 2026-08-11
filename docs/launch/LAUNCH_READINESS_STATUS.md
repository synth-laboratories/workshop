# Workshop v0.1 — launch readiness status

**Updated:** 2026-08-10
**Verdict:** **not Gate F yet.** Code paths for billing, Desktop settlement, Settings catalog, passive stable-channel checks, Trace V5 grading, and CRAFTAX-LUNA-010 harness are landed locally. Signed artifact, live CRAFTAX-LUNA-010, production Autumn checkout, manifest deployment, and clean-user rehearsal remain external/blocking.

## What closed this pass

| WP | Status | Evidence |
|---|---|---|
| WP0 scope/owners/ops docs | Landed | `workshop-v0.1/docs/launch/*` |
| WP1 source isolation | Partial | Clean evals worktree `evals-workshop-v01` @ `release/workshop-v0.1-evals`; clean frontend `frontend-upgrade-isolated` @ `release/workshop-v0.1-upgrade-deeplink` (2 commits ahead of `origin/dev`). Workshop `release/v0.1` ahead of origin locally. Backend billing still on `feat/desktop-account-snapshot` dirty tree. |
| WP2 Starter/Pro checkout | **Proven live against Autumn sandbox** | Both `starter`→`standard_monthly` and `pro`→`max_monthly` return `mode=provider` with Stripe `cs_test_…` URLs + session ids. `SYNTH_BILLING_REQUIRE_PROVIDER_CHECKOUT` default-on for local/staging/dev (503 on Autumn failure, not `hosted_web`). Desktop Upgrade refuses `hosted_web`. Re-run: `python scripts/prove_provider_checkout.py --live` (loads `backend/.env.local`). Fake path: omit `--live`. Units: `test_billing_checkout_session_route.py` + `test_fake_autumn_checkout.py` (13). |
| WP3 cloud metering | Landed locally | gateway settlement default-on for local/staging/dev; settlement unit tests green |
| WP4 Desktop settlement + Settings | Landed on `release/v0.1` | broker `SettledReceipt` drain → usage records; `tariff_catalog` → `synthTariffs.catalog()`; migration 8 tests green |
| WP5 Trace V5 + CRAFTAX harness | Landed locally | `trace_correlation_payload` + evals grading; `craftax-luna-010` fail-closed harness + unit tests (20/20). **Live 10-rollout run not executed** (needs instance + Craftax + credentials). |
| WP6 auth/web/handoff docs | Landed | `AUTH_WEB_HANDOFF.md`, clean-user rehearsal doc; frontend upgrade isolated |
| Updates / channels | Landed locally | Desktop stable-channel passive check + fixed download destination; site manifest/proxy exemption at frontend `8c8f5110`. Site deploy remains external. |
| WP7 deterministic gates | **Pre-freeze green** | Current dirty-tree run: evals 25/25 + typecheck; backend 31/31; Desktop typecheck; Node 112/112; Playwright 148/148; full Rust 327/327 with the one annotated real-bundle ignore. These are confidence checks, not artifact-bound Gate F receipts. |
| WP8 sign/notarize/ops | Docs only | `LAUNCH_OPS.md` present. **No signed artifact.** |

## Pin these SHAs when freezing Gate F

| Component | Branch / path | Tip / note |
|---|---|---|
| Workshop Desktop | `workshop-v0.1` `release/v0.1` | local tip (ahead of origin; review before push) |
| Evals launch | `evals-workshop-v01` `release/workshop-v0.1-evals` | base `fd02ce7e5` + uncommitted CRAFTAX-LUNA-010 wiring in worktree |
| Backend account+billing | `backend-desktop-account-snapshot` `feat/desktop-account-snapshot` | `ac9ae580f` + uncommitted fake-Autumn/checkout adapter |
| Frontend upgrade | `frontend-upgrade-isolated` `release/workshop-v0.1-upgrade-deeplink` | `c2b85dd3` (includes `337cbe45` / `bfd2d5a3` content) |

## Hard remaining Gate F blockers (cannot code away)

1. **Signed + notarized** Desktop `.app` with published checksum.
2. ~~Live Autumn Starter/Pro `mode=provider`~~ — **done** (sandbox Autumn via `prove_provider_checkout.py --live`; set `SYNTH_BILLING_REQUIRE_PROVIDER_CHECKOUT=1` on the friends deploy).
3. **CRAFTAX-LUNA-010 live** on the installed candidate (`WORKSHOP_INSTANCE` + `WORKSHOP_GATE_CRAFTAX_URL` + OpenRouter/Synth keys).
4. **Clean-machine rehearsal** on the downloaded artifact (production Clerk; no `+clerk_test`).
5. **Push/review** of clean branches; Workshop/evals dirty trees must not be Gate F receipts.
6. Named CUA tester / independent reviewer / rollback owner still TBD in owner matrix.
7. **Deploy** the site revision that anonymously serves `/releases/stable/latest.json`, then verify the production response is JSON rather than an auth redirect.

## How to run what is ready

```bash
# Deterministic subset
workshop-v0.1/scripts/run_launch_gates.sh

# CRAFTAX live (fails closed without creds)
cd evals-workshop-v01/workshop   # or evals/workshop
npm run craftax-luna-010 -- --instance "$WORKSHOP_INSTANCE" --craftax-url "$WORKSHOP_GATE_CRAFTAX_URL"
```
