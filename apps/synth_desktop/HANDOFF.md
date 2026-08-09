# Synth Desktop — Real App Handoff (v0)

**Date:** 2026-08-08  
**Location:** `workshop/apps/synth_desktop`  
**Status:** v0 ready for review — **real app**, not the mock  
**Mock (fixtures only):** `workshop/apps/mock` — labeled **MOCK** in the titlebar

---

## 0. One-liner

> Electron is the viewer. The Python **local-runtime daemon** owns sessions, runs, events, Laguna/OpenRouter/Intern adapters, inventory (containers / Trace V5 / visuals), and usage. Deep visual templates live in `workshop/visuals`.

---

## 1. Run

```bash
cd ~/Documents/GitHub/workshop
export OPENROUTER_API_KEY=...          # Luna / Laguna S 2.1
# optional: export SYNTH_API_KEY=... && SYNTH_INTERN_DEMO=0
npm run dev --workspace @synth/synth-desktop
# or: npm run dev:desktop
```

Optional local inference boundary (OpenAI-compatible; mock mode without mlx-vlm):

```bash
npm run dev:inference   # http://127.0.0.1:7332
# Electron auto-probes :7332 and sets SYNTH_LAGUNA_BASE_URL for the daemon when healthy
```

| Piece | Location |
|-------|----------|
| Real Electron app | `apps/synth_desktop` |
| Daemon | `services/local-runtime` |
| Local Laguna OpenAI proxy | `services/local-inference` |
| Deep visuals | `visuals/` (9 templates, registry, MCP, save-as-TSX) |
| Protocol / client | `packages/runtime-protocol`, `packages/runtime-client` |
| Fixture mock | `apps/mock` (titlebar **MOCK**) |

Daemon data: `~/.synth-desktop/runtime/`. Quitting Electron does **not** stop the daemon.

---

## 2. What shipped (v0 dogfood — PASS)

- Local Laguna XS 2.1 stream (**stub** by default; HF weights detected under `~/.cache/.../Laguna-XS-2.1`)
- OpenRouter Luna (`moonshotai/kimi-k2.5`)
- OpenRouter Laguna S 2.1 (`poolside/laguna-s-2.1`)
- Inventory seed (2 containers, 3 traces, 9 templates)
- Visual create + save TSX + live dock visual
- Intern sync demo
- Electron preview spawns daemon; UI polls `/v1/health` + sessions
- `npm test` green (14 runtime + 3 visuals + 4 a11y)

---

## 3. Architecture law

```text
Electron (synth_desktop)
    │ IPC → localhost runtime
    ▼
local-runtime daemon
    ├── Laguna adapter → stub OR SYNTH_LAGUNA_BASE_URL (local-inference / MLX)
    ├── OpenRouter adapter + local usage ledger
    ├── Intern adapter (demo | remote with SYNTH_API_KEY)
    └── Inventory: containers · Trace V5 CAS · visuals catalog
```

Do **not** call Intern HTTP from the renderer. Do **not** put orchestration in Electron.

---

## 4. Honest gaps → next pass

1. **Local Laguna is stub** until an MLX OpenAI server is up (`SYNTH_LAGUNA_BASE_URL`). Weights may be on disk; `mlx_lm` / `mlx-vlm` may not be installed.  
   - Path: `services/local-inference` (`SYNTH_INFERENCE_MODE=mlx`, optional deps).  
   - Electron now **auto-probes** `http://127.0.0.1:7332/health` when spawning the daemon.
2. **Real Intern** needs `SYNTH_API_KEY` + `SYNTH_INTERN_DEMO=0` against a synth-dev slot.
3. **Playwright Electron goldens** not yet; a11y is testid/surface + `window.__synthEval`.
4. Permission cards / richer Poolside activity parity still lean on mock learnings — port carefully with runtime events.
5. Titlebar pill now shows `Laguna·stub` / `Laguna·MLX`, OpenRouter, Intern mode, inventory counts.
6. **Local slot + Sync/Async depth** — see [`HANDOFF_INTERN_LOCAL_SLOT.md`](./HANDOFF_INTERN_LOCAL_SLOT.md) (mailbox vs slot vs Local Pilot; build order).

---

## 5. Product IA (from pin-down)

- Sidebar: **Chats/** (local) + **Cloud/** (Intern sync sessions + pinned Async) + **Inventory**
- Visuals: Artifacts-style; click chip / rail to toggle pane; Craftax-class templates in `visuals/`
- LoRAs first-class (picker + Settings → Finetunes) — wire through daemon when adapters exist
- Full product thesis: `workshop/HANDOFF.md`, mock UX notes: `apps/mock/HANDOFF.md`

---

## 6. Tests

```bash
npm test                 # runtime + visuals + a11y
npm run dogfood:models   # Laguna stub + OpenRouter + inventory + visuals
```

---

## 7. Edit map

| Want… | Edit… |
|-------|-------|
| Window / daemon spawn / Laguna probe | `apps/synth_desktop/src/main/index.ts` |
| UI shell | `apps/synth_desktop/src/renderer/src/App.tsx` |
| Runtime HTTP API | `services/local-runtime/src/synth_local_runtime/` |
| Laguna stub → MLX proxy | `services/local-inference/synth_inference/` |
| Visual templates | `visuals/templates/` |
| Protocol types | `packages/runtime-protocol/` |

---

## 8. Distinguish mock vs real

| | Mock | Real |
|--|------|------|
| Path | `apps/mock` | `apps/synth_desktop` |
| Titlebar | **MOCK** badge | Runtime pill (`Laguna·stub` / `Laguna·MLX` · OR · Intern) |
| Window title | `Synth MOCK` | `Synth Desktop` |
| Data | Fixtures only | Daemon + SQLite + visuals |
| Run | `npm run dev:mock` | `npm run dev:desktop` |
