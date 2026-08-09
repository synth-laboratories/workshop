# Desktop conversation and runtime lifecycle

Synth must treat a durable conversation, an agent turn, and a live process as
different things. Closing the window, quitting the app, changing chats, or an
agent-process crash must not corrupt a conversation or leave the UI claiming
that work is still running.

## Reference behavior

- Codex app-server makes the thread the durable primitive. A connection is
  initialized once, a stored thread is reopened with `thread/resume`, loaded
  threads are discoverable with `thread/loaded/list`, and an interrupted turn
  terminates with `status: "interrupted"`. Process attachment is not the source
  of truth for conversation existence. See the official
  [Codex app-server lifecycle](https://developers.openai.com/codex/app-server/).
- The Codex desktop app runs a long-lived app-server owned by the application,
  not one durable process per sidebar row.
- Poolside runs a long-lived helper and MLX sidecar shared across conversations.
  Its chat list presents conversational states such as waiting for input; a
  chat does not imply ownership of its own model process.

## Three independent state axes

| Axis | Durable? | Examples | Owner |
| --- | --- | --- | --- |
| Conversation | Yes | ready, archived, closed | thread/session store |
| Turn | Yes | running, completed, failed, interrupted | run/event store |
| Runtime attachment | No | detached, starting, attached, unhealthy | process supervisor |

`Working` and `Stop` are shown only when the latest durable turn is running
**and** its current runtime attachment is live. Historical `run.started` events
must never override a terminal or reconciled session state.

## Recovery contract

1. On desktop startup, every persisted `running` turn without a live attachment
   is atomically reconciled to `interrupted`. The conversation remains usable.
2. Stopping is idempotent. If the attachment or active turn is already gone,
   Stop succeeds and performs the same reconciliation instead of raising
   `session not started`.
3. Unexpected app-server EOF removes only the attachment generation that
   emitted it, marks an active turn interrupted, persists that terminal run,
   and emits `session/unhealthy` for immediate UI reconciliation.
4. A later message lazily creates a new attachment and calls `thread/resume`
   with the durable thread id. Users never need to repair the chat manually.
5. Closing or switching chats does not stop work. Quitting the desktop may end
   local processes, but startup reconciliation makes that outcome explicit and
   resumable.

Attachment generations fence asynchronous process exits: an old reader task
is never allowed to detach or fail a replacement process created for the same
conversation.

## Supervisor direction

The current implementation still launches an isolated app-server attachment
per active conversation because each existing Synth Codex home contains
provider and thread state. The next lifecycle change should move attachment
ownership to a provider supervisor keyed by instance, provider configuration,
and credential identity:

```text
durable conversations ── thread ids ──┐
                                      ├─ provider supervisor
durable turns/events ─── turn ids ────┤    ├─ Codex app-server connection
                                      │    └─ Laguna/MLX provider sidecar
ephemeral attachments ─ generation ──┘
```

The supervisor should initialize one app-server connection per compatible
provider context, resume/unsubscribe threads on demand, query
`thread/loaded/list` after reconnect, and restart with bounded backoff. Moving
to that pool requires an explicit migration from per-session Codex homes; it
must not be approximated by silently sharing homes or credentials.
