# Workshop Mermaid — execution plan

**Status:** Plan for a first-class diagram renderer in Desktop visuals (right pane + MCP).  
**Date:** 2026-08-12  
**Contract:** [`mermaid_visual.md`](./mermaid_visual.md) (2026-08-09). This file sequences that spec against today’s Workshop tree. Do not fork a second design.  
**Not A1–A8.** Live evals stay on `live.*.v1` templates. Diagrams do not replace Craftax / Harbor / dig.bench evidence views.

**Do not mix this into the uncommitted A1 / OAuth / optimizer tree.** Split a branch off a clean workshop cut before Phase A.

---

## When we win

You are in a Workshop chat. You ask the coding agent to explain how a live eval starts, or to sketch the GEPA loop, or to draw the policy pin. The agent does not paste a markdown fence and hope. It calls MCP, a diagram appears in the **right pane** while it is still talking, a card shows up in the chat, and the same object is in Visuals after you quit.

That picture is whatever the agent needed to say: a sequence of MCP → IPC → `POST /rollouts`, a class diagram of `policy_ref`, a feedback loop, a C4 of Desktop vs Containers. We do not ship those drawings. We enable the kinds. The agent authors the source; Rust renders it; the pane is just a visual.

Concretely, in one sitting:

1. User: “Draw how `policy_ref` gets to the container.”
2. Agent: `visual_manage.create` `diagram.mermaid.v1` with Mermaid `content`, then `show`.
3. Right pane: SVG, pan/zoom, title in the rail. Chat: thumbnail of the same `visual_id`.
4. User: “Add the visual-ready gate.” Agent `update`s `content`. New revision; old source still in CAS.
5. Quit, reopen Visuals, same diagram, no rerender flash of a blank canvas.
6. Copy source or export PNG for a doc. Failed layout still shows source, never a fake picture.

If the agent instead dumps SVG into `blank.canvas.v1`, or the pane is empty until you open Visuals, or chat and pane are two different ids — we have not won.

---

## Verdict

Ship **one** renderer: Mermaid, Rust-owned, agent-created via `synth_visuals` MCP, shown in the conversation right pane and the Visuals vault as the same `visual_id`.

The engine must accept the Mermaid families Grok already dispatches (flowchart, sequence, class, state, ER, C4, Sankey, …). Those are **capabilities**, not a catalog of shipped diagrams. Every real diagram is agent-authored `content`.

**Reference examples live in a prebuilt skill**, not in the renderer, not in the template tree, not in the `.app` as visuals. Same pattern as `use-synth-visuals/references/`: the skill body stays small; the agent loads `references/` only when it is about to author a diagram. It may copy a pattern, then write new source for this question. It must not `create` a visual whose `content` is a verbatim skill file unless it is testing the renderer.

PlantUML, Graphviz, D2, drag-edit, and Mermaid.js in the WebView are **out**. Grok Build’s pure-Rust `mermaid-to-svg` (pinned `8a14c91d`) is the engine. Vendor a trimmed copy; do not depend on the Grok workspace.

---

## Locked (do not reopen)

Same as [`mermaid_visual.md`](./mermaid_visual.md) §§1, 10, 15. Extra closures for this tree:

| Item | Decision |
| --- | --- |
| Renderer | `RendererKind::Mermaid`. Not `html`, not `blank.canvas.v1`. |
| Template | One built-in: `diagram.mermaid.v1`. Genre `diagram`. |
| Canonical bytes | UTF-8 Mermaid source in CAS (`content_digest`). SVG/PNG are derived renditions. |
| Who renders | Isolated child of the Desktop binary (`synth-desktop __render-mermaid`). TypeScript displays the rendition as an **image**, never `innerHTML`. |
| Who authors | Workshop coding agent via MCP `visual_manage` `create` / `update` with `content`. Humans can reopen/export; no drag canvas in 0.2. |
| Examples | Prebuilt skill `author-synth-diagrams` (or `use-synth-visuals/references/diagrams/`). Not `visuals/templates/.../examples`. Not compiled into Rust. |
| Pane | Default `presentation: "pane"` (right Visual pane). `canvas` is allowed for dense C4/flowcharts, not required. |
| Live evals | Unchanged. Do not bind `diagram.mermaid.v1` to slot `stream`. |
| A1 | This work does not start until A1’s MCP pin path is on its own branch, or this work is on a **split** branch. |

`blank.canvas.v1` stays for bespoke HTML/SVG that is not a diagram. Agents must not dump Mermaid into a canvas document once `diagram.mermaid.v1` exists.

---

## Today (gaps)

| Layer | Now | Need |
| --- | --- | --- |
| `RendererKind` | `template \| tsx \| html` (`visuals/models.rs`, protocol TS) | `mermaid` |
| Registry | `VisualCreateRequest.content` already CAS-stores bytes; `preview_digest` unused | Derive `rendererKind` from template; enqueue render; write renditions |
| MCP | `visual_create` does **not** forward `content` (`synth_visuals_mcp.rs`) | Require `content` when `template_id=diagram.mermaid.v1` |
| Skill | `use-synth-visuals` points ad-hoc work at `analysis.visual.v1` / `blank.canvas.v1` | Diagrams → `diagram.mermaid.v1` |
| Right pane | `VisualHost` loads a TSX shell from the template | Branch `rendererKind === "mermaid"` → `MermaidVisual` |
| Engine | None | Vendored Grok `mermaid-to-svg` + isolated child |

Architecture map still says UML is not Desktop UI. That line dies when Phase C exits.

---

## Agent loop (the product)

```text
coding agent
  visual_manage.create {
    template_id: "diagram.mermaid.v1",
    title: "Containers start path",
    content: "sequenceDiagram\nAgent->>MCP: policy_ref\nMCP->>IPC: start\nIPC->>Container: POST /rollouts",
    presentation: "pane",
    session_id: …
  }
        │
        ▼
Rust VisualRegistry
  validate UTF-8 + size + diagram prefix
  SQLite visual + revision
  CAS source  → content_digest
  spawn __render-mermaid (timeout 3s)
  CAS svg/png → visual_renditions + preview_digest
  journal: visual.created, visual.rendered | visual.render_failed
        │
        ▼
same visual_id
  chat card (PNG thumb)
  right Visual pane (SVG, pan/zoom, source fallback)
  Visuals vault
```

MCP returns `visual_id` immediately. Render may finish async (`queued` → `rendering` → `ready` | `failed`). Failed render **keeps source**.

---

## Phases

### M0 — branch hygiene (before code)

- New branch, e.g. `josh/workshop-mermaid-v1`, **not** `josh/aug12-optimizers-workshop-visuals`.
- Do not land renderer vendoring on the dirty A1 tree.
- Point `aug_12_update.md` §2.4 at this file as **out of A1–A8** (one sentence). Optional; do not rewrite the acceptance table.

### M1 — renderer spike (Phase A)

Exit: fixtures render to SVG/PNG with no Node, no network, no Grok workspace.

- Vendor pinned stack into `apps/synth_desktop/src-tauri/third_party/` (paths in `mermaid_visual.md` §7.1–7.2).
- Hidden mode `synth-desktop __render-mermaid` with the limits in §7.3 (64 KiB source, 3s, 32 MP, no file/HTTP resolve).
- Renderer tests: tiny inline strings per family we claim (not the skill examples). If a family is in the Grok dispatcher and the skill says the agent can use it, a unit test renders it. Families we cannot render stay **unsupported** in the skill, not silently blank.
- Tests: identical input → identical digest; oversized source rejected **before** spawn; hung child reaped.

### M2 — registry (Phase B)

Exit: create / revise / reopen / restart preserve one visual identity.

- `RendererKind::Mermaid` in Rust + `packages/runtime-protocol` + generated protocol (hand-write if Specta stays off).
- Register `visuals/templates/diagram.mermaid.v1/`.
- `visual_renditions` table as specified. Renderer version is part of the cache key.
- Create with `template_id=diagram.mermaid.v1` **requires** `content`. Derive `diagramKind` in Rust; caller metadata is a hint.
- Authenticated reads: content, rendition list, rendition bytes. Never expose CAS paths to MCP.

### M3 — MCP + right pane (Phase C)

Exit: one agent-created diagram in chat, right pane, and vault; survives restart.

MCP (`synth_visuals_mcp.rs` + skill):

```json
{
  "operation": "create",
  "arguments": {
    "template_id": "diagram.mermaid.v1",
    "title": "Runtime architecture",
    "content": "flowchart LR\nAgent --> MCP --> Registry",
    "presentation": "pane",
    "session_id": "ses_..."
  }
}
```

- Forward `content` on create/update. Fail closed if missing for this template.
- `list_templates` genre `diagram`.
- Provision `author-synth-diagrams` into the coding-agent home next to `use-synth-visuals`. Description is enough to select it; body + `references/` load only when authoring a diagram (same lazy-skill rule as visuals).
- Optional `render` operation = retry; does not invent a second source of truth.

UI:

- `VisualHost` branches on `rendererKind` **before** `loadVisualShell`.
- `MermaidVisual`: SVG for pane, PNG for thumb; pan/zoom/reset; source view; copy source; export SVG/PNG; typed queued/failed.
- Chat rail uses `preview_digest`. Same `visual_id` as vault.

### M4 — hardening (Phase D, 0.2 gate)

- Malformed corpus, crash/timeout, light/dark, high-DPI.
- License/NOTICE for Grok Apache-2.0 + mermaid-to-svg MIT + Dagre ancestry.
- Packaged `.app` renders without Node/Python.
- Dogfood: in a real chat the agent authors whatever diagram the question needs (not a canned fixture), it lands in the pane, and it reopens after quit.

---

## Files (intended)

```text
workshop_mermaid.md                          this plan
mermaid_visual.md                            contract (do not duplicate)

visuals/templates/diagram.mermaid.v1/
  template.json                              # id, genre, rendererKind — no example source
  README.md                                  # points at the skill, not a gallery

apps/synth_desktop/skills/author-synth-diagrams/
  SKILL.md                                   # when to use, MCP create/show/update, never blank.canvas
  references/families.md                     # which kinds render; which are unsupported
  references/flowchart.md                    # pattern, not a shipped visual
  references/sequence.md
  references/class.md
  references/state.md
  references/er.md
  references/c4.md
  references/feedback-loop.md
  references/sankey.md                       # only if M1 actually renders it

apps/synth_desktop/src-tauri/
  src/visuals/models.rs                      RendererKind::Mermaid
  src/visuals/mermaid.rs                     orchestrate + validate
  src/visuals/renditions.rs
  src/bin/synth_visuals_mcp.rs               content required
  src/bin/synth_desktop.rs or lib.rs         __render-mermaid
  third_party/mermaid-to-svg/ …
  licenses/grok-mermaid-NOTICE

apps/synth_desktop/src/renderer/src/
  components/MermaidVisual.tsx
  components/VisualHost.tsx                  branch before template shell

apps/synth_desktop/skills/use-synth-visuals/SKILL.md
apps/synth_desktop/skills/use-synth-visuals/references/visual-recipes.md
```

---

## Skill (prebuilt, lazy references)

Provisioned into the Codex/coding-agent home like `use-synth-visuals`. The catalog line is short: “Author a Mermaid diagram into the right Visual pane.” The body and `references/` load only when that workflow is selected, so diagram examples do not sit on every visuals turn.

`SKILL.md` says:

- write Mermaid for **this** question; do not open a stock visual;
- `create` `diagram.mermaid.v1` with `content`; `show`; revise with `update`;
- `blank.canvas.v1` is not a diagram path;
- live evals stay on `live.*.v1`.

`references/*.md` are **patterns**: a short valid source per family, plus when to pick it (sequence for ordered calls, class for nouns, flowchart/C4 for topology, state for loops). The agent reads them, then authors new `content`. Copy-pasting a reference file into MCP is only for renderer dogfood.

`use-synth-visuals` gets one line: if the artifact is a system/UML/flow picture, load `author-synth-diagrams` instead of `blank.canvas.v1`.

---

## Skill rule (agent)

When the user or the task needs a **picture of a system**, load `author-synth-diagrams` and write Mermaid for that question. Do not pick a stock diagram from the template registry.

1. Read `references/families.md`, then the one family file that matches.
2. `create` with **new** `content` in the body. Do not put source in `props`. Do not ship a reference file as the visual.
3. `show` so it lands in the right pane of this chat.
4. Revise by `update` with new `content` (new revision). Do not fork a blank canvas.

When the task needs **live evidence** (Craftax frames, Harbor trial, GEPA front): use the live/optimizer template. A diagram may **explain** that system beside it; it is not the eval stream.

---

## Acceptance (copy from spec, Workshop-shaped)

1. In a real chat, the agent authors Mermaid for the question (not a fixture file) → `visual_id` + CAS source.
2. Right pane shows that SVG without injecting markup; chat card and vault are the same id.
3. A follow-up `update` revises the picture; prior source remains.
4. Restart does not rerender a valid cached rendition.
5. Missing `content` on this template fails closed (no silent blank canvas).
6. Oversized / hung render cannot kill Tauri; source remains.
7. `live.craftax.v1` still binds slot `stream`; diagrams never steal that slot.
8. Packaged app has no Node Mermaid path.
9. Every family we tell the agent it can use actually renders; unsupported families are named, not empty panes.

---

## Non-goals (0.2)

- Full Mermaid.js parity.
- PlantUML / DOT / D2 / arbitrary SVG ingest.
- Live token-by-token redraw.
- Replacing eval/trace templates.
- Auto-promoting fenced ` ```mermaid ` in chat (follow-up: explicit agent or user action).
- Generating Mermaid from a live eval stream as a required producer (follow-up helper, not this cut).

---

## Order vs A1

```text
now     A1 live seed through coding agent MCP (policy_ref, visual subscribed)
then    M0 split branch
        M1 spike  → M2 registry → M3 MCP+pane → M4 0.2 gate
```

If A1 is blocked on a dirty tree, M1 can still proceed on the split branch: vendoring does not touch eval_driver.

---

## If you get stuck

- Agent “diagram” is HTML in `blank.canvas.v1` → MCP create is not forwarding `content`, or the skill still prefers canvas.
- Pane shows source only → child render failed; check `visual.render_failed` and keep the source view. Do not parse Mermaid in JS.
- Two different pictures in chat vs pane → two `visual_id`s or TS rendered a second copy. One registry, one rendition.
- Binary size spike → you vendored the whole Grok tree. Trim to `mermaid_visual.md` §7.2.
- A1 visual empty → you are on the wrong branch. Diagrams do not fix connect-before-start.
