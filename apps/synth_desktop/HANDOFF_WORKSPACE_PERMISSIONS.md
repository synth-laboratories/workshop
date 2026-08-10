# Handoff: per-conversation workspace permissions

## Goal

Implement the product contract in [`../../workspace_permissions.md`](../../workspace_permissions.md).

Workshop must let the user choose a conversation’s working workspace and add
additional explicit folders with **read-only** or **read/write** access. The
agent may request access but cannot self-grant it or directly mutate the
configuration. The desktop host owns validation, persistence, approval, and
session rebind.

## Why this matters

Current Desktop stores only `workspace.allowed_roots` in `config.toml`:

- the first root is the default working directory for new chats;
- every listed root becomes a writable Codex `workspace-write` root;
- a chat started in the default isolated workspace only sees `.` unless a root
  was configured;
- the UI does not distinguish its current workspace from additional scope.

The supplied CUA transcript demonstrated the failure: the agent honestly saw
an empty isolated workspace but neither the UI nor its tools could make the
user’s intended GitHub root explicit.

## Non-goals

- Do not implement “full access” as a folder attachment.
- Do not let a natural-language message directly update config.
- Do not scan the user’s disk to discover repositories.
- Do not broaden existing `allow-all` behavior or silently migrate it.
- Do not build Projects around this feature. Conversations remain
  workspace-backed and projectless.

## Product model

```text
App policy ceiling (user-managed defaults)
  ├─ allowed root: ~/Documents/GitHub              read/write
  └─ allowed root: ~/Documents/Codex               read-only

Conversation scope (durable)
  ├─ workspace: ~/Documents/GitHub/workshop        read/write
  ├─ attachment: ~/Documents/GitHub/Experiments     read-only
  └─ attachment: ~/Documents/Codex/research-notes   read/write

Active Codex app-server process
  └─ receives exact cwd + sandbox writable roots for the bound conversation
```

The conversation scope must be a subset of the user policy ceiling. The first
default root remains the default workspace for a new local conversation.

## Proposed Rust model and storage

Use Rust-owned SQLite persistence. Do not use renderer local storage as the
authority.

Suggested types:

```rust
enum WorkspaceAccessMode { ReadOnly, ReadWrite }

struct WorkspaceAttachment {
    path: String,              // canonical absolute path
    access: WorkspaceAccessMode,
    source: AttachmentSource,  // user_picker | agent_request | migrated_default
    created_at: String,
}

struct ConversationWorkspaceScope {
    session_id: String,
    workspace: String,         // canonical absolute, always read/write
    attachments: Vec<WorkspaceAttachment>,
    revision: i64,
}
```

Add an app-level policy record with canonical roots and access modes. Preserve
the existing TOML `workspace.allowed_roots` as a backwards-compatible input:
each migrated legacy root is **read/write**. Do not delete the legacy setting
until the new model is stable and a documented migration exists.

Likely ownership points:

- `src-tauri/src/synth_config.rs` — current legacy root validation/config.
- `src-tauri/src/storage/` — migrations and durable session metadata.
- `src-tauri/src/codex.rs` — app-server launch config and attachment lifecycle.
- `src-tauri/src/lib.rs` — narrow Tauri commands and native folder picker.
- `src/renderer/src/runtime/desktopBridge.ts` / `env.d.ts` — typed bridge.
- `src/renderer/src/App.tsx` — conversation state and rebind handling.

## Launch semantics

Codex supports `sandbox_workspace_write.writable_roots`; include only the
conversation workspace plus **read/write** attachments in that list. The
process `cwd` must be the conversation workspace.

Read-only attachments need an enforced read boundary. Do not merely display a
read-only badge while the Codex app-server has unrestricted OS visibility.
Before shipping, confirm the exact Codex sandbox configuration needed to expose
read-only extra paths on macOS. If the current Codex configuration cannot
express it, ship no read-only attachment feature yet; keep it behind a clearly
labelled unsupported state rather than lying.

Scope changes require a safe rebind:

1. Persist pending scope revision.
2. Interrupt/reconcile an active turn as necessary; never discard transcript.
3. Close only the old attachment generation.
4. Launch/resume the same durable thread with the new `cwd`/sandbox scope.
5. Persist bound revision and emit an auditable system activity item.
6. On failure, retain the previous known-good binding and surface a recoverable
   error. Never show the new scope as active until the new process is bound.

This must integrate with the established detached-session reconciliation work;
read `SESSION_LIFECYCLE.md` before editing lifecycle code.

## Native approval flow

Expose narrow host commands, conceptually:

```text
workspace_scope_get(sessionId)
workspace_scope_choose_and_attach(sessionId, proposedAccess)
workspace_scope_remove_attachment(sessionId, path)
workspace_scope_change_workspace(sessionId)
workspace_scope_request_agent_grant(sessionId, path, access, reason)
workspace_scope_approve_request(requestId)   // only after native picker confirmation
workspace_scope_deny_request(requestId)
```

The agent-facing grant request must create a pending record only. The agent
never receives a raw “update access policy” command. Approval must show the
exact canonical path, access level, and reason; requiring the user to choose
or reconfirm the folder through the native picker is preferred.

Avoid a broad generic config-writing Tauri command. Validate and canonicalize
in Rust at every boundary, reject duplicates/nested ambiguity deterministically,
and record who/what initiated the grant.

## Renderer design

1. **Composer/workspace chip**
   - Show the current workspace basename/path compactly.
   - Menu: Change workspace…, New isolated workspace…, Attached folders…,
     Add folder…
   - Do not overload the model, approval, or effort selector.

2. **Context panel**
   - Current workspace and additional folders with readable mode badges.
   - Remove/revoke action is available to the user; explain it rebinds future
     agent execution.

3. **Settings → Runtime**
   - Replace the ambiguous single list with `Default workspaces and folder
     policy`.
   - Keep a clear default start root and an explicit policy ceiling.
   - Show read/write modes, not only paths.

4. **Pending agent request**
   - Transcript activity card: folder, requested access, reason, Approve / Deny.
   - Approval must be keyboard accessible and must not itself grant until the
     native host confirmation returns.

Use calm factual copy. Never say “connected,” “mounted,” or “available” until
the rebind succeeds.

## Tests required

### Rust

- Canonicalize paths, reject relative/missing/file paths, and dedupe roots.
- Legacy `allowed_roots` migration preserves ordered default and read/write
  behavior.
- A conversation scope cannot exceed its policy ceiling.
- Launch payload has selected workspace as `cwd` and only read/write roots in
  `sandbox_workspace_write.writable_roots`.
- Pending request cannot change scope; denial changes nothing.
- Approved change increments revision and rebinds safely.
- Failed rebind restores/retains known-good scope and durable conversation.
- Restart/reconciliation correctly reports scope revision and health.

### Playwright

- Empty default workspace is visible as such; changing to a selected fixture
  root changes the next session’s `cwd`.
- Add a read/write attachment, persist across reload, and display it in both
  context panel and workspace menu.
- Read-only attachment is visibly distinct and cannot show a false writable
  claim.
- Scope selection/revocation has keyboard and screen-reader labels.
- Agent request creates a pending card; only host approval activates it.
- Existing active session displays honest “restarting to apply access” state;
  failure keeps old scope.
- Supported widths have no clipping/overflow for workspace menu, attachment
  list, and pending card.

### Bombadil

Create a stateful specification that opens the workspace chip, attachment
panel, and a pending request while fuzzing 960×640, 1024×700, 1280×840, and
1440×900. Invariants:

- menus/panels remain in bounds, topmost, keyboard-dismissable, and
  horizontally contained;
- access-mode labels remain visible and unambiguous;
- active composer, terminal, and workspace menus never overlap;
- a pending grant is never visually indistinguishable from an active grant.

### Packaged CUA

- Start a fresh packaged app with no configured roots and verify it identifies
  the isolated workspace honestly.
- Add a real selected folder through the native picker; verify new/restarted
  chat reports exact `cwd` and sees a known file.
- Add/revoke an attachment, restart the session, and verify the visible scope
  follows reality.
- Exercise approval/denial and a forced rebind failure.
- Record all findings in `polish.md` and add durable regressions to
  `CUA_FUZZ_INVARIANTS.md`.

## Acceptance criteria

- The user always knows which directory a chat starts in.
- The user can safely make a multi-repository task possible without choosing
  global Full access.
- An agent cannot self-escalate filesystem scope.
- Read-only is technically enforced before it is offered as a completed
  product capability.
- Changing scope never corrupts, loses, or falsely marks a running session.
- All tests above are green, and a packaged CUA pass validates the real native
  flow.

## Working-tree caution

This repository has concurrent in-progress changes. Preserve unrelated work;
do not reset, clean, or bulk-stage. Use `rg` before edits and make focused,
reviewable commits.
