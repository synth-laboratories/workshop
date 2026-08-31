# Containers — first-class registry, chat, visuals

Status: **First-class local registry and Craftax agent workflow shipped; richer task catalogs and chat chips remain follow-ups.**  
Source of truth for the wire contract: [`synth-containers` OpenAPI](https://github.com/synth-laboratories/containers/blob/main/openapi/container-contract-v1.yaml) (`container-contract-v1.yaml` v0.2.0).  
Package: PyPI `synth-containers` · repo [`containers`](https://github.com/synth-laboratories/containers).

Desktop should treat a container as a durable registry record (URL + hydrated metadata), show it in chat, probe “still up,” and bind visual templates (dataset viewer, live rollouts, eval matrix) to that handle.

**First dogfood:** GameBench Craftax Rust gold lane on `:8098` (see §4). That service is a **related interactive gold HTTP** surface, not a full synth-containers optimizer container yet.

---

## 1. synth-containers endpoints (required vs optional)

Authority: OpenAPI + package README + containers skill. Advertise only what you implement; capability metadata must be conservative.

### Required (core optimizer / eval consumers)

| Route | Method | Required? | Notes |
| --- | --- | --- | --- |
| `/health` | GET | **Required** | Liveness / readiness. Prefer 200 only after assets/datasets are usable. Payload often `{ status, contract_version, … }`. |
| `/metadata` | GET | **Required** | Contract version, route inventory, runtime kind, task id, capabilities, `optimizer_contracts` when GEPA-ready. Schema: `InfoResponse` (`runtime`, `capabilities`, `metadata`, …). |
| `/info` | GET | **Required (alias)** | **Same payload as `/metadata`.** Prefer probing `/info` or `/metadata` interchangeably when hydrating Inventory; both are first-class in OpenAPI. |
| `/rollout` | POST | **Required** | Blocking (or async-accept) rollout: candidate × row/seed → reward + usage (+ artifacts). Body: `RolloutRequest` (see below). Response must include a numeric reward via `reward`, `summary.outcome_reward`, or `reward_info.outcome_reward`. |

### Strongly expected for GEPA / public cookbooks

These are required for GEPA when `metadata.optimizer_contracts.gepa` is advertised; otherwise a container may omit them and return 404.

| Route | Method | Options / body | Notes |
| --- | --- | --- | --- |
| `/program` | GET | — | Prompt program. GEPA expects: `version`, `program_id`, `modules`, `target_modules`, `seed_candidate`. |
| `/dataset` | GET | — | Split names, counts, seed/sampling notes. |
| `/dataset/rows` | POST | Body optional in OpenAPI; GEPA sends split + seeds | Concrete rows; each row needs stable `example_id` or `id`. N seeds → N rows or fail. |
| `/task_info` | GET | Query **optional**: `seed`, `split_group`, `family`, `task_instance_id`, `task_id` | Rich task description for agents/proposers (objective, output contract, metrics, constraints). |

### Optional discovery / catalog

| Route | Method | Options | Notes |
| --- | --- | --- | --- |
| `/` | GET | — | Root discovery (health + metadata + task hints). |
| `/task_catalog` | GET | — | Multi-task catalogs / instances. |
| `/compatibility` | GET | Query **optional**: `target` (e.g. `go_ex`, `mipro`, `token_rl`) | Consumer compatibility matrix or target report. |

### Optional async / interactive lifecycle

Only claim these in capabilities when real. Useful for live visuals / long-horizon envs:

| Route | Method | Notes |
| --- | --- | --- |
| `/rollouts` | POST | Alias for submit (sync 200 or async 202). |
| `/rollouts/{id}` | GET | Poll lifecycle / final result. |
| `/rollouts/{id}/state` | GET | Control + progress + env_state. |
| `/rollouts/{id}/summary` | GET | Summary metrics. |
| `/rollouts/{id}/usage` | GET | Token / cost usage. |
| `/rollouts/{id}/artifacts` | GET | Artifacts list. |
| `/rollouts/{id}/events` | GET | Emitted events (pull; **not** SSE in the base contract). |
| `/rollouts/{id}/trace` | GET | Trace payload. |
| `/rollouts/{id}/checkpoints` | GET/POST | List / create checkpoints. |
| `/rollouts/{id}/checkpoints/{checkpoint_id}` | GET | Fetch checkpoint. |
| `/checkpoints`, `/checkpoints/{id}`, `…/labels` | GET/POST | Global checkpoint discovery. |
| `/rollouts/{id}/resume` | POST | Resume from checkpoint. |
| `/rollouts/{id}/fork` | POST | Branch. |
| `/rollouts/{id}/pause` | POST | Pause. |
| `/rollouts/{id}/terminate` | POST | Terminate. |

**Not in the base OpenAPI today:** SSE (`text/event-stream`), dedicated `/reward`, or frame/`render.png` streaming. Live game frames are a **GameBench / env-specific** extension (see §4) until folded into capabilities + optional routes.

### `POST /rollout` request (OpenAPI `RolloutRequest`)

Body **required** as a JSON object; almost all fields are optional on the wire — task containers document which they need.

| Field | Required? | Role |
| --- | --- | --- |
| `rollout_id` | optional | Client-supplied id; else server generates. |
| `trace_correlation_id`, `trial_id`, `run_id` | optional | Correlation. |
| `mode`, `submission_mode` | optional | Sync vs async accept. |
| `env` | optional | `{ config, seed, … }`. |
| `policy` | optional | Typed OpenAI-compatible target; if present, **`provider` + `model` required**. Credentials via `credential_mode`, never raw keys. |
| `candidate` / `candidate_overlay` | optional | GEPA prompt field overlays. |
| `dataset_row` | optional | Concrete row after `/dataset/rows`. |
| `dataset` | optional | `{ path, split, limit, config }`. |
| `task_id`, `task_instance_id`, `task_payload`, `task_metadata` | optional | Task selection. |
| `checkpoint`, `checkpoint_id`, `checkpoint_data_base64` | optional | Resume/fork inputs. |
| `actors`, `actor_ids`, `actor_overrides` | optional | Multi-actor; actor entries need `actor_id`. |
| `metadata` | optional | Free-form. |

### `POST /rollout` response (minimum)

OpenAPI requires `trace_correlation_id` + `rollout_id`. Consumers also require a numeric **reward** (see GEPA contract). Prefer also: status, summary/scores, usage, artifacts/trace refs when available.

### GEPA advertisement (in `/metadata` or `/info`)

```json
{
  "metadata": {
    "optimizer_contracts": {
      "gepa": {
        "version": "synth_optimizers.gepa.v1",
        "program_route": "/program",
        "dataset_route": "/dataset",
        "dataset_rows_route": "/dataset/rows",
        "rollout_route": "/rollout"
      }
    }
  }
}
```

(OpenAPI notes GEPA sub-contract bump to `synth_optimizers.gepa.v2` with typed `RolloutPolicySpec`.)

---

## 2. Desktop product model

Same pattern as visuals: **register → durable record → chat chip → pane → templates**.

```
Agent / Attach UI
  → containers.register({ baseUrl, name?, sessionId? })
  → CoreRuntime Container Registry (SQLite + journal)
  → hydrate: GET /health + GET /info|/metadata (+ /task_info, /dataset when present)
  → chat: container_ref (“Craftax · ready · probed 12s ago”)
  → open inspector / “Open as visual”
  → visuals.create(template, bindings: { containerId, baseUrl, rolloutId? })
```

### Discovery (locked)

**Registration is authority — not scanning.**

| Mode | Role |
| --- | --- |
| **Register** (agent MCP, Attach UI, policy runner, seed) | Source of truth: `containers.register({ baseUrl, … })` then hydrate that URL |
| **Probe / hydrate** | Verify one registered endpoint: `/health` + `/info`\|`/metadata` (not a LAN sweep) |
| **Optional assist** | Soft suggest only (“`:8098` looks healthy / Craftax-like”) → user/agent still Attaches. Never invent inventory rows from a port scan |

Do **not** scan all local ports or “every HTTP service” looking for `/metadata`. Ambiguous (GameBench `/info` ≠ full synth-containers `InfoResponse`), noisy, bad default.

**Still up?** Probe `/health` (and refresh `/info` on open). Inventory Probe today only hits `/health` — extend hydrate.

### Tasks + `task_info` in Desktop (fetch + cache)

Yes for containers that expose the contract. Desktop does **not** invent the task list; it **hits the container and caches**.

| UI | Fetch | Cache |
| --- | --- | --- |
| Task list / families | `GET /task_catalog` (or catalog-ish fields in `/info`/`/metadata` when that’s all there is) | On the container registry row: snapshot JSON + `fetchedAt` |
| Per-task detail | `GET /task_info?task_id=…` (optional `seed`, `family`, `task_instance_id`, `split_group`) | Per `(containerId, task_id[, instance])` cached blob |
| Dataset splits / rows | `GET /dataset` · `POST /dataset/rows` | Always cache split metadata; fetch rows on demand (heavier) |

**UX flow:** open container inspector → if capabilities advertise catalog / task_info → show Tasks → select task → fetch `/task_info` → render. Gate the Tasks UI on hydrate capabilities; no empty tab when routes are absent.

**Cache policy:**

- Durable on the registry (SQLite / CAS pointer): last good `/info`|`/metadata`, `/task_catalog`, per-task `/task_info`.
- Invalidate or stale-while-revalidate on Probe, manual Refresh, or focus after TTL.
- Cache is **discovery/docs only** — not live rollout state (reward, frames, events stay on rollout routes).

**Craftax / GameBench caveat:** Rust gold today has `/info`, not `/task_catalog` or `/task_info`. Dogfood panel = thin env view from `/info` (+ local task fixtures if needed) until a catalog shim exists. Don’t treat GameBench `/info` as a full task catalog.

**Downstream visuals (bindings, not a second store):**

| Template | Reads |
| --- | --- |
| Dataset viewer | `/dataset`, `/dataset/rows` (or GameBench task fixtures) |
| Live / scrub rollout | interactive rollout routes + frames (GameBench) or `/rollouts/{id}/…` |
| Eval matrix / live evals | batched `/rollout` or policy sweeps |
| Reward / Pareto | traces + container metadata |

---

## 3. What Desktop has today

| Surface | Status |
| --- | --- |
| Inventory · Containers | List rows from SQLite; **Probe** = `GET {baseUrl}/health` only |
| Seed demo | `craftax-local` → `http://127.0.0.1:8100` (placeholder; not GameBench `:8098`) |
| Chat `container_ref` | Follow-up; MCP tool calls are visible in chat today |
| MCP registry | Register/list/get/probe only; policy execution belongs to the coding agent and benchmark harness |
| Visual bindings | Bind outputs emitted by the real harness; Desktop does not manufacture rollout projections |
| Rust inventory | register/list/get/probe hydration + last rollout tracking |
| Python inventory | upsert + seed (migration path) |

---

## 4. First example — GameBench Craftax Rust (`:8098`)

Repo: `gamebench/tasks/craftax-singleplayer/` · binary `craftax_gold` · port **8098** (Python gold **8097**).

Bring-up:

```bash
cd ~/Documents/GitHub/gamebench/tasks/craftax-singleplayer
python3 scripts/run_service.py --lane rust --port 8098
# or: cargo run --release --bin craftax_gold -- --host 127.0.0.1 --port 8098
```

Policies already use **register-by-URL**:  
`run_policy_sweep.py --lane rust --base-url http://127.0.0.1:8098`.

### GameBench routes (actual)

| Route | Role |
| --- | --- |
| `GET /health` | ok, lane, env_family, sessions, replay_enabled |
| `GET /info` | capabilities, action_names, glyph_legend (hydrate helper — **not** full synth-containers `InfoResponse`) |
| `POST /rollouts` / `/reset` | create rollout `{ task?, seed? }` |
| `POST /rollouts/{id}/step` | action → payload with **`reward`**, readout progress, terminated/truncated |
| `GET /rollouts/{id}/readout` | symbolic observation / ASCII |
| `GET /rollouts/{id}/event_log` | NEV events (pull; **no SSE**) |
| `GET /rollouts/{id}/render.png` | **current** game frame (sprites or symbolic RGB) |
| `GET /rollouts/{id}/render.svg` | ASCII SVG |
| `GET /rollouts/{id}/frames/manifest` | captured frame index |
| `GET /rollouts/{id}/frames/{step}.png` | per-step PNG |
| `GET /rollouts/{id}/replay.gif` | ffmpeg GIF |
| checkpoint / restore / simulate | present |

**Gaps vs synth-containers:** no `/metadata` twin with optimizer_contracts, no `/dataset` / `/program` / blocking `POST /rollout` GEPA shape, no SSE. Reward is on step/create payload, not `/reward`.

**Live visual v1 (no GameBench changes):** poll `readout` + `render.png` (or frames manifest) after steps; cache `reward` from last step payload; enable `--replay` + task `stream` gates for automatic frame capture.

**Nice later on gold service:** SSE event stream, bundled `rollout_info`, thin `/metadata` shim for Inventory hydrate.

### Dogfood loop

1. Start Craftax Rust on `:8098`.
2. Register in Desktop → chip in chat + Inventory row.
3. Create rollout / run a few steps (agent or “Run seed” action).
4. Open `craftax.rollout_scrub.v1` bound to `{ containerId, rolloutId }` → live PNG + text + reward.
5. Optional: seed batch → dataset-style viewer; sweep → eval matrix.

Replace demo `:8100` seed with this real endpoint (or dual-seed and label clearly).

---

## 5. Implementation plan (when building)

1. **Container Registry (Rust)** — upsert/list/get; hydrate `/health` + `/info|/metadata`; cache `/task_catalog` + per-task `/task_info` (+ `/dataset` metadata); store capability snapshot + `lastRolloutId`.
2. **MCP / bridge** — `containers_register`, `containers_probe`, `containers_show` (journal + chat ref).
3. **UI** — chat chip + inspector (Tasks gated on capabilities; cached task_info detail); Inventory as vault.
4. **Visual templates** — live bindings for Craftax scrub first; dataset viewer next for full synth-containers; eval matrix on sweeps.
5. **Tests** — Playwright: register mock baseUrl → probe → chip; optional GameBench smoke when `:8098` up.

### Acceptance (Craftax slice)

- [x] Register `http://127.0.0.1:8098` → Inventory shows Craftax Rust with hydrated `/info`.
- [x] Probe refreshes readiness and `/info` metadata.
- [ ] Chat shows container chip; open inspector.
- [x] Visual scrub shows real per-step symbolic state, readout, action, vitals, inventory, achievements, and reward.
- [x] Live eval matrix is derived from the same two real rollouts.
- [x] Demo `:8100` placeholder retired from the first-class attach path.

---

## 6. Related docs

- `apps/synth_desktop/local_lora.md` — LoRA wiring (separate).
- `synth_desktop_research_eng.md` §5 — high-level container/rollout intent.
- `visuals/templates/craftax.rollout_scrub.v1/`, `craftax.eval_matrix.v1/` — fixture-backed templates to live-bind.
- External: `containers/openapi/container-contract-v1.yaml`, `gamebench/.../shared/http_contract.md`, `HANDOFF_RUST.md`.
