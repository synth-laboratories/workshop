# Composer steering contract

Branch: `v0.4-container-capability-gating`.
Implementation: `apps/synth_desktop/src/renderer/src/runtime/steering.ts`.

## The gesture

While an assistant turn is active and `submission.activeEnterAction` is
`enqueue`:

1. **First Return** queues the composer text under **Next turns** and arms it.
2. **Second Return** promotes that prompt to an immediate steer of the running
   turn.

The second Return works from the main composer or from the queued-prompt row,
identically. The state machine never sees focus, so the gesture is
keyboard-accessible and never requires clicking the queue.

## States

```
idle ──queued──▶ armed ──return──▶ promoting ──acknowledged──▶ idle
                   │                    │
                   │                    └──rejected──▶ failed (prompt still queued)
                   └──turnEnded / queueReconciled / window lapse──▶ idle
```

- `promoting` is terminal for further Return presses: a second press is the
  same intent, not a second steer.
- A prompt leaves **Next turns** only on backend acknowledgement, and only the
  acknowledgement that names it.
- A rejection leaves the prompt queued and recoverable; it still sends through
  the normal next-turn path.

## The window

`STEER_PROMOTION_WINDOW_MS = 30_000`.

This bounds a *forgotten* arm, nothing else. The arm is ended by something
meaningful — the turn finishing, the prompt leaving the persisted queue, or new
text being typed into the composer — not by a stopwatch.

The previous 2.5s window was the reported bug. It was long enough for a
synthetic double press and far too short for a person, who has to see the
prompt land under **Next turns** before deciding to promote it. Once it lapsed,
the composer's second Return did nothing, and the only way through was to click
the queued row and press Return twice more.

## Keyboard rules

| Input | Behavior |
|---|---|
| `Return` (composer empty, prompt armed) | promote |
| `Return` (new text typed) | enqueue as a distinct prompt; the arm survives |
| `Shift+Return` | newline; never steers |
| Held `Return` (key repeat) | delivers exactly once |
| IME composition commit (`isComposing`, keyCode 229) | passes through untouched; never steers or submits |
| `Cmd/Ctrl+Return` | the alternate action; never promotes |

## Reconnect

`queueReconciled` carries the persisted queue's prompt ids. An armed prompt
that no longer exists disarms rather than steering a prompt that is gone. A
promotion already in flight keeps waiting for its acknowledgement and still
cannot be delivered twice, so a reconnect can neither lose nor duplicate a
steer.

## Error presentation

Every rejection passes through `normalizeSteerFailure` before it can be
rendered. It produces:

- `code` — a stable public code (`steer_turn_finished`, `steer_unsupported`,
  `steer_empty`, `steer_unauthorized`, `steer_rejected`, `steer_unavailable`),
  also exposed as `data-steer-error-code` on the alert.
- `message` — one user-facing sentence: what happened and what to do next.
- `detail` — the full structured original, for logs and Advanced diagnostics.

Rules:

- Identifiers are redacted. The Rust manager rejects with
  `session <uuid> has no active turn to steer`; the UUID reaches `detail` and
  the log, never the composer.
- Any shape normalizes to a string. An object, an array, `null`, a number, or
  an `Error` with an empty message all produce real copy — `[object Object]`
  cannot be rendered.
- An already-normalized failure passes through rather than being re-wrapped,
  so its public code survives.

## Tests

- `apps/synth_desktop/tests/composer_steering.test.mjs` — the state machine and
  error normalization.
- `apps/synth_desktop/tests/playwright/composer-steering.spec.ts` — the gesture
  at human pace in the renderer, held Return, rejection hygiene, and Advanced
  not blocking the composer.
- `apps/synth_desktop/tests/playwright/poolside-polish.spec.ts` — the
  pre-existing steer/enqueue coverage, unchanged.
