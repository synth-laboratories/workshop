# Handoff: local/dev account, $200/month plan, account menu, and UI invariants

Date: 2026-08-10

## Requested outcome

1. Workshop should be genuinely signed into the local/dev Synth backend (not a renderer-only flag).
2. That dev account should have an authoritative **$200/month** allowance.
3. The bottom-left account control should open a compact Codex-style menu with:
   - account identity / signed-in state
   - usage remaining and tracked usage
   - Settings
   - Log out
4. Add Playwright and Bombadil coverage, including the model-picker layout failure in the 12:54 screenshot.
5. Install/restart the canonical app and confirm through CUA.

## Critical shared-worktree warning

Another engineer/merge process is actively editing `App.tsx`. At handoff time it contains unresolved conflict markers at approximately lines 1150, 1259, and 2145 (`HEAD` versus `codex/context-compaction`). Do **not** blindly replace or auto-resolve this file. Preserve both the context-compaction work and the account-usage work.

Run before editing:

```bash
rg -n '^(<<<<<<<|=======|>>>>>>>)' apps/synth_desktop/src/renderer/src/App.tsx
git status --short
```

## Work already present

### Account menu renderer

`src/renderer/src/components/Sidebar.tsx` currently contains a first-pass account footer/menu:

- `accountSignedIn`, `accountDisplayName`, `accountUsage`, `onOpenAccount`, `onSignOut` props
- bottom-left `account-menu-trigger`
- popup containing `Usage remaining`, account management, `Settings`, and `Log out`
- expandable local usage section
- outside-click and Escape dismissal with focus restoration

`src/renderer/src/styles/app.css` contains the corresponding `.account-*` styles.

This first pass currently says `Weekly budget: Not reported`. Replace that with real backend plan data once the contract below exists. Do not hardcode "$200" only in React.

### Usage ledger aggregation

`src/renderer/src/App.tsx` has `summarizeAccountUsage(entries)` and loads `synthInventory.listUsage(2000)` to derive rolling seven-day and total tracked usage. Preserve this while resolving the file.

### Playwright edits

`tests/playwright/account-sign-in.spec.ts` was updated to enter Account Settings through the popup and to assert Usage/Settings/Log out. It has not been rerun after the concurrent merge.

## What is not implemented

### Real local/dev account + plan

Current account state is inferred from `BackendSettings.apiKeyConfigured`; device auth only returns/stores an API key. There is no renderer contract for account identity, plan, allowance, used amount, or reset date.

Implement an authoritative account summary, ideally:

```ts
type SynthAccountSummary = {
  signedIn: boolean;
  accountId?: string;
  displayName?: string;
  environment: "local" | "dev" | "prod";
  plan?: {
    name: string;
    monthlyAllowanceUsd: number;
    usedUsd: number;
    remainingUsd: number;
    resetsAt: string;
  };
};
```

Recommended ownership:

- Rust/backend owns authentication, identity, plan, and usage truth.
- Renderer only renders `account_get_summary()` and subscribes to account-change events.
- Local/dev provisioning should live in the local backend database/config, not `localStorage`.
- Seed the intended local dev user with a 20,000-cent monthly allowance and a deterministic monthly reset boundary.
- Enforce the allowance server-side for billable cloud operations; UI display alone is insufficient.
- Never log or expose the API key.

Relevant existing paths:

- `src-tauri/src/device_auth.rs`
- `src-tauri/src/lib.rs` (`account_begin_sign_in`, `account_poll_sign_in`, `account_sign_out`)
- `src/renderer/src/runtime/desktopBridge.ts`
- `src/renderer/src/env.d.ts`
- `src/renderer/src/components/BackendSettings.tsx`
- `src/renderer/src/App.tsx`
- `src/renderer/src/components/Sidebar.tsx`

First determine which local/dev backend instance and datastore the canonical app is using. Provision there through a checked-in dev seed/migration or explicit dev-only command. Do not silently mutate production.

## Bombadil invariant requested from 12:54 screenshot

The screenshot shows the model picker extending downward behind/through the composer and off the usable viewport. Add an invariant suite that fuzzes viewport size, sidebar/terminal visibility, composer height, and picker scroll position.

Minimum invariants while the model picker is open:

- picker bounding box remains inside the app viewport with at least 8 px inset
- picker does not overlap the composer rectangle
- picker is fully reachable; if content is taller than available space, the list scrolls internally
- picker flips above/below its trigger according to available space
- selected item remains visible after opening and after viewport/layout changes
- local, OpenRouter, and Synth Cloud groups remain visible/reachable
- no horizontal clipping or body-level overflow
- opening/closing does not move the composer or landing hero
- invariants hold at narrow/short windows and with the terminal open

Suggested Bombadil extraction:

```ts
const pickerLayout = extract((state: any) => {
  const picker = state.document.querySelector('[data-testid="model-picker"]');
  const composer = state.document.querySelector('[data-testid="composer"]');
  if (!picker || !composer) return { open: false };
  const p = picker.getBoundingClientRect();
  const c = composer.getBoundingClientRect();
  return {
    open: true,
    insideViewport: p.left >= 8 && p.top >= 8 && p.right <= innerWidth - 8 && p.bottom <= innerHeight - 8,
    overlapsComposer: !(p.right <= c.left || p.left >= c.right || p.bottom <= c.top || p.top >= c.bottom),
    bodyOverflowX: state.document.documentElement.scrollWidth > innerWidth
  };
});
```

Then enforce `always(() => !open || (insideViewport && !overlapsComposer && !bodyOverflowX))` and drive layout mutations similar to the existing `tests/bombadil/layout.spec.ts` suite.

Also add Playwright assertions using real `boundingBox()` values for at least:

- 1728×1117 normal window
- 1100×700 short window
- terminal open
- sidebar hidden/shown

## Account tests required

### Rust/integration

- local/dev sign-in persists across app restart
- account summary returns the dev identity and `$200.00` monthly allowance
- used/remaining arithmetic is exact and clamped at zero
- reset boundary advances monthly and is timezone-safe
- sign-out clears credentials and summary becomes signed out
- production profiles cannot receive the dev seed accidentally

### Playwright

- footer says signed in after loading the real/stubbed summary
- popup shows identity, `$200 monthly`, used, remaining, and reset date
- Settings navigates correctly
- Log out updates footer without reload
- Escape/outside click closes and restores trigger focus
- menu remains inside the sidebar/window at minimum supported size

### Bombadil

- account popup never crosses viewport/sidebar bounds
- footer trigger stays visible while chat/sidebar content grows
- plan arithmetic displayed by the UI never yields negative remaining or more than allowance
- signed-out state never exposes Log out or stale plan data

## Verification/handoff checklist

1. Resolve concurrent `App.tsx` conflicts deliberately.
2. Run typecheck and focused account/model-picker Playwright tests.
3. Run Bombadil layout suite with a bounded time limit.
4. Run the full desktop install gate; do not bypass unrelated failures silently.
5. Restart the canonical installed app.
6. CUA-confirm:
   - footer is signed into local/dev
   - popup shows real $200/month plan and usage
   - Settings and Log out work
   - model picker remains contained at normal and short window sizes
7. Append results and any remaining debt to `polish.md`.

## Last observed UI

The installed app did show the new popup shell, but it remained signed out (`Sign in to Synth / Local mode`). The $200 plan was not provisioned. The model picker still overflowed behind the composer in the supplied screenshot. Treat both as open acceptance failures.
