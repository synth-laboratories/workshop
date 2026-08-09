# @synth/visuals

First-class **deep visuals** for Synth Desktop. Coding agents form blog-quality eval visuals (Craftax-class, PostTrainBench rollout viewer, reward / annotation / model compare) into the Desktop visual pane, save them as TSX, and stream live dock / harbor / intern acceptance results into dedicated templates.

Colocated under `workshop/visuals/` (not cookbooks yet). Accent: Synth orange `#F05F22` / `#FF5C00`. Light chrome compatible with Poolside-like Desktop.

## Architecture

```text
visuals/
  chrome/           # shared VisualChrome, scrubber, live hook, tokens.css
  fixtures/         # small JSON samples
  templates/<id>/   # template.json + shell.tsx + examples + README
  registry/         # list + resolve + shell importers
  runtime/          # types, bind, save_tsx
  mcp/              # tool schemas + server notes
  instances/        # agent-saved .tsx shells (generated)
```

```text
agent (MCP)
  → create instance from template
  → bind fixture | trace_v5 | local_cas | live_sse
  → save_tsx → visuals/instances/<id>.tsx
  → open_in_pane → Desktop VisualPane loads Shell
```

Desktop talks to templates through `@synth/visuals` exports only — no Next.js, relative imports, React peer dep.

## How agents create visuals

1. List templates: MCP `visual_list_templates` or `listTemplates()` from `@synth/visuals`.
2. Create: `visual_create_from_template` with a `template_id`.
3. Bind slots: `visual_bind_data_source` (`fixture` for demos; `trace_v5` / `local_cas` / `live_sse` in product).
4. Persist: `visual_save_tsx` writes a thin shell wrapper under `instances/`.
5. Show: `visual_open_in_pane`.
6. Live: `visual_stream_live_eval` for `live.eval_stream.v1` · `live.dock_harbor.v1` · `live.intern_acceptance.v1`.

See [`mcp/server.md`](./mcp/server.md) and [`mcp/tools.json`](./mcp/tools.json).

## How Desktop loads them

```ts
import {
  listTemplates,
  resolveTemplate,
  getShellImporter,
  bindTemplateSlots
} from "@synth/visuals";

const template = resolveTemplate("craftax.eval_matrix.v1");
const load = getShellImporter(template.id);
const { Shell } = await load();

// After binding slots with daemon loaders:
// <Shell title="…" data={matrixSlice} bindings={instance.bindings} />
```

Saved instances:

```ts
import Instance from "@synth/visuals/instances/<id>.tsx"; // or dynamic import of file path
```

Import chrome tokens once in the renderer:

```ts
import "@synth/visuals/chrome/tokens.css";
```

Package name: `@synth/visuals` — wire via workspace path in Desktop `package.json`:

```json
"@synth/visuals": "workspace:*"
```

or

```json
"@synth/visuals": "file:../../visuals"
```

## Templates

| Id | Purpose |
| --- | --- |
| `craftax.eval_matrix.v1` | Pareto cost/perf + achievement matrix |
| `craftax.rollout_scrub.v1` | Frame scrubber + HUD + accessible text projection |
| `posttrain.rollout_viewer.v1` | Trajectory timeline, steps, rewards, actions |
| `reward.breakdown.v1` | Typed reward component chart |
| `annotation.overlay.v1` | Markers on sealed Trace V5 (overlay only) |
| `model.compare.v1` | Multi-model table + sparklines |
| `live.eval_stream.v1` | Live eval / acceptance event stream |
| `live.dock_harbor.v1` | Dock/harbor job status + rollout stream |
| `live.intern_acceptance.v1` | Intern sync/async acceptance cell stream |

Each template directory contains `template.json`, `shell.tsx`, `examples/`, `README.md` (plus `components/` when needed).

## Bindings

| Kind | Use |
| --- | --- |
| `fixture` | Offline / demo JSON under `fixtures/` |
| `trace_v5` | Sealed trace (read-only) |
| `local_cas` | Desktop content-addressed blobs |
| `live_sse` | Streaming eval / job / acceptance events |

Annotations never mutate sealed traces — `annotation.overlay.v1` is overlay-only.

## Accessibility

Scrubbers expose `aria-valuetext` / `role="group"`. Charts use `role="img"` or `role="meter"`. Live regions use `aria-live="polite"`. Craftax scrub always shows `observation_text` beside the canvas.
