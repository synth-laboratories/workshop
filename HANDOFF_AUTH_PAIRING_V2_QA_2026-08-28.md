# Handoff — Device pairing v2 (per-device keys, pairing code, revocation) QA

**For:** an engineer driving the real app with a Clerk dev org + user, a local
backend slot, and CUA.
**Date:** 2026-08-28
**Contract:** [`AUTH_FLOW.md`](AUTH_FLOW.md) (updated in the same change, per its law)

---

## 1. What changed, and where

Two branches, one in each repo. **Nothing is pushed.**

| Repo | Branch / commit | Contents |
|---|---|---|
| workshop (worktree `~/GitHub/workshop-v08-auth-pairing`) | `auth/device-pairing-v2` @ `db2473ca` | Desktop: pairing code in the sign-in UI, origin-guarded verification link, host-paced polling with 429 slow-down, sign-out revocation |
| frontend (`~/GitHub/frontend-v08-release`) | `auth/device-pairing-v2` @ `fa53e0fb` | Web: per-device key minting with atomic single consumption, `user_code` on init + `/device`, `POST /api/auth/device/revoke` |

The workshop branch was cut from `eval/inline-first-admission` @ `1f443583` in a
**separate worktree** because another agent has in-flight edits (including
`lib.rs`) in the primary `~/GitHub/workshop-v08-release` checkout. Merge this
branch there when that work settles; the overlap is one function
(`account_sign_out`).

No DB migration. `user_code` is hash-derived from `device_code`
(`frontend src/lib/device-auth/userCode.ts`), and per-device keys reuse the
existing `api_keys` insert.

### Files worth reading before you test

```
frontend  src/lib/device-auth/userCode.ts        # shared pairing-code derivation
frontend  src/lib/device-auth/service.ts         # atomic COMPLETE→EXCHANGED gate, per-device mint
frontend  src/app/api/auth/device/revoke/route.ts
frontend  src/app/(pages)/(protected)/device/    # approval page shows the code
workshop  apps/synth_desktop/src-tauri/src/device_auth.rs   # user_code, interval, 429, revoke_key
workshop  apps/synth_desktop/src-tauri/src/lib.rs           # account_sign_out revocation
workshop  apps/synth_desktop/src/renderer/src/components/BackendSettings.tsx  # AccountSignIn
```

## 2. The three behavior changes in one paragraph each

**Per-device keys.** `POST /api/auth/device/token` now mints a **fresh**
`sk_synth_user_…` key per pairing instead of returning the account's shared
key. The COMPLETE→EXCHANGED transition is a conditional UPDATE, so a raced
double-poll can never mint two keys (the loser gets `409 DEVICE_CODE_USED`).
Consequence: every paired desktop holds a different key, and revoking one
touches nothing else.

**Pairing code.** Init returns an RFC 8628 `user_code` (`XXXX-XXXX`, derived
from the device code — no schema change). The desktop shows it under "Finish
sign-in in your browser" (`data-testid="sign-in-user-code"`); the `/device`
approval page derives and shows the same code
(`data-testid="device-user-code"`) with copy telling the user not to approve on
a mismatch. This is the consent-phishing mitigation: a victim opening an
attacker's pairing link sees a code their own desktop is not showing.

**Sign-out revocation.** `account_sign_out` deletes the key locally (exactly
as before — local removal never waits on the network), then best-effort calls
`POST /api/auth/device/revoke` with the key as bearer, which sets
`is_active = false`. A process-env `SYNTH_API_KEY` override is never revoked
(it isn't the app's key; `synth_config::desktop_managed_api_key` reads only
the env file). Polling also got server pacing: init's `interval` drives the
loop and a 429 is a slow-down (Retry-After honored, capped at 30 s), not an
error.

## 3. Cross-version compatibility (deliberate)

- **Old desktop → new web:** `verification_uri` shape and the 428/409/410
  status vocabulary are unchanged; the old app simply never shows a code and
  receives a per-device key it stores like before.
- **New desktop → old web:** `user_code`/`interval` are optional in the init
  parse — no code line renders, polling defaults to 4 s; a 404 from the
  missing revoke route is logged and sign-out completes.

## 4. Build and launch

```bash
# Frontend (Clerk dev instance + local DB env in .env.local as usual)
cd ~/GitHub/frontend-v08-release   # on auth/device-pairing-v2
bun run dev                        # localhost:3000

# Desktop dev instance against a local slot
cd ~/GitHub/workshop-v08-auth-pairing
scripts/desktop-instance.sh dev authv2
```

With the default `local-slot1` profile (backend on 127.0.0.1) the desktop
auto-targets `http://localhost:3000` for pairing; `SYNTH_WORKSHOP_URL`
overrides explicitly. To exercise the staging web deploy instead, export
`SYNTH_WORKSHOP_URL` — but note the deploy must carry the frontend branch or
you are testing lane "new desktop → old web".

Identity: use a disposable Clerk dev-instance user
`e2e-authv2-<date>+clerk_test@…` with the test OTP, per the release-gate
convention in `AUTH_FLOW.md`; delete the user and org afterward.

## 5. QA scenarios (dev org + user + slot)

Verification SQL against the frontend DB:

```sql
-- keys for the QA user, newest first
SELECT id, left(key_value, 24) AS key, is_active, created_at
FROM api_keys WHERE user_id = '<app_users.id>' ORDER BY created_at DESC;

-- handshakes
SELECT status, created_at, completed_at FROM sdk_handshakes
ORDER BY created_at DESC LIMIT 5;
```

1. **New-user pairing with code match.** Sign in with browser → desktop shows
   `sign-in-user-code`; complete Clerk signup + org task; `/device` shows the
   identical code; Approve → Authenticated badge; one metered Intern action
   works. DB: one new active key, `sdk_handshakes` row EXCHANGED.
2. **Single consumption.** Re-POST `/api/auth/device/token` with the same
   `device_code` → `409 DEVICE_CODE_USED`, and no second key row.
3. **Second device, distinct key.** Pair a second instance
   (`scripts/desktop-instance.sh dev authv2b`) as the same user. DB: two
   active keys with different values.
4. **Sign-out revokes only this device.** Sign out on instance A: its badge
   drops, its key row flips `is_active = false`, instance B keeps working and
   B's key stays active. Backend requests with A's revoked key must now 401.
5. **Mismatch refusal (the phishing drill).** Start pairing on A, but open
   B's (or a hand-edited) `/device?device_code=…` link in the browser: the
   page's code differs from A's display. This is the case the copy exists
   for; confirm the wording steers a reasonable person to refuse.
6. **Slow-down, not error.** Run two pairings concurrently from one IP (or
   drop the token route's limit locally): desktop must stay on "Finish
   sign-in in your browser…" and stretch its poll spacing — never the error
   state. (Unit-covered: `rate_limited_poll_is_pending_with_backoff_not_an_error`.)
7. **Expired / reopened.** Let a pairing sit 10+ min → poll shows "browser
   link expired" with **Sign in with browser** as the one recovery action.
   "Reopen browser" during a live pairing reuses the same link and code
   (idempotent begin).
8. **Process-env override untouched.** Launch with `SYNTH_API_KEY` exported:
   sign-out still errors with the existing "remove it from the launching
   environment" message and the override key's DB row stays active.

## 6. CUA lane (release artifact)

The 24-hour release gate is unchanged in shape — run
`SYNTH_AUTH_E2E_COMMAND='<Clerk dev-instance desktop driver>'
./scripts/auth-pre-release.sh '/path/to/Synth Desktop.app'` — with two new
assertions for the driver, using the stable test ids:

- Desktop `sign-in-user-code` and browser `device-user-code` render the same
  string before Approve is clicked.
- After sign-out, the paired key's `is_active` is false (or an API probe with
  the captured fingerprint 401s).

Existing per-PR gates already pass on the branch: renderer Playwright
`account-sign-in.spec.ts` (5/5, now covering the code display and host-paced
polling), Rust `device_auth` (6/6, incl. origin-guard, 429 backoff, revoke
bearer), `regenerate_protocol_bindings` committed, frontend `tsc` + eslint +
vitest (`userCode.vitest.ts`; the two `nanohorizon.vitest.ts` failures
pre-exist on the base branch — verified at clean HEAD).

## 7. Known gaps / follow-ups (not blockers for this QA)

- **Key accumulation & dashboard interplay.** Every pairing adds an active
  key; web surfaces that show "your API key" via `selectActiveKeyValue …
  LIMIT 1` will show the newest (a device key). Fine for dev; before GA,
  device keys deserve a label column (backend alembic migration) and a
  Devices & security list with remote revocation.
- **No revoke-all** on account compromise; per-device keys make it possible,
  a `/settings` action should expose it.
- **Rate limiter is per-serverless-instance** (documented in
  `rateLimit.ts`); the 429 pacing works but isn't a distributed guarantee.
- **`device_code` still rides the signin redirect URL** (history/session
  storage). The pairing code mitigates the consent side; moving the secret
  out of the URL entirely is the V1 OAuth loopback promotion, which should
  extract the shared core from `codex_oauth.rs` rather than add a third
  stack (see the review that produced this change).
