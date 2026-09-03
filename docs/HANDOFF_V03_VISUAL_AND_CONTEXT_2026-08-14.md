# Handoff — v0.3 visual workspace + Context

**For:** the engineer staffing the collaboration substrate after v0.2 GO.
**Do not start until v0.2 GO.** Do not mix into SYN-3202 / SYN-3215 / SYN-3212.
**Do not push** unless Josh asks.

This is **one cut**: the visual is the shared workspace, and Context is how the
practitioner sees what the agent will see next. E2–E4 consume this. They are a
separate handoff.

Project: [Workshop v0.3 Collaboration](https://linear.app/synth-ai/project/workshop-v03-collaboration-d74df67fa366)
Freeze: `docs/launch/v0.3-themes.md` on `josh/v03-approval-broker`

---

## Tickets (staff together)

| Order | Ticket | Job |
| --- | --- | --- |
| 1 | [SYN-3218](https://linear.app/synth-ai/issue/SYN-3218) | E1 VisualsBench in `evals/` — independent grader for live Workshop visuals |
| 1 | [SYN-3217](https://linear.app/synth-ai/issue/SYN-3217) | Families, generic Trace V5, Mermaid/systems, splitter. Stable template ids. |
| 1 | [SYN-3219](https://linear.app/synth-ai/issue/SYN-3219) | Live revise + click-to-label. Overlay never mutates a sealed Trace V5. |
| 2 | [SYN-3220](https://linear.app/synth-ai/issue/SYN-3220) | Settings → Context. Required before E4. |

Write the “what good looks like” note **before** adding templates. VisualsBench
is that note, encoded as an eval. Do not invent a fourth Craftax shell that
cannot show zombie/skeleton attacks, or a Harbor DEO view that cannot compare
three Codex arms.

Gemini ([SYN-3216](https://linear.app/synth-ai/issue/SYN-3216)) is a separate
lane and may proceed now. Reports / upload / approval broker / E5 are later.

---

## 1. The visual is the workspace

The shared object is the visual, not the transcript.

A good visual for this cut:

- is bound **before** mutation and stays the same instance as evidence arrives
- shows missing as missing (never coerces reward / attack / score to 0)
- is scrubbable, comparable across arms, and labelled without mutating a sealed Trace V5
- makes the claim inspectable without reading the transcript
- can be revised live after a user click / label

VisualsBench scores: legibility, honesty, update-in-place, label round-trip,
compare-across-arms. Task metrics stay on Craftax / DungeonGrid / Harbor
graders. **Both must pass.**

This is **not** Artifacts VisualBench (SYN-3110–3112). Do not reuse
`evals/reference/old/visualbench` GIF/HTML corpora as the contract.

### Families (SYN-3217)

Finish the layout already specified in the v0.2 header. Keep ids stable
(`live.craftax.v1`, `optimizer.gepa.live.v1`, …). No second bind architecture.

```text
visuals/
  runtime/ chrome/ registry/ mcp/
  families/
    first_class_example_containers/   craftax, harbor, digbench
    optimizers/                       _shared, gepa, gelo, sft
    diagrams/                         mermaid, systems, systems.dynamic
    analysis/                         analysis, blank canvas, compare, reward, annotation, posttrain
```

Also: generic Trace V5 viewer (unfamiliar archive useful without a per-benchmark
parser); Mermaid and systems as agent-native diagrams (native Rust render path);
reconcile the preserved inner visual-library splitter from
`preserve/aug12-stash-4` with responsive stacking.

Not this ticket: optimizer sidecar download / lifecycle.

### Live loop (SYN-3219)

1. Agent updates live — same instance mutates as evidence arrives and as the
   agent revises the spec. Steering and visual MCP are the same conversation.
2. User clicks to label — frame, span, candidate, trial, or chart mark → durable
   annotation (`note` / `bug` / `highlight` / `reward` / `acceptance`) the agent
   reads on the next turn.

`annotation.overlay.v1` exists as a template with fixtures. The product is the
round-trip in a live session, not another shell.

**Pass (visual):** a session can open a live visual, the agent keeps writing
into it, the user marks a point, and the next turn sees that mark — without
exporting a CSV or starting a new run. VisualsBench can score that object.

---

## 2. Context — what the agent sees next

Today context is scattered: workspace `AGENTS.md` is discovered from the
working directory; bundled skills are `include_str!` copies with no toggle or
editor; Workshop does not clone cookbooks; MCP groups do not change
`enabled_tools`; V1/V2 lives under Settings → Models.

Add **Settings → Context**:

| Block | User can |
| --- | --- |
| Workshop `AGENTS.md` | Read bundled, versioned, read-mostly instructions. Copy. Edits are a product change, not a per-user override. |
| Your `AGENTS.md` | Open/edit the workspace overlay Codex already discovers. See present / empty / overriding. |
| Cookbooks | Opt-in clone + pin of `synth-cookbooks-public` (sparse: `skills/` + selected recipes, **never** `runs/`). Off by default. |
| Skills | Edit user copies; toggle include/exclude. Cookbook skill on only if the pin is ready. |
| MCP groups | Group on/off that actually changes `enabled_tools`. |
| Subagents | Existing V1/V2/none controls, moved here from Settings → Models. Do not invent a second control. |

Cookbooks is **one action**: clone → pin ready → skill on (`use-synth-cookbooks`).
Clone without the skill (or skill without a successful pin) is a broken state.
Friends: public repo only. Same bar as Laguna download: progress, digest,
cancel, Update. Uncheck stops advertising; Uninstall deletes the pin.

**Pass (Context):** a practitioner can answer “what will this agent see on the
next turn?” from Settings without opening `~/.synth-desktop` or a Codex home.
Cookbooks-off sessions never mention the checkout. E4 will read the V1/V2 flags
from this surface — they must be real, not decorative.

Do not: auto-clone on first launch; clone the full monorepo; let chat rewrite
Workshop’s bundled `AGENTS.md`; toggle MCP by guessing tool names.

---

## Sequence inside this cut

1. Write VisualsBench (what good is) before new templates.
2. Families + Trace V5 + diagrams so E2–E4 have a landing place.
3. Click-to-label round-trip on one live visual (Craftax or GEPA is enough).
4. Settings → Context, including the V1/V2 move.

Stop. Hand E2–E4 to [`HANDOFF_V03_PROOFS_E2_E4_2026-08-14.md`](./HANDOFF_V03_PROOFS_E2_E4_2026-08-14.md).

---

## Out of this handoff

- E2 Craftax alignment ladder, Subagents rail, E3 DungeonGrid, E4 Harbor DEO, GELO/OHCO
- Local report seal / Share upload / approval broker
- Intern / mailbox
- Restoring Open Research Visual SDK
- Mixing into the v0.2 GO line

Named instances follow [`TEST_INSTANCE_LOGIN_CONTRACT.md`](./TEST_INSTANCE_LOGIN_CONTRACT.md).
Do not prefix launches with credential exports.
