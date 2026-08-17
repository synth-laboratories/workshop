# Workshop Browser Protocol v1

Status: Playwright/Chromium reference backend implemented for development and acceptance testing. CEF is the embedded production target only after the proof-of-concept gate below. This document does not claim CEF production readiness.

## Invariants and boundary

`workshop.browser.v1` is backend-neutral. Rust domain types live in `src-tauri/src/browser/protocol.rs`; the v1 adapter and Playwright service exchange the same response envelope. A future CEF, WKWebView, or Servo service must not change agent-visible semantics.

Every observation and action identifies `sessionId`, `tabId`, `documentRevision`, `origin`, `truncated`, `stale`, and `continuationCursor`. Tab IDs are random and never reused. Element refs carry session, tab, document revision, and element id. A ref fails closed when any dimension changes. Semantic role/name locators may be used directly, but exactly one element must match.

Browser state—including Playwright handles, ref maps, profiles, tabs, dialogs, downloads, and canonical page state—remains in the browser process. The transcript receives only bounded semantic text and metadata. Snapshot/query text defaults to 16,000 characters, has a 20,000-character hard ceiling, and supports cursors and focused subtree reads. No raw DOM tool exists. Password values are omitted at the page boundary.

The Playwright child receives no Tauri IPC credential. Navigation is restricted to an operator-configured origin allowlist stored in a private Workshop app-data file and reloaded on every navigation. Only human-facing Tauri commands can mutate it; the agent's read-only `browser_status` tool can report but not change it. Localhost is allowed by host name with arbitrary development ports. Navigation, popups, dialogs, origins, and actions are written to the profile audit log. Uploads require explicit paths under a folder selected with Workshop's native picker; downloads use the managed profile directory. The host prepares every action against one tab/document revision, asks the existing durable approval broker for consequential actions, and consumes a one-time token before execution. Changed documents, expired/reused tokens, ambiguous locators, and already-handled dialogs fail closed.

Routing is:

- managed browser for ordinary sites and local web apps;
- opt-in claimed Chrome tab only for existing authenticated Chrome state; the loopback CDP endpoint and exact title/URL match require human approval, and Workshop never closes the claimed user tab;
- signed native helper for native apps and explicitly requested Safari;
- visual fallback only for canvas/WebGL or missing semantic coverage.

## Reference backend and current limits

`apps/synth_desktop/browser/playwright_backend.mjs` launches headed persistent Chromium by default. `SYNTH_BROWSER_HEADLESS=1` exists only for automated verification. Named profiles persist cookies/storage between sessions. Session closure touches only its own context and tabs.

The reference backend is end-to-end viable in the development tree. Context Settings reports backend/Node/Playwright/Chromium readiness, service/crash state, restart control, per-origin approvals, and native upload-folder selection. The host owns and supervises the backend process; the MCP adapter is an authenticated loopback proxy. A pinned Node 24.18.0, Playwright 1.62.1, and full headed Chromium 151.0.7922.34 runtime is assembled from `runtime.lock.json`, checksummed, pruned, and ready for Developer-ID signing. The current Apple Silicon assembly is 491 MiB. `scripts/verify-browser-production-gates.sh` is the fail-closed installed-app, updater/profile, and live-helper gate. Developer ID credentials, notarization credentials, an installed update pair, and live TCC grants are external release inputs, so this source change does not itself claim those gates passed.

Claimed Chrome is implemented but deliberately off by default (`SYNTH_BROWSER_ENABLE_CHROME_CLAIM=1`). Chrome must itself be launched with a loopback debugging endpoint. Claiming requires exact host approval and exactly one title/URL match; closing the Workshop session preserves the claimed tab and closes only tabs Workshop created. This is a privileged compatibility path, not the default browser route.

## Acceptance evidence (2026-08-17, Apple Silicon macOS)

- Deterministic SPA test: bounded snapshot, password redaction, modal mutation, stale-ref refusal, ambiguous-locator refusal, fill, tab create/close/non-reuse, profile persistence, screenshot, cleanup, live origin approval reload, and post-revocation refusal passed. Immediate ref reuse after an input action is invalidated synchronously rather than depending on MutationObserver scheduling.
- `scripts/browser-workshop-e2e.mjs` passed through the public MCP adapter, authenticated loopback IPC, Rust service/approval boundary, and Playwright backend. It additionally covered destructive-action and upload refusal without an owning agent session, native-dialog enumeration and dismissal, cross-origin refusal, controlled download, audit redaction, backend SIGKILL detection, and restart recovery.
- `example.com`: navigation, heading query, link click, and protocol `browser_back` passed.
- `usesynth.ai/evals/craftax`: focused Craftax and Trajectories reads were 1,879 and 134 characters under a 4,000-character ceiling, without truncation.
- Measured cold Chromium session startup ranged from 490 ms to 2,646 ms through the complete Workshop path (the high sample followed a fresh desktop rebuild); the direct public-site harness measured 712 ms. Warm navigation to the Craftax SPA was 2,526 ms.
- Latest measured backend RSS was 172.5 MiB; backend plus Chromium descendant RSS was 1,168.2 MiB. RSS is a practical one-run observation, not a steady-state guarantee and varies substantially with Chromium process state.
- Closing the Rust client now gives the backend up to five seconds to close persistent contexts before force termination. The regression test immediately reopens the same persistent profile after stdin EOF, guarding against orphaned Chromium processes and `ProcessSingleton` locks.
- The complete desktop Rust suite passed serially (775 passed, 3 ignored), the native helper suite passed (34), renderer source tests passed (254), and frontend typecheck/build passed. The desktop suite's default parallel run exposed two pre-existing shared-state Codex session-test races; both passed individually and in the full serial run.

Run deterministic verification with `node --test apps/synth_desktop/browser/playwright_backend.test.mjs`. Run the network-dependent smoke check with `node apps/synth_desktop/browser/acceptance.mjs`. The full-path harness requires a deliberately isolated named Workshop instance and accepts its app-data root, MCP adapter path, and app PID as arguments; it must never be pointed at another user's running instance.

## Embedded-engine bakeoff and CEF gate

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

- **CEF + cef-rs — primary embedded POC.** Gate production on real Workshop evidence for child-surface embedding, event-loop coexistence, focus/keyboard/mouse/IME, resizing and multi-display behavior, profile persistence, packaging, hardened runtime, signing/notarization, updater behavior, GPU stability, and renderer/browser crash isolation. A protocol adapter or compile-only demo is not a pass. The POC must run inside a signed Workshop build and publish the acceptance receipt and measurements above.
- **WRY/WKWebView semantic bridge — lightweight challenger.** Run in an isolated content process/boundary and prove the same semantic/ref contract; do not grant page script direct Tauri IPC.
- **Servo WebView — 2–3 day adversarial compatibility test.** Exercise authentication, modern SPAs, accessibility semantics, downloads/uploads, WebGL, and failure isolation before considering further investment.
- **Lightpanda — later background extraction only.** It is not a candidate for the visible managed browser.

No candidate advances by demo quality alone; it must pass the same acceptance suite and publish the same measurements.
The machine-checkable receipt gate is `scripts/check-embedded-browser-bakeoff.sh RECEIPT.json`; it requires all embedding, input, persistence, signing/updater, stability, protocol, and measurement fields above.

CEF remains **not production-ready** in this branch: no CEF distribution, Developer ID identity, notarization profile, or installed updater pair is present in the worktree, and no child-surface/GPU/crash-isolation receipt exists. Those are evidence gates, not safe assumptions the implementation can substitute for.

The executable preflight is `scripts/cef-workshop-poc.sh preflight`. It records host/toolchain/signing blockers as a machine-readable receipt and intentionally exits non-zero until all inputs exist. On the August 17 development host, macOS ARM64 and Rust are available, but only Command Line Tools are selected: `xcodebuild` reports that full Xcode is required. A development signing identity exists, but there is no notary profile. The current `cef-rs` release supports macOS ARM64, but downloading or running `cefsimple` alone would not satisfy this gate; the first accepted receipt must come from a child surface inside Workshop and cover the measurements above.

## Production acceptance evidence (August 17)

- The pinned runtime passes checksum verification plus executable Node, Chromium, and one-page Playwright browser/renderer probes.
- A freshly rebuilt 0.5.0 application contains the exact tested backend, runtime and `synth-browser-mcp`; its final ad-hoc resource seal and development-installed receipt passed. This is a non-notarized packaging smoke only. After preserving framework symlinks, the staged app was 548 MiB on disk and the runtime reported 513,224,704 installed bytes.
- The packaging smoke found that Tauri's generic resource copy dereferenced Chromium's versioned framework symlinks. `scripts/finalize-browser-app.sh` now replaces only the generated bundle copy using `ditto`, seals the outer app, launches Node/Chromium/browser-renderer probes, and then repeats strict deep verification. The npm build, canonical desktop build, and named CUA build paths all invoke this finalizer.
- `scripts/browser-profile-compat.mjs` created browser state with one packaged backend, closed it, and reopened the same persistent profile with a second packaged backend. The development same-build compatibility receipt passed. The production updater gate now runs the same semantic check and additionally requires distinct signed/notarized app versions and the existing profile sentinel.
- Claimed Chrome passed a live loopback-CDP test: claims are disabled by default, require one exact title/URL match, reject non-loopback discovery, and preserve the user's tab during session cleanup.
- The staged native helper passes the `development-helper-live` code-seal and live probe gate with Accessibility and Screen Recording granted on this host. Its receipt is explicitly non-production because the helper is ad-hoc signed; `helper-live` still requires the signed/notarized production artifact.
- Developer ID/notarization, a real installed updater before/after pair, and the CEF Workshop embedding receipt remain external evidence gates. No production-readiness claim is made for them.
