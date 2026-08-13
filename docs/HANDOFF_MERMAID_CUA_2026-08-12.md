# Handoff: Mermaid diagrams — CUA QA

**For:** the person dogfooding this in Desktop (coding agent / CUA)  
**Date:** 2026-08-12 ~17:47 ET  
**Nothing committed or pushed.** Do not reopen locked decisions. Do not commit unless asked.

Contract: [`../mermaid_visual.md`](../mermaid_visual.md)  
Plan: [`../workshop_mermaid.md`](../workshop_mermaid.md)  
**Not A1–A8.** Do not mix this into live-eval / policy-pin proof. Do not use [`aug_12_notes.md`](./aug_12_notes.md).

---

## What you are proving

In a **real Workshop chat**, the coding agent authors Mermaid for **this** question, calls MCP, and the diagram appears in the **right pane** while it is still talking. Chat card, pane, and Visuals vault are the **same `visual_id`**.

If it dumps SVG into `blank.canvas.v1`, the pane is empty until Visuals, or chat and pane are two ids — we have not won.

---

## Tree

| Path | Branch | Git |
| --- | --- | --- |
| `/Users/joshuapurtell/Documents/GitHub/workshop` | `josh/aug12-optimizers-workshop-visuals` | **Uncommitted.** Mixed with A1 / OAuth / optimizer WIP. Split before commit. |

Leave `containers`, `optimizers`, `optimizers-beta` alone for this slice.

---

## Before you sit down

1. Rebuild and **restart** Synth Desktop from this workshop tree (the running app will not have the renderer, MCP `content` forward, or the skill).
2. Start a **new chat**. Existing Codex homes will not have `author-synth-diagrams` until `ensure_home` runs again.
3. Do not QA from Visuals-only create. The product path is **agent MCP in a conversation**.

---

## CUA prompt (copy this)

> Draw how `policy_ref` gets from this chat to the container `POST /rollouts`. Load `author-synth-diagrams`. Create `diagram.mermaid.v1` with real Mermaid `content` for this question (not a skill file dump), then `show` it in this pane. Do not use `blank.canvas.v1`. Do not bind slot `stream`.

Follow-up if the first picture lands:

> Add the visual-ready gate to that diagram. Update the same visual; do not fork a canvas.

Optional second family (only after the first pass):

> Now draw `policy_ref` as a class diagram (harness + config) on a new `diagram.mermaid.v1`, then `show`.

---

## Pass

1. Agent calls `visual_manage` `create` with `template_id: "diagram.mermaid.v1"` and a `content` string that starts with a real family (`sequenceDiagram`, `flowchart`, `classDiagram`, …).
2. Then `show` with that `visual_id`.
3. Right pane: SVG as an **image** (pan/zoom/title). Not a TSX shell, not innerHTML, not a blank canvas.
4. Chat card uses the **same** `visual_id`.
5. Follow-up `update` with new `content` revises the picture (revision bumps; old source still in CAS).
6. Visuals vault reopens the same object. Restart should not flash a blank canvas if the rendition is cached.
7. If you omit `content` on this template, create **fails closed** (no silent empty visual).

## Fail (stop and say which)

| Symptom | Likely cause |
| --- | --- |
| HTML/SVG in `blank.canvas.v1` | Skill still prefers canvas, or MCP `content` not forwarded |
| Pane empty / “Loading visual shell” | `VisualHost` did not branch on mermaid before the TSX loader |
| Source-only, no picture | Child render failed; keep source view; check `metadata.renderStatus` / `renderError` |
| Two different pictures in chat vs pane | Two `visual_id`s |
| Agent pastes a markdown fence and stops | Did not load `author-synth-diagrams` / did not `create`+`show` |
| Sankey / Gantt / pie empty pane | Unsupported family — skill should say so, not create |

---

## What landed (uncommitted)

One renderer: `rendererKind: mermaid`, template `diagram.mermaid.v1`, genre `diagram`. Canonical bytes = UTF-8 Mermaid in CAS. SVG is a derived rendition. TypeScript displays it as `<img>`, never `innerHTML`.

| Layer | Where |
| --- | --- |
| Engine + isolated child `synth-desktop __render-mermaid` | `apps/synth_desktop/src-tauri/src/visuals/mermaid.rs` |
| Rendition rows + CAS previews | `…/visuals/renditions.rs`, migration 13 `visual_renditions` |
| Require `content`; derive kind; refuse slot `stream` | `…/visuals/registry.rs` |
| MCP `content` on create/update; optional `render` | `…/src/bin/synth_visuals_mcp.rs` |
| IPC content / rendition / render routes | `…/visuals_ipc.rs` |
| Right pane | `src/renderer/src/components/MermaidVisual.tsx`, `VisualHost.tsx` |
| Template (id/genre only — **no examples**) | `visuals/templates/diagram.mermaid.v1/` |
| Lazy skill | `apps/synth_desktop/skills/author-synth-diagrams/` |
| Provisioned into Codex home | `session/codex/home.rs`, `skills.rs` |
| Pointer from visuals skill | `skills/use-synth-visuals/SKILL.md` |

MCP shape the agent should emit:

```json
{
  "method": "visual_manage",
  "operation": "create",
  "arguments": {
    "template_id": "diagram.mermaid.v1",
    "title": "How policy_ref reaches the container",
    "content": "sequenceDiagram\nAgent->>MCP: policy_ref\nMCP->>IPC: start\nIPC->>Container: POST /rollouts",
    "presentation": "pane"
  }
}
```

Then `show` with the returned `visual_id`.

---

## Honest limits (do not paper over in QA)

- **Grok `mermaid-to-svg` (`8a14c91d`) is not vendored** (private). This cut is a first-party bounded layout: flowchart, sequence, class, state, ER, C4. Sankey / Gantt / git / mindmap / pie are **unsupported** in the skill — a blank pane for those is a product bug, not “try harder.”
- Preview/thumbnail is the **SVG** digest, not a separate PNG raster. Export SVG from the pane works. PNG export is not a 0.2 gate for this CUA pass.
- Production render is supposed to be the isolated child. Tests use in-process (`cfg(test)`). If the pane works in the packaged/debug app, the child path is what you are exercising.
- Layout is not Mermaid.js parity. Boxes-and-arrows that match the source is a pass. Pixel-perfect mermaid.live is not.

---

## Tests already green (do not re-run hoping)

```text
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib mermaid
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib visuals::registry
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --bin synth-visuals-mcp
node --test visuals/tests/registry.test.mjs
```

Unit tests do **not** count as CUA. The remaining proof is the in-app agent loop above.

---

## If you get stuck

- Skill missing in the session → you reused an old Codex home. New chat after restart.
- `create` 400 “requires content” → agent put source in `props` / a fence. It must be `arguments.content`.
- Pane shows mermaid source only → `metadata.renderStatus=failed`; keep the source; do not parse Mermaid in JS.
- You are tempted to fix A1 empty live visuals → wrong handoff. Diagrams do not fix connect-before-start.

---

## After CUA

Write back: pass/fail, the `visual_id`, family used, and whether chat + pane + vault matched. Do not commit. Do not vendor Grok on this dirty A1 tree — split `josh/workshop-mermaid-v1` first if the engine work continues.
