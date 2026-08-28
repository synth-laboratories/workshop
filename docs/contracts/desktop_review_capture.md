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

## Two observers, two counts

A template's `TemplateReadinessContract` is answered by two different
observers, and its knobs do not cross between them.

`minimumRolloutCount`, `minimumRenderedFrameCount`,
`minimumSemanticEventCount` and `requireTerminal` are read from the *rendered*
observation the pane publishes: claims about what the projector folded and what
the DOM then drew. `minimumTransportEnvelopeCount` is read from the host's own
stream receipt at the poll seam: non-control envelopes the transport actually
delivered, counted before any fold has an opinion about them.

**`minimumTransportEnvelopeCount` is deliberately not
`minimumSemanticEventCount`.** The semantic count is a claim about what the
projector produced, and only the fold can answer it; the receipt counts at the
transport level, where a heartbeat and a verifier result are told apart by
envelope kind and nothing more. A template that renders one summary line out of
a hundred envelopes, and a template that fans one envelope into a hundred rows,
both exist — so satisfying a projector claim with a transport count would
certify a fold nobody ran, and satisfying a transport claim with a projector
count would veto a stream that did arrive. Conflating the two is the exact
error the second knob exists to prevent.

Both observers use the same readiness allowlist — `live` or `terminal`, never a
denylist of states somebody remembered to reject (see
`docs/contracts/visual_replay_transport.md`). `minimumTransportEnvelopeCount`
defaults to `0`, so a template that declares nothing keeps precisely the
behaviour it had before the receipt gate existed. The transport gate is
conditioned exactly the way the observation contract is: a visual whose
bindings declare no poll URL has no transport to prove and passes vacuously.
Mermaid, charts and trace-bound projections pass because they declare no
stream, not because anyone listed them.

## The gate's refusals are named

`mark_ready` runs the host's receipt through `stream_receipt_gate`, and every
refusal carries a code. A gate that answers "not ready" tells an agent to try
the same thing again; a named refusal tells it which repair to make, and they
are five different repairs.

- **`visual_observation_unavailable`** — the host recorded no stream poll for
  this visual at this revision. Shared with the capture-path code above, and
  deliberately: both mean the visual was never actually shown in Desktop — a
  browser preview polls with raw `fetch` and never reaches the host seam — and
  both are answered by showing it and retrying, not by changing the visual.
  Retryable.
- **`visual_stream_unsettled`** — the receipt rests in something other than
  `live` or `terminal`: `declared`, `replaying`, `idle`, or last seen failing.
  The declared streams never settled, so the producer and the declared poll
  URLs are the repair, not the template. Carries time in state, whether the
  receipt ever left `declared`, and the declared/responding stream counts.
  Retryable.
- **`stream_replay_gap`** — the replayed history has sequence gaps. A partial
  history is not evidence. This is the one code the gate borrows from
  `diagnostics::codes` rather than owning, so it answers with that vocabulary's
  own remediation. Retryable.
- **`visual_stream_conflict`** — one envelope identity was delivered twice
  carrying two different bodies, so the pane's fold depends on which copy it
  kept. **Not retryable**: a visual cannot be certified over a history that
  contradicts itself, and only the producer's envelope identity or sequencing
  can fix it.
- **`visual_stream_no_evidence`** — the transport settled and delivered fewer
  non-control envelopes than `minimumTransportEnvelopeCount`. Streams that open
  and carry nothing but heartbeats, pings and subscription notices render as a
  believable empty pane, which is the failure this count exists to catch.
  Retryable.

A pass is recorded, not just assumed: `qualityGate.streamCertification` keeps
the receipt's transport state, the declared/responding/closed stream counts,
`envelopeCount`, `nonControlEnvelopeCount` and the
`minimumTransportEnvelopeCount` it was judged against, alongside the
first/last observation timestamps. Telling a current certification from a stale
one should not require re-deriving it.

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
