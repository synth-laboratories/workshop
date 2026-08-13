# Synth Desktop — Real App Handoff (v0)

**Date:** 2026-08-08  
**Location:** `workshop/apps/synth_desktop`  
**Status:** v0 ready for review — **real app**, not the mock  
**Mock (fixtures only):** `workshop/apps/mock` — labeled **MOCK** in the titlebar

---

## 0. One-liner

> Tauri is the secure desktop host. The Python **local-runtime daemon** routes every session to either Codex app-server or Synth Intern/cloud and owns persistence, normalized events, inventory, and usage. Deep visual templates live in `workshop/visuals`.

---

## 1. Run

```bash
cd ~/Documents/GitHub/workshop
# optional: export SYNTH_API_KEY=... && SYNTH_INTERN_DEMO=0
npm run dev --workspace @synth/synth-desktop
# or: npm run dev:desktop
```

Optional local inference boundary (OpenAI-compatible; mock mode without mlx-vlm):

```bash
npm run dev:inference   # http://127.0.0.1:7332
# Tauri auto-probes the configured Laguna Responses server
```

| Piece | Location |
|-------|----------|
| Real Tauri app | `apps/synth_desktop` |
| Daemon | `services/local-runtime` |
| Local Laguna OpenAI proxy | `services/local-inference` |
| Deep visuals | `visuals/` (9 templates, registry, MCP, save-as-TSX) |
| Protocol / client | `packages/runtime-protocol`, `packages/runtime-client` |
| Fixture mock | `apps/mock` (titlebar **MOCK**) |

Daemon data: `~/.synth-desktop/runtime/`. Quitting Tauri does **not** stop the daemon.

---

## 2. What shipped (v0 dogfood — PASS)

- Local Laguna XS 2.1 through Codex app-server and `/v1/responses`
- Configured Responses-compatible model providers through Codex app-server
- Inventory seed (2 containers, 3 traces, 9 templates)
- Visual create + save TSX + live eval visual
- Intern sync demo
- Tauri host supervises/probes the daemon; UI polls `/v1/health` + sessions
- `npm test` green (14 runtime + 3 visuals + 4 a11y)

---

## 3. Architecture law

```text
Tauri (synth_desktop)
    ├── Rust Codex manager → app-server → Responses provider
    │                                      └── Laguna shim → MLX sidecar
    ├── Rust PTY manager → user terminal sessions
    └── compatibility runtime, started lazily
         ├── Intern adapter (demo | remote with SYNTH_API_KEY)
         └── Inventory: containers · Trace V5 CAS · visuals catalog
```

Every session is either `intern` or `codex`. There is no normal-chat adapter or
direct model fallback. Do **not** call Intern, model-provider, MLX, or Codex HTTP
from the renderer. Do **not** put orchestration in Tauri. The binding system
architecture is [`../../architecture.md`](../../architecture.md).

---

## 4. Honest gaps → next pass

1. **Local Laguna requires its Responses server** at `SYNTH_LAGUNA_BASE_URL`. Codex app-server is the agent; Laguna/MLX is the model provider. The desktop never falls back to direct chat.
   - Path: `services/laguna-daemon` → MLX sidecar.
   - Tauri probes the configured server and reports readiness explicitly.
2. **Real Intern** needs `SYNTH_API_KEY` + `SYNTH_INTERN_DEMO=0` against a synth-dev slot.
3. **Native WebDriver goldens** are not yet included; renderer Playwright and `window.__synthEval` cover deterministic layout and semantics.
4. Permission cards / richer Poolside activity parity still lean on mock learnings — port carefully with runtime events.
5. Titlebar pill reports Codex/Laguna Responses readiness, configured providers, Intern mode, and inventory counts.
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
npm run dogfood:models   # Codex/Laguna + configured providers + inventory + visuals
```

---

## 7. Edit map

| Want… | Edit… |
|-------|-------|
| Window / daemon spawn / Laguna probe | `apps/synth_desktop/src-tauri/src/` |
| UI shell | `apps/synth_desktop/src/renderer/src/App.tsx` |
| Runtime HTTP API | `services/local-runtime/src/synth_local_runtime/` |
| Laguna Responses → MLX shim | `services/laguna-daemon/` |
| Visual templates | `visuals/templates/` |
| Protocol types | `packages/runtime-protocol/` |

---

## 8. Distinguish mock vs real

| | Mock | Real |
|--|------|------|
| Path | `apps/mock` | `apps/synth_desktop` |
| Titlebar | **MOCK** badge | Runtime pill (Codex · Laguna/Responses · provider · Intern) |
| Window title | `Synth MOCK` | `Synth Desktop` |
| Data | Fixtures only | Daemon + SQLite + visuals |
| Run | `npm run dev:mock` | `npm run dev:desktop` |
