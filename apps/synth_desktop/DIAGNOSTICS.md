# Local diagnostics

A fully local, app-bundled diagnostic system: one envelope emitted by every
surface, persisted in the authoritative journal, indexed into a bundled
loopback VictoriaLogs, and queried through a typed MCP.

It exists because of a specific failure. A ten-lane Craftax run completed and
sealed ten Trace V5 records; the visual stayed empty and eventually rendered
`Unsupported trace projection schema: synth.trace.v5`. That fact lived only in
a webview console. Finding it took repeated MCP calls, CUA inspection, and
source archaeology. The definition of done is that the same failure is now one
typed query away.

## Shape

```
renderer / tauri / mcp / containers / visuals / optimizers / providers
                             |
                    DiagnosticBus (bounded, non-blocking)
                             |
                    batched writer -> event journal (authoritative)
                             |
                    indexer (by durable sequence) -> VictoriaLogs
                             |
                    synth_diagnostics MCP  /  Diagnostics pane
```

Two properties carry the design:

**The index is disposable.** VictoriaLogs is queried for *journal sequences*,
never for records; the records always come back from SQLite. A wiped, stale,
crashed, or entirely absent index changes how fast a question is answered and
never what the answer is. It is also why replay after a restart cannot produce
duplicate logical events: the sequence is the identity.

**Nothing on a producer path waits.** Emission is a mutex, a push, and a
notify. Persistence is a batched background write. Indexing only ever reads
committed rows. A saturated queue drops informational events first, counts what
it dropped by severity and component, and emits exactly one bounded saturation
diagnostic on recovery.

## Files

| Path | What it owns |
| --- | --- |
| `src-tauri/src/diagnostics/event.rs` | `synth.diagnostic-event.v1`: validation, bounds, correlation fields |
| `src-tauri/src/diagnostics/redact.rs` | Central redaction — runs once, before the queue |
| `src-tauri/src/diagnostics/bus.rs` | Bounded two-lane queue and drop accounting |
| `src-tauri/src/diagnostics/store.rs` | Journal-backed persistence and typed SQL search |
| `src-tauri/src/diagnostics/query.rs` | The typed query contract and its hard ceilings |
| `src-tauri/src/diagnostics/victorialogs.rs` | Client plus the typed-query → LogsQL compiler |
| `src-tauri/src/diagnostics/sidecar.rs` | Process supervisor, descriptor, retention, quota |
| `src-tauri/src/diagnostics/indexer.rs` | Durable-sequence cursor and catch-up |
| `src-tauri/src/diagnostics/explain.rs` | Deterministic cause / symptom ordering |
| `src-tauri/src/diagnostics/codes.rs` | Stable codes, causal rank, remediation text |
| `src-tauri/src/bin/synth_diagnostics_mcp.rs` | The agent-facing stdio adapter |
| `src/renderer/src/runtime/diagnostics.ts` | Renderer emitter |
| `src/renderer/src/components/DiagnosticsPanel.tsx` | The Diagnostics surface |

## Staging the binary

```
./scripts/diagnostics/fetch-victorialogs.sh
```

The executable is not committed. It lands at
`services/victoria-logs/victoria-logs`, which `tauri.conf.json` bundles into
`Synth Workshop.app/Contents/Resources/services/victoria-logs/`. Without it,
diagnostics report `degraded` and every query answers from the journal — which
is a supported mode, not a broken one.

Per instance, runtime state lives under the instance data root:

```
<data root>/diagnostics/
  victorialogs-data/      # the disposable index
  bundles/                # redacted local support artifacts
  descriptor.json         # url, pid, state, retention, quota (0600)
  indexer-cursor.json     # durable journal sequence
```

## The agent surface

`synth_diagnostics` exposes one tool, `diagnostics_manage`, with five
operations: `status`, `query`, `tail`, `explain`, `bundle`.

There is no raw LogsQL parameter, no SQL, no path, and no URL — not guarded,
*absent*. Filters are allow-listed fields; codes, events, and components must
match an identifier shape; correlation identities are refused if they contain
anything that could change the meaning of a compiled query. Ranges are capped at
7 days, limits at 500 rows, responses at 256 KB, and queries at a 5 second
timeout.

`explain` is the operation worth knowing. Given identities you already hold, it
expands one hop through the correlations the matched events name, then orders
the result by causal rank before time — so a container capability rejection is
reported as the cause of the blank visual it produced, even though the renderer
noticed first. It is deterministic and involves no model call.

```json
{
  "operation": "explain",
  "arguments": { "visual_id": "vis_9", "since": "20m" }
}
```

## Privacy

Redaction runs once, before the queue, so no later stage can be the thing that
leaks. Secret-shaped keys lose their values; secret-shaped substrings (bearer
tokens, provider key prefixes, JWTs, URL userinfo, PEM blocks) are scrubbed from
free text; prompt-shaped keys collapse to `{length, digest}`; environment
snapshots are dropped whole, both by key name and by shape. `diagnostics_bundle`
writes a `0600` file locally and uploads nothing.

Retention defaults to 7 days or a 2 GB quota, whichever comes first, enforced by
VictoriaLogs. "Clear diagnostic index" deletes the index and its cursor; the
authoritative journal, sealed traces, and run evidence are untouched, and the
next indexing pass rebuilds from zero.

## Tests

```
cargo test --manifest-path src-tauri/Cargo.toml --lib diagnostics::
cargo test --manifest-path src-tauri/Cargo.toml --test diagnostics_index
../../node_modules/.bin/playwright test tests/playwright/diagnostics-reporting.spec.ts
```

`diagnostics_index` starts the real sidecar. If the binary has not been staged,
the three tests that need it **skip loudly** rather than passing quietly — a
green run that never started a process would be the exact failure the file
exists to catch.

## Adding an emitter

Use the stable code from `codes.rs` (add one there, with remediation text, if
none fits), name every identity you hold, and keep `details` a summary rather
than a payload:

```rust
core.diagnostics_service().emit(
    DiagnosticInput::new(
        Severity::Error,
        "containers",
        "container.rollout.failed",
        codes::CONTAINER_ROLLOUT_FAILED,
        message,
    )
    .retryable(true)
    .with_correlation("rollout_id", Some(rollout_id))
    .with_correlation("container_id", Some(container_id)),
);
```

From the renderer, `reportDiagnostic` in `runtime/diagnostics.ts`. Emit
alongside `console.error`, never instead of the structured record.
