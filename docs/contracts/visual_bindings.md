# Contract: visual bindings

Governs `visuals::models::canonicalize_bindings` (Rust) and
`resolveVisualBindings` (TypeScript). Those two decide, respectively, what a
visual may persist and what the renderer may read. They must agree.

## The envelope

```json
{
  "schemaVersion": "synth.visual-bindings.v1",
  "inputs": [
    {
      "input": "stream",
      "slot": "stream",
      "kind": "live_sse",
      "source": "http://127.0.0.1:8114/rollouts/roll_a/stream",
      "poll_url": "http://127.0.0.1:8114/rollouts/roll_a/events",
      "schema": "synth.trace-stream-event.v1"
    }
  ],
  "slots": [
    {
      "input": "stream",
      "slot": "stream",
      "kind": "live_sse",
      "source": "http://127.0.0.1:8114/rollouts/roll_a/stream",
      "poll_url": "http://127.0.0.1:8114/rollouts/roll_a/events",
      "schema": "synth.trace-stream-event.v1"
    }
  ]
}
```

`inputs` is the canonical flat array. `slots` is a one-release copy of the same
descriptors. A template input declaring `"multiple": true` accepts several
entries with the same `input` name — ten rollout streams on one `stream` input
is the supported case, not a workaround.

Readers accept `input` or `slot` (and `inputs` or `slots`). If both names are
present and disagree, fail closed. Writers stamp both fields.

`kind` is drawn from a closed vocabulary: `inline`, `trace_v5`, `local_cas`,
`run_ref`, `live_sse`, `fixture`, `optimizer_run`, `query_snapshot`. An unknown
kind fails the write.

`query_snapshot` addresses an immutable result set by snapshot id. A visual must
never bind to a live query: it would return different rows on every render, and
the page could not state what the reader is looking at.

## Three outcomes, no fourth

Every write path — HTTP, MCP, import, migration — canonicalises. Every read path
resolves through the same rules.

| Input | Outcome |
| --- | --- |
| The envelope above | accepted unchanged |
| An empty object | an empty envelope; an authoring default, not an upgrade |
| A slot-keyed descriptor map, `{"stream": [{…}]}` | **upgraded**, reported at warn with `visual_bindings_upgraded` |
| A legacy inline prop bag, `{"matrix": […]}` | **upgraded** to `inline` slots, reported the same way |
| Anything else | **refused** with a message naming what could not be read |

There is deliberately no "return nothing and let the caller carry on". A shape
that cannot be read renders an empty pane, and an empty pane is
indistinguishable from a stream that produced no evidence. The v0.4 CUA
acceptance run failed exactly there: ten correct `live_sse` descriptors were
persisted under a slot key, every layer accepted them, and the pane sat at
`connecting` with zero counts and no error.

## Telling a descriptor from inline data

A legacy prop bag and a slot-keyed descriptor map are both bare JSON objects, so
the shape alone has to separate them. One heuristic, in one function, in each
language:

> A value is a binding descriptor when it names a `kind` from the vocabulary
> **and** carries at least one field only a descriptor has: `input`, `slot`,
> `source`, `data`, or `poll_url`.

So `{"chart": {"kind": "bar"}}` stays inline chart data. A slot map that mixes
descriptors and raw data is refused rather than guessed at.

The slot key is authoritative: a descriptor filed under `"stream"` is a stream
binding whatever its own `slot` field claims.

**COMPAT.** Both upgrade paths are compatibility code. They are loud so writers
get fixed, and they are removed once `visual_bindings_upgraded` stops firing in
the field. Write the envelope.

## Digests

`bindings_digest` is computed **after** canonicalisation, so it identifies what
the renderer will actually read. Rendered-observation receipts compare against
it, and a digest over an unreadable shape would compare equal while proving
nothing.

Existing rows are brought forward by `visuals::backfill`, which runs at database
open. It upgrades `visuals.bindings_json` and re-stamps the current revision's
digest, records the superseded digest under `metadata_json.bindingsUpgrade`, and
leaves historical revisions exactly as authored — they are the audit trail, and
a receipt that already named an old digest must keep resolving. A row it cannot
canonicalise is left untouched and counted, never silently repaired.

## Authoring

`visual_bind_data_source` is the supported way to write bindings: it emits the
envelope. Use `mode: "append"` with `bindings: [...]` for a `multiple` input.

`visual_update` still accepts a raw `bindings` object because importers use it.
Send the envelope.
