# Annotation-aware trace views

All layers consume the existing trace visual projection. They do not introduce a new trace storage format.

```
GeneralTraceView — selection, evidence selectors, annotations, filtering
  ├─ ReActTraceView — observation / recorded rationale / action / result
  ├─ CodexAppServerTraceView — projected native reasoning, commands, MCP, files
  └─ ContainerTraceView — extension composition over either protocol view
       └─ RuneBenchTraceView — game action targets and recorded rationale
```

`AgentTraceInspector` exposes General, ReAct and Codex presentations. Pass `extensions={[runeBenchTraceExtension]}` to offer RuneBench. Other applications supply `TraceViewExtension.renderItem` and optionally `renderContext`; return null for unknown events to retain the protocol/general fallback. Do not discard unknown payloads or create synthetic reasoning.

Annotations default to projected `evidence.annotation` records. Hosts can supply an `annotations` array from their evidence store. Target matching checks trace ID, sealed digest, entity kind and source entity ID, independent of presentation IDs. Part IDs, JSON pointers, ranges and evidence selectors remain intact and inspectable. Mismatched or missing targets appear as unresolved. Superseded annotations remain inspectable but do not count as current badges.

`onAnnotate(target)` and `onOpenAnnotation(note)` delegate authoring/review to the host's evidence workflow. No callback means no local save affordance; the components do not pretend to persist notes. The shared wrapper exposes `data-trace-selector` for host integration. Do not map an event selector to a span ID merely to fit a legacy visual-label API.

The Codex view handles projected native app-server item payloads and existing projected Codex event names; it is not a JSON-RPC stream importer. Streaming delta assembly belongs in the capture/projector layer. RuneBench recordings with action.reason show stated rationale, not provider thinking.

RuneBench now registers its extension and supplies synchronized game context. `cursorMs` highlights recorded events and the Follow replay control governs scrolling. Common markers support failure/message/reward/tool/annotation navigation; unknown execution times remain unset.

`useTraceEvidence` and `AnnotationEditor` connect the local demo to Containers' append-only evidence store. The client intentionally accepts only the local demo endpoint at port 8118. Writes require its bound capability and an expected evidence digest. Production remote transport, text-selection annotation authoring, and live stream assembly remain separate work.

`craftaxTraceExtension` proves reuse on retained real frames/terminal summaries. It does not manufacture model/tool messages missing from that source.
