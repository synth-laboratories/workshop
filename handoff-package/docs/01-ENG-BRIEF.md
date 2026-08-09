# Eng Brief — Synth Desktop First Pass

**Goal:** Ship a native Mac workbench where agents run **locally (Laguna XS 2.1)** or in **Synth Intern (live sync / background async)**, and every run is inspectable through one Session/Run/Event model.

**Not the goal:** Another VS Code/Cursor clone. Coding UX is secondary to run/artifact/inspectability.

---

## Product one-liner

> Synth Desktop lets you work with a fast private Laguna XS 2.1 agent on your Mac, and hand work to Synth Intern for live or long-running cloud agents — all in the same session/run interface.

---

## V1 execution targets (only two)

```ts
type ExecutionTarget =
  | { kind: "local"; model: "laguna-xs-2.1"; adapter: string | null }
  | { kind: "intern"; mode: "sync" | "async"; intern?: InternRef };
```

| Target | Meaning | Existing system |
|--------|---------|-----------------|
| Local Laguna | Sync session on-device via MLX | **Invent** local inference + local agent loop |
| Intern sync | Operator-present cloud session | `POST /smr/research-intern/sync-sessions` + commands + SSE |
| Intern async | Org-singleton long-running runtime | `POST /smr/research-intern/async/ensure` + messages + SSE |

Both must emit the same desktop domain objects:

```text
Session · Turn · Message · ToolCall · Artifact · Metric · RolloutStep · Outcome
```

Wire-level Intern already has projections + ordered events. Desktop maps those into the domain objects above in the **local runtime daemon**, not in Electron.

---

## Explicitly deferred (do not build in pass 1)

```text
Qwen policy routing
Auto local/cloud routing
Full ACP / full Codex App Server as desktop ontology
Multi-LoRA hot swap / training UI
Custom renderer SDK
Full Harness Revision graph
Full eval comparison UI
Full IDE/editor
Legacy Intern /sessions plane
```

Preserve schema room (`adapter: null`, harness ids optional) so these don’t force rewrite.

---

## Architecture rule

```text
Electron (UI)
    │  IPC / local HTTP
    ▼
Synth Runtime Daemon (owns sessions, events, provenance)
    ├── Local Laguna adapter (MLX / OpenAI-ish)
    └── Intern adapter (synth-ai SynthClient → backend)
```

Do **not** put Intern SSE, API keys, or orchestration in the renderer.

---

## What “done” looks like for first eng pass

| Milestone | Done when |
|-----------|-----------|
| M1 Runtime contract | Shared TS/Python types for Session/Run/Event; cursor replay works offline against fixtures |
| M2 Local Laguna | Prompt → stream tokens → UI; cancel works; model identity recorded on Run |
| M3 Intern sync | Same UI, target=Intern sync; create → send → tail → transcript |
| M4 Intern async | ensure → send → disconnect → reconnect from `after_sequence`; pause/resume |
| M5 Run viewer | Local + Intern events in one timeline; artifact refs clickable |
| M6 Eval hooks | Stable `data-testid` / ARIA on primary surfaces (full eval mode later) |

---

## Team split suggestion

| Track | Owner focus |
|-------|-------------|
| **Runtime protocol + daemon** | Session store, IPC, event log, Intern adapter via `synth-ai` |
| **Local inference** | MLX Laguna lifecycle, OpenAI-compatible stream, hardware checks |
| **Desktop shell** | Electron + React; reuse `researchIntern` patterns; Sync cockpit layout as reference |
| **Contracts** | Lockstep OpenAPI ↔ generated types ↔ daemon DTOs |

---

## Auth / environment

| Surface | Auth |
|---------|------|
| Desktop → Synth Cloud | Org API key (`SYNTH_API_KEY`) via keychain; `SYNTH_BACKEND_URL` |
| Frontend web today | Clerk + Next `/api/smr` BFF — **do not copy** for Electron |
| Local Pilot (optional later) | Loopback + `INTERN_LOCAL_PILOT_ENABLED` — desktop can become the pilot host |

---

## Source of truth priority

1. This package’s `docs/02–05` (review against live code, 2026-08-08)
2. `plans/intern_interaction_boundaries.md` (binding adapter law)
3. `contracts/research-v1.json` (machine contract)
4. Full product thesis in `docs/00-PRODUCT-HANDOFF.md` Appendix (narrowed V1)
5. Live sibling repos when implementing
