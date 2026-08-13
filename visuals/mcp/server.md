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
