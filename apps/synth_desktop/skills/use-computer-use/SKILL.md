---
name: use-computer-use
description: Observe and drive native macOS apps through mcp__synth_computer_use__computer_use — read an app's accessibility tree, click, type, scroll, and select by element index. Use when a task requires operating a GUI application the operator has allowed. Do not use for shell commands, file edits, or anything a terminal can do.
---

# Use Computer Use

Use `mcp__synth_computer_use__computer_use`. Every call takes a `verb` and, except for `list_apps`, an `app` bundle identifier such as `com.apple.mail`.

Check `mcp__synth_computer_use__computer_use_status` first. It is advertised even when the plugin is not ready, so a refusal tells you which permission is missing instead of leaving you to guess.

## The loop

Read, act, read again. Element indexes are invalidated by any change to that app's interface, including the change your own action just made.

1. `get_app_state` — returns the accessibility tree with an index per element. Diffed against your last read by default.
2. Act using an index from **that** read.
3. `get_app_state` again before the next action.

Skipping step 3 does not produce a wrong click. It produces a refusal with code `stale_element_index`, because an index from before a mutation points at whatever now occupies that slot.

## Rules

- **`element_index` first.** Coordinates are a fallback for canvas surfaces — Figma, Blender, WebGL, custom renderers — where there is nothing in the tree to target. A task completed entirely through indexes is the goal, and it is measured.
- **Never sleep.** The runtime waits for the interface to settle before it answers. A `get_app_state` that returns has already settled. Polling in a loop wastes the budget and changes nothing.
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
