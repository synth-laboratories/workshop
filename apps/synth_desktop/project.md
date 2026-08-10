# Projects — parked product design

**Status:** Removed from the active Synth Desktop product on 2026-08-09.  
**Reason:** The existing UI exposed a repository-style “Projects” concept without giving it enough real Workshop behavior. Keep this document as the re-entry contract; do not restore the surface until the product model below is implemented and tested.

## Why Poolside has Projects

Poolside Assistant is repository-centric. In the observed Poolside 1.4.0 UI, a project selects a folder/repository and becomes the context for:

- Files and Changes panes
- Terminal working directories
- Opening the repository in an IDE
- Worktree creation
- Project-scoped conversations
- Restored workspace/panel layouts

The visible product behavior supports the inference that Poolside’s Project answers: “Which codebase is this agent working in?” CUA does not establish Poolside’s private persistence architecture.

## Why the old Workshop surface was premature

Workshop is a research-engineering workbench, not only a repository coding assistant. The old implementation registered `{id, name, path, vcs, metadata}` in Rust/SQLite and could attach `project_id` to a session, but native Codex/Laguna conversations explicitly used a projectless default workspace. Selecting a project mostly changed terminal context and some cloud/Intern creation paths. It did not reliably group the research objects users would expect.

That made “Projects” a promise the product did not yet keep.

## Terms for a future implementation

| Term | Meaning |
| --- | --- |
| **Conversation** | One task/thread and its durable run/event history |
| **Workspace** | One filesystem root plus its execution/sandbox permissions |
| **Project** | An optional durable research boundary grouping conversations, workspaces, terminals, artifacts, and runtime context |

Ordinary conversations must remain possible without a project.

## Future Workshop Project contract

A restored Project should be able to own or associate:

- One or more repository/workspace roots
- Conversations and Intern sessions
- Terminals and their working directories
- Registered containers relevant to the work
- Imported traces, rollouts, datasets, and evaluations
- Visuals and eval matrices
- Model/runtime defaults and supported knobs
- Saved pane/tab layout
- Workspace access policy
- Experiment metadata, tags, and provenance

Example: a **Craftax Rust** project could collect the GameBench repository, its terminal sessions, the explicitly registered Craftax service, rollout traces, evaluation visuals, and all related chats. Until those relationships are real, a folder bookmark should be called a Workspace, not a Project.

## UX requirements before restoration

- Projects are optional; new Luna/Laguna chats never require one.
- Creating/selecting a project materially changes visible context.
- The active project is unambiguous in the shell and composer.
- Moving a conversation between projects has explicit artifact/workspace semantics.
- Removing a project never deletes its filesystem or silently destroys conversations/artifacts.
- Missing or moved workspace roots surface a repair flow.
- Search, archive, unread state, terminal restore, layouts, and artifacts behave correctly across project boundaries.
- Project status is not confused with agent/runtime health.
- “Workspace” remains the filesystem/sandbox concept; “Project” remains the organizational/research concept.

## Persistence and architecture

- Rust/SQLite should remain authoritative for project identity and relationships.
- Existing `projects` rows and `sessions.project_id` columns are retained while the feature is parked so removing the UI is not destructive and future migration remains possible.
- Renderer preferences may reference a project only after validating that the Rust record exists.
- Filesystem paths must be canonicalized and constrained by workspace-access policy.
- Project removal should be a reversible archive/detach operation before any permanent deletion is considered.
- Event-journal mutations should make create/update/archive/attach/detach auditable.

## Previous implementation map

These locations describe the parked/legacy implementation and are useful when rebuilding intentionally:

- `src-tauri/src/projects.rs` — Rust project registry
- `src-tauri/src/storage/migrations.rs` — durable tables and `sessions.project_id`
- `src-tauri/src/lib.rs` — former project IPC commands
- `src/renderer/src/runtime/desktopBridge.ts` — former renderer bridge
- `src/renderer/src/App.tsx` — former project selection and terminal binding
- `src/renderer/src/components/Sidebar.tsx` — former Projects sidebar section
- `src/renderer/src/components/LandingPage.tsx` — former “add project” quick action
- `packages/runtime-protocol/src/index.ts` — compatibility project/session types

Do not delete old SQLite rows or migrations merely because the current product no longer exposes Projects.

## Test bar for bringing Projects back

Playwright must cover create/select/rename/archive/repair, projectless chats, missing folders, cross-project search, terminal roots, restored layouts, and non-destructive removal. Bombadil must generate cross-project conversation/artifact/layout operations and maintain invariants preventing wrong-project execution or data loss. Installed-app CUA must confirm folder selection, relaunch restoration, moved-folder repair, keyboard-only operation, and at least two simultaneous projects.

## Re-entry decision

Restore the feature only when Workshop needs a durable research boundary rather than visual parity with Poolside. Until then:

- Use configured workspace roots for filesystem access.
- Keep chats projectless.
- Scope terminals to the active conversation/default workspace.
- Organize containers, traces, and visuals through their existing registries.

