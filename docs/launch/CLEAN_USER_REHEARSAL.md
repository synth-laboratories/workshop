# Clean-user download / signup / sign-in rehearsal

Gate F requires a short clean-machine rehearsal on the **downloaded signed artifact**. Gate P requires the full 37-item CUA runbook on that same artifact within 24 hours of publish.

Production Clerk does **not** use `+clerk_test` / `424242`.

## Friends rehearsal (Gate F)

On a machine that has never had Synth Desktop:

1. Open usesynth.ai (desktop + one mobile width). Confirm desktop-only copy on mobile, version, checksum, requirements, pricing, privacy, support, known issues. No Intern, no staging hosts, no fixture-as-live claims.
2. Download the candidate. Verify filename, MIME, content-length, SHA256, interrupted/range retry.
3. Install / open. Confirm notarization + Gatekeeper path.
4. Sign up with a fresh production account. Pair via device-init **JSON** (not a redirect). Exercise expiry, denial, wrong browser profile, offline, backend 5xx, duplicate callback, app close mid-pair.
5. First local Laguna response. First Synth Cloud response.
6. Open Account: Free plan, no invented dollars. Select Starter, open provider checkout (`mode=provider`). Human completes payment. Refresh: `status=active`, `degraded=[]`, allowance 2000 cents from entitlement.
7. One metered Laguna turn: exactly one backend fact, Desktop `usage_records` billed amount matches, allowance decrements. Over-limit refused without provider execution.
8. Sign out: copy says this device; local work remains. Sign in existing account. Restart. Upgrade/restart/data-durability smoke.
9. Support zip: secret canaries absent.
10. Record artifact SHA, account ids (not keys), and a short screen recording.

## Public rehearsal (Gate P)

Repeat Gate F plus the full 37-item independent CUA checklist, Pro checkout, portal/cancel/return, CRAFTAX-LUNA-010 on the installed app, and post-publish smoke in `LAUNCH_OPS.md`.

## Failure handling

Any fail is a no-go for that gate. Do not “pass with notes” on pairing, money, secrets, or Intern visibility.
