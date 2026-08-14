# Handoff — build VisualsBench as a Harbor benchmark for Codex

**For:** the engineer packaging E1 VisualsBench.
**Ticket:** [SYN-3218](https://linear.app/synth-ai/issue/SYN-3218)
**Shape:** Harbor task family. Policy is **Codex**. Workshop is where you watch it.
**Do not start until v0.2 GO.** Do not mix into SYN-3202 / SYN-3215 / SYN-3212.
**Do not push** unless Josh asks.

This is **not** Artifacts VisualBench (SYN-3110–3112). Do not reuse
`evals/reference/old/visualbench` GIF/HTML corpora. Those are sealed Craftax
report packs. VisualsBench grades **agent-authored / live-updated Workshop
visuals**.

Sibling: [`HANDOFF_V03_VISUAL_AND_CONTEXT_2026-08-14.md`](./HANDOFF_V03_VISUAL_AND_CONTEXT_2026-08-14.md)
(write “what good looks like” here, as Harbor tasks, before new templates).
Dicken papers/harnesses dogfood this family:
[`HANDOFF_V03_DICKEN_PAPERS_HARNESSES_2026-08-14.md`](./HANDOFF_V03_DICKEN_PAPERS_HARNESSES_2026-08-14.md).

Freeze: `docs/launch/v0.3-themes.md` on `josh/v03-approval-broker`.
Harbor fold: [`container_compat.md`](./container_compat.md) § Harbor / A2.
A2 operator clock: [`launch/HANDOFF_HARBOR_GAMEBENCH_DEO_LUNA.md`](./launch/HANDOFF_HARBOR_GAMEBENCH_DEO_LUNA.md)
(the fold, not the GameBench content).

---

## What this job actually is

Two objects. Do not collapse them.

```text
Harbor trial  (OUTER — live.harbor_eval.v1)
  Policy   harbor_fused + Codex   AUTHOR
  World    Harbor env: instruction.md + workspace fixtures + hidden tests
  Eval     tests/test.sh → reward.json   SCRIPT NODE after Codex exits
  Stream   planned → launched → Codex tools → submission → verifier
  live_frames = unsupported on the fold

Product visual  (INNER — whatever Codex created)
  Policy   same Codex turn, via synth_visuals MCP
  World    this Desktop's visual registry + CAS
  Eval     VisualsBench dimensions (honesty, bind, compare, …)
  Stream   the visual Codex bound; not a fake Craftax map
```

GameBench A2: Codex writes a **player**, verifier scores the **env**.
VisualsBench: Codex authors a **Workshop visual**, verifier scores that
**visual**. Craftax / DungeonGrid / Harbor-DEO task metrics stay on those
graders. **Both must pass** when a theme uses both.

Missing VisualsBench reward stays **null**, never `0.0`. A verifier `0.0` with
a completed script is a **task** score (the visual failed a gate), not infra
failure. Rig failure (MCP down, no visual_id, scorer crashed) is a distinct
status and must not look like “honesty = 0”.

---

## Harbor package

Standard Harbor anatomy. Light task, heavy workspace, sealed tests. Agent must
not run the scorer.

```text
evals/visualsbench/<task-id>/
  task.toml              name, policy pin, verifier image, combiner
  instruction.md         what Codex must do (visible)
  workspace/             fixtures Codex may read (traces, arms, labels.json, paper excerpt, harness log)
  environment/           Dockerfile if the trial needs a sandbox besides Desktop MCP
  tests/                 HIDDEN from Codex
    test.sh              trusted script node
    cases/               expected visual_id stability, missing fields, digest pins
  reference/             sealed gold (optional); never in the agent workspace
```

`instruction.md` is the Codex prompt. It names the visual claim, the template
family when one is required, and the MCP tools (`synth_visuals.visual_manage`,
`authoring_context`, `capture_review`, `review`, `mark_ready`). It does **not**
name the hidden gates.

Pin the policy explicitly. First cut: `harbor_fused` + Codex Luna med (or the
ChatGPT Luna composer target this Desktop already uses). Later: Gemini Flash
via OpenRouter as a second policy on the **same** tasks. Host does not default
the model.

---

## How Codex reaches the visual

Workshop is the world the practitioner watches. Codex in the Harbor trial must
call **this Desktop’s** `synth_visuals`, not invent SVG in the sandbox.

1. Register the VisualsBench Harbor task like any other Harbor fold.
2. Probe must advertise `live.harbor_eval.v1`, slot **`stream`**,
   `liveFrames: unsupported`, and `mcp_bind` for `synth_visuals`.
3. Open `live.harbor_eval.v1` on the **declared** SSE **before** start
   (`run-live-container-evals` clock). That card is trial progress.
4. Codex `create`s the product visual, `show`s it, revises the **same**
   `visual_id`. The practitioner sees that pane update.
5. After Codex exits, the verifier execution (separate image / script node)
   reads the sealed visual revision + captures + receipt from the instance
   export. It does not scrape the pane.

If `synth_visuals` is not bound, **refuse start**. Do not let Codex write
`blank.canvas.v1` HTML, a markdown screenshot, or a file named `visual.json`
in the sandbox and call it a pass.

`stream` on the Harbor fold is the trial log. Product visuals (Dicken, Craftax,
compare) use their own slots. Never bind a live eval `stream` onto a diagram
template.

---

## What good is (encode as tasks, not a blog)

A good visual for this cut:

- is bound **before** mutation and stays the same instance as evidence arrives
- shows missing as missing (never coerces reward / attack / score to 0)
- is scrubbable, comparable across arms, and labelled without mutating a sealed Trace V5
- makes the claim inspectable without reading the transcript
- can be revised live after a user click / label

VisualsBench scores those properties. Suggested `reward.json` fields (names
stable; combiner declared in `task.toml`):

| Field | Means |
| --- | --- |
| `bind_stable` | One `visual_id` from create through last update |
| `honesty_missing` | Fixture nulls still null in bindings / SVG / capture |
| `update_in_place` | Later evidence changed the same instance, not a second visual |
| `compare_across_arms` | Two arms share a cursor/family; claim is the delta |
| `label_round_trip` | Overlay present; Trace V5 digest unchanged |
| `legibility` | `capture_review` wide+compact recorded; required checks pass |
| `claim_inspectable` | Hidden probe: a grader that cannot see the transcript can still name the claim |

Combiner is fail-closed on the structural gates (`bind_stable`,
`honesty_missing`). Legibility cannot waive a coerced zero. Rubric/judge text
cannot waive a missing `visual_id`.

---

## First Codex tasks (build these)

| ID | Fixture Codex sees | Hidden gate |
| --- | --- | --- |
| `honesty-missing-reward` | Trace / live-eval JSON with `reward: null` (and/or absent attack count) | Fail if the visual or its capture renders `0` / `$0` / empty bar for that field |
| `bind-before-mutate` | Create instruction, then a second file of later events | Same `visual_id`; revision incremented; first bind precedes first mutation |
| `compare-two-arms` | Two arm bundles, same seeds | One comparison visual (or flip with shared cursor). Delta is visible without the transcript |
| `dicken-harness-turn` | One Codex/harness log excerpt | `diagram.systems.dynamic.v1`, ≥3 beats, useful poster, no `stream` slot, no JS |
| `label-overlay` | `labels.json` planted in workspace | Overlay kinds applied; sealed Trace V5 digest byte-identical to fixture |

Click-to-label in a live pane (user mouse) is **SYN-3219**, not a Harbor
script. `label-overlay` is the automatable stand-in: Codex must apply the
planted overlay. A separate Workshop CUA pass still required for “user clicks,
next turn sees it.”

Do not add FLE / NetHack / paper-PDF tasks until these five are green on Codex.
Those are dogfood of the same family (see Dicken handoff).

---

## Verifier (script node)

After Codex exits:

```text
tests/test.sh
  → read exported visual revision (id, revision, template_id, bindings, content_digest)
  → read capture_review PNGs + review receipts
  → read Trace V5 digest if the task planted one
  → write /logs/verifier/reward.json
```

Rules:

- Trusted copy of `tests/`. Never the workspace copy Codex could have edited.
- Distinct execution from the Codex trial. Agent does not invoke `test.sh`.
- Native-vs-wrapped verifier must agree (same as A2).
- No network from the bundle. No reading `~/.synth-desktop/.env`.
- Incomplete export → refuse (null reward), not `0.0`.
- Do not OCR a screenshot as the only honesty proof. Bindings + digest first;
  PNG is for legibility gates that already exist (`noTextCollisions`,
  `focalDensity`, `screenshotInspected`).

ATIF is a projection of the Harbor trial, not VisualsBench authority.

---

## Operator clock (Workshop)

Named instance. [`TEST_INSTANCE_LOGIN_CONTRACT.md`](./TEST_INSTANCE_LOGIN_CONTRACT.md).
No credential prefixes.

1. Register the VisualsBench Harbor package. `container_list` → `container_probe`.
2. Confirm `live.harbor_eval.v1`, slot `stream`, `liveFrames: unsupported`,
   `synth_visuals` bound, policy_ref is Codex (named pin).
3. Prepare, do not start. Keep `rollout_id`, `stream_id`, declared URLs.
4. Create and `show` `live.harbor_eval.v1` on the **declared** SSE. Wait for
   `stream.subscribed` / `ready: true`.
5. Start with the prepared identity + `visual_id` + explicit `policy_ref`.
6. Watch two panes: Harbor trial card, and the product visual Codex creates.
7. Terminal: verifier script node, `reward.json` present vs missing. Seal Trace
   V5 of the **trial**. Reopen after Harbor containers are gone. The product
   visual must reopen from the registry, not from SSE.

`container_run_rollouts` is engine acceptance only — never this path.

---

## Pass

1. Five tasks above run as Harbor trials with Codex. Each has a verifier
   `reward.json`. Honesty-missing cannot be passed by a visual that shows `0`.
2. Practitioner watched `live.harbor_eval.v1` **and** the product visual in
   Workshop. Connect-before-start. Missing stays missing on both.
3. Same task, second Codex policy (optional): Gemini Flash. Combiner still
   fail-closed. Do not claim a winner from one n.
4. VisualsBench name collision is documented in the task README: this family
   ≠ Artifacts VisualBench.
5. No GIF/HTML from `evals/reference/old/visualbench` in `workspace/` or tests.

---

## Out

- Artifacts VisualBench / ReportBench / Open Research Visual SDK
- Scoring Craftax engine reward as VisualsBench
- Intern as the runner
- Per-harness VisualHost templates as a substitute for tasks
- Auto-upload / Share (SYN-3226 / 3230 come after a visual this family can grade)
- Animated Mermaid or arbitrary JS as the product visual
- Mixing this into v0.2 GO tickets

Named instances follow the login contract. Do not invent `$0` for missing
evidence.
