# Mermaid Visual — Synth Desktop 0.2 Feature Spec

**Target:** Synth Desktop 0.2  
**Status:** planned; implementation intentionally deferred  
**Owner:** Rust CoreRuntime / Visual Registry  
**Last updated:** 2026-08-09

## 1. Decision

Mermaid becomes a first-class, agent-native visual format backed by the existing Rust Visual Registry.

The canonical artifact is Mermaid source stored in the filesystem CAS and referenced by an immutable visual revision in SQLite. Rust validates and renders the source into derived SVG and PNG renditions. TypeScript displays those renditions through the shared `VisualHost`; it does not own parsing, persistence, revisioning, or canonical rendering.

Use one format and semantic subtypes:

```text
renderer_kind: mermaid
template_id:   diagram.mermaid.v1
diagram_kind:  flowchart | sequence | class | state | er | c4 | ...
```

“UML” is not a separate renderer. Mermaid class, sequence, state, ER, and C4 diagrams are UML-like semantic variants of `diagram.mermaid.v1`. PlantUML is not in scope for 0.2.

## 2. Product outcome

An agent can create a Mermaid diagram through Synth Visuals MCP. The diagram then:

1. appears as a visual reference/card in the originating chat;
2. opens in the conversation’s right Visual pane;
3. is searchable and reopenable from the Visuals vault;
4. retains its source, provenance, revisions, and render status in the centralized Rust registry;
5. can be copied as Mermaid source or exported as SVG/PNG.

The chat card, right pane, and vault inspector must all resolve the same `visual_id` and use the same `VisualHost`.

## 3. Reference implementation

Grok Build provides the reference architecture:

- [`xai-grok-mermaid`](https://github.com/xai-org/grok-build/blob/8a14c91d88875a831a38b3a066b1683116bcb31c/crates/codegen/xai-grok-mermaid/src/lib.rs) is a Rust wrapper with a swappable engine boundary.
- Its default pure-Rust path converts Mermaid to SVG with a vendored Dagre-based renderer, then rasterizes SVG to PNG with `resvg`, `usvg`, and `tiny-skia`.
- The [diagram dispatcher](https://github.com/xai-org/grok-build/blob/8a14c91d88875a831a38b3a066b1683116bcb31c/third_party/mermaid-to-svg/src/lib.rs) supports flowchart, sequence, class, state, ER, C4, requirement, Gantt, mind-map, Git graph, timeline, journey, Kanban, Sankey, XY, and other Mermaid families.
- Its [worker](https://github.com/xai-org/grok-build/blob/8a14c91d88875a831a38b3a066b1683116bcb31c/crates/codegen/xai-grok-pager/src/app/mermaid_worker.rs) renders model-authored input in a short-lived child process with a hard timeout.
- Grok’s separate terminal Markdown renderer may remain inspiration for a future Unicode fallback; it is not required for Desktop 0.2.

Synth should vendor a pinned, trimmed copy of the relevant renderer stack. Do not take a floating dependency on the complete Grok workspace.

## 4. Existing foundations

The following are already present and should be extended rather than duplicated:

- Rust `CoreRuntime` composition root;
- Rust `VisualRegistry`;
- SQLite `visuals` and `visual_revisions` tables;
- filesystem content-addressed store;
- `content_digest` and `preview_digest` fields;
- normalized journal events;
- authenticated Visuals MCP loopback;
- chat visual references and rail;
- shared `VisualHost` and right `VisualPane`;
- Visuals vault/library;
- template manifests and revision-aware visual CRUD.

Known gaps for this feature:

- `RendererKind` currently permits only `template`, `tsx`, and `html`;
- Visuals MCP creation does not expose Mermaid source as canonical content;
- no Rust Mermaid render service or isolated worker exists;
- no renderer-independent CAS asset read API is exposed to `VisualHost`;
- `preview_digest` is modeled but not populated by a Mermaid rendition pipeline;
- `VisualHost` has no Mermaid renderer branch.

## 5. System placement

```text
 Agent / MCP
     |
     | visual_create {
     |   template_id: "diagram.mermaid.v1",
     |   title: "Runtime architecture",
     |   content: "sequenceDiagram ..."
     | }
     v
+-------------------------- RUST CORE ---------------------------+
| Visual command validation                                     |
|     |                                                         |
|     v                                                         |
| Visual Registry ----> SQLite record + revision + journal       |
|     |                                                         |
|     +-------------> CAS: canonical Mermaid source              |
|     |                                                         |
|     v                                                         |
| isolated render child: Mermaid -> SVG -> PNG                   |
|     |                                                         |
|     +-------------> CAS: derived renditions                    |
|     +-------------> visual.rendered / visual.render_failed     |
+-----|----------------------------------------------------------+
      |
      | visual_ref + asset reads
      v
+----------------------------- TS -------------------------------+
| chat card ---- shared VisualHost ---- right pane                |
|                         |                                      |
|                         +------------- Visuals vault            |
|                                                                |
| source view · pan/zoom · copy · export · retry                  |
+----------------------------------------------------------------+
```

This remains one Rust product runtime. The renderer child is an ephemeral crash-isolation boundary, not a daemon, service, or second product authority.

## 6. Canonical data contract

### 6.1 Renderer and template

Add `Mermaid` to Rust and TypeScript `RendererKind`:

```text
template | tsx | html | mermaid
```

Register a built-in template manifest:

```json
{
  "id": "diagram.mermaid.v1",
  "title": "Mermaid diagram",
  "genre": "diagram",
  "rendererKind": "mermaid"
}
```

The manifest identifies capabilities and presentation defaults; it does not contain or render the diagram source.

### 6.2 Visual record

Example logical record:

```json
{
  "id": "vis_...",
  "templateId": "diagram.mermaid.v1",
  "rendererKind": "mermaid",
  "title": "Intern event flow",
  "status": "saved",
  "contentDigest": "sha256-of-mermaid-source",
  "previewDigest": "sha256-of-default-png",
  "metadata": {
    "mediaType": "text/vnd.mermaid",
    "diagramKind": "sequence",
    "rendererVersion": "grok-build-8a14c91d",
    "renderStatus": "ready"
  }
}
```

`content_digest` is authoritative and points to UTF-8 Mermaid source. Generated SVG/PNG files are disposable derived assets. Updating source creates a new visual revision.

### 6.3 Renditions

`preview_digest` remains the default vault/chat thumbnail. Add a normalized rendition store rather than placing authoritative digest maps inside metadata:

```sql
CREATE TABLE visual_renditions (
  visual_id TEXT NOT NULL,
  revision INTEGER NOT NULL,
  format TEXT NOT NULL,          -- svg | png
  theme TEXT NOT NULL,           -- light | dark
  size_class TEXT NOT NULL,      -- thumbnail | pane | export
  content_digest TEXT NOT NULL,
  media_type TEXT NOT NULL,
  renderer_version TEXT NOT NULL,
  width_px INTEGER,
  height_px INTEGER,
  created_at TEXT NOT NULL,
  PRIMARY KEY (visual_id, revision, format, theme, size_class),
  FOREIGN KEY (visual_id, revision)
    REFERENCES visual_revisions(visual_id, revision)
);
```

Rendition keys include renderer version so an engine upgrade never silently changes an existing cached output. A rerender may replace derived cache rows without rewriting canonical source history.

## 7. Rust implementation

### 7.1 Source layout

Proposed placement:

```text
apps/synth_desktop/src-tauri/
  src/visuals/
    mermaid.rs              orchestration and registry integration
    renditions.rs           rendition records and CAS lookup
  src/bin/
    synth_visuals_mcp.rs    exposes Mermaid content creation
  third_party/
    mermaid-to-svg/
    dagre_rust/
    graphlib_rust/
    ordered_hashmap/
  licenses/
    grok-mermaid-NOTICE
```

If the extracted wrapper becomes large enough to justify an independent crate, create `src-tauri/crates/synth-mermaid/`; do not introduce a top-level workspace solely for this feature.

### 7.2 Vendoring policy

Pin the initial source to Grok Build commit `8a14c91d88875a831a38b3a066b1683116bcb31c`.

Bring over only:

- the pure Rust engine boundary and safety limits from `xai-grok-mermaid`;
- the locally improved `mermaid-to-svg` stack;
- Dagre, Graphlib, and ordered-map dependencies;
- required font and license assets.

Remove:

- `mmdc` and Node support;
- Grok pager/TUI integration;
- `xai-tty-utils` dependency;
- Grok-specific theme naming;
- OS-open and clipboard behavior owned by Grok’s pager.

Maintain a patch ledger recording the upstream repository, commit, copied paths, local modifications, licenses, and update procedure.

### 7.3 Render boundary

The main process must not render untrusted Mermaid in-process in production.

Preferred implementation:

```text
synth-desktop __render-mermaid
  --input <validated temporary source path>
  --output <validated temporary output path>
  --format svg|png
  --theme light|dark
  --size thumbnail|pane|export
```

The main binary inspects this hidden mode before initializing Tauri. The parent owns process spawning, timeout, termination, output validation, atomic CAS insertion, and cleanup.

Initial limits:

- source: 64 KiB;
- render timeout: 3 seconds;
- output area: 32 megapixels;
- output axis: 16,384 pixels;
- no network access;
- no arbitrary file resolution;
- bounded stderr capture;
- only explicit temporary input/output paths;
- failure falls back to source view and records a typed error.

### 7.4 Events

Append normalized events after their associated database transaction commits:

```text
visual.created
visual.updated
visual.render_requested
visual.rendered
visual.render_failed
```

`visual.created` makes the chat reference available immediately. Rendering may finish asynchronously. The UI must represent `queued`, `rendering`, `ready`, and `failed` without inventing a second source of truth.

## 8. MCP and runtime API

Extend `visual_create` and `visual_update` with canonical content:

```json
{
  "template_id": "diagram.mermaid.v1",
  "title": "Rust runtime flow",
  "content": "flowchart LR\nAgent --> MCP --> Registry",
  "session_id": "ses_...",
  "metadata": {
    "diagramKind": "flowchart"
  }
}
```

Rules:

- `content` is required for `renderer_kind=mermaid` creation;
- `props`/bindings may contain presentation hints but never duplicate canonical source;
- Rust derives and verifies `diagramKind`; caller metadata is only a hint;
- MCP receives `visual_id` immediately and may receive render status separately;
- a visual reference is associated with the originating session/message through existing fields.

Add authenticated read routes/commands conceptually equivalent to:

```text
GET /v1/visuals/{visual_id}/content
GET /v1/visuals/{visual_id}/renditions
GET /v1/visuals/{visual_id}/renditions/{format}?theme=dark&size=pane
POST /v1/visuals/{visual_id}/render
```

CAS filesystem paths must never be exposed directly to an MCP client. The Rust API validates the visual, revision, digest, media type, and requested rendition.

## 9. TypeScript presentation

`VisualHost` branches on `rendererKind === "mermaid"` before template loading and mounts a dedicated `MermaidVisual` component.

The component:

- reads an SVG or PNG rendition through the authenticated desktop bridge;
- uses SVG for the pane/export surface and PNG for compact thumbnails/fallback;
- displays generated SVG as an image/asset resource, never by injecting model-provided markup with `innerHTML`;
- supports fit, zoom, pan, reset, source view, copy source, export SVG, and export PNG;
- shows typed queued/rendering/failure states;
- retries through Rust rather than rendering Mermaid independently in JavaScript;
- preserves accessible title/description and a source-text fallback.

Do not bundle Mermaid.js for the canonical path in 0.2. One Rust renderer prevents source/render drift between chat, vault, export, and future non-WebView clients.

## 10. Security requirements

Mermaid source is untrusted model/tool output.

Required controls:

- validate UTF-8, source size, and recognized diagram prefix before spawning;
- run layout and rasterization out of process;
- kill and reap the child on timeout;
- disable `file:`, HTTP(S), and arbitrary local-path resolution;
- use bundled fonts by default and explicitly decide whether system glyph fallback is permitted;
- escape all source-derived labels in SVG generation;
- validate generated media before CAS insertion;
- display SVG as an image resource rather than active DOM;
- enforce response and decompressed-output size limits;
- never log full diagram source at normal log levels;
- return typed parse/layout/raster/timeout/unsupported errors;
- preserve the source even when rendering fails.

## 11. Licensing

The Grok Build first-party wrapper is Apache-2.0. Its vendored `mermaid-to-svg` code is MIT; Dagre/Graphlib ancestry and Rust ports include MIT and Apache-2.0 obligations. Roboto carries Apache-2.0 terms.

Before release:

- copy all applicable license and notice files;
- retain copyright headers;
- add the renderer stack to packaged third-party notices;
- document local modifications;
- verify the distributed `.app` and DMG include required notices.

Licensing review is a release gate, not a follow-up.

## 12. Delivery plan

### Phase A — renderer spike

- vendor the pinned engine stack;
- render representative flowchart, sequence, class, state, ER, and C4 fixtures;
- prove deterministic SVG/PNG output;
- prove timeout, source-size, and output-size enforcement;
- record binary-size and render-latency impact.

Exit gate: the isolated Rust path works without Node, a browser renderer, network access, or Grok workspace dependencies.

### Phase B — registry integration

- add `RendererKind::Mermaid` across Rust and TypeScript protocols;
- register `diagram.mermaid.v1`;
- add rendition schema and migration;
- store canonical source in CAS;
- add render orchestration and journal events;
- expose authenticated content/rendition reads.

Exit gate: create, reopen, revise, restart, archive, and migrate preserve one consistent visual identity.

### Phase C — MCP and UX

- expose `content` in visual MCP create/update schemas;
- emit chat visual references immediately;
- add `MermaidVisual` to `VisualHost`;
- support chat, right pane, and vault;
- add source/copy/export/retry controls and accessibility states.

Exit gate: one MCP-created diagram appears in all three surfaces and survives application restart.

### Phase D — hardening and 0.2 release

- fuzz/parser corpus and malformed-input tests;
- child crash/timeout/reaping tests;
- light/dark and high-DPI visual QA;
- packaging and notices verification;
- migration/rollback test against an existing Visual Registry;
- installed-app dogfood with a real agent-created diagram.

## 13. Acceptance tests

Minimum automated coverage:

1. Flowchart, sequence, class, state, ER, and C4 fixtures produce valid non-empty SVG and PNG.
2. Identical source, renderer version, theme, and size produce identical rendition digests.
3. Oversized source is rejected before the renderer child starts.
4. A hung or crashing child is killed/reaped and cannot terminate the Tauri host.
5. Generated SVG contains no external resource references.
6. Creation writes one visual, one revision, canonical CAS content, and committed journal events.
7. Updating source creates a revision and preserves the prior source digest.
8. Failed rendering preserves source and exposes a source-view fallback.
9. Chat card, pane, and vault open the same `visual_id`.
10. Restart reconstructs the visual from SQLite/CAS without rerendering a valid cached rendition.
11. MCP cannot read arbitrary digests or filesystem paths.
12. The packaged app renders without Node or Python.
13. Third-party notices are present in release artifacts.

## 14. 0.2 release gates

The feature is ready for 0.2 only when:

- Rust is the sole authority for source, revision, render status, and derived assets;
- production rendering is isolated and bounded;
- no Mermaid.js, Node, or Python runtime is required;
- the agent-to-MCP-to-chat-to-pane-to-vault loop passes end to end;
- SVG/PNG export works from a persisted visual;
- application restart and one real legacy-registry migration are verified;
- the installed `.app` is dogfooded with at least one real agent-created UML-style diagram;
- all relevant Rust, TypeScript, Playwright, packaging, and license checks pass.

## 15. Explicit non-goals for 0.2

- full Mermaid.js syntax parity;
- PlantUML, Graphviz/DOT, D2, or arbitrary SVG ingestion;
- collaborative or cloud visual synchronization;
- editing diagrams with a drag-and-drop canvas;
- live incremental rendering on every streamed token;
- a permanent renderer daemon;
- replacing specialized eval/trace templates with generic diagrams;
- terminal Unicode rendering.

## 16. Follow-ups after 0.2

- optional Unicode/ASCII projection inspired by Grok’s Markdown renderer;
- syntax-aware source editor with parse diagnostics;
- diagram diff between visual revisions;
- additional first-class source formats behind the same rendition interface;
- template helpers that produce Mermaid from eval, trace, and system-topology data;
- promotion of fenced Mermaid Markdown blocks into registry visuals, gated by an explicit user or agent action.
