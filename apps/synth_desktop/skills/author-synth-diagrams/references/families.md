# Modes and Mermaid families

Pick the smallest set that answers the question.

| Need | Mode | Template | Reference |
| --- | --- | --- | --- |
| Broad topology, ownership, before/after, missing edges | static 2D systems map | `diagram.systems.v1` | [systems-map.md](systems-map.md) |
| Movement or state changing over time | Benjamin Dicken Style dynamic explanation | `diagram.systems.dynamic.v1` | [dynamic-systems.md](dynamic-systems.md) |
| Exact formal or automatically laid-out semantics | Mermaid/UML | `diagram.mermaid.v1` | family table below |

For a broad architecture question with exact behavioral detail, create a 2D overview and a focused Mermaid sequence/state view. Add 4D only when intermediate change is itself the lesson.

## Mermaid families

| Need | Family | File |
| --- | --- | --- |
| Conventional auto-laid-out topology or pipeline | flowchart | [flowchart.md](flowchart.md) |
| Ordered calls over time | sequence | [sequence.md](sequence.md) |
| Nouns, fields, inheritance | class | [class.md](class.md) |
| Lifecycle / loops | state | [state.md](state.md) |
| Records and relations | er | [er.md](er.md) |
| System context (Desktop vs Containers vs agent) | c4 | [c4.md](c4.md) |
| Feedback / GEPA-style loop | flowchart or state | [feedback-loop.md](feedback-loop.md) |

**Unsupported** (do not create a visual): sankey, gantt, gitGraph, mindmap, timeline, journey, kanban, pie, xy, requirement. Tell the user the family is not rendered yet.

Copy-pasting a reference file into MCP is only for renderer dogfood.
