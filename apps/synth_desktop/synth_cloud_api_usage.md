# Synth Cloud — API, plans, sign-in, and usage (sketch)

**Status:** built — see [Implementation status](#implementation-status-2026-08-10) for what shipped against this sketch and what is deliberately still open.  
**Updated:** 2026-08-10  
**Related:** [`AUTH_FLOW.md`](../../AUTH_FLOW.md), [`AUTH_BILLING_FLOW.md`](../../AUTH_BILLING_FLOW.md), [`HANDOFF_LOCAL_DEV_ACCOUNT_200_PLAN_AND_UI_INVARIANTS.md`](HANDOFF_LOCAL_DEV_ACCOUNT_200_PLAN_AND_UI_INVARIANTS.md)

## How it works (target)

```text
                          WEB (usesynth.ai + Clerk)
  ┌──────────────────────────────────────────────────────────────────────┐
  │  Sign up / Sign in ──► choose plan ──► hosted checkout / portal     │
  │         │                 │                                         │
  │         │                 ├─ Starter  $20 / mo                      │
  │         │                 └─ Pro      $200 / mo                     │
  │         ▼                                                           │
  │  Device pair / approve Desktop ──► issue key or session tokens      │
  └───────────────────────────────┬──────────────────────────────────────┘
                                  │
                                  │  Account Snapshot
                                  │  GET /api/v1/desktop/account-snapshot
                                  │  (identity · org · plan · cloud usage · billing URLs)
                                  ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │                     DESKTOP (Rust host owns secrets)                 │
  │                                                                      │
  │   First run:  [ Continue locally ]     [ Sign in to Synth ]          │
  │                      │                          │                    │
  │                      │                          └── browser pair     │
  │                      ▼                                               │
  │              Local Laguna / Muse                                     │
  │              (no account required)                                   │
  │                                                                      │
  │   Signed in:                                                         │
  │   ┌─ Account menu ───────────────────────────────────────────────┐   │
  │   │  identity · plan ($20 | $200) · remaining · resets           │   │
  │   │  Usage ▸                                                     │   │
  │   │    ┌─ Usage sheet ─────────────────────────────────────────┐ │   │
  │   │    │  SYNTH CLOUD     ← snapshot (authoritative)           │ │   │
  │   │    │  THIS DEVICE     ← usage_ledger (local only)          │ │   │
  │   │    └───────────────────────────────────────────────────────┘ │   │
  │   │  Settings · Manage billing · Log out                         │   │
  │   └──────────────────────────────────────────────────────────────┘   │
  │                                                                      │
  │   Model picker                                                       │
  │     Local ──► Laguna XS / Muse        (device)                       │
  │     Cloud ──► Synth Cloud Laguna S    (needs key + allowance)        │
  │     Ext   ──► OpenRouter …            (separate bill unless wrapped) │
  └──────────────────────────────────────────────────────────────────────┘


  TODAY (stub)                         TARGET (integrated)
  ─────────────                        ───────────────────
  signed in ≈ API key present          signed in ≈ snapshot status
  $200 seed in runtime_settings        $20 / $200 from backend catalog
  used $ = local usage_ledger          cloud used $ = Account Snapshot
  prod: often empty plan UI            prod: real plan + hosted billing
  SMR /smr/billing/* unused by menu    Snapshot may compose SMR internals
```

## One-line verdict

**Synth Cloud billing exists for Managed Research (SMR). Workshop Desktop account / plan / usage is a UI + device-pairing shell with a local/dev plan stand-in. Real “signed in → see monthly/weekly cloud allowance → manage $20/$200 billing” is not fully built or integrated.**

That is why the account menu still behaves like a stub against production: there is no shipped Desktop Account Snapshot, and the only seeded dollars are a **local/dev $200/month** ledger charged from the **device** `usage_ledger`.

---

## Why the local plan is a stub

Desktop needs an account popup that can render *something* deterministic in local/dev without waiting on the full cloud contract. So Rust `account_get_summary` (`src-tauri/src/account.rs`):

| Behavior | Detail |
| --- | --- |
| “Signed in” | ≈ `apiKeyConfigured` after device pairing — not org/session identity |
| Plan seed | `account.dev_plan.v1` in `runtime_settings`, **$200/mo** (`20_000` cents), name `Synth Dev` |
| Who gets the seed | `local` / `dev` origins only — **never** `usesynth.ai` / prod |
| Used / remaining | Sum of local `usage_ledger.cost_usd` for the UTC calendar month |
| Reset | First of next month UTC |
| Prod when signed in | Identity/plan may be empty; UI must not invent cloud dollars |

Why not just wire `/smr/billing/plan` today:

1. **Different product shape** — SMR windows (premium/value, 5h / weekly) ≠ “Desktop account menu $20/$200 monthly.”
2. **No Desktop-facing account surface** — pairing returns a key; it does not return display identity + allowance snapshot.
3. **UI could ship earlier** — menu/sign-in chrome needed a deterministic local/dev stand-in.
4. **Local must work offline** — device ledger stays, and must stay labeled **This device**, never merged into cloud totals.

---

## What is real today

### Auth / sign-in (V0)

Standing contract: `AUTH_FLOW.md`.

```text
Sign in with browser
  → POST {workshop}/api/auth/device/init
  → Clerk signup/sign-in in system browser
  → /device “Connect Synth Desktop?” approve
  → complete + poll token
  → Rust writes synth_api_key to 0600 env file
  → runtime reload → “Authenticated”
```

- Clerk owns signup/verification on the web.
- Rust is the only credential custodian; renderer never sees the long-lived key.
- Sign out clears the desktop-managed key (process-level `SYNTH_API_KEY` override is outside app custody).
- Local-only path remains first-class: **Continue locally** without an account.

V1 (planned, not the current path): OAuth 2.1 + PKCE → OS keychain → `GET /api/v1/desktop/account-snapshot`.

### Cloud models (Desktop picker)

Local and cloud targets are separate. Typical cloud-facing targets:

| Target id | Role |
| --- | --- |
| `synth-cloud-laguna-s` | Synth Cloud Laguna S 2.1 — requires API key |
| `openrouter-laguna-s` / Luna | External provider — separately billed unless backend wraps it |

Selecting Synth Cloud without a key is gated in Composer/Landing. That is **connection** gating, not plan-allowance enforcement.

Desktop hardcodes:

```text
openrouter/poolside/laguna-s-2.1
```

(`SYNTH_CLOUD_LAGUNA_S_MODEL` in `types/landing.ts`). Rust rewrites the Codex provider to `synth-cloud` and `base_url = {backend}/api/v1` from `synth_config` (`SYNTH_BACKEND_URL` / `[intern.endpoints].{profile}`).

The bundled third-party Codex client does not currently send
`max_output_tokens` on Responses requests. The governed backend therefore
defaults a missing value to the smaller of the execution-policy ceiling and
the pinned model-route ceiling, and injects that exact value into the provider
request. Explicit client values remain validated and values above either
ceiling are refused.

### Local-slot Laguna S smoke (working path for live topology)

Use a **synth-dev slot** as the Synth Cloud Responses host — this is what provider-parity `--slot-provider synth-cloud` / `--slot-base-url` exercises.

| Piece | Value (this machine, 2026-08-10) |
| --- | --- |
| Slot | `slot1` → `http://127.0.0.1:41109` (`python` / backend-api) |
| Health | `GET /health` → `{"status":"healthy","profile":"research"}` |
| Key file | `~/.synth-desktop/.env.local-slot1` (`SYNTH_API_KEY`) |
| Model | `openrouter/poolside/laguna-s-2.1` (Desktop id) |
| Call | `POST {slot}/api/v1/responses` with Bearer key |

Proven: non-stream Responses completed as `poolside/laguna-s-2.1`, assistant text `pong`.

Config already aimed at slot1 for local dogfood:

- `~/.synth-desktop/config.toml` (Applications / canonical)
- instances `dev`, `eval-1`, `laguna-gate`, `muse-glimmer` → `profile=staging`, `staging=http://127.0.0.1:41109`

**UI dogfood:** open Synth Desktop → model picker → Synth Cloud → Laguna S 2.1 → one turn. Host must resolve backend to `:41109` and have the key file above.

**Gate dogfood** (needs live Workshop instance + MLX for full parity):

```bash
# slot path only (parity still requires local-mlx second path)
cd ../evals/workshop
# --slot-model must match Desktop / what the slot admits:
#   openrouter/poolside/laguna-s-2.1
# --slot-base-url http://127.0.0.1:41109/api/v1
```

### Hosted catalog mismatch (do not assume prod = Desktop id)

Against the same `SYNTH_API_KEY`, live cloud hosts reject Desktop’s model id:

| Host | `openrouter/poolside/laguna-s-2.1` | Laguna-ish alternative |
| --- | --- | --- |
| Local slot `:41109` / `:41209` | **works** | — |
| `api-dev.usesynth.ai` | **400** not supported | lists `synth_internal/laguna-s-2.1-nvfp4` but **503** provider control plane unreachable |
| `api.usesynth.ai` | **400** not supported | **no** Laguna S in available-models list (aliases like `synth-medium` exist) |

So “Synth Cloud Laguna S” in Desktop today is honest against **local slots** (OpenRouter-routed Laguna S). It is **not** yet the same catalog entry as hosted api-dev/prod. Fixing that is a product/catalog decision (change Desktop model id vs expose `openrouter/poolside/laguna-s-2.1` on hosted), not a Desktop wiring bug on the slot path.

### Backend billing that already exists (SMR / Research)

Useful inputs, **not** the Desktop account menu contract:

- `GET /api/v1/billing/entitlements`
- `GET /smr/billing/plan`
- `GET /smr/billing/catalog`
- `GET /smr/runs/{run_id}/usage-summary`

These power Managed Research / cloud spend, not the Workshop footer popup.

### Desktop usage surfaces that exist

| Surface | Source of truth | Label |
| --- | --- | --- |
| Account menu → **Usage remaining** expand | `account_get_summary().plan` (dev seed) + local ledger rollup | Dev dollars / device tokens |
| Account menu weekly tokens | `synthInventory.listUsage` → 7-day aggregate in renderer | Device-local |
| Inventory → **Usage** | Same `usage_ledger` via inventory IPC | This device |
| Inference / Laguna telemetry | Laguna sidecar | Local runtime — not billing |

There is still copy debt (“Weekly budget: Not reported” era) until cloud snapshot lands.

---

## What is not integrated

| Gap | Notes |
| --- | --- |
| `GET /api/v1/desktop/account-snapshot` | Not shipped; required for identity + plan + usage + billing CTAs |
| `$20` and `$200` as real cloud SKUs | Product intent; Desktop only seeds **$200** in local/dev |
| Checkout / billing portal from Desktop | Need hosted URLs from backend; Desktop never takes cards |
| Org switch | Missing from Desktop account model |
| Cloud usage today / 7d / 30d | Snapshot field; must not be filled from local ledger |
| Server-side allowance enforcement for Desktop cloud turns | UI display alone is insufficient |
| Keychain session (V1) | Still V0 env-file API key |

---

## Product: $20 and $200 monthly plans

Sketch for Workshop consumer plans (names flexible; cents are authoritative):

| Tier | Monthly allowance | Intent |
| --- | --- | --- |
| **Starter — $20/mo** | 2_000 cents | Light Synth Cloud / Laguna S use; upgrade CTA when limited |
| **Pro — $200/mo** | 20_000 cents | Default “serious” Desktop cloud allowance; matches current **dev seed** |

Rules:

1. Backend owns catalog, subscription state, renews_at, and remaining.
2. Desktop only renders Account Snapshot; never hardcodes tier dollars in React for prod.
3. Local/dev seed may continue to mirror **Pro ($200)** for CUA — never seed Starter or Pro into prod profiles.
4. When allowance is exhausted: local Laguna keeps working; only billable Synth Cloud actions block with a backend-authored recovery CTA (`Upgrade` / `Manage billing`).
5. Decide explicitly whether OpenRouter usage counts against the Synth monthly dollars or is external — default: **external unless Synth wraps billing**.

Open decisions (from `AUTH_BILLING_FLOW.md`):

- Whether monthly dollars are the unit, or we also show SMR-style weekly/5h windows in the same menu.
- Individual vs org billing authority.
- Whether v0.1 ships Starter+Pro checkout or Pro-only + local/dev seed.

---

## Target UX: account menu + usage modal

### Account footer (signed in)

```text
[avatar] Display name · Signed in
  ▸ Usage remaining     → expands plan + used + remaining + resets
  ▸ Usage               → Inventory Usage (This device) and/or cloud Usage sheet
  Settings
  Log out
```

### Usage modal / sheet (desired)

Two clearly separated sections — never one blended total:

1. **Synth Cloud** (from Account Snapshot)  
   - Plan name + tier (`Starter $20` / `Pro $200`)  
   - Used / remaining / resets  
   - Today · 7 days · 30 days cloud usage  
   - Primary action: Upgrade or Manage billing (hosted URL)

2. **This device** (from local `usage_ledger`)  
   - Tokens / estimated cost this week and all-time  
   - Link into Inventory → Usage  
   - Explicit subtitle: not your Synth Cloud allowance

Signed-out / local-only: show **Sign in to Synth** / **Use Synth Cloud** instead of empty plan chrome.

### Account Settings page (later)

Profile & org · Plan & allowances · Usage · Devices & security · Advanced connection (API key / backend URL demoted here).

---

## Required API surface (Desktop-facing)

Reuse `AUTH_BILLING_FLOW.md`; minimum for this sketch:

```text
# Auth (V0 today / V1 next)
POST /api/auth/device/init|complete|token     # V0 pairing
POST /api/v1/auth/device/authorize|exchange   # V1 PKCE
POST /api/v1/auth/refresh|revoke
GET  /api/v1/account/me
GET  /api/v1/account/organizations
POST /api/v1/account/organizations/{id}/select

# Single shell contract
GET  /api/v1/desktop/account-snapshot

# Billing actions (hosted)
POST /api/v1/billing/checkout-session   # body: { plan: "starter"|"pro" }
POST /api/v1/billing/portal-session
```

Account Snapshot must include: account, organization, plan `{ tier, display_name, state, renews_at }`, allowances (or monthly used/limit/remaining), usage `{ today, seven_days, thirty_days }`, `billing_actions.{checkout_url,portal_url}`, freshness metadata, and status enum: `unauthenticated | active | limited | past_due | canceled | unknown`.

---

## Desktop implementation sketch

| Layer | Work |
| --- | --- |
| Rust `AccountManager` | Pairing/PKCE, secret store (V1), snapshot fetch/cache, emit account-change events |
| IPC | `account_get_summary` → evolve to snapshot; `account_begin_sign_in`, `account_sign_out`, `account_open_billing`, `account_select_org` |
| Renderer | States: `local_only \| signed_out \| pairing \| active \| limited \| error`; menu + Usage sheet; label scopes |
| Keep | Local `usage_ledger` + Inventory Usage as device facts |
| Kill for prod | Treating seeded `Synth Dev` $200 as cloud truth; merging device tokens into cloud remaining |

---

## Build order

1. **Backend:** Account Snapshot + Starter/Pro catalog + checkout/portal URLs + enforce cloud spend server-side.  
2. **Desktop:** Fetch snapshot when key/session present; render real plan in menu; keep local ledger labeled.  
3. **Web:** Signup/signin → choose $20/$200 → checkout → download/pair.  
4. **Usage sheet:** Cloud section from snapshot; device section from ledger.  
5. **V1 auth:** PKCE + keychain; replace env-file key as primary.  
6. **Hardening:** revoke, org switch, limited/past_due CTAs, offline cache, redaction.

---

## Acceptance (short)

- Local-only: no account → Laguna works; no fake $20/$200.  
- Local/dev seed: signed in → menu shows **$200** Synth Dev (or explicit “Dev stand-in”) charged from device ledger.  
- Prod paired: menu shows **backend** Starter/Pro; used/remaining match snapshot; device usage separate.  
- Checkout: Desktop opens only hosted URLs; no card data in-app.  
- Exhausted Pro/Starter: cloud actions blocked with Upgrade; local continues.  
- Sign out: clears cloud session/key; local history/ledger remain.  
- CUA: footer identity, plan dollars, Usage sheet scopes, Settings, Log out.

---

## Touchpoints

| Area | Path |
| --- | --- |
| Dev plan seed | `apps/synth_desktop/src-tauri/src/account.rs` |
| Sign-in IPC | `device_auth.rs`, `lib.rs` (`account_begin_sign_in` / poll / sign_out) |
| Bridge | `runtime/desktopBridge.ts`, `env.d.ts` |
| Account menu / usage expand | `components/Sidebar.tsx` |
| Device usage list | `components/InventoryPage.tsx` + usage ledger |
| Cloud model gate | `Composer.tsx`, `LandingPage.tsx`, `types/landing.ts` |
| Contracts | `AUTH_FLOW.md`, `AUTH_BILLING_FLOW.md` |

---

## Implementation status (2026-08-10)

Branches: `backend@feat/desktop-account-snapshot`, `workshop@feat/desktop-cloud-account`,
`frontend@feat/desktop-upgrade-deeplink`.

### Backend — Account Snapshot is the contract

| Endpoint | Behaviour |
| --- | --- |
| `GET /api/v1/desktop/account-snapshot` | API-key authed. Composes identity (org/user rows), plan (`free` / `starter` / `pro` mapped from the public `free` / `standard` / `max` billing ids), allowance, usage windows, hosted billing URLs, catalog, `status`, `generated_at`. |
| `GET /api/v1/desktop/plan-catalog` | Tier dollars alone, for pre-snapshot chrome. |
| `POST /api/v1/billing/checkout-session` | Body `{ plan: "starter" \| "pro" }`. Tries the provider session; falls back to the hosted web purchase URL rather than dead-ending Upgrade. |
| `POST /api/v1/billing/portal-session` | Hosted billing-management URL. |

Allowance precedence: the provider's metered spend entitlement when it reports a
limit, otherwise the catalog allowance minus month-to-date **billed** usage facts.
`limit_cents: null` means *not metered in dollars* — the shell then shows no
dollar figure at all. Each section degrades independently: a provider outage
returns `status: "unknown"` with the failed sections named in `degraded`, never a
500 that blanks the account menu.

Catalog cents are authoritative and match the web catalog: Starter `2_000`, Pro
`20_000`. `SYNTH_WEB_APP_URL` (new) points the hosted URLs at the web app.

### Desktop — snapshot in, hosted URLs out

- `account_cloud.rs`: snapshot client with a 60s TTL cache, per-connection cache
  identity (a key or backend change never reads another account's copy), schema
  guard, and stale-on-failure fallback. Errors are display-safe and never carry
  the key.
- `account.rs`: composes the shell summary. Cloud snapshot wins; **prod without a
  snapshot shows an error and no dollars**; local/dev keeps the labelled `Synth
  Dev` $200 stand-in charged from the device ledger. New state machine:
  `local_only | signed_out | active | limited | past_due | canceled | error |
  unknown` (`pairing` stays renderer-owned). A device that has paired once reads
  `signed_out` rather than `local_only`.
- IPC: `account_get_summary`, `account_refresh` (force), `account_open_billing`
  (opens a backend-issued hosted URL in the system browser), plus cache clearing
  on pair and sign-out.
- Renderer: `runtime/accountView.ts` owns presentation rules; `UsageSheet.tsx`
  renders **Synth Cloud** and **This device** as separate sections; the account
  menu shows real plan, dev-stand-in labelling, stale notes, and one recovery
  action. An exhausted allowance blocks only the billable cloud target in both
  pickers — local Laguna is untouched.

### Web — the handoff completes

`/usage?upgrade=<tier>&source=desktop` opens the plan sheet on arrival and
consumes the param; `/usage?source=desktop` is the manage-billing landing.

### Still open

| Gap | Why |
| --- | --- |
| Server-side enforcement of Desktop cloud spend | Snapshot gates the UI; the backend must still refuse billable turns. Desktop preflight is UX only. |
| Org switch (`/account/organizations`) | Snapshot reports the active org; selecting another is not built. |
| V1 OAuth 2.1 + PKCE and keychain storage | Still the V0 device-pairing env-file key. |
| Provider-hosted checkout sessions | `create_checkout_session` has no provider implementation, so checkout falls back to the hosted web flow. |
| Account Settings page (profile · plan · usage · devices) | Menu and sheet ship; the full page does not. |

---

## Out of scope for this sketch

- Muse local billing (on-device; see `muse_sidecar.md`).  
- OpenRouter as Synth-metered by default.  
- Intern async budgets (separate Intern entitlements).  
- Replacing SMR `/smr/billing/*` — keep for Research; compose into Snapshot if needed.
