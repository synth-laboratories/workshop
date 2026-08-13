# Synth Desktop × Research Engineering

Working synthesis of product intent, architecture, evidence stack, and research
infra that Synth Desktop should host and visualize. Companion to
[`HANDOFF.md`](./HANDOFF.md), [`mock_app_plan.md`](./mock_app_plan.md), and
[`handoff-package/`](./handoff-package/).

Status: planning / scope pin-down. Not an implementation commit plan.

---

## 1. Product framing

Synth Desktop is **not** primarily another coding IDE.

> Synth Desktop is a local-first agent research and development workbench where
> agents can run locally or in Synth Cloud, and where every run produces
> inspectable, replayable, quantitative, version-linked artifacts.

Core loop:

**observe → understand → modify → evaluate → fine-tune → deploy**

Center of gravity:

1. universal agent sessions
2. universal rollout / Trace V5 representation
3. rich artifacts and **visuals** (Claude Artifacts–class, Synth-shaped)
4. standardized metrics and reward metadata
5. exact harness / model / adapter / environment provenance
6. local ↔ cloud execution parity
7. local fine-tuned model deployment
8. easy desktop-agent evaluation

V1 UI is closer to Codex Desktop + Claude Artifacts + PostTrainBench-style
trajectory inspection + Synth eval visualizations than to VS Code/Cursor.

---

## 2. V1 execution wedge

Two first-class cloud/local agent targets, plus remote API models as
compatibility routes:

| Target | Mode | Notes |
| --- | --- | --- |
| **Local Laguna XS 2.1** | sync session | MLX / Metal; LoRA as `base + adapter` |
| **Synth Intern · Live** | sync | operator-present mailbox |
| **Synth Intern · Background** | async | durable job; `disconnect ≠ pause` |
| **Remote APIs** (e.g. OpenRouter Luna, OpenRouter Poolside S2.1) | via Codex App Server + ACP | same session/run/event model; **usage tracked locally** |

Codex App Server and ACP are **compatibility surfaces**, not the internal
architecture:

```text
Synth Runtime
 ├── native Synth protocol
 ├── Codex App Server adapter
 └── ACP adapter
```

Electron is a client. A **local runtime daemon** owns sessions, turns, tool
calls, approvals, checkpoints, artifacts, metrics, rollout construction, local
inference, cloud delegation, persistence, and provenance.

---

## 3. Visuals — the Claude Artifacts analogue

Desktop should be a **visual home** for agent R&D.

- Conversation left; **artifact / visual pane** right.
- Agents emit or attach visuals (`html`, `react_app`, charts, environment
  frames, rollouts).
- Transcript shows a chip/icon when a visual is created; click or agent
  `show` opens the renderer.
- Goal: the polish of pages like
  [Craftax evals](https://www.usesynth.ai/evals/craftax) (cost/perf pareto,
  achievement matrices, trajectory frames + metrics) becomes a **native product
  output**, not a one-off website.

Reference inspiration in product handoff: Claude Artifacts extended to
quantitative / agentic research outputs. Harnesses emit artifacts + metrics;
desktop auto-builds timeline / scrubber / charts; optional custom renderers
(e.g. `craftax.viewer.tsx`) when needed.

Semantic projection alongside canvas (accessibility + CUA + agent reasoning)
remains a hard requirement from `HANDOFF.md`.

---

## 4. Evidence stack: Trace V5, rewards, annotations, storage

### 4.1 Sealed Trace V5

`synth.trace.v5` is the canonical, content-addressed capture of what happened.
Publish into the **Artifact Platform** (immutable manifests + blobs). A
**central Trace V5 catalog** indexes factual metadata only:

- reward, named metrics, tokens, cost, timing
- harness / model / env / dataset / scorer versions
- project / Factory / Effort / Run / experiment / Result identities

Catalog reports facts. It does **not** decide “best,” DEO frontiers, or
scientific verdicts. Zero reward / metric / cost values are present facts, not
absence.

### 4.2 Reward metadata

Rewards are typed evidence, not a float stuck in chat:

- `RewardDefinitionV1` — intent, emission, subject scope, components, bounds
- `RewardRecordV1` — value, components, provenance, evidence selectors into the
  sealed trace

### 4.3 Annotations run on top

`TraceEvidenceBundleV5` is **append-only derived evidence** about a sealed
trace: annotations, verifiers, reward records, evaluation results. Appending
never changes the trace digest. Annotations cite exact selectors; UI draws them
as overlays (markers on scrubbers / timelines).

Lighter HTTP-facing overlay for environments:

- `synth.rollout_annotations.v1` — achievements, survival, histograms,
  parse/continuity, teacher labels (Craftax-style)

### 4.4 Visuals consume the stack

Visual Registry sources include `trace_publication` and `artifact_publication`
at exact digests. Agents should bind visuals to catalog / trace / artifact
identity rather than inventing numbers in HTML.

```text
sealed Trace V5 + CAS
        → catalog facts (incl. reward)
        → annotations / evidence overlay
        → visuals / evals that cite them clearly
```

---

## 5. synth-containers and rollouts

Repo / package: [`containers`](https://github.com/synth-laboratories/containers)
→ PyPI `synth-containers`.

A Synth container is a small HTTP service around a task. Optimizers and evals
see a URL and a typed rollout contract — they never import the task package.

| Route | Purpose |
| --- | --- |
| `GET /metadata` | contract version + capabilities |
| `GET /program` | mutable prompt fields + seed candidate |
| `GET /dataset` | splits + counts |
| `POST /dataset/rows` | rows for seeds |
| `POST /rollout` | candidate × row → reward + usage (+ artifacts) |
| `GET /health` | liveness |

The package also owns rollout / execution records, rollout annotations, Trace
V5 sealing / evidence, reward helpers, tunnels helpers, rubrics, and
`synth-trace` CLI.

Evals authority split (from eval standard):

| Surface | Authority |
| --- | --- |
| Trace V5 sealing + provenance | synth-containers |
| Harbor trajectories / native verifiers | Harbor |
| Private Evals package materialization | repository-internal |
| Execution lifecycle | SMR / backend |
| Cross-source attempt indexing | evals |

Desktop should eventually **inventory local and remote containers / pools** in
the app (attach as execution targets; open last rollouts / traces).

---

## 6. Broader research infra Desktop should track / visualize

Beyond Trace V5, containers, and visuals:

| Layer | Role |
| --- | --- |
| **evals** | TOML matrix: `run → score → save evidence → index`; private + Harbor lanes, both through Containers |
| **Harbor** | Benchmark harness over container envs |
| **Private Evals runner** | Package materialization + managed runtime launch through Containers |
| **synth-optimizers** | GEPA (local) + hosted GELO; consumer of container contract |
| **cookbooks** (private → public) | Runnable recipes; **visual template distribution** |
| **synth-ai SDK** | Research Factory / Efforts / Intern / projects |
| **Artifact Platform + Trace catalog** | Content-addressed pubs + factual index |
| **Pools / synth-tunnel** | Deployed container pools + sticky agent data plane |
| **understudy** | Local exe.dev stand-in for slots |
| **jesterky** | Pinned Rust workflow substrate (record / replay) |
| **REB / experiments / gamebench** | Task families and env packaging |
| **synth-dev slots** | Local multi-slot bring-up |

---

## 7. Systems diagram — Desktop with local vs cloud

```text
╔══════════════════════════════════════════════════════════════════════════════════════╗
║                           SYNTH DESKTOP  (Electron client)                           ║
║  ┌────────────┐  ┌──────────────┐  ┌─────────────┐  ┌──────────────┐  ┌───────────┐ ║
║  │ Sessions   │  │ Transcript   │  │ Visual /    │  │ Runs·Rollouts│  │ Inventory │ ║
║  │ + targets  │  │ + event chip │  │ Artifact    │  │ Trace scrub  │  │ Models    │ ║
║  │ Local      │  │ show/open →  │  │ pane        │  │ + annotation │  │ Containers│ ║
║  │ Intern L/B │  │              │  │ (Craftax-   │  │ overlay      │  │ Pools     │ ║
║  │ Remote API │  │              │  │  class UX)  │  │              │  │ Adapters  │ ║
║  └─────┬──────┘  └──────┬───────┘  └──────┬──────┘  └──────┬───────┘  └─────┬─────┘ ║
║        └────────────────┴─────────┬───────┴────────────────┴────────────────┘       ║
║                                   │  Synth Protocol (SSE / IPC)                     ║
╚═══════════════════════════════════╪══════════════════════════════════════════════════╝
                                    ▼
╔═══════════════════════════════════╪══════════════════════════════════════════════════╗
║              LOCAL MACHINE — synth-runtime daemon owns orchestration                 ║
║                                   │                                                  ║
║  ┌────────────────────────────────▼───────────────────────────────────────────────┐  ║
║  │  LOCAL DURABLE STORE (stays on disk; restart / leave-safe)                     │  ║
║  │  SQLite: sessions · runs · event log · cursors · usage ledger                  │  ║
║  │  CAS blobs: prompts · adapters · local artifacts · visual HTML · frames        │  ║
║  │  Pointers: remote ids (intern session/job, publication, pool, trace digest)    │  ║
║  └────────────────────────────────────────────────────────────────────────────────┘  ║
║                                                                                      ║
║  ┌─ EXECUTION ADAPTERS ───────────────────────────────────────────────────────────┐  ║
║  │  LOCAL                 COMPAT / REMOTE API              CLOUD AGENTS           │  ║
║  │  Laguna XS 2.1         Codex App Server + ACP           Intern Live (sync)     │  ║
║  │  MLX (+ LoRA)          OpenRouter Luna / Poolside S2.1  Intern Background      │  ║
║  │  usage → local ledger  usage → local ledger             (async; leave-safe)    │  ║
║  │  Local synth-containers (/rollout) ← optimizers (GEPA, …)                      │  ║
║  └────────────────────────────────────────────────────────────────────────────────┘  ║
║                                                                                      ║
║  ┌─ EVIDENCE PIPE (same shapes local or remote) ──────────────────────────────────┐  ║
║  │  RuntimeEvent stream → optional seal synth.trace.v5                            │  ║
║  │  reward metadata · rollout_annotations.v1 · TraceEvidenceBundle (overlay)      │  ║
║  │  Visual draft + resource_refs                                                  │  ║
║  └────────────────────────────────────────────────────────────────────────────────┘  ║
╚══════════════════════════════════╤═══════════════════════════════════════════════════╝
                                   │ publish / track when opted in
                                   ▼
╔══════════════════════════════════════════════════════════════════════════════════════╗
║                         SYNTH CLOUD  (desktop tracks; does not own bytes)            ║
║  Factory · Effort · Project · Intern mailbox · Swarm / SMR                           ║
║  Artifact Platform (CAS) · Trace V5 catalog (facts) · Visual Registry                ║
║  Container pools · synth-tunnel · Harbor / private Evals jobs · optimizers (GELO/…) ║
║  Desktop tracks: ids, digests, cursors, status, spend summaries, open/resume handles ║
╚══════════════════════════════════════════════════════════════════════════════════════╝
```

### Data placement

| LOCAL ONLY (default) | LOCAL + POINTER TO CLOUD | CLOUD AUTHORITY |
| --- | --- | --- |
| chat drafts / UI state | Intern session / job ids | sealed Trace V5 publications |
| Laguna weights / LoRAs | pool URLs + health | Artifact manifests |
| local usage ledger | visual revision ids | Trace catalog facts |
| unsynced CAS artifacts | publication digests | published Visuals |
| local container inventory | annotation overlay refs | Factory / Effort truth |
| private files / workspaces | remote model usage mirrors | remote pool leases |

### Visualization loop

```text
agent / eval / container rollout
        │
        ├─► events in transcript (chip when visual/artifact created)
        ├─► optional agent “show” → open right pane
        ├─► pane binds TraceSource / artifact_publication / local CAS
        ├─► scrubber + metrics from catalog or local event log
        └─► annotations drawn ON TOP (never rewrite sealed trace)
```

---

## 8. Existing Craftax research (reference corpus)

Desktop visuals should render **this** stack, not invent a parallel one.

**Public / blog**

- [usesynth.ai/evals/craftax](https://www.usesynth.ai/evals/craftax) — model
  cost/perf, achievement matrix, effort sweeps
- `frontend/content/blog/research-factory-craftax` — Research Factory acceptance
- `frontend/public/evals/craftax/v5` — matrix + replays

**Evals / harness / comparisons**

- `evals/suites/nonproduct/craftax` — harness, achievements, native frames,
  panels, failure analysis
- FactoryBench Craftax profiles (`craftax`, `craftax_harness_prompt`, rust nano)
  with Intern sync/async matrix configs
- Local-harness vs Factory comparisons; speedrun bakeoffs (`compare_*`)

**SFT / RLVR / training**

- `experiments/experiments/craftax_speedrun` — CISPO/PPO, action-sequence RLVR,
  reward curriculum, V5 linkage
- `evals-reb-authority-dev/experiments/craftax_mlx_sft_rlvr` — MLX SFT/RLVR
  artifacts
- REB tasks: SFT filtration, curriculum RLVR, Qwen4B PPO/CISPO/RIPO/… variants
- Craftax-first Task Factory plan (`HANDOFF_craftax_first.md`)

**Traces / visuals / gamebench**

- Trace V5 pins, coop V5 fixtures, observation-uplift → visual artifacts
- Rollout annotations + visualbench corpus
- `gamebench` + DEO / pool packaging worktrees

---

## 9. Visual templates for research genres

SFT, RLVR, GEPA, GELO, Craftax evals, etc. should ship **starter visual kits** —
consistent chrome and data contracts that agents can fill or hack at runtime.

### Distribution

| Where | What |
| --- | --- |
| `synth-cookbooks-private/cookbooks/visuals/…` | Draft kits until public-safe |
| `synth-cookbooks-public/cookbooks/visuals/…` | Promoted templates + example bindings |
| optional `packages/synth-visual-templates/` | Versioned installable package |
| Desktop local CAS | Imported kit digests + agent forks |
| Visual Registry | Published revisions bound to Trace / Artifact sources |

Keep `evals/core/visuals` as the **render contract / chrome tokens**; cookbooks
hold the **product templates** agents start from.

### Example kit layout

```text
cookbooks/visuals/
  craftax.eval_matrix.v1/       # pareto + achievement grid (blog shape)
  craftax.rollout_scrub.v1/     # frame scrubber + annotations overlay
  sft.curve_compare.v1/         # loss/reward curves, checkpoint table
  rlvr.training_run.v1/         # CISPO groups, reward breakdown, V5 links
  gepa.pareto_frontier.v1/      # candidate frontier + rollout samples
  gelo.plugin_lane.v1/          # SFT/RLVR lane status + spend
```

Each kit:

```text
<template_id>/
  template.json          # id, schema, data slots, renderer kind
  shell.html|tsx         # consistent chrome / charts
  components/            # reusable pieces
  examples/              # fixture TraceSource bindings
  README.md
```

### Desktop flow

```text
browse / import cookbook kit
  → pin by content digest
  → agent fills slots or edits shell
  → save as local visual (CAS)
  → optional publish to Visual Registry
```

Same promotion pattern as cookbooks → optimizers today: private incubate →
public promote → consumer downloads pinned revision.

---

## 10. Workshop build posture (near-term)

| Lane | Purpose |
| --- | --- |
| `apps/mock` | UX pin-down (landing → chat → inspector) with fixtures; no daemon/MLX |
| `DesktopRuntime` interface | UI talks only to a runtime contract; mock then real daemon |
| `handoff-package/` | Reuse Intern / backend / SDK excerpts; avoid parallel mailbox models |
| Early Desktop attempts | `~/Downloads/synth-desktop-first-pass*` — prior runnable stubs |

Do not put orchestration in Electron. Prefer establishing the central
session / run / event / artifact / visual abstractions over a disposable UI
demo.

---

## 11. In-scope checklist (this synthesis)

- [x] Local-first research workbench framing (not full IDE V1)
- [x] Laguna local + Intern Live/Background + remote APIs via Codex/ACP
- [x] Local usage ledger for remote model calls
- [x] Visual home (Artifacts-class) with chip → open pane
- [x] Trace V5 seal + Artifact Platform + central catalog
- [x] Reward metadata as typed evidence
- [x] Annotations / evidence overlays (do not mutate sealed traces)
- [x] synth-containers rollouts; inventory local + remote pools later
- [x] Track Factory / Effort / Intern / pools / publications by id+digest
- [x] Craftax blog/evals/SFT/RLVR corpus as visual reference
- [x] Genre visual templates distributed via cookbooks (import → fork → publish)

---

## 12. Related docs

- [`HANDOFF.md`](./HANDOFF.md) — full product + architecture handoff
- [`mock_app_plan.md`](./mock_app_plan.md) — UX pin-down plan
- [`handoff-package/README.md`](./handoff-package/README.md) — eng reuse bundle
- Backend: Artifact Platform, Trace V5 catalog, Visual Registry specs
- `evals/docs/eval-standard.md` — authority boundaries
- `containers` README — optimizer/eval task contract
