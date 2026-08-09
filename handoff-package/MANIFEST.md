# Manifest — what’s in this zip and why

All paths below are relative to the zip root. **Source** is the live sibling repo path at review time (2026-08-08).

---

## Docs (authored for this handoff)

| File | Purpose |
|------|---------|
| `README.md` | Package index |
| `docs/01-ENG-BRIEF.md` | What to build / not build |
| `docs/02-REUSE-GAP-MATRIX.md` | Concept → existing Synth mapping |
| `docs/03-V1-ARCHITECTURE.md` | Process model + packages |
| `docs/04-IMPLEMENTATION-SEQUENCE.md` | Milestones + first PR |
| `docs/05-API-CHEATSHEET.md` | HTTP/SDK/SSE |
| `docs/00-PRODUCT-HANDOFF.md` | Full product thesis (Appendix = authoritative V1 cut) |
| `docs/00-WORKSHOP-README.md` | Workshop repo framing |

---

## Plans (binding product/API law)

| File | Source |
|------|--------|
| `plans/intern_interaction_boundaries.md` | `backend/notes/plans/smr/intern_interaction_boundaries.md` |
| `plans/intern_async_sdk_mcp_interface.md` | `backend/notes/plans/smr/intern_async_sdk_mcp_interface.md` |
| `plans/intern_sync_clarity.md` | `backend/notes/plans/smr/intern_sync_clarity.md` |
| `plans/intern_async_clarity.md` | `backend/notes/plans/smr/intern_async_clarity.md` |
| `plans/intern_frontend_interaction_spec.md` | `backend/notes/plans/smr/intern_frontend_interaction_spec.md` |
| `plans/intern_product_positioning.md` | `backend/notes/plans/smr/intern_product_positioning.md` |

---

## Contracts

| File | Source |
|------|--------|
| `contracts/research-v1.json` | `synth-ai/openapi/research-v1.json` (~1.3MB) |

---

## Backend excerpts

| File | Source | Why |
|------|--------|-----|
| `excerpts/backend/contracts.py` | `backend/packages/intern/contracts.py` | Projection / request models |
| `excerpts/backend/mailbox_*.py` | `backend/packages/intern/mailbox/*` | Cursor/command/event/receipt protocol |
| `excerpts/backend/runtime_events.py` | `backend/services/intern/runtime_events.py` | SSE + page semantics |
| `excerpts/backend/local_pilot.py` | `backend/services/intern/local_pilot.py` | Desktop-as-agent-host precedent |
| `excerpts/backend/research_intern_api.py` | `backend/app/api/v1/managed_research/research_intern.py` | HTTP routes |
| `excerpts/backend/intern_services_README.md` | `backend/services/intern/README.md` | Infra map |

---

## SDK excerpts

| File | Source | Why |
|------|--------|-----|
| `excerpts/sdk/research_intern_contracts.py` | `synth-ai/synth_ai/sdk/research/contracts/research_intern.py` | Client DTOs |
| `excerpts/sdk/research_intern_api_extract.py` | sliced from `.../research_intern.py` | Sync/Async API surface (full module is ~6k LOC) |

Install `synth-ai` for the real client; don’t vendor from this extract alone.

---

## Frontend excerpts

| File | Source | Why |
|------|--------|-----|
| `excerpts/frontend/researchIntern.ts` | `frontend/src/lib/researchIntern.ts` | **Primary reuse** — typed API + SSE |
| `excerpts/frontend/runtime-client.tsx` | `frontend/.../smr/intern/runtime-client.tsx` | Sync/Async shell pattern |
| `excerpts/frontend/useSyncPresence.ts` | same Intern folder | Presence lease |
| `excerpts/frontend/syncSessionProjection.ts` | same | Projection helpers |
| `excerpts/frontend/syncProductEvents.ts` | same | Event parsers |

Not included (too large / Next-specific): `SyncCockpit.tsx`, EffortBoard, BFF routes — clone `frontend` and open `/smr/intern/*` when implementing UI.

---

## References

| File | Source |
|------|--------|
| `references/HANDOFF_SDK_SURFACES.md` | `synth-ai/HANDOFF_SDK_SURFACES.md` |
| `references/CODEX_ACTIVITY_STREAM_HANDOFF.md` | `backend/CODEX_ACTIVITY_STREAM_HANDOFF.md` |
| `references/INTERN_ASYNC_RUNTIME_PHASE_REFACTOR_HANDOFF_2026-08-08.md` | backend |
| `references/INTERN_ASYNC_LIVENESS_HANDOFF_2026-08-08.md` | backend |
| `references/INTERN_SYNC_FRONTEND_TEST_HANDOFF_2026-08-06.md` | backend |
| `references/understudy_README.md` | `understudy/README.md` |

---

## Intentionally excluded

| Exclusion | Reason |
|-----------|--------|
| Full `backend` / `frontend` / `synth-ai` trees | Size; eng should clone siblings |
| `INTERN_24_7_*` burn/debug round handoffs | Noise; phase/liveness trio is enough |
| Training / optimizer / Magi depth | Out of V1 wedge |
| Electron template app | Greenfield — eng creates in M0 |
| Laguna MLX weights / inference code | Not in these repos; separate track |

---

## Top 10 files to read first (inside zip)

1. `docs/01-ENG-BRIEF.md`
2. `docs/02-REUSE-GAP-MATRIX.md`
3. `docs/03-V1-ARCHITECTURE.md`
4. `docs/04-IMPLEMENTATION-SEQUENCE.md`
5. `docs/05-API-CHEATSHEET.md`
6. `plans/intern_interaction_boundaries.md`
7. `excerpts/frontend/researchIntern.ts`
8. `excerpts/backend/runtime_events.py`
9. `excerpts/sdk/research_intern_contracts.py`
10. `docs/00-PRODUCT-HANDOFF.md` (Appendix: narrowed V1)
