# Diagnostics push 2 — from spine to true

Push 1 (`fe6fada`, branch `v0.4-local-diagnostics`) built the spine: envelope,
redaction, bounded queue, journal store, typed query, LogsQL compiler, sidecar,
indexer, explain, MCP, pane. It is proven against a real VictoriaLogs process.

It is not yet *true*. The system claims to correlate failures across the
renderer, Tauri, MCP, containers, streams, visuals, optimizers, and providers.
Today it observes about a third of those, and the third it misses includes the
live container stream — the path the ten-lane Craftax run actually used.

This push closes that, decides retention, finishes the surfaces, and validates
the whole thing in a packaged app.

Read `DIAGNOSTICS.md` first for the two invariants. Nothing below may break
either one:

1. The index returns journal sequences; records always come from SQLite.
2. Nothing on a producer path waits.

---

## Decisions needed before coding

Three, and two of them are Josh's.

### D1 — Journal retention for diagnostics (blocking, Josh)

VictoriaLogs is capped at 7 days / 2 GB. The authoritative journal has **no**
diagnostic retention: `kind='diagnostic.event'` rows accumulate forever. Pick
one:

- **(a) Trim.** Delete diagnostic rows older than N days (proposed default 30 —
  deliberately longer than the index, so the journal outlives what it feeds).
  Bounded background pass, diagnostic kinds only.
- **(b) Permanent.** Diagnostics are journal rows like any other and are never
  deleted. Simplest, and consistent with "the journal is authoritative", but the
  file grows without bound on a busy install.

Recommendation: **(a) with a generous default.** The evidence that matters —
traces, run records, seals — is stored elsewhere and untouched either way; a
diagnostic older than a month has no reader. If (a), the trim must be
cursor-aware only in one direction: it may delete rows the indexer already
passed, and it must never move the cursor backward.

### D2 — How visuals emit (blocking, technical; propose and proceed)

`visuals/` is a separate package with React as its only peer dependency. It
cannot import the renderer's `runtime/diagnostics.ts`, and its bundles also run
outside the app (browser preview, frozen runtime, exported visuals). So the
live-eval stream hooks — `visuals/chrome/useLiveEvalStream{,s}.ts` — need their
own seam.

Proposed: a `visuals/runtime/diagnostics.ts` inside the package that emits
through a host-installed sink:

```ts
// The renderer installs the sink at startup. Absent host, absent sink:
// a visual rendered in a browser preview reports nothing and breaks nothing.
type DiagnosticSink = (report: VisualDiagnostic) => void;
declare global { interface Window { __synthDiagnosticSink?: DiagnosticSink } }
```

The renderer installs `window.__synthDiagnosticSink = reportDiagnostic` once.
No package dependency, no import cycle, and the no-host path is a no-op rather
than a thrown error inside someone's chart.

### D3 — No global emitter handle (technical; already decided, do not revisit)

Do **not** add a process-wide `OnceLock<Arc<DiagnosticsService>>`. Two
CoreRuntimes exist in one process in tests, and a global makes one silently win.
Thread handles the way the codebase already threads `core`:

- Call sites that hold `core` → `core.diagnostics_service()`.
- Long-lived services constructed before diagnostics → an `Arc<OnceLock<…>>`
  slot plus `attach_diagnostics()`, exactly as `OptimizerService` does.
- Free functions (`container_stream.rs`) → an explicit
  `Option<&Arc<DiagnosticsService>>` parameter from their callers, which already
  hold `core`.

---

## W1 — Instrumentation breadth (the bulk of the push)

Closes acceptance criterion 4 and most of 3, 5, 6. Everything else in this
document is small by comparison.

| File | What it must emit |
| --- | --- |
| `src-tauri/src/container_stream.rs` | SSE/WS open, subscribed, interrupted, retry, closed, replay gap; subscribe timeout; transport refusal; prepare/start/poll/reward/Trace-V5 failures |
| `src-tauri/src/container_capabilities.rs` | Probe and preflight decisions **at the source**, so the eval-driver path records a rejection that never traverses IPC |
| `src-tauri/src/visuals/registry.rs` | create, bind, render, capture, review, ready, seal failures with visual id, revision, operation, status, remediation |
| `src-tauri/src/optimizers/manager.rs` | Sidecar install/start/stop/health/version, preflight, worker spawn, terminal status |
| `src-tauri/src/session/codex/manager.rs` | Local-agent connection lifecycle, provider heartbeat, rate-limit and token-usage summaries, compaction, terminal failures |
| `visuals/chrome/useLiveEvalStream{,s}.ts` | The renderer half of the live stream, through the D2 sink |

Rules for every emitter:

- Use a stable code from `diagnostics/codes.rs`; add new ones **with remediation
  text** (a test enforces this) and a causal rank. Rank is what lets `explain`
  say a capability rejection caused a blank visual rather than the reverse.
- Name every identity in scope. A stream event that knows its rollout but not
  its visual is half a correlation.
- `details` is a summary, never a payload. Pointers to evidence (log paths,
  digests), not the evidence itself.
- Successes are `info` at most, and only for lifecycle transitions worth
  correlating — not per frame, per event, or per poll.

**Acceptance for W1** (the test that proves criterion 4): extend the fake-service
harness in `tests/container_capability_gating.rs` into a
`tests/diagnostics_correlation.rs` — a fake container that accepts a rollout,
opens an SSE stream, and drops it mid-run. Then assert that
`diagnostics_explain` on the **visual id alone** returns the stream interruption
correlated to its stream, rollout, container, and session, with the upstream
cause named. That single test is the definition of done for this workstream.

---

## W2 — Retention (after D1)

If (a): a bounded trim pass in `diagnostics/store.rs`, run from the same
background loop as indexing, deleting only `kind='diagnostic.event'` beyond the
window and beyond a row-count ceiling. Report retained window and row count in
`status`.

Tests: only diagnostic kinds are ever deleted; a trim never moves the index
cursor backward; the trim is bounded per pass (it must not lock the database on
a huge backlog).

---

## W3 — Finish the surfaces (small, high value)

1. **Explain in the pane.** The operation the system exists for is agent-only
   today. Add a per-event and per-group "Explain" action calling
   `diagnostics_explain` with that event's identities, rendering cause →
   symptoms → remediation. No prose; the remediation string is the text.
2. **Deep links.** `routes.tsx` passes only `onOpenVisual` and
   `onOpenContainer`; wire `onOpenOptimizer` and `onOpenTrace`.
3. **Filters.** Pass `visualId` when the pane opens from a visual, and derive
   rollout/optimizer filter chips from the result set — identities that are on
   screen, not free-text inputs.
4. **a11y.** Add the pane to the existing axe-based playwright gate.

---

## W4 — `use-synth-diagnostics` skill (small)

`apps/synth_desktop/skills/use-synth-diagnostics/SKILL.md`, shaped like
`use-synth-containers`, plus `Load the use-synth-diagnostics skill.` appended to
the tool description in `synth_diagnostics_mcp.rs`.

The workflow it must teach is explain-first: *start from the identity you
already hold, call `explain`, then narrow with `query` on the code it names.*
An agent that reaches for `query` first will page through symptoms and never
find the cause.

---

## W5 — Packaged acceptance (last; it validates the rest)

- **Criterion 11.** `cargo tauri build`, then verify the nested binary exists at
  `Contents/Resources/services/victoria-logs/victoria-logs`, is signed with the
  app, and actually starts from inside the bundle. This is the single largest
  unverified claim in push 1.
- **Criterion 12.** Two live instances via `scripts/desktop-instance.sh`: two
  descriptors, two ports, two data directories, no cross-talk.
- **Criterion 2 / the performance test.** Measure startup → first paint, chat
  send → first token, task restore, and a 10-stream visual across four index
  states: absent, ready, hung, crashed. Drive it headlessly through the
  eval-driver API rather than CUA.
- **Remaining injection.** Port collision (bind the reserved port between
  reserve and spawn), quota exceeded (tiny `quota_bytes`), retention expiry
  (short `retentionPeriod`, assert rows leave the index and stay in the journal).

---

## W6 — Slow-call telemetry (optional)

The MCP wrapper records failures only, so there is no latency baseline and "this
call got slow" is invisible. Add a duration threshold (~1s) that emits one
`info` diagnostic for a slow *successful* call. Thresholded, never sampled —
a sampled latency record answers no specific question.

---

## Sequencing

```
D1 (Josh) ──────────────┐
D2, D3 (decided) ──► W1 ├─► W2
                        ├─► W3 ─┐
                        └─► W4 ─┴─► W5
                                     W6 (any time)
```

W1 first and alone if effort is limited: it is what makes the system's central
claim true. W3 and W4 are small enough to ride alongside. W5 last, because it
validates everything above it.

## Risks

- **Branch drift.** `v0.4-local-diagnostics` is unpushed and cut from
  `v0.4-container-capability-gating` @ `1a15f52`. That lane is live and shared.
  Reconcile against wherever `josh/v04` has moved *before* starting, not after.
- **Instrumentation as a flood.** The bounded queue protects the process, but a
  per-frame or per-poll emitter will fill it with noise and push real errors
  into the drop counters. Lifecycle transitions and failures only.
- **`instance_id` is null in a shipped install.** It comes from
  `instance::name()`, set only for named dev instances. Per-instance isolation
  still holds through separate data roots, but decide whether the canonical app
  should stamp a stable local id before this ships.
- **Pre-existing red.** `layout-invariants › a long prompt never hides the
  active turn beneath the composer` fails at `1a15f52` with no diagnostics
  changes applied. Track it; do not fix it here.
