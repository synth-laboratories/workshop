# Unified account, authentication, billing, and usage flow

**Status:** product/contract plan — no sign-in or checkout API is implemented by Desktop yet.

## Outcome

A person should be able to move through one understandable journey:

```text
Workshop web → create/sign in to Synth account → choose plan → download Desktop
→ pair Desktop with the account → use local or cloud capabilities
→ see remaining cloud allowance and manage billing
```

The Desktop must remain useful without a cloud account for local Laguna and local files. Cloud Intern, account-scoped plans, and Synth-cloud usage must always state when they require sign-in.

## Current facts

- Desktop stores an API key, backend URL, and provider keys in a private env file through `Settings → Account`. This is connection configuration, not account identity.
- The account avatar opens that configuration page; the adjacent Account-menu chevron is a stub.
- The checked-in Research API contract exposes billing data:
  - `GET /api/v1/billing/entitlements`
  - `GET /smr/billing/plan`
  - `GET /smr/billing/catalog`
  - `GET /smr/runs/{run_id}/usage-summary`
- The contract does **not** expose a user identity, sign-up, sign-in, device pairing, checkout, billing portal, organization membership, or token-revocation endpoint.
- The desktop’s local usage ledger is distinct from Synth-cloud billing. OpenRouter usage is also distinct unless the backend explicitly bills it through Synth.

## Product model

### Poolside reference: what to match

CUA inspection of the installed Poolside Desktop Assistant and its current official documentation shows a deliberately provider-first flow:

- the desktop remains fully useful with local MLX and does not put billing/profile controls into its main Settings navigation;
- `pool login` asks the user to choose an access mode before asking for credentials: Poolside Platform, an organization deployment, OpenRouter, or any OpenAI-compatible provider;
- hosted/enterprise mode opens a browser; standalone/provider mode accepts a token; credentials are persisted locally; and logout removes the local credential for the selected endpoint;
- Poolside Platform’s documented free path opens the web platform, asks the user to create/sign in, creates an API key, and asks the CLI user to paste it back.

Synth should match the **mode choice and local-first posture**, but improve the final handoff: use browser pairing/PKCE rather than presenting API-key paste as the normal consumer path.

| Poolside access choice | Synth equivalent | Primary UI |
|---|---|---|
| Poolside Platform | Synth Cloud account | Browser sign-in + native device pairing |
| Organization deployment | Organization Synth backend | Backend URL + browser SSO or administrator-issued token |
| OpenRouter account | OpenRouter provider | Advanced provider setup, separately billed |
| OpenAI-compatible provider | Local Laguna or advanced custom endpoint | Local-first setup / Advanced connection settings |

Do not copy Poolside’s absence of billing information where Synth sells account-scoped plans. Synth should add plan/remaining-allowance only after an account is paired, and keep it out of the local-only default chrome.

### States

| State | What Desktop can do | What the user sees |
|---|---|---|
| Local-only | Laguna, local files, local history | “Use local models without an account.” |
| Signed out | Everything local; cloud controls gated | “Sign in to use Synth Cloud, Intern, and plan allowances.” |
| Pairing | Local remains usable; cloud requests paused | Browser handoff with a clear cancel path. |
| Signed in / active | Local plus authorized cloud capabilities | Name/org, plan, allowance summary, usage link. |
| Signed in / limited | Local remains usable; billable cloud actions blocked only where necessary | Remaining allowance, reset, and one recovery action. |
| Signed in / expired or revoked | Local remains usable; cloud disconnected | “Reconnect account” with an error reason safe to show. |

### Source-of-truth rules

1. Backend is authoritative for identity, organization, plan, entitlements, cloud usage, billing state, and revocation.
2. Desktop stores only device-scoped refresh/session material in the native secret store; the renderer never sees a long-lived secret.
3. Local Laguna and local Desktop ledger data are device-local facts, not cloud-account allowance.
4. Every number identifies its scope: **Synth cloud**, **local device**, or **external provider**.

## User journey

### 1. Workshop acquisition and account creation

1. Workshop has `Download Desktop` and `Use Synth Cloud` CTAs.
2. If unauthenticated, both enter the same browser auth flow: email/password, magic link, SSO, or whichever mechanisms backend supports.
3. After authentication, show organization selection when more than one organization is available.
4. Show the current plan and a compact allowance summary before checkout. Do not force a plan for local-only use.
5. On successful download, include a one-time, short-lived Desktop pairing link or code bound to the authenticated account and selected organization.

### 2. Desktop first run and pairing

1. First run offers two equal choices: `Continue locally` and `Sign in to Synth`.
2. `Sign in to Synth` opens the system browser using OAuth 2.1 Authorization Code + PKCE, or consumes the pairing link/code from Workshop.
3. Browser confirmation returns to Desktop through a registered loopback callback or universal link. The Desktop exchanges the code in the native host only.
4. Desktop calls Account Snapshot and renders identity, selected organization, plan, key entitlement states, and allowance totals.
5. If the browser is unavailable, show a copyable verification URL and one-time code. Never ask users to paste a permanent API key as the primary sign-in flow.

### 3. Everyday account menu

The avatar remains the direct shortcut to the full Account page. The adjacent chevron becomes a compact menu:

- identity/org row: avatar or initials, display name, selected organization;
- `Plan · allowance remaining` summary;
- `Usage`;
- `Account & billing`;
- `Connection settings` for advanced endpoint/debug configuration;
- `Sign out of this device`.

Use status text, not raw billing numbers, while loading. A local-only user sees `Use Synth Cloud` rather than empty plan chrome.

### 4. Account page

The full page has four user-facing sections:

1. **Profile & organization** — display name, verified email, selected organization, role, switch organization.
2. **Plan & allowances** — plan display name, billing mode, included/enabled capabilities, remaining allowance, reset/renewal time, and blocked state. One primary billing action only.
3. **Usage** — today, 7 days, and 30 days of Synth-cloud use. Drill into project/run detail where supported.
4. **Devices & security** — this device’s last sync, sign out, and optionally other device/session revocation when backend supports it.

Keep the existing endpoint/profile/API-key editor under **Advanced connection settings**, not as the main Account experience.

### 5. Upgrade, payment, and billing management

1. A user selects `Upgrade` from Workshop or Desktop.
2. Desktop opens a backend-issued hosted checkout URL in the system browser; it never handles payment details.
3. Backend/webhook completes the purchase and updates the plan/entitlement snapshot.
4. Desktop polls or receives an account change notification, then refreshes Account Snapshot and unblocks newly enabled cloud actions.
5. `Manage billing` opens a backend-issued billing-portal URL. Downgrade/cancellation messaging and effective dates remain backend-authored.

## Required backend contract

The existing billing endpoints are a good input, but Desktop needs a stable account surface rather than independently composing raw billing APIs.

### Authentication and device endpoints

```text
POST /api/v1/auth/device/authorize
  → authorization_url, state, code_verifier policy, expires_at

POST /api/v1/auth/device/exchange
  → access token, rotating refresh token, expires_at, account_id, org_id

POST /api/v1/auth/refresh
POST /api/v1/auth/revoke
GET  /api/v1/account/me
GET  /api/v1/account/organizations
POST /api/v1/account/organizations/{org_id}/select
```

`/account/me` should return display-only identity and current organization. No API key, payment data, or secret material belongs in the response.

### Desktop Account Snapshot

Provide one versioned endpoint optimised for first-run and the account menu:

```text
GET /api/v1/desktop/account-snapshot
```

```json
{
  "schema_version": "synth.desktop-account.v1",
  "account": {
    "id": "acct_…",
    "display_name": "…",
    "email": "…",
    "avatar_url": null
  },
  "organization": {
    "id": "org_…",
    "display_name": "…",
    "role": "member"
  },
  "plan": {
    "tier": "pro",
    "display_name": "Synth Pro",
    "state": "active",
    "renews_at": "…"
  },
  "allowances": [
    {
      "id": "cloud_runs",
      "display_name": "Synth Cloud runs",
      "enabled": true,
      "used": 12,
      "limit": 100,
      "unit": "runs",
      "resets_at": "…"
    }
  ],
  "usage": {
    "today": { "events": 2, "billed_microcents": 0 },
    "seven_days": { "events": 12, "billed_microcents": 0 },
    "thirty_days": { "events": 42, "billed_microcents": 0 }
  },
  "billing_actions": {
    "checkout_url": null,
    "portal_url": "https://…"
  },
  "generated_at": "…"
}
```

The server may derive this from the existing entitlement and plan snapshots. It must distinguish: `unauthenticated`, `active`, `limited`, `past_due`, `canceled`, and `unknown`.

### Links and events

- `POST /api/v1/billing/checkout-session` returns a hosted checkout URL for a requested plan.
- `POST /api/v1/billing/portal-session` returns a hosted billing-management URL.
- An authenticated server-sent event or short-lived polling ETag tells Desktop when account/plan/entitlements change.
- Every response must have cache/freshness metadata. Desktop should use a short cache and always provide `Last updated` plus retry.

## Desktop implementation boundaries

### Native host

- Add `AccountManager` in Tauri/Rust; it owns PKCE, callback exchange, refresh, revocation, token persistence, snapshot fetches, and account-change events.
- Store refresh material in the macOS Keychain (or an equivalent OS secret store), keyed by Desktop instance/account/org. Do not keep it in TOML or renderer storage.
- Expose redacted IPC only: `account_get_snapshot`, `account_begin_sign_in`, `account_sign_out`, `account_open_billing`, and `account_select_org`.
- Preserve `synth_config` for advanced endpoint configuration and development profiles. Do not overload it as the user account model.

### Renderer

- Add typed account state: `local_only | signed_out | pairing | active | limited | error`.
- Replace the stub chevron with an accessible menu and focus/escape/outside-click behavior.
- Replace the literal `S` avatar with fetched initials/avatar only after identity arrives; use a neutral account glyph before then.
- Build the Account page from Account Snapshot, with independently retryable plan/usage sections.
- Link local usage to Inventory Usage and label it `This device`; never merge it into cloud totals.

### Workshop / web frontend

- Own acquisition, browser auth, organization choice, hosted checkout return, billing portal return, download association, and pairing-code recovery.
- Use the same Account Snapshot model or a deliberately compatible web account model.
- Allow the download page to create/recover a pairing link after sign-in, rather than requiring an API-key copy/paste.

## Implementation sequence

1. **Backend contract first:** define auth/session lifetime, organization selection, Account Snapshot, hosted billing links, status/error codes, audit expectations, and webhook-to-snapshot latency.
2. **Native auth shell:** secret-store integration, signed-out state, begin/cancel callback flow, redacted snapshot IPC. No payment UI yet.
3. **Desktop account UI:** real menu, account overview, retry/error/limited states; retain advanced connection settings separately.
4. **Workshop flow:** shared sign-in, plan/checkout, download pairing, post-checkout return.
5. **Usage integration:** render server totals and per-run details; clearly label local/external usage.
6. **Security and lifecycle:** refresh rotation, revocation, expired session, org switch, device sign-out, network-offline cache, telemetry redaction.

## Acceptance tests

- New user: sign up → select plan → checkout in browser → download → pair Desktop → sees plan and allowance.
- Existing account: sign in → selected org persists → Desktop restarts → session refreshes without exposing a token to the renderer.
- Local-only: decline sign-in → Laguna/local features work; cloud actions explain the requirement.
- Limited/past-due: local features continue; only affected cloud actions are blocked with backend-authored recovery CTA.
- Usage: cloud today/7d/30d values match Account Snapshot; local usage stays labeled separately.
- Revoked/expired: refresh fails safely, Desktop becomes signed out for cloud capability, local data remains untouched.
- Account menu: keyboard accessible; no stub toast; avatar and chevron have complementary, non-duplicative actions.
- Checkout/portal: Desktop opens only backend-issued hosted URLs; no billing data is collected or stored by Desktop.

## Decisions needed before implementation

1. Auth mechanism: magic link, password, SSO, or a combination; whether device authorization is OAuth PKCE or pairing-code only.
2. Account hierarchy: individual account vs organization/workspace billing authority, and organization switch behavior.
3. Billing provider and checkout/portal ownership; Desktop should only receive hosted URLs.
4. What counts as Synth-cloud usage versus separately billed OpenRouter/local usage.
5. Entitlement enforcement location: backend must enforce it; Desktop may preflight for UX but cannot be the gate.
6. Whether v0.1 supports sign-out and device revocation, or only API-key disconnect as an advanced recovery operation.

## Reference evidence

- [Poolside: Log in to Poolside](https://docs.poolside.ai/get-started/log-in) — current access-mode chooser, browser/deployment login, API-key flow, and logout guidance.
- [Poolside: Install Poolside Agent CLI](https://docs.poolside.ai/cli/install) — credential storage and provider-mode setup guidance.
- [Poolside: Introducing Poolside Desktop Assistant](https://poolside.ai/blog/introducing-poolside-desktop-assistant) — local MLX/offline positioning.
- CUA, 2026-08-09: installed Poolside Desktop Assistant showed settings for General, Projects, Agents, On-Device Models, Voice Recognition, keyboard shortcuts, GitHub, and Remote Access; no account/billing section was present in the desktop Settings navigation.
