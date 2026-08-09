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
  saveVisualInstanceTsx,
  markInstanceSaved
} from "@synth/visuals";
```

## Tool → runtime mapping

| Tool | Behavior |
| --- | --- |
| `visual_list_templates` | `listTemplates()`, optional `genre` filter |
| `visual_create_from_template` | `resolveTemplate(id)` → new `VisualInstance` in daemon memory / local store |
| `visual_bind_data_source` | append/replace `VisualBinding`; validate slot via template meta; optionally `bindTemplateSlots` |
| `visual_save_tsx` | `saveVisualInstanceTsx` → `visuals/instances/<id>.tsx` |
| `visual_open_in_pane` | IPC to renderer VisualPane: load shell importer or saved TSX |
| `visual_stream_live_eval` | ensure live.* instance, bind `live_sse` or fixture, open pane, start SSE subscribe |

## Binding kinds

| Kind | Source |
| --- | --- |
| `fixture` | Path relative to `visuals/` (e.g. `fixtures/craftax_matrix_slice.json`) |
| `trace_v5` | Sealed trace digest / catalog id (read-only) |
| `local_cas` | Content-addressed blob in Desktop local CAS |
| `live_sse` | Absolute SSE URL; shell uses `EventSource` / daemon proxy |

## Agent happy path

1. `visual_list_templates` → pick `craftax.eval_matrix.v1`
2. `visual_create_from_template` `{ template_id, title }`
3. `visual_bind_data_source` `{ slot: "matrix", kind: "fixture", source: "fixtures/craftax_matrix_slice.json" }`
4. `visual_save_tsx` → path under `instances/`
5. `visual_open_in_pane` `{ instance_id }`

Live evals:

1. `visual_stream_live_eval` `{ template_id: "live.dock_harbor.v1", sse_url: "http://127.0.0.1:…/events" }`

## Security notes

- Do not allow agents to write outside `visuals/instances/` via `visual_save_tsx`.
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
