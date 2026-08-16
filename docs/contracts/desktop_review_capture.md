# Contract: Desktop review capture

Governs `capture_review` and the macOS window resolver in
`synth_visuals_mcp.rs`, and `resize_review_window` in `visuals_ipc.rs`.

## The rule

**Identity, then geometry — in that order, never mixed.**

A window is the right window or it is not. How large it happens to be is a
separate question with a separate answer.

The resolver used to filter candidates on `width >= 640, height >= 400`
*before* evaluating bundle identity. A compact review at 390×844 — a viewport
the public schema advertises — therefore reported `DesktopWindowNotFound` for
an app that was on screen, running, and correctly identified. Widening the
number would have hidden the defect; the number was in the wrong place.

## Identity

Strongest available signal wins:

1. **Process id**, when `resize_review_window` reported one. Exact, free, and
   unambiguous even with several named instances running.
2. **Bundle id**, exactly matched. Authoritative for named instances.
3. **Owner name**, only when no bundle id is available.

macOS truncates owner names (`Synth Workshop v0.4 · sync-rep`). A truncated name
is reported for humans and never used to reject a window that identity already
matched.

## Geometry

One declared bound, shared by the tool schema, the resize endpoint, and the
capture policy:

```
320×400 … 2400×1800     // REVIEW_VIEWPORT_{WIDTH,HEIGHT}_{MIN,MAX}
```

A unit test asserts the tool schema and the runtime check agree, because their
disagreeing is the exact defect this replaced.

After a window is selected, its observed bounds are compared against what the
resize actually produced, within `REVIEW_BOUNDS_TOLERANCE_POINTS` (window
managers round and add chrome). Drift is **reported**, not fatal: the capture is
still of the identified window, and the receipt records what was photographed.

## One identity, established once

`resize_review_window` holds the window it resizes and returns
`{previous, current, scaleFactor, windowLabel, processId}`. Capture carries that
receipt in and *verifies* rather than rediscovering. This removes the second
lookup, and with it the race between resizing one window and photographing
another.

When identity yields several windows and none is distinguishable by size, the
capture fails with `DesktopWindowAmbiguous` listing bundle, pid, window number,
and bounds for each. It never falls back to the largest — three Desktop
instances with different bundle ids were on screen during the v0.4 acceptance
run, and picking the biggest would have silently captured the wrong product.

## Restore is transactional and reported

The window is resized before capture, so every exit path restores it. A failed
restore reaches the caller **even when the capture itself failed** — it is the
one side effect this operation cannot undo, and it used to be dropped on that
path.

## Receipt

Written beside every review PNG, in `…​.observations.json` under `window`:

- requested viewport, resized viewport, previous viewport, scale factor
- process id, bundle id, resolved window number, observed bounds
- restore outcome and any restore error

Reconstructing which window a review captured, at what bounds, and whether the
user's window was put back should not require reading an agent transcript.
