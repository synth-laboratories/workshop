# Handoff: First-class Containers + Craftax Rust dogfood

**Date:** 2026-08-08  
**Audience:** Engineer shipping a thin Container Registry slice in `apps/synth_desktop`  
**Status:** Complete. Register/hydrate, Inventory inspector, embedded-agent discovery MCP + skill, and the loopback-only two-rollout acceptance aggregate are implemented. The canonical installed app passed the live native-Laguna/Codex gate on 2026-08-09. Chat `container_ref` chip remains follow-up polish.
**Contract / product plan:** [`containers.md`](./containers.md) (read first)  
**Before screenshot:** [`refs/inventory-containers-empty.png`](./refs/inventory-containers-empty.png) — Inventory → Containers **0** · “No containers yet.”

---

## 0. One-liner

> Spin up **GameBench Craftax Rust** on `:8098`, **register** that URL into Desktop’s container vault, hydrate `/health` + `/info`, show it in Inventory (and a minimal inspector), then **CUA-dogfood** that an agent can find the container and the right metadata shows up.

Out of scope for this handoff: full visuals live scrub, SSE, synth-containers GEPA `/dataset`/`/program`, chat `container_ref` polish beyond a thin chip if time allows, port scanning.

---

## 1. Starting point (honest)

| Piece | Today |
| --- | --- |
| Inventory · Containers tab | Renders list; empty state as in screenshot |
| Rust | `inventory_containers_list` / `_get` / `_probe` (`GET {baseUrl}/health` only) |
| Bridge | `window.synthInventory` — list / get / probe / traces / usage / counts — **no register** |
| Seed | Demo used to point at `:8100` + fake `synth-containers` — do **not** rely on that; use real `:8098` |
| Discovery | **Register only** (see `containers.md` § Discovery). No LAN scan |

---

## 2. Phase A — Spin up Craftax Rust (local)

Repo (sibling checkout): `~/Documents/GitHub/gamebench`  
Task: `tasks/craftax-singleplayer/`

```bash
cd ~/Documents/GitHub/gamebench/tasks/craftax-singleplayer

# build once
cargo build --release --manifest-path gold_rust/Cargo.toml

# serve (default Rust port 8098)
python3 scripts/run_service.py --lane rust --port 8098
# or:
# cargo run --release --manifest-path gold_rust/Cargo.toml --bin craftax_gold -- --host 127.0.0.1 --port 8098
```

Smoke (another terminal):

```bash
curl -s http://127.0.0.1:8098/health | jq .
curl -s http://127.0.0.1:8098/info | jq .
```

Expect roughly:

- `/health`: `ok`, `lane: "rust"`, `env_family: "craftax-singleplayer"`, `sessions`, `replay_enabled`
- `/info`: `capabilities` (rollout, render_png, …), `action_names`, `glyph_legend`

Leave this process running for Desktop dogfood. Docs: `HANDOFF_RUST.md`, `shared/http_contract.md` in that task.

---

## 3. Phase B — Basic first-class containers (minimal slice)

Goal: Inventory shows **one real row** for Craftax Rust with **correct hydrated info**, Probe updates status, Refresh works.

### B1. Register API (Rust + bridge)

Add (names can match existing style):

- `inventory_containers_upsert` / `register` — body: `{ name?, baseUrl, location?: "local", taskFamily?, metadata? }`
- Persist to `containers` table (see `inventory.rs` / migrations)
- On upsert: fetch **`GET {baseUrl}/health`** and **`GET {baseUrl}/info`** (fallback `/metadata` if `/info` 404)
- Store:
  - `status` from health (`ready` / `unhealthy`)
  - `health` JSON (raw health + ok flag)
  - `metadata` merge: `{ contractHint, info: <info payload>, hydratedAt }`
  - `taskFamily` from info `env_family` when present (`craftax-singleplayer`)
- Extend **Probe** to re-fetch health **and** info (same hydrate), not health-only
- Optional: journal `container.registered` / `container.probed` via CoreRuntime if cheap

Wire `window.synthInventory.registerContainer(...)` (and types in `env.d.ts`).

### B2. UI — Attach + show info

On Inventory · Containers (empty state and header):

- **Attach container** control: URL default `http://127.0.0.1:8098`, name default `Craftax Rust`, call register, refresh list
- Row should show at least: **name**, **location**, **status**, **taskFamily**, **baseUrl** (or short host:port)
- Row expand / detail panel (keep simple): pretty-print cached `/info` capabilities (and health). No full Tasks tab required unless `/task_catalog` exists (Craftax won’t)
- Probe button already exists — must refresh the hydrated info after Probe
- `data-testid`s: keep `inventory-containers`, `inventory-container-{id}`, `probe-container-{id}`; add `attach-container`, `container-info` (or similar) for CUA/Playwright

### B3. Do not

- Do not port-scan to “discover” containers
- Do not claim synth-containers `/task_catalog` / `/task_info` for Craftax (only `/info`)
- Do not block on live `render.png` visual — follow-up

---

## 4. Phase C — Dogfood via CUA

With Desktop running (`npm run dev --workspace @synth/synth-desktop` or packaged app) **and** Craftax on `:8098`:

### CUA script (give the agent this)

1. Confirm Craftax is up (or tell the human to start Phase A).
2. Open Synth Desktop → sidebar **Inventory** (Containers · Traces · Usage).
3. Confirm starting state: Containers **0** / “No containers yet” (matches `refs/inventory-containers-empty.png`) **or** clear old demo rows if present.
4. Use **Attach container** → `http://127.0.0.1:8098` → submit.
5. Assert list shows **1** container: name mentions Craftax/Rust, status **ready**, task family / meta mentions `craftax-singleplayer` or lane rust.
6. Open detail / expand → assert `/info`-derived fields visible (capabilities include something like `render_png` or `rollout`; lane rust if shown).
7. Click **Probe** → still **ready**; `hydratedAt` / updated time moves if shown.
8. Stop Craftax process → Probe → status **unhealthy** (or equivalent).
9. Restart Craftax → Probe → **ready** again.
10. Screenshot Inventory with the live row for the PR / handback.

### Pass / fail

| Check | Pass |
| --- | --- |
| Found after attach | Containers count ≥ 1, not empty copy |
| Right info | Hydrated from live `:8098` `/info`, not hardcoded fake digest-only demo |
| Probe up/down | Reflects process lifecycle |
| No scan | Attach/register path only |

Optional Playwright smoke mirroring steps 4–7 with a mock HTTP server if CUA env lacks GameBench — still run real CUA once against `:8098`.

---

## 5. Suggested file touch list

| Area | Paths |
| --- | --- |
| Registry | `src-tauri/src/inventory.rs`, `lib.rs` commands |
| Bridge / types | `src/renderer/src/runtime/desktopBridge.ts`, `env.d.ts` |
| UI | `src/renderer/src/components/InventoryPage.tsx`, CSS if needed |
| Tests | `tests/playwright/` — attach + probe happy path |
| Docs | Update `containers.md` acceptance checkboxes when done |

---

## 6. Acceptance checklist

- [x] Craftax Rust serves `:8098` (`/health` + `/info` OK)
- [x] `registerContainer` persists + hydrates info into SQLite
- [x] Inventory Attach and MCP register → row appears with correct family/status/url
- [x] Probe refreshes health + info; unit/renderer coverage exercises status refresh
- [x] Real low/high LLM policy rollouts seal as canonical Trace V5 and open in the Desktop inspector
- [x] `containers.md` Craftax acceptance items updated

### 2026-08-09 policy-rollout acceptance evidence

The former Workshop-owned two-session acceptance shortcut was removed. The
authoritative acceptance path now runs real low/high Luna policies through the
workspace benchmark harness, promotes their calls/actions/rewards/usage into
canonical Trace V5, and opens the digest-bound rollout inspector in Desktop.

---

## 7. Related

- [`containers.md`](./containers.md) — endpoints, discovery lock, task_info cache (later), Craftax route table  
- [`HANDOFF_CONTAINERS_CRAFTAX.md`](./HANDOFF_CONTAINERS_CRAFTAX.md) — this slice  
- GameBench: `tasks/craftax-singleplayer/HANDOFF_RUST.md`, `shared/http_contract.md`  
- Protocol: `ContainerDeployment` in `packages/runtime-protocol`  
- Later: chat chip, `craftax.rollout_scrub.v1` live bindings, dataset viewer for full synth-containers
