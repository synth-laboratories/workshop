# Synth Desktop — First-Pass Eng Handoff Bundle

**Date:** 2026-08-08  
**Audience:** Eng team implementing the first Electron + local-runtime pass  
**Repos:** Local Laguna XS 2.1 + Synth Intern (sync + async) through one Runtime API

This zip is a **curated review package**, not a dump of `backend` / `frontend` / `synth-ai`. It answers the product handoff’s reuse/gap questions against the live Synth codebase and includes the contracts, plans, and client excerpts you need to start without cloning everything.

---

## Start here (30–60 min)

1. [`docs/01-ENG-BRIEF.md`](./docs/01-ENG-BRIEF.md) — what to build, what not to build
2. [`docs/02-REUSE-GAP-MATRIX.md`](./docs/02-REUSE-GAP-MATRIX.md) — existing Synth objects → desktop concepts
3. [`docs/03-V1-ARCHITECTURE.md`](./docs/03-V1-ARCHITECTURE.md) — packages, process model, IPC
4. [`docs/04-IMPLEMENTATION-SEQUENCE.md`](./docs/04-IMPLEMENTATION-SEQUENCE.md) — milestone order + first PR
5. [`docs/05-API-CHEATSHEET.md`](./docs/05-API-CHEATSHEET.md) — HTTP/SDK/SSE cheat sheet

Then skim:

- [`docs/00-PRODUCT-HANDOFF.md`](./docs/00-PRODUCT-HANDOFF.md) — full product thesis (Appendix at end is the **authoritative V1 cut**)
- [`plans/intern_interaction_boundaries.md`](./plans/intern_interaction_boundaries.md) — binding Sync/Async adapter law
- [`excerpts/frontend/researchIntern.ts`](./excerpts/frontend/researchIntern.ts) — typed Intern client + SSE replay/tail (highest frontend reuse)

---

## Package layout

```text
handoff-package/
├── README.md                          ← you are here
├── MANIFEST.md                        ← source paths + why each file is included
├── docs/                              ← eng-facing review (this work)
├── plans/                             ← binding Intern product/API plans from backend
├── contracts/research-v1.json         ← Research OpenAPI (generate TS types from this)
├── excerpts/
│   ├── backend/                       ← contracts, mailbox, events, local pilot, HTTP API
│   ├── sdk/                           ← Python SynthClient Intern contracts + API extract
│   └── frontend/                      ← researchIntern.ts + Sync projection helpers
└── references/                        ← supporting handoffs (phase model, Codex stream, SDK map)
```

---

## Sibling repos (not in zip — clone if implementing)

| Repo | Role for Desktop V1 |
|------|---------------------|
| `synth-laboratories/backend` | Authority for Intern HTTP, mailbox, SSE, Local Pilot |
| `synth-laboratories/synth-ai` | Python SDK + MCP (`intern_sync_*` / `intern_async_*`) |
| `synth-laboratories/frontend` | React reference UI (`/smr/intern/*`); extract, don’t fork Next pages |
| `synth-laboratories/understudy` | Local exe.dev control-plane reference (Rust); optional |
| `synth-laboratories/workshop` | This product handoff + greenfield desktop home |

---

## Non-negotiables (from review)

1. **Electron is a client.** Orchestration lives in a local runtime daemon.
2. **Do not invent a second Intern mailbox.** Reuse `/smr/research-intern/*` + generation-fenced commands + `after_sequence` SSE.
3. **Sync and Async stay separate wire planes**, but Desktop presents them as `ExecutionTarget.kind = "intern"` with `mode: "sync" | "async"`.
4. **Local Laguna is greenfield** in these repos (MLX daemon). Cloud Laguna model IDs exist; local MLX serving does not.
5. **MCP ≡ SDK ≡ HTTP ≡ same mailbox.** Prefer Python `SynthClient` in the daemon; generate TS types from OpenAPI for the Electron UI if needed.
6. **Postgres-backed event sequence is product authority.** Internal Codex activity SSE is evidence-only.

---

## Suggested first shippable slice

```text
Electron shell
  → IPC
  → local Python daemon
       ├─ Laguna local stub (OpenAI-ish stream)   # or real MLX if ready
       └─ SynthClient → Intern sync create/send/tail
  → one Run viewer that consumes the same Event stream for both targets
```

See [`docs/04-IMPLEMENTATION-SEQUENCE.md`](./docs/04-IMPLEMENTATION-SEQUENCE.md) for the full milestone plan.
