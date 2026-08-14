# Handoff — v0.3 proofs (E2–E4, then GELO / OHCO)

**For:** the engineer running the collaboration proofs after the visual
workspace and Context exist.
**Depends on:** [`HANDOFF_V03_VISUAL_AND_CONTEXT_2026-08-14.md`](./HANDOFF_V03_VISUAL_AND_CONTEXT_2026-08-14.md)
(E1 VisualsBench, live label loop, Settings → Context including V1/V2).
**Do not start until v0.2 GO.** Do not mix into SYN-3202 / SYN-3215 / SYN-3212.
**Do not push** unless Josh asks.

A theme is not done because a template exists. It is done when Workshop can run
it live, the agent keeps updating the visual, the user can label, and
VisualsBench scores the visual independently of the task score.

Project: [Workshop v0.3 Collaboration](https://linear.app/synth-ai/project/workshop-v03-collaboration-d74df67fa366)
Freeze: `docs/launch/v0.3-themes.md` on `josh/v03-approval-broker`

---

## Tickets (one proof chain)

| Order | Ticket | Proof |
| --- | --- | --- |
| 1 | [SYN-3221](https://linear.app/synth-ai/issue/SYN-3221) | **E2** Craftax alignment ladder — zombie / skeleton attacks |
| 2 | [SYN-3222](https://linear.app/synth-ai/issue/SYN-3222) | Subagents rail, workspace, spawn/wait that actually parallelizes |
| 3 | [SYN-3223](https://linear.app/synth-ai/issue/SYN-3223) | **E3** DungeonGrid concurrent multi-agent (blocked on the rail) |
| 4 | [SYN-3224](https://linear.app/synth-ai/issue/SYN-3224) | **E4** Harbor GameBench + CardBench DEO, three Codex arms (needs Context) |
| 5 | [SYN-3225](https://linear.app/synth-ai/issue/SYN-3225) | Iterate GELO, then add OHCO |

Do not treat Intern as the runner. Do not pick a winner from one n.

---

## E2 — Craftax alignment ladder

Three arms, **same seeds**, **same visual family**. Attack evidence from engine
NEV / combat payloads, not guessed from ASCII.

| Arm | Policy | What the visual must show |
| --- | --- | --- |
| Baseline | Unaligned Craftax | Zombie and skeleton attacks (player→mob and mob→player) as a first-class series |
| Aligned | VeganCraftax / REB-009 shape | Attacks drop on protected classes; progress/survival still visible; missing combat stays **null** |
| Aligned + RLVR | Same objective, then RLVR | Whether RLVR preserves the refusal, recovers progress, or reintroduces attacks — the visual is the comparison |

**Pass:** one Workshop session opens the three arms side by side (or flips with
a shared cursor), labels a zombie/skeleton incident, and the next turn sees
that label. VisualsBench grades the visual; Craftax grades the policy.

---

## Subagents, then E3 — DungeonGrid

Transport already exists (V1 `collabAgentToolCall`, V2 `subAgentActivity`,
VisualHost groups Working / Needs attention / Completed). Remaining work is
the Codex-like rail, a dedicated Subagents workspace, and spawn/wait that
**overlaps in wall-clock**. Child process is legible on the rail *and* on the
live visual. Raw child chat does not flood the parent.

See [`SUBAGENTS_UX_PROPOSAL_2026-08-12.md`](./SUBAGENTS_UX_PROPOSAL_2026-08-12.md).
Parallelization the user cannot see is just another black box.

**E3** is GameBench `dungeongrid-multiplayer` (party on a grid, **not**
Craftax-Coop). Parent Codex + children on the rail/workspace, live visual of
the shared dungeon, not a post-hoc GIF pack.

**Pass (rail):** when a task splits, children run together; parent stays in
flow; status is ambient, detail is on demand.

**Pass (E3):** child roles/actions are legible in the visual *and* the subagent
chrome; parallel children actually overlap in wall-clock (not a serialized
multi-agent story); the parent transcript is not a dump of child chat;
VisualsBench can score the live visual without the GameBench HTML report.

E3 is blocked on SYN-3222.

---

## E4 — Harbor DEO, three Codex arms

Harbor-folded **GameBench and CardBench** code-policy DEO, same recipes. The
point is to see **where the differences lie**.

| Arm | Model | Codex collab |
| --- | --- | --- |
| Terra · V2 | GPT-5.6 Terra | preset V2 (bundled) |
| Luna · V1 | GPT-5.6 Luna | preset V1 (bundled) |
| Luna · forced V2 | GPT-5.6 Luna | override V2 via `model_multi_agent_update` |

The forced Luna V2 arm exists so you cannot claim “V2 is better” from Terra-V2
vs Luna-V1 alone.

**Pass:** all three arms run through Harbor in Workshop; live DEO visuals
update in place; Settings → Context shows the actual V1/V2 flags that were
used; a comparison visual (plus VisualsBench) can attribute deltas to
**model vs protocol**.

Needs Context (SYN-3220) so those flags are visible and real. Needs the visual
loop so the DEO view updates in place rather than after the run is dead.

---

## Then GELO, then OHCO

GoEx prompt-only is **already live-proven**. Do not re-litigate that floor.

Receipt: `craftax_goex_luna_med_live_09` — 840 contiguous events, 785 proposer
deltas, 2 candidates, 5 Luna-medium rollouts, 28 checkpoints (25 branchable),
one restore from a named checkpoint. Proposal did not uplift; GoEx correctly
rejected it. Infrastructure **PASS**; no performance-uplift claim. Evidence
under `/Users/joshuapurtell/Documents/Codex/2026-08-12/let/receipts/external-acceptance/gelo-craftax-luna/`.

v0.3 work on that floor:

1. Iterate GELO: more than prompt-only, honest scores, checkpoint/resume UX in
   the live visual, fail-closed on missing affordances, family ids
   `optimizer.gelo.*` so it is not stuck as a `go-ex` overlay. The Optimizers
   page already lists GELO as a **Plan with agent** card — iterate the
   algorithm and visual, not the catalog noun.
2. Add **OHCO** as a first-class algorithm next to GELO (same envelope,
   inspectable children, Workshop visual). Do not smuggle OHCO into v0.2
   launch notes or friends copy.

**Pass:** a practitioner can run GELO, see why a proposal was rejected, restore
from a named checkpoint, and run an OHCO campaign on the same Optimizers
surface without guessing `/events`. VisualsBench grades the live visual; the
optimizer grades the campaign.

---

## Order

```text
visual + Context handoff
        ↓
E2 Craftax alignment          (needs live visual + labels)
        ↓
Subagents rail + spawn/wait
        ↓
E3 DungeonGrid                (blocked on the rail)
        ↓
E4 Harbor DEO three-arm       (needs Context V1/V2)
        ↓
GELO iterate → add OHCO
```

Stop. Reports (local seal + Share) and the approval broker are a later
milestone. E5 (Lightning SFT) is an explicit last try, not a gate.

---

## Out of this handoff

- Building VisualsBench, families, or Settings → Context (previous handoff)
- Local ArtifactBundle seal / Synth blob upload
- Intern / mailbox / projects-as-boundary
- Generic RLVR / MAPO / OpenEnv catalogs, LoRA picker, per-task pricing
- Declaring V2 better from Terra-V2 vs Luna-V1
- Re-running prompt-only GoEx as if it were unproven
- Bulk-synthetic ResearchAssistantBench

Named instances follow [`TEST_INSTANCE_LOGIN_CONTRACT.md`](./TEST_INSTANCE_LOGIN_CONTRACT.md).
Do not prefix launches with credential exports. Do not invent `$0` for missing
evidence.
