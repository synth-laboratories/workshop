# Visuals MCP server (local)

How Desktop / the local runtime daemon exposes `@synth/visuals` tools to coding agents.

## Transport

- **Local stdio MCP** preferred for Desktop-attached agents (same pattern as research MCP).
- Tool schemas live in [`tools.json`](./tools.json) — register them verbatim.
- Implementation hosts under `workshop/visuals/` and may import:

```ts
import {
  listTemplates,
  resolveTemplate,
  bindTemplateSlots,
} from "@synth/visuals";
```

## Tool → runtime mapping

| Tool | Behavior |
| --- | --- |
| `visual_list_templates` | `listTemplates()`, optional `genre` filter |
| `visual_create_from_template` | `resolveTemplate(id)` → new `VisualInstance` in daemon memory / local store |
| `visual_bind_data_source` | append/replace `VisualBinding`; validate slot via template meta; optionally `bindTemplateSlots` |
| `visual_open_in_pane` | IPC to renderer VisualPane or full canvas: load a trusted registered shell |
| `visual_stream_live_eval` | ensure live.* instance, bind `live_sse` or fixture, open pane, start SSE subscribe |
| `visual_chart` | write a `synth.visual.chart-spec.v1` spec to an `analysis.chart.v1` visual, render it in the host, and return the PNG — the whole authoring loop in one call |
| `visual_authoring_context` | return revision, template/example evidence, presentation, review count, and required checks |
| `visual_review` | persist a rendered viewport critique against the exact current revision |
| `visual_mark_ready` | require two passing distinct-width reviews and write the `qualityGate` receipt |

## Binding kinds

| Kind | Source |
| --- | --- |
| `fixture` | Path relative to `visuals/` (e.g. `fixtures/craftax_matrix_slice.json`) |
| `trace_v5` | Sealed trace digest / catalog id (read-only) |
| `local_cas` | Content-addressed blob in Desktop local CAS |
| `live_sse` | Absolute SSE URL; shell uses `EventSource` / daemon proxy |

## Agent happy path

1. `visual_list_templates` → pick the task-family trusted template.
2. `visual_create_from_template` `{ template_id, title, presentation: "canvas", visual_config }`.
3. Bind real or explicitly labeled replay evidence.
4. Open, render, critique, and revise at least twice.
5. Record wide and compact `visual_review` receipts.
6. `visual_mark_ready` for the current revision.

Live evals:

1. Prepare the container without starting it and bind the returned declared SSE URL.
2. Open the live visual in canvas mode, iterate twice, and mark its current revision ready.
3. Start only with that visual id and exact prepared descriptor. Slot is `stream`, never a guessed `/events` path.

## Ad-hoc data charts

`analysis.chart.v1` is the ad-hoc chart family: the agent authors a bounded
JSON spec, the host renders it to SVG in-process, and the pane displays that
same rendition. Because the image exists without a pane, `capture_review`
takes the deterministic path — no window, no show, no observation handshake —
so the loop is `visual_chart` → look at the PNG → `visual_chart` again with
the same `visual_id`. Contract: [`docs/contracts/visual_chart_spec.md`](../../docs/contracts/visual_chart_spec.md).

Nulls are absence, not zero: a null `y` breaks the line, a null bar value is a
hatched stub, a null heatmap cell is hatched, a null table cell is an em dash.

Panels do not have to carry their numbers. A `from` block names a bound slot, a
path into it, a transform pipeline, and which columns become which channel, so
"chart this trace" is a binding plus a mapping rather than a paste:

```json
{"kind":"series","title":"Cumulative reward","from":{
  "source":{"slot":"rollout","path":"steps","transform":[
    {"op":"sort","by":"turn"},
    {"op":"derive","field":"total","from":{"cumulative":"reward"}}]},
  "series":[{"name":"cumulative","x":"turn","y":"total"}]}}
```

`visual_chart` takes `slot`/`kind`/`source` alongside `spec`, so the binding and
the chart land in one call. Readable kinds are `inline`, `fixture`, `local_cas`,
`trace_v5`, `query_snapshot`, and `optimizer_run` — the last of which puts an
eval's per-trial ledger (`path: "run.summary.records"`) on a table or a distribution
instead of collapsing it into a mean. An optimizer run read before it seals is
recorded as a snapshot with the cursor it was taken at. `live_sse` is refused,
because a still image has no single value to draw from a stream.

## Security notes

- Arbitrary agent-authored TSX is retained only as source evidence; Desktop does not execute it. Interactive viewers are trusted registered templates with bounded configuration.
- `trace_v5` bindings are read-only; annotation templates bind overlay slots separately.
- SSE URLs should be localhost / runtime-proxied unless user-approved.

## Desktop wiring sketch

```ts
// renderer
import { getShellImporter } from "@synth/visuals/registry";

const load = getShellImporter(instance.templateId);
const mod = await load?.();
// <mod.Shell data={boundSlots} title={instance.title} bindings={instance.bindings} />
```
