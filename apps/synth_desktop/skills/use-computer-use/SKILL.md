---
name: use-computer-use
description: Operate allowed native macOS apps, including Safari and accessible webpages, through mcp__synth_computer_use__computer_use. Use for GUI tasks that require reading app state, clicking, typing, selecting, scrolling, dragging, or pressing keys. Do not use shell commands, logs, prior sessions, or filesystem searches to discover this tool's syntax; this skill contains the complete contract.
---

# Use Computer Use

Use only `mcp__synth_computer_use__computer_use` to operate the GUI. Never search files, logs, source code, or prior sessions to learn its syntax. This skill is the complete operating guide.

Use this native path for macOS applications and when the user explicitly asks for Safari. For ordinary websites and local web development, use the managed Workshop Browser skill instead. A screenshot is a fallback for canvas/WebGL, not the default observation path.

Every call takes a `verb`. Every verb except `list_apps` also takes an `app` bundle identifier such as `com.apple.Safari` or `com.apple.mail`.

Always pass the `id` returned by `list_apps`, never its `displayName`. For Safari, use exactly `"app": "com.apple.Safari"`; never pass `"Safari"`. `list_apps` returns `pids` for each identifier. If more than one pid is listed, do not guess — `launch` starts a new copy; driving requires a single running process.

Check `mcp__synth_computer_use__computer_use_status` first. It is advertised even when the plugin is not ready, so a refusal tells you which permission is missing instead of leaving you to guess.

## Verbs

- `list_apps`: `{ "verb": "list_apps" }`
- `launch`: `{ "verb": "launch", "app": "com.apple.mail" }` — starts a new copy and returns the pid it created. Never a side effect of a read.
- `get_app_outline`: `{ "verb": "get_app_outline", "app": "com.apple.Safari", "max_chars": 4000 }`
- `find_elements`: `{ "verb": "find_elements", "app": "com.apple.Safari", "role": "AXButton", "name": "Back" }`
- `get_subtree`: `{ "verb": "get_subtree", "app": "com.apple.Safari", "element_index": 12, "depth": 3, "max_chars": 6000 }`
- `get_app_state`: `{ "verb": "get_app_state", "app": "com.apple.Safari", "disable_diff": true, "max_chars": 16000 }`
- `click`: `{ "verb": "click", "app": "com.apple.Safari", "element_index": 42 }`
- `set_value`: `{ "verb": "set_value", "app": "com.apple.Safari", "element_index": 18, "value": "https://example.com" }`
- `type_text`: `{ "verb": "type_text", "app": "com.apple.Safari", "text": "hello" }`
- `press_key`: `{ "verb": "press_key", "app": "com.apple.Safari", "key": "RETURN" }`
- `scroll`: `{ "verb": "scroll", "app": "com.apple.Safari", "direction": "down", "pages": 1 }`
- `select_text`: `{ "verb": "select_text", "app": "com.apple.TextEdit", "element_index": 12, "text": "needle" }`
- `drag`: `{ "verb": "drag", "app": "com.example.app", "from_x": 100, "from_y": 100, "to_x": 300, "to_y": 300 }`
- `perform_secondary_action`: `{ "verb": "perform_secondary_action", "app": "com.apple.Safari", "element_index": 42, "action": "AXShowMenu" }`

Do not invent verbs such as `open`, `navigate`, `insert_text`, or `key`. If the app is not running, call `launch` with its bundle identifier; never treat a read as a launch. Two running copies of the same bundle refuse with `ambiguous_target` rather than picking one. Navigate and edit through the exposed controls using the verbs above. Mutating verbs check `{pid, instance_id}` against a Workshop target's `/health` before acting.

## Safari: open a URL

Prefer this clean-window keyboard path. It avoids returning the accessibility
tree for a large existing tab set before navigation:

1. Call `press_key` for `com.apple.Safari` with `key: "cmd+n"` to open a new one-tab window.
2. Call `press_key` with `key: "cmd+l"` to focus its address field.
3. Call `type_text` with the complete URL.
4. Call `press_key` with `key: "Return"`.
5. Only now call bounded `find_elements`, `get_app_outline`, or `get_app_state` and verify the returned URL, page title, or heading.

After a screen unlock, Only now call `get_app_state` with `disable_diff: true`; that non-diffed but still bounded read is required before any cached index can be trusted.

If the first `press_key` is refused because the helper requires an initial read,
call `get_app_state` once, do not analyze unrelated page or tab content, and
immediately continue with `cmd+n`. Never search the old tree for its address
field when the task only needs a fresh page.

Reuse the current Safari window only when the user explicitly asks to preserve
that window or tab. In that case, read state, locate the smart search field,
set its value to the full URL, press `Return`, and read state again.

For web browsing, links, buttons, headings, text fields, and page text appear in Safari's accessibility tree. Treat them like native elements. Open a new tab with `CMD+L` only when the current tab should be preserved; do not reuse unrelated content unnecessarily.

## The loop

Read, act, read again. Element indexes are invalidated by any change to that app's interface, including the change your own action just made.

1. `get_app_state` — returns the accessibility tree with an index per element. Diffed against your last read by default.
2. Act using an index from **that** read.
3. `get_app_state` again before the next action.

Skipping step 3 does not produce a wrong click. It produces a refusal with code `stale_element_index`, because an index from before a mutation points at whatever now occupies that slot.

## Rules

- **`element_index` first.** Coordinates are a fallback for canvas surfaces — Figma, Blender, WebGL, custom renderers — where there is nothing in the tree to target. A task completed entirely through indexes is the goal, and it is measured.
- **Never sleep.** The runtime waits for the interface to settle before it answers. A `get_app_state` that returns has already settled. Polling in a loop wastes the budget and changes nothing.
- **Never research the tool contract.** Do not run shell commands or inspect skill directories, source, logs, or previous traces. Use the verbs and recipes in this file directly.
- **`press_key` and `type_text` are scoped to the app.** They cannot invoke global shortcuts. This is deliberate: it is what keeps the operator's own session intact while you work.
- **`perform_secondary_action` requires an action the element reported.** The tree lists them as `actions=[…]`. Guessing a name fails; it does not approximate.
- **Read the whole refusal.** Refusals are typed and carry a remediation. `needs_full_read` means the screen was locked and every cached index is stale. `app_denied` means the app can never be driven and no retry will change that.

## What you cannot do

Terminal emulators, credential stores, and Privacy & Security settings are refused under every policy and every approval. There is no argument, no scope, and no setting that opens them.

For terminals specifically: use the shell tool. It is gated, logged, and reviewable. Typing into a terminal window would route around all three, which is exactly why it is refused.

## Approvals

The operator approves each app the first time you touch it, and approves hazard-class actions individually — sending, submitting, paying, deleting, confirming — with the actual content shown on the card.

An app-scope approval can be remembered for the session or permanently. A hazard approval never is: the consent is about that specific payload, so the next one asks again even in an app you already have.

Do not send an approval decision in tool arguments. The host owns approvals. A rejection leaves the interface untouched.

## Locking

If the screen locks mid-task, the run pauses. Nothing is delivered while locked — a synthesized keystroke can reach the login window. On unlock, call `get_app_state` with `disable_diff` set before doing anything else: windows may have moved, displays may have been disconnected, and every index you hold is stale.

A lock longer than the ceiling ends the run. Start a new one rather than trying to resume.
