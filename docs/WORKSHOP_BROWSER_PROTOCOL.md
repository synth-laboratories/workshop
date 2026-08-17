# Workshop Browser Protocol v1

Status: Playwright/Chromium reference backend implemented for development and acceptance testing. CEF is the embedded production target only after the proof-of-concept gate below. This document does not claim CEF production readiness.

## Invariants and boundary

`workshop.browser.v1` is backend-neutral. Rust domain types live in `src-tauri/src/browser/protocol.rs`; the v1 adapter and Playwright service exchange the same response envelope. A future CEF, WKWebView, or Servo service must not change agent-visible semantics.

Every observation and action identifies `sessionId`, `tabId`, `documentRevision`, `origin`, `truncated`, `stale`, and `continuationCursor`. Tab IDs are random and never reused. Element refs carry session, tab, document revision, and element id. A ref fails closed when any dimension changes. Semantic role/name locators may be used directly, but exactly one element must match.

Browser state—including Playwright handles, ref maps, profiles, tabs, dialogs, downloads, and canonical page state—remains in the browser process. The transcript receives only bounded semantic text and metadata. Snapshot/query text defaults to 16,000 characters, has a 20,000-character hard ceiling, and supports cursors and focused subtree reads. No raw DOM tool exists. Password values are omitted at the page boundary.

The Playwright child receives no Tauri IPC credential. Navigation is restricted to an operator-configured origin allowlist stored in a private Workshop app-data file and reloaded on every navigation. Only human-facing Tauri commands can mutate it; the agent's read-only `browser_status` tool can report but not change it. Localhost is allowed by host name with arbitrary development ports. Navigation, popups, dialogs, origins, and actions are written to the profile audit log. Uploads require explicit paths under a configured root; downloads use the managed profile directory. Consequential labels fail closed unless a future Workshop host confirmation broker authorizes the exact action.

Routing is:

- managed browser for ordinary sites and local web apps;
- future claimed Chrome tab only for existing authenticated Chrome state;
- signed native helper for native apps and explicitly requested Safari;
- visual fallback only for canvas/WebGL or missing semantic coverage.

## Reference backend and current limits

`apps/synth_desktop/browser/playwright_backend.mjs` launches headed persistent Chromium by default. `SYNTH_BROWSER_HEADLESS=1` exists only for automated verification. Named profiles persist cookies/storage between sessions. Session closure touches only its own context and tabs.

The reference backend is end-to-end viable in the development tree. Context Settings now reports backend/Node/Playwright/Chromium readiness and provides the human-only per-origin approval UI. Production packaging is not complete: Workshop still needs a signed, pinned Node/Playwright/Chromium runtime (or replacement host), updater integration, and installed-app verification. Until the host confirmation broker is connected, consequential browser actions remain disabled rather than silently self-approved. Claimed existing Chrome tabs are also future work.

## Acceptance evidence (2026-08-16, Apple Silicon macOS)

- Deterministic SPA test: bounded snapshot, password redaction, modal mutation, stale-ref refusal, ambiguous-locator refusal, fill, tab create/close/non-reuse, profile persistence, screenshot, cleanup, live origin approval reload, and post-revocation refusal passed.
- `example.com`: navigation, heading query, link click, and protocol `browser_back` passed.
- `usesynth.ai/evals/craftax`: focused Craftax and Trajectories reads were 1,879 and 134 characters under a 4,000-character ceiling, without truncation.
- Measured cold Chromium session startup: 277 ms. Warm navigation to the Craftax SPA: 2,375 ms.
- Measured backend RSS: 157.8 MiB; backend plus Chromium descendant RSS: 722.2 MiB. RSS is a practical one-run observation, not a steady-state guarantee.
- The JS backend source is small; the real bundle cost is the pinned Chromium/Node runtime and is not yet measured because it is not packaged.

Run deterministic verification with `node --test apps/synth_desktop/browser/playwright_backend.test.mjs`. Run the network-dependent smoke check with `node apps/synth_desktop/browser/acceptance.mjs`.

## Embedded-engine bakeoff

Use identical pages, actions, safety checks, and measurement scripts for every candidate:

1. `example.com` navigation/heading/click/back.
2. Craftax/Trajectories bounded reads without transcript compaction.
3. SPA mutation, modal, form fill, stale refs, ambiguous direct locators, and sensitive-field redaction.
4. Tab create/switch/cleanup while unrelated user tabs remain untouched.
5. Persistent profile restart and crash recovery.
6. Upload/download/dialog controls and per-origin policy.
7. Cold/warm startup, snapshot latency and size, idle/active RSS, CPU/GPU use, packaged bytes, crash containment, and 30-minute GPU stability.
8. Signed/notarized install, hardened runtime, updater replacement/rollback, and post-update profile compatibility.

Candidates:

- **CEF + cef-rs — primary embedded POC.** Gate production on real Workshop evidence for child-surface embedding, event-loop coexistence, focus/keyboard/mouse/IME, resizing and multi-display behavior, profile persistence, packaging, hardened runtime, signing/notarization, updater behavior, GPU stability, and renderer/browser crash isolation.
- **WRY/WKWebView semantic bridge — lightweight challenger.** Run in an isolated content process/boundary and prove the same semantic/ref contract; do not grant page script direct Tauri IPC.
- **Servo WebView — 2–3 day adversarial compatibility test.** Exercise authentication, modern SPAs, accessibility semantics, downloads/uploads, WebGL, and failure isolation before considering further investment.
- **Lightpanda — later background extraction only.** It is not a candidate for the visible managed browser.

No candidate advances by demo quality alone; it must pass the same acceptance suite and publish the same measurements.
