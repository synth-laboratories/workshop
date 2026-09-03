# Handoff — Dicken explainers for papers and harnesses

**For:** the engineer staffing the first paper/harness visual loop after the
substrate handoff exists.
**Not a new renderer.** v0.2 already has `diagram.systems.dynamic.v1`,
`author-synth-diagrams`, sandboxed play/scrub/poster SVG.
**Depends on:** [`HANDOFF_V03_VISUAL_AND_CONTEXT_2026-08-14.md`](./HANDOFF_V03_VISUAL_AND_CONTEXT_2026-08-14.md)
(E1 VisualsBench, families, click-to-label). Codex V1/V2 also feeds
[`HANDOFF_V03_PROOFS_E2_E4_2026-08-14.md`](./HANDOFF_V03_PROOFS_E2_E4_2026-08-14.md) E4.
**Do not start until v0.2 GO.** Do not mix into SYN-3202 / SYN-3215 / SYN-3212.

Layout: [dicken papers and harnesses](../../../../../.cursor/projects/Users-joshuapurtell-Documents-Codex-2026-08-14-let-worktrees-workshop-v03-gemini/canvases/v03-dicken-papers-harnesses.canvas.tsx)
is the map of the scratch notes. This file is the staff plan.

---

## Product

A Workshop session can take a **paper** or an **agent harness**, author a
Benjamin Dicken Style explainer whose beats match evidence, survive label
round-trip, and later seal as a report. The visual is the blog figure, not a
screenshot of the transcript.

Grammar (already decided in `uml_2d_visuals.md`):

| Need | Template |
| --- | --- |
| Change over time is the lesson | `diagram.systems.dynamic.v1` |
| Topology / ownership / before-after | `diagram.systems.v1` |
| Exact sequence / state / types | `diagram.mermaid.v1` |

Do not create all three by reflex. Do not dump SVG, HTML, or JavaScript into a
canvas. Do not bind `stream`.

---

## Loop

1. **Source** — attach the paper (PDF/md) or the harness tree. Cite files and
   sections. Invented mechanisms fail.
2. **Claim** — one sentence. Choose grammar. Dicken only if a still would force
   the reader to simulate time.
3. **Author** — ≥3 named beats → scene → bounded timeline → useful poster.
   Parent owns evidence. A subagent may draft storyboard/timeline when the rail
   exists (skill already says this).
4. **QA** — `authoring_context`, then `capture_review` at wide and compact.
   Inspect the PNGs. Update the same `visual_id`. `mark_ready` only after both
   pass.
5. **Grade** — VisualsBench: bind-before-mutate, missing≠0, claim inspectable
   without the transcript, label round-trip.
6. **Later** — seal (SYN-3226), optional human Share (SYN-3230).

Paper beats cite section/figure/table. Harness beats cite a real spawn, tool,
wait, or grade. Unproven paths use missing/unproven treatment. Overfitting is a
beat (train vs held-out), not a caption apology.

---

## First tries (in order)

| # | Session | Pass |
| --- | --- | --- |
| 1 | **Codex harness** — one turn, same model, V1 vs forced V2 | ≥3 beats; spawn/wait overlap visible; missing child is null; VisualsBench + both captures pass. This is the Dicken sibling of E4, not a second architecture. |
| 2 | **Craftax SFT method-diff** as a Synth open-artifact paper | Poster stands alone as a figure. Train vs held-out not coerced. Label revises the same visual. |
| 3 | **Craftax harness shapes** — shared / quality / single | Three beats bound to real recipes. No invented steps. OHCO + git-server is a later beat on the GELO floor (SYN-3225), not v0.2 friends copy. |
| 4 | **Second env** — FLE or NetHack, same loop | No new template id. Trace V5 + Dicken make an unfamiliar archive readable. |
| 5 | **Harness series** — Claude Code, then DeepSeek or Prime; Mander and Jesterky after | Claim vs observed Trace V5. Jesterky is record/replay, not Intern. |

Then: DeepSeek / Prime / Mander / Jesterky as a series, not a template dump.
Harness-overfitting paper only if a real held-out receipt exists.

---

## Scratch notes that are *not* this cut

| Note | Where it actually lives |
| --- | --- |
| Shared reports / blogs | SYN-3226 / SYN-3230, after `mark_ready` |
| Nemotron Nano SFT → RL? | **No.** Freeze student is Lightning 30B-A3B + CISPO (E5). Nano needs a new note. |
| Serving via Modal, also OSS | E5 is Shoal + Modal B200. OSS serve is extra. |
| Intern / mailbox | v0.4 |

---

## Out

- A new GSAP/JS authoring environment or animated Mermaid as a substitute
- Per-harness VisualHost templates (`live.codex.v1`, `live.claude.v1`, …)
- Declaring V2 better from Terra-V2 vs Luna-V1
- Auto-upload, live SSE permalinks
- Staffing this before VisualsBench can grade a live visual

Canonical substrate: `uml_2d_visuals.md`,
`apps/synth_desktop/skills/author-synth-diagrams/`,
`SystemsDynamicVisual.tsx`.
