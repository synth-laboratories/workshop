# Contract: live template replay transport

Governs `visuals/runtime/replayClient.ts`, `visuals/chrome/useLiveEvalStreams.ts`,
and the `visual_stream_poll` Tauri command.

## Who owns transport

Workshop does. A live template receives a `ReplayClient` and consumes it:

```ts
type ReplayStream = { streamId: string; pollUrl: string; sseUrl?: string };
type ReplayClient = {
  streams: ReplayStream[];                                    // required
  poll(stream, after, limit): Promise<ReplayPage>;            // required
};
```

Templates do not read bindings to find URLs, and do not receive an optional
callback they might silently lose. Both were true before, and both failed
quietly: a template that derives its own transport can derive nothing and render
as though nothing had been declared.

The host builds the client in `TemplateVisualHost` from the resolved binding
slots (see `visual_bindings.md`) and the native poll bridge.

## Native polling

`visual_stream_poll` allowlists the requested URL against the visual's declared
poll authorities, resolved through the same canonicaliser that writes them, then
polls with `reqwest` — so replay does not depend on WKWebView's view of CORS or
CSP. An undeclared URL is a binding defect and is reported as one.

## The state machine

```
idle → declared → replaying → live → terminal
                     ↘  error  ↙
```

| State | Meaning |
| --- | --- |
| `idle` | No stream declared. Nothing pending, nothing wrong. |
| `declared` | Streams declared, first response outstanding. |
| `replaying` | Reading durable history from cursor zero. |
| `live` | Caught up, at least one stream open. |
| `terminal` | Every declared stream reported closed. |
| `error` | Bounded and named. |

`connecting` is not a state. What it used to mean — "streams exist and nothing
has happened" — is now `declared`, and `declared` carries a deadline:
`REPLAY_FIRST_RESPONSE_TIMEOUT_MS`. Exceeding it emits
`stream_subscribe_timeout` and moves to `error`. A pane cannot rest in an
unexplained pending state, because that state no longer exists to rest in.

Readiness accepts `live` and `terminal` only, as an allowlist
(`READY_TRANSPORT_STATES` in `visuals_ipc.rs`). A denylist would accept any
state a future template invents, and an unknown state is the least likely to be
showing settled evidence.

## Replay semantics

- Replay works from the durable poll authority alone. A closed EventSource is
  never data loss, and a completed evaluation reopens without converting Trace
  V5 into a different input schema.
- Each stream keeps its own cursor. Cursors are not comparable across rollouts,
  so a global maximum would strand a slower lane forever.
- Every stream starts at cursor zero and relies on envelope-identity
  de-duplication, so opening a visual after every stream is terminal replays the
  whole history.
- A cursor that regresses, fails to advance, or exceeds `REPLAY_PAGE_LIMIT_MAX`
  pages is an error, not a retry.

## Page shapes

`parseReplayPage` normalises three producer shapes in one place: `page.events`
with a cursor, top-level `events`, and a bare array. A bare array carries no
cursor, so it is read as one closed page — the only reading that cannot silently
drop rows or spin. A body with neither shape throws; it never becomes an empty
page.

**COMPAT.** The bare-array and top-level-`events` arms are compatibility. Remove
them once every producer emits `page` + `cursor`.

## Observability

`visual_stream_poll` records `stream.poll.page` / `stream.poll.closed` on
success and `stream.poll.failed` on error, carrying visual id, revision, cursor,
row count, high water, status, and duration. Identifiers and integers only —
envelope bodies carry model output and rollout payloads and never enter a
diagnostic.

Success is recorded because its absence is evidence: without it, "the renderer
never asked" and "the stream returned nothing" are the same empty pane and the
same empty query.
