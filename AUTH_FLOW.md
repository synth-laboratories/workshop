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
  → system browser: Clerk sign-in or signup + verification
  → /device asks "Connect Synth Desktop?" and the user approves
  → POST /api/auth/device/complete
  → Rust polls POST /api/auth/device/token
  → synth_api_key is written to a 0600 env file
  → fail-closed runtime reload
  → badge: Authenticated
```

Sign out removes the desktop-managed key from the private env file and reloads
the runtime fail-closed. A process-level `SYNTH_API_KEY` override is intentionally
outside the app's custody and must be removed by the launching environment.

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
| Existing-account actions, install → authenticated | Live count pending | ≤ 4 | Button → browser auth → Approve → return |
| Brand-new user | Signup + key creation + paste | Signup + verify; no credential handling | Live count pending |
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

As of 2026-08-09, production `POST /api/auth/device/init` returns `307` and the
live artifact pass is blocked until the public-route fix is deployed. Existing,
new-user, expiry, and `ORG_MISSING` results must remain unchecked until that
deployment and the dated live pass are recorded here.
