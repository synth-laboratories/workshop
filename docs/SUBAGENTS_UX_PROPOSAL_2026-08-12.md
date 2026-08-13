# Subagents UX proposal — 2026-08-12

**Status:** Transport foundation implemented and regression-tested. The
presentation is derived from the supplied Codex Desktop captures and the
transport is verified against the [Codex OSS app-server at `91d6f489`](https://github.com/openai/codex/tree/91d6f48992ad8db636b3ca52a3a36c2fb6d75537).
The rail, dedicated workspace, and child-detail work below remain the next UX
phases; this work does not change Codex's collaboration protocol or
child-thread persistence model.

## What the reference does well

Codex makes delegated work legible at three levels, each with a different job:

| Surface | Reference behavior | Why it works |
| --- | --- | --- |
| Parent turn | Small, named agent chips inline with the parent’s work | Communicates that delegation happened without flooding the transcript. |
| Context rail | A one-line **Subagents** summary such as `3 working` / `3 done`, with distinct agent marks | Provides ambient progress while the user stays in the parent task. |
| Drill-in | A dedicated list of active agents; selecting one opens its own focused timeline | Gives detail on demand, while preserving the parent as the primary conversation. |

The details are deliberately quiet: status is a word, duration sits at the far
right, active rows receive a soft neutral hover/selection state, and each agent
has a persistent colored mark. Completion is summarized in the parent turn as
a compact file-change outcome, not replayed as child-chat text.

## Current Synth Workshop baseline

Workshop now has a cross-version transport foundation:

- **V1:** `collabAgentToolCall` binds the spawn to a child thread; its
  `agentsStates` values drive the normalized lifecycle.
- **V2:** `subAgentActivity` binds the child path and thread; the child’s turn
  events drive the same normalized lifecycle.
- The artifact auto-opens on the first child spawn.
- `VisualHost` groups agents as **Working**, **Needs attention**, and
  **Completed**, with title, latest summary, status mark, and elapsed time.
- Child completion text stays out of the parent transcript.

The remaining gap is information architecture: the current artifact is still a
generic visual pane. Its normalizer explicitly treats `ThreadStatus::Idle` as
current thread liveness, never as a child-agent terminal result.

## Verified Codex app-server contract

The app-server emits items through `item/started` and `item/completed`. Synth's
stdio pump journals those raw notifications unchanged, so this is the wire
contract the renderer must normalize.

Primary references: [the public item schema](https://github.com/openai/codex/blob/91d6f48992ad8db636b3ca52a3a36c2fb6d75537/codex-rs/app-server-protocol/src/protocol/v2/item.rs#L346-L365), [V1 event mapping](https://github.com/openai/codex/blob/91d6f48992ad8db636b3ca52a3a36c2fb6d75537/codex-rs/app-server-protocol/src/protocol/event_mapping.rs#L79-L255), [V2 spawn activity](https://github.com/openai/codex/blob/91d6f48992ad8db636b3ca52a3a36c2fb6d75537/codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs#L155-L164), and [child-status derivation](https://github.com/openai/codex/blob/91d6f48992ad8db636b3ca52a3a36c2fb6d75537/codex-rs/core/src/agent/status.rs#L6-L27).

| Protocol | Identity and activity | Authoritative result state | Synth treatment |
| --- | --- | --- | --- |
| V1 | `collabAgentToolCall` has `id`, `tool`, `senderThreadId`, `receiverThreadIds`, optional `prompt`, `model`, and `reasoningEffort`. Spawn begins without a receiver; completion binds its child thread ID. | `agentsStates[threadId]` has `PendingInit`, `Running`, `Interrupted`, `Completed(message?)`, `Errored(message?)`, `Shutdown`, or `NotFound`. | Bind provisional spawn call → child thread ID. Use the `agentsStates` entry for the terminal label and result preview. |
| V2 | `subAgentActivity` has `id`, `kind` (`started`, `interacted`, `interrupted`), `agentThreadId`, and canonical `agentPath`. V2 `spawn_agent`, `send_message`/`followup_task`, and `interrupt_agent` emit these items. | The child thread's own turn lifecycle provides its current result: `turn/started`, `turn/completed` (with the last agent message), `turn/failed`, `turn/interrupted`, and shutdown. | Create/bind from `started`, retain the canonical path and stable spawn order, then update that child only from its own lifecycle. |

`wait_agent` is deliberately **not** a source of a V2 child-state snapshot:
the V2 handler emits a wait `collabAgentToolCall` whose receiver list and
`agentsStates` are empty. It is a parent action (“waiting”), not evidence that
any child has completed.

The OSS server exposes `thread/read { threadId, includeTurns: true }` and
`thread/items/list` for a focused child view. Synth should add a native,
ownership-checked bridge for those reads rather than reconstructing a child
timeline from the parent transcript.

### Normalized state contract

Normalize at the desktop boundary into a version-neutral record keyed by child
thread ID. Keep action history distinct from lifecycle state.

```text
Subagent {
  threadId, agentPath?, spawnCallId?, title, model?, reasoningEffort?,
  lifecycle: starting | working | completed | interrupted | failed | stopped | unavailable,
  resultPreview?, lastAction?, startedAt, updatedAt
}
```

- V1 mappings: `PendingInit → starting`, `Running → working`, `Completed →
  completed`, `Interrupted → interrupted`, `Errored → failed`, `Shutdown →
  stopped`, and `NotFound → unavailable`.
- V2 mappings: `subAgentActivity.started → starting`; child `turn/started →
  working`; child completion/failure/interruption → `completed` / `failed` /
  `interrupted`. A V2 interaction means “contacted”, not “working” by itself.
- A late event may update text but must not revive a terminal lifecycle state
  unless a new V2 child turn explicitly starts.
- Never map `thread/status/changed: idle` to `completed`.

This reducer must be idempotent and monotonic by Synth journal sequence. It
must accept camelCase and snake_case only at the adapter boundary; UI components
receive the normalized record above.

## Proposed Synth Workshop update

### 1. Add a persistent collaboration rail summary

Place a **Subagents** card in the existing right-side context/outputs rail only
when the selected session has child agents.

```text
Subagents                                         ›
✺ ✣ ✺  3 working
```

- Show up to three stable agent marks, then `+N` for overflow.
- Use `N working`, `N waiting`, `N needs attention`, or `N done`; never imply
  progress percentages that the app-server stream does not provide.
- The card opens the Subagents workspace. It collapses from the rail when the
  session has no child agents.

### 2. Replace generic auto-open with a dedicated Subagents workspace

Open a `Subagents` tab/surface beside the parent task, not a generic “Visual
artifact” pane. The default view is a quiet list:

```text
Subagents

Active · 3
  ✺  API boundary review                     Working        55s
  ✣  README location                         Working        57s
  ✺  Documentation audit                     Working         6s

Done · 2
  ✺  Test coverage                           Completed      1m 12s
  ✣  Data model review                       Failed         42s
```

- Entire rows are selectable. A soft neutral selection state mirrors the
  reference and avoids treating active work as an error or warning.
- Status remains textual and accessible; color/mark is supplemental.
- Children that need attention keep their final reason and are never folded into
  “Done” without a visible state label.
- Group by **Working**, **Needs attention**, and **Completed**. `Interrupted`,
  `Stopped`, and `Unavailable` remain visible in Needs attention; only
  `Completed` belongs in Completed.

### 3. Add a child-agent detail view

Selecting a row replaces the list with a dedicated child timeline and a back
button:

```text
←  ✺  API boundary review
    Working for 55s

    Latest activity
    • Reading runtime-regressions.spec.ts
    • Searching collaboration event mappings

    Latest result
    No result yet
```

On completion, show the final child summary, changed files, and checks when
the event stream provides them. Do not invent file or test outcomes.

### 4. Make delegation visible but compact in the parent transcript

Replace individual start/finish prose lines with one expandable collaboration
event attached to the parent turn:

```text
✺ API boundary review   ✣ README location   ✺ Documentation audit   started working
```

- Chips open the selected child detail.
- The completion form becomes `3 subagents finished · 1 file changed +26 −6`
  only when a trustworthy aggregate is available; otherwise use
  `3 subagents finished`.
- Preserve the existing rule that raw child messages do not duplicate into the
  parent transcript.

## Interaction and state rules

| Parent state | Rail copy | Parent-turn treatment | Drill-in default |
| --- | --- | --- | --- |
| Delegating | `Starting N agents` | pending chips | list |
| Working | `N working` | named chips | working group |
| Waiting | `Waiting on N agents` | named chips | active group |
| Needs attention | `N need attention` | failing/blocked chip clearly labeled | first attention item |
| Synthesizing | `Synthesizing results` | completed chips | done group |
| Complete | `N done` | concise aggregate outcome | done group |

The UI must tolerate partial and out-of-order lifecycle events. An unknown
child is shown as `Starting`; missing completion evidence remains `Working` or
`Status unavailable`, never `Done`.

## Delivery sequence

1. **Correct transport first — complete:** replace `eventsToSubagents` with a tested V1/V2
   reducer. Preserve V1 provisional spawn IDs until receiver binding; create
   V2 child records from `subAgentActivity.started`; never derive completion
   from a thread's `idle` status.
2. **Add focused reads:** expose ownership-checked `thread/read` and paginated
   `thread/items/list` through the native Codex bridge for a selected child.
3. **Surface the normalized data:** introduce a `SubagentsPanel` route and
   move the current `SubagentsVisual` list into it.
4. **Add rail and transcript affordances:** session-scoped summary card,
   compact chips, open/selection wiring.
5. **Child detail:** render the selected child from its app-server history;
   add file/check rollups only once the transport guarantees those fields.
6. **Polish:** stable per-agent mark assignment, selection state, elapsed-time
   wording, empty/failed/reconnect cases.

## Acceptance criteria

- First child spawn exposes all three levels: parent chip, rail summary, and
  Subagents workspace.
- Rail count/state changes on child lifecycle events without polling.
- Selecting a chip/row opens the correct child detail; back returns to the
  same filtered list and scroll position.
- Parent transcript never contains raw child completion text.
- A failed/blocked child is distinguishable from a completed child in every
  surface.
- V1 fixtures include spawn begin/end, a multi-child `wait` result, and every
  `CollabAgentStatus`; V2 fixtures include `subAgentActivity`, child turn
  started/completed/failed/interrupted, and a follow-up reactivation.
- A V2 `thread/status/changed: idle` fixture proves it does not produce a Done
  card.
- Small widths retain the state and count; labels truncate before status or
  duration.
- Bombadil verifies: rail card is present only with child agents, counts match
  visible rows, selected row remains in the viewport, and long task titles do
  not overlap status/duration. Playwright verifies the event-to-surface state
  transitions and no-transcript-duplication contract.

## Non-goals for this cut

- Manual agent spawning, cancellation, or assignment editing from the UI.
- A synthetic progress bar or percent-complete estimate.
- A visual DAG when the source protocol only provides parent/child identity.
- Cross-session/global agent monitoring.
