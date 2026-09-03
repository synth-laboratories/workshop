# Contract: Desktop review capture

Governs `capture_review` in `synth_visuals_mcp.rs` and
`resize_review_window` / `capture_review_window` plus
`visuals::snapshot` in the host (`visuals_ipc.rs`, `visuals/snapshot.rs`).

## The rule

**The app photographs its own surface. Nothing else is ever in frame.**

Desktop review capture is a host-owned `WKWebView takeSnapshot` of the main
window's webview, executed inside the signed app process and returned through
the authenticated visuals IPC. There is no OS-level screen capture, so:

- **No Screen Recording TCC grant is required.** A fresh build with no grant
  captures successfully, and System Settings never opens.
- **No window-identity resolution exists.** The host snapshots the webview it
  owns; photographing the wrong window is not expressible. The previous
  pipeline (`CGWindowListCopyWindowInfo` resolver → `/usr/sbin/screencapture
  -l` → `sips`) and its whole error vocabulary — `DesktopWindowNotFound`,
  `DesktopWindowAmbiguous`, `ScreenRecordingDeniedOrUnavailable`,
  `BOUNDS_DRIFT` — are gone with it.
- **Occlusion does not matter.** The snapshot renders the webview's own
  content, so capture works while the app is backgrounded, covered, or on
  another Space. `screencapture -x` could never guarantee that.

## One call, host-side transaction

The helper's `capture_review` issues `POST /v1/visuals/{id}/show` and then a
single `POST /v1/review-window/capture` with
`{width, height, outputPath}`. The host performs resize → settle →
snapshot → restore as one operation and returns
`{path, width, height, previous, current, scaleFactor, windowLabel,
processId, captureMode, restored}`.

Because resize and restore never leave the host, a helper that dies
mid-capture can no longer strand the user's window at the review size (the
open defect recorded against the previous split pipeline). The snapshot has a
hard timeout; on timeout the restore still runs.

## Geometry

One declared bound, shared by the tool schema, the resize/capture endpoints,
and the capture policy:

```
320×400 … 2400×1800     // REVIEW_VIEWPORT_{WIDTH,HEIGHT}_{MIN,MAX}
```

A unit test asserts the tool schema and the runtime check agree, because their
disagreeing is the exact defect this replaced. Sizes on these endpoints are
logical (CSS) pixels; the receipt records the display scale factor and the
PNG's actual pixel dimensions.

## Restore is transactional and reported

The window is resized before capture, so every exit path restores it. A failed
restore reaches the caller **even when the capture itself failed** — it is the
one side effect this operation cannot undo, and it used to be dropped on that
path.

## Output-path confinement

The helper names the output file; the host refuses to write outside the
visuals data root it was spawned with (canonicalized prefix check). The
capture is only as trustworthy as the process that wrote it, and that process
is the signed host.

## Evidence checks, unchanged

- `assert_non_blank_png` still gates every capture: a uniform/transparent
  snapshot fails. With OS capture this doubled as the TCC-denial detector; it
  remains the guard against a blank or unpainted webview.
- Rendered-observation freshness (`visual_observation_stale` /
  `visual_observation_unavailable`) still gates templates that declare an
  observation contract.
- The `…​.observations.json` sidecar (`synth.visual-capture-observation.v1`)
  is still written beside every PNG and still required by review submission.

## Receipt

Written beside every review PNG, in `….observations.json` under `window`
(`schemaVersion: synth.visual-capture-window.v1`,
`captureMode: host-webview-snapshot`):

- requested viewport, resized viewport, previous viewport, image size,
  scale factor
- process id and window label of the window that was actually snapshotted
- restore outcome

Reconstructing what a review captured, at what bounds, and whether the user's
window was put back should not require reading an agent transcript.
