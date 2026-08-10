# Workshop workspace permissions

**Status:** provisional product decision — 2026-08-09

## Decision

Workshop treats the following as different concepts. They must never be
collapsed into one ambiguous “workspace access” setting.

| Concept | Scope | Purpose |
| --- | --- | --- |
| Working workspace | One conversation | The directory in which the agent starts and discovers repository instructions. |
| Attached folders | One conversation | Additional explicit folders the agent may read or, when approved, edit. |
| Default roots | App instance | A user-managed list from which new conversations choose their initial workspace and permitted attachments. |
| Permission profile | One conversation / current turn | The sandbox and approval behavior for commands; it does not silently add filesystem scope. |

The user owns filesystem scope. An agent may request access to a named folder,
but it must not directly edit Workshop configuration or expand its own scope.

## Desired user experience

Every local Codex/Laguna conversation shows a compact workspace chip:

```text
~/Documents/GitHub/workshop  ▾
```

Its menu provides:

- Change workspace…
- New isolated workspace…
- Attached folders…
- Add folder…

Attached folders are visible in the chat’s context panel and in the workspace
menu, including their access level:

```text
Workspace
  ~/Documents/GitHub/workshop                 Read/write

Attached folders
  ~/Documents/GitHub/Experiments              Read-only
  ~/Documents/Codex/research-notes            Read/write
```

The default is intentionally narrow: a new app instance with no configured
root starts in its own isolated workspace. A user can configure defaults in
**Settings → Runtime → Workspace access**, but a conversation may only select
or attach folders within that policy ceiling.

## Access levels

1. **Read-only**
   - Agent may inspect files and use them as context.
   - It may not create, edit, rename, or delete files there.
   - This is the default for an additional folder.

2. **Read/write**
   - Agent may modify files in the folder when its normal sandbox and approval
     profile permits it.
   - It is appropriate for a sibling repository or a deliberate multi-repo
     task.

3. **Full access**
   - This is a distinct, high-trust sandbox/approval profile, not an attached
     folder type.
   - It must never be represented as “all folders attached.”

`deny` rules, when introduced, override any allowed root or attachment.

## Grant flow

An agent can emit a structured request, for example:

```text
Needs read-only access to ~/Documents/GitHub/Experiments
Reason: compare the experiment harness with this repository.
```

Workshop then—not the agent—performs the grant:

1. Shows the exact proposed folder, access level, and reason.
2. Requires the user to confirm the folder in a native folder picker or an
   explicit approval surface.
3. Canonicalizes and validates the directory.
4. Persists the grant with its scope: current conversation only or saved as a
   default policy root.
5. Restarts/rebinds the affected agent session before the new access applies.
6. Adds an audit event to the conversation explaining what changed.

An ordinary natural-language “yes” is not sufficient to mutate the persistent
configuration. It may authorize a displayed, exact pending request, but the
native host remains responsible for the write.

## Current behavior and gap

Today, **Settings → Runtime → Agent workspace access** stores one ordered
`allowed_roots` list. The first root is the default working directory for new
conversations. All roots are emitted as Codex `workspace-write` writable
roots. Existing conversations retain their initial working directory.

This is useful for a single trusted source tree, but it lacks:

- per-conversation folder attachments;
- read-only versus read/write access levels;
- native, auditable agent-requested grants;
- a clear visible explanation of the active workspace and additional scope;
- an enforced policy ceiling separate from a user’s one-off session grants.

The existing setting remains the migration source for default **read/write**
roots. It must not be silently broadened or removed.

## Security and lifecycle rules

- Canonicalize every folder and reject nonexistent/non-directory paths.
- Never accept an agent-provided path as a grant without a user-visible,
  exact approval.
- Store access decisions in Rust-owned persistent state, not only in
  renderer local storage.
- Apply changed scope only after a session restart/rebind; never claim a live
  process has access that it was not launched with.
- Preserve the durable conversation if a rebind fails. Mark access as pending
  or failed with a recoverable explanation.
- Do not infer filesystem access from file mentions, screenshots, terminal
  text, a selected model, or a provider.
- Keep filesystem permissions, command approvals, network access, and external
  connectors as independently visible controls.

## Product reference points

Codex supports extending `workspace-write` with explicit writable roots rather
than treating full access as the normal multi-directory solution. Poolside’s
documented settings distinguish allow/deny paths, write permission, and
sandbox mounts. Workshop should adopt that separation while preserving a
simple desktop workflow.

## Definition of done

No access-control change is complete until:

- The rendered conversation identifies its current workspace and every attached
  folder with access level.
- A user can add/remove/change an attachment through a native-host approval
  flow.
- A model/agent cannot increase its own scope through a config edit or hidden
  renderer call.
- New, resumed, interrupted, and failed-to-rebind sessions report their actual
  access state honestly.
- Playwright tests cover the UI and persistence; Rust tests cover validation,
  migration, persistence, and launch payloads; Bombadil checks narrow layouts
  and menu/panel containment; CUA verifies the packaged app and native picker
  flow.
