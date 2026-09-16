# Contract: live template replay transport

Governs `visuals/runtime/replayClient.ts` and
`visuals/chrome/useLiveEvalStreams.ts`.

## Who owns transport

Workshop does. A live template receives a `ReplayClient` and consumes it:

```ts
type ReplayStream = { streamId: string; sseUrl: string; pollUrl?: string };
type ReplayClient = {
  streams: ReplayStream[];
  subscribe(stream, handlers): () => void;
  poll?(stream, after, limit): Promise<ReplayPage>; // explicit snapshots only
};
```

Templates do not read bindings to find URLs, and do not receive an optional
callback they might silently lose. Both were true before, and both failed
quietly: a template that derives its own transport can derive nothing and render
as though nothing had been declared.

The host builds the client in `TemplateVisualHost` from the resolved binding
slots (see `visual_bindings.md`). Templates never construct an EventSource;
the host-owned client does so outside the sourced-template sandbox.

## Push transport

Live delivery is SSE only. There is no interval, long poll, or periodic snapshot
fallback in the visual hook. An SSE authority persists an envelope before
publishing it, replays committed history on a fresh connection, emits monotonic
SSE `id` fields, and honors `Last-Event-ID` on reconnect. The browser's
EventSource reconnect is therefore gap repair, not a best-effort notification.

`pollUrl` and `visual_stream_poll` remain compatibility surfaces for explicit
snapshot and seal operations. They are never scheduled by `useLiveEvalStreams`.

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

- Each SSE connection replays its durable prefix, then tails the live broadcast.
- EventSource reconnects with the last received SSE id. Exact reconnect
  duplicates are de-duplicated by envelope identity; conflicts and gaps remain
  visible errors.
- A fresh visual starts without `Last-Event-ID`, so the authority replays from
  the beginning. A terminal authority includes `stream.closed` and closes only
  after the retained manifest has been flushed.
- Cursors remain stream-local and are never compared across rollouts.

## Page shapes

`parseReplayPage` normalises three producer shapes in one place: `page.events`
with a cursor, top-level `events`, and a bare array. A bare array carries no
cursor, so it is read as one closed page — the only reading that cannot silently
drop rows or spin. A body with neither shape throws; it never becomes an empty
page.

**COMPAT.** The bare-array and top-level-`events` arms are compatibility. Remove
them once every producer emits `page` + `cursor`.

## Observability

The client records bounded subscription timeout and interruption diagnostics.
Identifiers and integers only—envelope bodies carry model output and rollout
payloads and never enter a diagnostic. A transient interruption preserves the
last good prefix while EventSource reconnects.

The first envelope clears the bounded subscription deadline. Without one,
"the renderer never subscribed" and "the authority returned nothing" become
the same named timeout rather than an indefinite placeholder.

## Harbor/Codex tool feed

`scripts/harbor-live-visual.mjs` adapts an exact Harbor trial directory to
this transport. Start it once `trial.log` exists, while the agent is still
running:

```sh
node scripts/harbor-live-visual.mjs \
  --trial-dir /absolute/path/to/harbor/trial \
  --visual-id unique-visual-id \
  --connection-file /absolute/path/to/visuals-ipc.json \
  --title 'Harbor trial' \
  --workspace-source /absolute/path/to/credential-free-workspace-descriptor.json
```

The workspace source is optional. The producer binds a declared `live_sse`
source, publishes cursor-ordered `tools` envelopes from
Harbor's continuously written `agent/codex.txt`, and uses archived session
JSONL only if that live log is absent. It never publishes raw commands,
outputs, instructions, messages, reasoning or credential values. Loopback
SSE carries sanitized tool identity/status/duration/exit code only. Filesystem
notifications wake the producer; there is no interval tailer.

After `verifier/reward.txt` and Harbor's `result.json` agree, the producer
emits verifier and terminal status evidence, replaces its ephemeral SSE
binding with the retained envelopes inline, marks the visual saved, and
releases its loopback listener. A reopened result no longer needs the sidecar
or a manual score binding. Missing or contradictory scores fail closed.

## Live pixel cuts

Review capture remounts a visual after the native session checkpoint is
frozen. During that remount, `useVisualEvidence` reads the checkpoint's pinned
`source.live-streams` digest; it does not commit a newer poll answer. A capture
with no resolved prefix waits boundedly and fails rather than certifying a
blank connecting placeholder. The capture barrier then verifies zero DOM
mutations before saving pixels and its semantic state receipt.
