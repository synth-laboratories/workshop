# Handoff — Synth Cloud account, plan, usage, and billing (CUA QA)

**For:** an engineer who will drive the real app with CUA and verify every claim below.
**Date:** 2026-08-10
**Spec this implements:** [`synth_cloud_api_usage.md`](synth_cloud_api_usage.md) · contracts: [`../../AUTH_FLOW.md`](../../AUTH_FLOW.md), [`../../AUTH_BILLING_FLOW.md`](../../AUTH_BILLING_FLOW.md)

---

## 0. Read this first: the screenshot that started this handoff

A screenshot of the account menu at 12:32 showed the **old** UI — "Used this month",
"Tracked this week", no `Usage` row, no dev-stand-in label. That was not a bug in the
feature; it was a **stale window**. The running bundle was launched at 11:55 and the
merge landed at 12:28. `Used this month` / `Tracked this week` exist only in the
pre-merge `Sidebar.tsx`; the merged build says `Used this period` / `This device, this
week`.

**Before QA'ing anything, rebuild and relaunch.** If you see the old strings, you are
looking at a stale process — quit every running `Synth Desktop`, rebuild, relaunch.
This is the single most likely way to waste an hour here.

---

## 1. What landed, and where

All three repos are merged into local `dev`. **Nothing is pushed.**

| Repo | `dev` commit | Contents |
|---|---|---|
| workshop | `03ac8de` (merge), `579e734` (Account page) | Rust snapshot client + shell states, renderer account view, Usage sheet, model gating, consolidated Settings → Account |
| backend | `8f11d139a` (merge of `feat/desktop-account-snapshot`) | Account Snapshot + billing session endpoints |
| frontend | `b87262a9` (merge of `feat/desktop-upgrade-deeplink`) | `/usage?upgrade=<tier>` opens the plan sheet |

Backend `dev` lives in the worktree `~/Documents/Codex/2026-08-04/intern-e2e-watch/work/backend-dev-merge`;
frontend `dev` in the sibling `frontend-dev-merge`. Workshop `dev` is the primary checkout.

### The contract in one paragraph

Desktop renders **one** backend document, `GET /api/v1/desktop/account-snapshot`. It
carries identity, organization, plan (`free|starter|pro`), allowance, usage windows
(today/7d/30d), hosted billing URLs, and the tier catalog. Desktop hardcodes no prices,
composes nothing from raw billing endpoints, and never takes card details — Upgrade and
Manage billing open backend-issued URLs in the system browser.

### Files worth reading before you test

```
backend  app/api/v1/routes_desktop_account.py      # snapshot composition, allowance precedence
backend  app/api/v1/routes_billing.py              # checkout/portal sessions
workshop apps/synth_desktop/src-tauri/src/account_cloud.rs   # fetch, cache, staleness
workshop apps/synth_desktop/src-tauri/src/account.rs         # shell state machine
workshop apps/synth_desktop/src/renderer/src/runtime/accountView.ts  # presentation rules
workshop apps/synth_desktop/src/renderer/src/components/UsageSheet.tsx
workshop apps/synth_desktop/src/renderer/src/components/AccountPage.tsx
```

---

## 2. Build and launch (do this first)

```bash
# 1. Kill stale windows — there are usually several.
pkill -f "Synth Desktop" || true

# 2. Workshop dev
cd ~/Documents/GitHub/workshop
git log --oneline -1                      # expect 579e734 or later on dev
npm run --prefix apps/synth_desktop frontend:build
cd apps/synth_desktop && npm run dev       # or scripts/desktop-instance.sh dev codex
```

**Confirm you are on the new build before testing anything else:** open the account
menu — it must contain a `Usage` row beneath `Usage remaining`, and the expanded panel
must say **"Used this period"** and **"This device, this week"**. If it says "Used this
month", stop and rebuild.

### Backend for the cloud path

The snapshot is API-key authed. Run the backend from the `dev` worktree:

```bash
cd ~/Documents/Codex/2026-08-04/intern-e2e-watch/work/backend-dev-merge
# your usual local API run (uvicorn app.api.app:create_app --factory --port 8000)
```

Point Desktop at it: Settings → Account → **Advanced connection** → Profile `Local`,
Backend API `http://127.0.0.1:8000`, save. In local/dev the backend accepts the local
dev API key (`core/auth.py`, `_LOCAL_DEV_API_KEY`) when `APP_ENVIRONMENT=local|dev`.

Sanity-check the contract without the app:

```bash
curl -s -H "Authorization: Bearer $SYNTH_API_KEY" \
  http://127.0.0.1:8000/api/v1/desktop/account-snapshot | jq
curl -s -X POST -H "Authorization: Bearer $SYNTH_API_KEY" \
  -H 'content-type: application/json' -d '{"plan":"pro"}' \
  http://127.0.0.1:8000/api/v1/billing/checkout-session | jq
curl -s -X POST -H "Authorization: Bearer $SYNTH_API_KEY" \
  http://127.0.0.1:8000/api/v1/billing/portal-session | jq
```

Expected shape: `schema_version: "synth.desktop-account.v1"`, `status` in
`active|limited|past_due|canceled|unknown`, `plan.tier` in `free|starter|pro`,
`allowance.source` in `entitlement|catalog_minus_usage|none`, `billing_actions.*` URLs
pointing at the web app, `catalog[]` with Starter $20 / Pro $200 in **cents**.

---

## 3. The invariants you are checking

These are the product rules. Everything in §4 is a way of trying to break one.

1. **Prod never invents dollars.** No snapshot, or an unmetered account → no dollar
   figure anywhere. Not `$0.00`, not a seeded plan — nothing.
2. **The dev stand-in is always labelled.** The local/dev `Synth Dev` $200 plan is
   charged from the device ledger and must say so wherever it appears.
3. **Cloud and device usage never blend.** Two sections, two labels, never one total.
4. **Local never breaks.** Exhausted allowance, past due, cancelled, backend down,
   signed out — local Laguna keeps working and stays selectable.
5. **Desktop never takes card details.** Upgrade/Manage billing open a browser URL
   issued by the backend, and nothing else.
6. **The key never reaches the renderer.** No API key in any DOM node, IPC response,
   console log, or error string.

---

## 4. CUA checklist

Each row: drive the real app, observe, record pass/fail with a screenshot. Testids are
given because CUA can assert on them, but **verify the visible text too** — a testid can
be right while the copy lies.

### A. First run and local-only

| # | Steps | Expected |
|---|---|---|
| A1 | Fresh profile (`SYNTH_DESKTOP_INSTANCE` pointing at a clean data root), launch | First-run choice offers **Continue locally** and **Sign in to Synth** as equals (`first-run-account-choice`) |
| A2 | Choose Continue locally | Footer trigger reads **Sign in to Synth** / **Local mode**; no plan, no dollars |
| A3 | Open the account menu | No **Log out** row; `Usage remaining` expands to "Sign in to Synth to see a cloud allowance" — never `$0.00` |
| A4 | Menu → **Usage** | Sheet opens; Synth Cloud section shows the sign-in invitation (`usage-sheet-signed-out`), device section still shows real device tokens |
| A5 | Send a message to local Laguna | Works with no account |

### B. Pairing (V0 device flow)

| # | Steps | Expected |
|---|---|---|
| B1 | Settings → Account → Devices & security → **Sign in with browser** | System browser opens the Workshop `/device` approval page; status reads "Finish sign-in in your browser" |
| B2 | Approve in the browser | Within ~4s the desktop flips: Account header badge → `Signed in`, `account-sign-in-note` shows "Signed in · runtime reconnected" |
| B3 | Cancel mid-pairing instead | Returns to the idle affordance; no partial state |
| B4 | After signing in, **quit and relaunch** | Still signed in; identity comes back from the snapshot, not from a cached UI string |
| B5 | Sign out | Cloud state clears; local chats, history, and Inventory usage remain |

### C. Cloud account (the main event)

| # | Steps | Expected |
|---|---|---|
| C1 | Signed in against the local backend, open the account menu | Identity row shows the **snapshot's** display name and organization, not "Synth Dev" |
| C2 | Expand `Usage remaining` | Plan name, `Monthly allowance`, `Used this period`, `Remaining`, `Resets` — all matching the `curl` snapshot to the cent |
| C3 | Menu → **Usage** | Sheet shows **SYNTH CLOUD** (plan, used, remaining, today/7d/30d, `Last updated`) and **THIS DEVICE** (tokens, estimated cost, link to Inventory) as separate sections with the "not your Synth Cloud allowance" subtitle |
| C4 | Compare against the backend | `usage-sheet-today/7d/30d` equal `usage.*.billed_cents / 100` from the snapshot |
| C5 | Settings → Account | Profile & organization → Plan & allowances → Usage → Devices & security, in that order, then a collapsed **Advanced connection** |
| C6 | Expand Advanced connection | The old endpoint/env/key editor, unchanged, badge showing `Authenticated` |
| C7 | Inventory → Usage | Device rows only; nothing here should reference the cloud allowance |

### D. Money paths

| # | Steps | Expected |
|---|---|---|
| D1 | **Manage billing** (menu, sheet, or Account page) | System browser opens `<web>/usage?source=desktop`. No in-app payment UI anywhere |
| D2 | **Upgrade** on a free/starter account | Browser opens `<web>/usage?upgrade=<tier>&source=desktop` **and the plan sheet opens by itself** on that page (frontend deep link). Refreshing the page must not reopen it |
| D3 | Complete or cancel checkout, return to Desktop | Within a few seconds the snapshot refetches (also try Account → **Refresh**) and the plan updates |
| D4 | Inspect network/logs during D1–D3 | No card data, no key, in any request Desktop makes |

### E. Limited / past due / cancelled

Force these by editing the org's plan or allowance in the backend, or by stubbing
`status` in the snapshot response.

| # | Steps | Expected |
|---|---|---|
| E1 | `status: limited` | Menu shows "…monthly allowance is used up. Local models keep working."; primary action is **Upgrade** |
| E2 | Open the model picker (landing and composer) | `Synth Cloud Laguna S` is disabled with the same reason and a **Manage plan** button; **Laguna XS stays selectable and usable** |
| E3 | Click **Manage plan** | Usage sheet opens on the blocked state with the Upgrade action |
| E4 | `status: past_due` | Copy says billing needs attention; action is **Manage billing**, not Upgrade |
| E5 | `status: canceled` | Copy says the plan is no longer active; local still works |
| E6 | Try to send a cloud turn while limited | Composer is disabled with the blocked copy — and note in your report whether the **backend** also refuses it (see §6 gap 1) |

### F. Failure and edge states

| # | Steps | Expected |
|---|---|---|
| F1 | Kill the backend, reopen the menu within 60s | Cached snapshot still renders (TTL) |
| F2 | Kill the backend, wait >60s, hit **Refresh** | Plan still shown, plus "Showing the last known plan — Synth Cloud is unavailable right now"; `stale` note visible |
| F3 | Point at a **prod** backend with a bad key | Menu shows the error state, **no dollars at all**, action is **Retry**; error text says "sign in again" and contains no key material |
| F4 | Point at a backend without the endpoint (404) | "This Synth backend does not serve the desktop account snapshot yet" — not a crash, not a blank menu |
| F5 | Return a snapshot with `schema_version: v2` | Desktop refuses it and asks the user to update — it must not render a half-parsed plan |
| F6 | `allowance.limit_cents: null` | Plan name renders, "not metered in monthly dollars", **no** `$0.00` anywhere |
| F7 | Switch API keys (sign out, sign in as another account) | No figure from the previous account survives; the cache is per (backend, key) |
| F8 | Local/dev profile with the backend down | `Synth Dev` $200 appears **with** the "Dev stand-in — charged from this device" label |
| F9 | Prod profile with the backend down | No `Synth Dev`, no dollars — prod is never seeded |

### G. Accessibility and chrome

| # | Steps | Expected |
|---|---|---|
| G1 | Keyboard only: Tab to the account trigger, Enter, arrow through, Esc | Menu opens/closes, focus returns to the trigger |
| G2 | Usage sheet: Esc, click outside, close button | All three close it; focus lands somewhere sensible |
| G3 | Account page at a narrow window | Sections stack; no horizontal scroll; numbers stay right-aligned |
| G4 | Look at the footer trigger and the menu identity row together | **Known cosmetic issue:** identity is rendered twice (menu header + trigger) and the trigger carries a `?` glyph. Pre-existing; confirm whether it should be cleaned up |

---

## 5. Automated coverage that already exists

Run these first — if one fails, the app is in a state the checklist cannot interpret.

```bash
# Rust: snapshot parsing, cache identity, staleness, summary composition (20 tests)
cd ~/Documents/GitHub/workshop/apps/synth_desktop/src-tauri && cargo test --lib account

# Renderer view-model rules (11 tests)
cd ~/Documents/GitHub/workshop && node --test apps/synth_desktop/tests/account_view.test.mjs

# Account UI, usage sheet, gating, consolidated Account page (7 tests)
cd apps/synth_desktop && ../../node_modules/.bin/playwright test --config playwright.config.ts \
  tests/playwright/account-cloud-usage.spec.ts tests/playwright/account-sign-in.spec.ts

# Backend snapshot composition (17 tests)
cd ~/Documents/Codex/2026-08-04/intern-e2e-watch/work/backend-dev-merge && \
  .venv/bin/python -m pytest tests/units/test_desktop_account_snapshot.py -q
```

### Known-failing before you start (verified identical on an untouched `dev`)

Do not chase these; they are not this feature:

- `laguna::tests::daemon_env_pins_7333_and_clears_every_upstream_variable` — fails in any
  git worktree (reads the machine's selected model; Muse Glimmer sets
  `SYNTH_LAGUNA_EXTERNAL_URL`), passes in the primary checkout.
- Playwright: 12 failures across `design-debt`, `gaps`, `poolside-polish`, plus 3 in
  `sidebar-navigation`. The full suite also aborts early on maxFailures and flakes specs
  that pass in isolation — run per-spec.
- `node --test apps/synth_desktop/tests/*.test.mjs`: 1 pre-existing failure ("v0.2 Intern
  bridge remains typed").

---

## 6. Gaps — expected to fail, do not file as bugs

These are known and deliberate; the checklist asks you to *confirm* them, not fix them.

1. **No server-side enforcement of Desktop cloud spend.** The snapshot gates the UI. A
   determined caller with a key can still spend past the allowance. E6 asks you to
   record what the backend actually does today.
2. **No org switch.** The snapshot reports the active org; there is no picker.
3. **Still V0 auth** — device pairing into a `0600` env file. No OAuth 2.1 + PKCE, no
   keychain.
4. **Provider-hosted checkout is not implemented** (`AutumnClient` has no
   `create_checkout_session`), so `/billing/checkout-session` always returns the hosted
   web URL with `mode: "hosted_web"`. That is the intended fallback, not a failure.
5. **`pairing` state is renderer-owned** and only reflected in Settings, not in the
   footer menu.
6. **Duplicate identity chrome** in the footer (G4).

---

## 7. What to report back

For each checklist row: pass/fail, screenshot, and for any failure the exact snapshot
JSON (`curl` output) alongside what the UI showed. The most valuable failures are ones
where **the UI and the snapshot disagree** — that is the whole contract. Second most
valuable: any place where a dollar figure appears that the backend did not report, or
where cloud and device numbers are added together.
