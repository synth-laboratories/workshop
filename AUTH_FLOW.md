# Synth authentication flow

This file is the standing contract for desktop authentication. Any auth-flow
change must update this document in the same pull request.

## Law

- Identity is created once, in the browser; Clerk owns signup and verification.
- The Rust host is the only desktop credential custodian. The renderer receives
  display-safe state only.
- Local use never requires an account.
- The UI has one cloud-auth action, **Sign in with browser**, and every failure
  offers one visible recovery action.

## V0 — device pairing (alpha)

```text
Desktop "Sign in with browser"
  → Rust POST {workshop}/api/auth/device/init
    (device_code + user_code + interval; RFC 8628 field names)
  → desktop shows the pairing code; system browser: Clerk sign-in or signup
  → /device shows the same code and asks "Connect Synth Desktop?"
    — the user approves only if the codes match
  → POST /api/auth/device/complete
  → Rust polls POST /api/auth/device/token at the server-directed interval
    (429 is a slow-down signal, never an error)
  → a per-device synth_api_key is minted for this pairing and written to a
    0600 env file
  → fail-closed runtime reload
  → badge: Authenticated
```

The pairing code (`user_code`) is hash-derived from the device code on the
server; the desktop learns it from the init response and the `/device` page
derives it again, so the two displays agree without a schema change. It is a
comparison aid against consent phishing (someone else's pairing link), not a
credential.

Each pairing mints its own API key (atomic single consumption of the device
code), so one desktop's key can be revoked without touching any other device.
The desktop refuses to open a verification link that is not on the Workshop
origin the pairing started against.

Sign out removes the desktop-managed key from the private env file, reloads
the runtime fail-closed, and then best-effort revokes that key server-side via
`POST /api/auth/device/revoke` (bearer-authenticated by the key itself; local
deletion never waits on the network). A process-level `SYNTH_API_KEY` override
is intentionally outside the app's custody, is never revoked by the app, and
must be removed by the launching environment.

## V1 — OAuth 2.1 + PKCE

```text
Rust mints PKCE and binds 127.0.0.1:<port>
  → /oauth/authorize (Clerk-fronted consent and organization choice)
  → loopback ?code
  → Rust exchanges for short access + rotating refresh token
  → tokens stored in the OS keychain
  → GET /api/v1/desktop/account-snapshot
  → unauthenticated | pairing | active | limited | expired | error
```

V1 replaces V0 plumbing under the same button. It does not change the renderer
contract or the local-without-account path.

## Ergonomics scorecard

Poolside's documented consumer path is the comparison baseline recorded in
`AUTH_BILLING_FLOW.md`: create an account, create an API key in the web product,
then paste it into the client. Counts below distinguish a documented baseline
from measurements that still require a live CUA run.

| Metric | Poolside free path | Synth requirement | Current evidence |
|---|---:|---:|---|
| Manual credential copy/paste | 1 | 0 | V0 transfers the key host-to-host; renderer never sees it |
| Existing-account actions, install → authenticated | Live count pending | ≤ 4 | Synth path is button → browser auth → Approve; no credential handling |
| Brand-new user | Signup + key creation + paste | Signup + verify; no credential handling | Production new-user approval and single-use token issuance passed 2026-08-09 |
| Time to first cloud action | Live timing pending | < 90 s existing; < 3 min new | Live timing pending |
| Failure recovery | Repeat login/key flow | One visible action | Reopen browser / Sign in again / Try again |
| Local-only use without account | Yes | Yes | First run offers **Continue locally** equally with sign-in |

Do not replace pending measurements with estimates. Record the dated CUA run,
artifact identifier, action counts, and timings here after the production route
is live.

## Release gate

No release ships unless the auth pass ran against the actual release artifact
within the preceding 24 hours. Run:

```bash
SYNTH_AUTH_E2E_COMMAND='<approved Clerk dev-instance desktop driver>' \
  ./scripts/auth-pre-release.sh '/path/to/Synth Desktop.app'
```

The driver must use a Clerk dev-instance `+clerk_test` identity and test OTP,
assert the Authenticated badge, perform one metered Intern action, sign out, and
cover existing-user, new-user, expired-code, and `ORG_MISSING` paths. Nightly
Lane B uses disposable `e2e-*+clerk_test` users and deletes them afterward.

Per-PR gates remain the renderer Playwright suite (including
`account-sign-in.spec.ts`) and Rust `device_auth`/`synth_config` tests. A green
unit suite is not a substitute for the live artifact pass.

## Production closeout record

2026-08-09 production deployment `dpl_3o7mu5QAEAdJJbKS4JHVbJDNBAcC`
(frontend commit `3ae4b3cc`):

- `POST /api/auth/device/init` returned `200 application/json` while signed out,
  with a 600-second code and a verification URL carrying the code to `/device`.
- Signed-out token polling reached the handler and returned typed JSON rather
  than an auth redirect.
- `/device` remained protected and preserved the full target through sign-in.
- A disposable brand-new user encountered Clerk's required organization task,
  approved the device, and reached the explicit return-to-app success state.
- Token polling returned one non-empty `synth_api_key`; a second poll returned
  `409 DEVICE_CODE_USED`, proving single consumption.
- The test key was revoked and the disposable Clerk organization and user were
  deleted after the pass.

The live run exposed that Clerk's organization task could drop the pending
device destination. Frontend PR #210 now keeps only a shape-validated `/device`
target in same-tab session storage for at most 10 minutes, restores it after the
task, and clears it on arrival. The follow-up is promoted to `usesynth.ai`; the
production init, typed-invalid-token, and protected-device probes all passed.
**Reopen browser** remains the tested one-action recovery if a third-party auth
task interrupts navigation.

Clean desktop commit `95a7e39` built a macOS `.app` successfully after removing
dangling registrations for unfinished optimizer/eval modules. The ad-hoc signed
candidate archive has SHA-256
`b9ce3c778ecef132aa383b0bffdc6f967354b8cebff563d8927c746b5ef27daa`.

Still required for a release artifact record: install the exact candidate,
assert its native Authenticated badge and one metered Intern action, then attach
the gate timestamp here. Route/browser success and a clean signed build do not
satisfy the artifact-within-24-hours rule by themselves.
