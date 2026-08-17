---
name: use-workshop-browser
description: Operate ordinary websites and local web apps in Workshop's managed browser with bounded semantic observations, stable revision-bound refs, and persistent profiles. Use native Computer Use only for macOS apps or when Safari is explicitly requested.
---

# Workshop Browser

Use the `mcp__synth_browser__browser_*` tools for ordinary websites and local development. Browser state stays in the browser service; never copy an entire page or raw DOM into the conversation.

Routing is strict:

- Managed Workshop Browser: ordinary websites and local web apps.
- Claimed Chrome tab: only when the user needs existing authenticated Chrome state, the operator explicitly enabled it, Chrome exposes a loopback CDP endpoint, and the exact claim receives human approval. Never use it by default.
- Native Computer Use: native macOS apps and explicitly requested Safari.
- Screenshots: fallback for canvas/WebGL or when semantic reads are insufficient.

Call `browser_status` first when runtime readiness or origin approval is uncertain; it does not start Chromium and cannot change policy. Then use `browser_create_session`, reusing a named profile when persistence matters. Keep the returned `sessionId` and `tabId`. Navigate only to origins the operator has approved in Workshop settings; an `origin_not_approved` refusal is final until the human changes that setting.

Read narrowly:

- `browser_query` when role/name is known.
- `browser_snapshot` for a bounded page overview. Default `max_chars` is 16,000 and the hard ceiling is 20,000.
- `browser_subtree` for focused continuation beneath a result.
- Follow `continuationCursor` only when the missing portion is necessary.
- Never seek or request raw full DOM.

Targets are either a revision-bound `ref` assembled from the response metadata plus an element id such as `e1`, or a semantic `locator` such as `{"locator":{"role":"button","name":"Save","exact":true}}`. Direct locator actions are allowed without a prior snapshot, but ambiguity fails closed. A ref is bound to its session, tab, and document revision; after navigation or DOM mutation, discard it and query again.

Actions are `browser_click`, `browser_fill`, `browser_press`, `browser_scroll`, and `browser_back`. Values filled into credential fields must come from the user/browser flow and must never be repeated in chat. Upload only explicit files beneath a folder the human selected in Context Settings. Downloads go only to the managed profile destination. Use `browser_list_dialogs` and `browser_handle_dialog` for native page dialogs, and `browser_audit` for a bounded event tail. Send, publish, purchase, delete, submit, dialog acceptance, upload, and similar consequential actions require an exact Workshop confirmation; never work around a refusal.

Use `browser_claim_chrome` only for the authenticated-state exception above. Provide enough title/URL text to match exactly one existing tab. A disabled, ambiguous, non-loopback, or denied claim is final. Never close or repurpose the user's claimed tab; Workshop preserves it when the claimed session closes.

Use `browser_new_tab`, `browser_list_tabs`, and `browser_close_tab` to manage only Workshop-created tabs. Close the session when the task is complete unless the user asked to keep it running. Never touch unrelated user tabs.
