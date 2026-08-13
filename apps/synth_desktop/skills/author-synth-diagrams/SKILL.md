---
name: author-synth-diagrams
description: Author a Mermaid diagram into the right Visual pane for this question.
---

# Author Synth diagrams

Use when the user needs a **picture of a system**: a sequence of calls, a class of nouns, a state loop, an ER of records, or a C4 of Desktop vs Containers. Write Mermaid for **this** question. Do not open a stock visual.

Live eval evidence stays on `live.*.v1`. Optimizer runs stay on `optimizer.*.v1`. `blank.canvas.v1` is not a diagram path once `diagram.mermaid.v1` exists.

## Workflow

1. Choose a supported family from the rules below and author the Mermaid now. The common path does not require template discovery, MCP resources, or filesystem search.
2. Call the `synth_visuals.visual_manage` tool directly with `operation: "create"` and **new** `content` in `arguments`. Do not put source in `props`.
3. Read the returned `visual.id`, then call the same tool with `operation: "show"` and that ID so one visual lands in the right pane of this chat.
4. Revise the same ID with `update` + new `content` (new revision). Do not fork a blank canvas.

Do not call `resources/list` or `resources/read`: `synth_visuals` is a tool-only MCP server. Do not shell-read this skill or search the workspace after it has loaded. Reference files are optional syntax help only when the requested family is unfamiliar.

```json
{
  "operation": "create",
  "arguments": {
    "template_id": "diagram.mermaid.v1",
    "title": "How policy_ref reaches the container",
    "content": "sequenceDiagram\nAgent->>MCP: policy_ref\nMCP->>IPC: start\nIPC->>Container: POST /rollouts",
    "presentation": "pane",
    "session_id": "ses_..."
  }
}
```

Then call `visual_manage` again with `{"operation":"show","arguments":{"visual_id":"vis_..."}}`.

## Rules

- Canonical bytes are UTF-8 Mermaid in `content`. Desktop renders SVG; you do not paste SVG into a canvas.
- Default `presentation` is `"pane"`.
- Never bind slot `stream`.
- Missing `content` fails closed. Do not retry as `blank.canvas.v1`.
- Supported families: flowchart, sequence, class, state, er, c4. Sankey, Gantt, git, mindmap, pie, and the rest are unsupported — say so rather than creating an empty pane.
- Family choice: ordered calls → `sequenceDiagram`; topology/pipeline → `flowchart`; nouns/fields → `classDiagram`; lifecycle → `stateDiagram-v2`; records → `erDiagram`; system context → `C4Context`.
