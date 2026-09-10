# Handoff: Trace V5 live correlation (v0.1 launch blocker)

**Date:** 2026-08-10  
**Status:** Open — `LIVE-TRACE-CORRELATION` is hardcoded `fail` / blocking  
**Audience:** Engineer closing the Container → Visual → Trace V5 join for `gate:local` / `gate:release`  
**Related:**  
[`HANDOFF_TRACES_V5.md`](./HANDOFF_TRACES_V5.md) · [`HANDOFF_CONTAINERS_CRAFTAX.md`](./HANDOFF_CONTAINERS_CRAFTAX.md) · [`containers.md`](./containers.md) · [`EVAL_DRIVER.md`](./EVAL_DRIVER.md) · [`V0P1_RELEASE_PREP_NOTES.md`](./V0P1_RELEASE_PREP_NOTES.md) · [`LAUNCH_V0P1_RELEASE_PREP_HANDOFF.md`](../../LAUNCH_V0P1_RELEASE_PREP_HANDOFF.md) · evals `runner/live.ts`

---

## 0. One-liner

> Prove that **one real Craftax rollout** and its **model/tool events** share a single identity (seed / rollout / run / digest), then expose that join on the eval-driver so the launch gate can grade it — not a fixture, not a constant pass.

Without this, Workshop can show a Container, a Visual, and a Trace that look related but are **cross-bound**. That is a hard **NO-GO** for v0.1.

---

## 1. End-to-end picture

```text
                    LIVE PATH (must be proven)
  ┌─────────────────────────────────────────────────────────────────────┐
  │  Craftax GameBench rust façade :8080                                │
  │    /health (gold_ok) · POST /rollouts + policy_ref · /events        │
  │                              │                                      │
  │                              ▼                                      │
  │  Eval-driver (debug instance only)                                  │
  │    /v1/containers/.../rollouts | policy_rollouts | open_visual      │
  │    ★ NEW: correlation evidence route / richer rollout payload       │
  │                              │                                      │
  │              ┌───────────────┼───────────────┐                      │
  │              ▼               ▼               ▼                      │
  │         Container       Visual pane      Trace V5 vault             │
  │         inspector       (live scrub)     (sealed digest)            │
  │              │               │               │                      │
  │              └───────────────┴───────────────┘                      │
  │                        same seed · rolloutId · runId                │
  │                        obs + action + reward + frame + model event  │
  └─────────────────────────────────────────────────────────────────────┘

  LAYER MAP (do not blur authorities)
  ───────────────────────────────────
  Seal / package     synth-containers (+ Harbor for bench trajectories)
  Store / query     Desktop Rust inventory vault (CAS + SQLite)
  Assets             sealed events, frames, evidence, projections
  Visuals            templates bound to digest / live container binding
  Gate proof         evals `LIVE-TRACE-CORRELATION` via eval-driver
```

**Desktop is a consumer.** It must not invent readiness or rewrite Harbor/Craftax trajectories. The gate grades the **join**, not pretty UI alone.

---

## 2. What already exists

| Piece | Where | State |
| --- | --- | --- |
| Craftax register / catalog / two scripted rollouts | evals `live.ts` + driver `/v1/containers/.../rollouts` | Graded (`LIVE-CRAFTAX-*`) |
| Open live visual | driver `/v1/open_visual` → `live.container_rollouts.v1` | Graded (`LIVE-VISUAL-BINDING`) |
| Policy rollout with actions + event_log fetch | `eval_driver.rs` `run_policy_rollout` | Builds `actions`, `eventLog`, rewards, etc. in the **response JSON** — not yet a correlation API for the gate |
| Trace V5 ingest / vault / rollout-inspector visual | `inventory.rs`, `trace_ingest.rs`, templates | Local store+view path dogfooded |
| GameBench routes | `:8098` readout, event_log, frames, render.png | Real sources of obs/frame/reward |
| Manual CUA-018 | `CUA_MANUAL_GATE.md` | Same correlation requirement for humans |

### The hardcoded red (do not leave as-is)

```112:112:../../evals/workshop/runner/live.ts
checks.push({ id: "LIVE-TRACE-CORRELATION", title: "Rollout trace correlation evidence", class: "product", status: "fail", blocking: true, detail: "Current eval-driver contract exposes rollout records and visuals but no route proving observation/action/reward/frame/model-event Trace V5 correlation. This remains a launch blocker." });
```

After `LIVE-CRAFTAX-ROLLOUTS` and `LIVE-VISUAL-BINDING`, the gate **always** fails correlation. Replacing this with `status: "pass"` without evidence is forbidden.

---

## 3. Outcome required

For **at least one** live Craftax path (prefer a short policy or instrumented scripted rollout that still produces model/tool events — or explicitly attach Codex/session tool events if the scripted path has none):

| Fact | Must match |
| --- | --- |
| `seed` | Task instance / create body |
| `rolloutId` | Container + visual binding + trace metadata |
| `runId` / session id (if model path used) | Journal / Trace V5 provenance |
| Observation | Readout or event_log obs for a step |
| Action | Step action / `actions_taken` entry |
| Reward | Step or cumulative reward for that step/rollout |
| Frame | `frames/{step}.png` or `render.png` / manifest entry for that step |
| Model or tool event | Policy LLM call, Codex tool, or sealed Trace V5 tool span for the same rollout |

All five evidence kinds (obs, action, reward, frame, model/tool) must be present and **identity-bound**. Cross-seed or fixture-only joins fail.

Also satisfy manual **CUA-018** with screenshots/paths when running the 37-item receipt.

---

## 4. Recommended build order

### Step A — Decide the minimal live producer

Pick **one** producer the gate can call every time:

1. **Preferred:** Extend `POST /v1/containers/{id}/policy_rollouts` (or a thin sibling) so the returned payload (or a follow-up GET) includes a **correlation object** built from data already gathered in `run_policy_rollout` (`actions`, `eventLog`, rewards, stream/frame refs, model id, call count).  
2. **Alt:** Scripted `/rollouts` + a deterministic “synthetic tool event” is **not** enough for model correlation — you need a real model/tool event or an explicit sealed Trace V5 that includes tool spans from a real agent turn.  
3. **CUA path:** Agent in Desktop drives Craftax → seal Trace V5 → Open visual — still need the **driver route** for automated `LIVE-TRACE-CORRELATION`.

Do not grade from `open_visual` alone (`EVAL_DRIVER.md`: dogfood only).

### Step B — Eval-driver contract

Add a versioned evidence shape, e.g. on policy/scripted response or:

```text
GET /v1/containers/{id}/rollouts/{rolloutId}/trace_correlation
```

Suggested payload (names flexible; fields are the contract):

```json
{
  "schemaVersion": "synth.trace-correlation.v1",
  "containerId": "…",
  "rolloutId": "…",
  "seed": 2001,
  "taskInstanceId": "craftax:test:2001",
  "traceDigest": "sha256:…",
  "visualId": "…",
  "observation": { "step": 0, "source": "readout|event_log", "excerpt": "…" },
  "action": { "step": 0, "name": "do" },
  "reward": { "step": 0, "value": 0.0 },
  "frame": { "step": 0, "url": "…/frames/0.png", "sha256": "…" },
  "modelEvent": {
    "kind": "policy_llm|codex_tool|trace_span",
    "id": "…",
    "model": "…",
    "boundRolloutId": "…"
  }
}
```

Rules:

- Fail closed if any required field missing or `boundRolloutId` / seed mismatches.
- Prefer content hashes for frames over “URL exists.”
- If Trace V5 seal is in-scope for the automated check, ingest then return `traceDigest` and prove projection/tool span membership; if seal is deferred, document that CUA-018 still requires export/reopen separately — but **gate check still needs model/tool event**, not only env facts.

### Step C — Implement `LIVE-TRACE-CORRELATION` for real

In `evals/workshop/runner/live.ts`:

1. After successful rollouts (+ optional policy call if needed for model events), call the correlation route (or read correlation from rollout response).  
2. Assert: all five evidence kinds present; single `rolloutId`/`seed`; frame hash/URL resolvable; model event bound to that rollout.  
3. Optional: second seed must **not** satisfy the first correlation (anti–cross-bind).  
4. Record ids/digests in the check `detail` for the receipt.  
5. Keep cleanup (`LIVE-CLEANUP-CONTAINER`) best-effort as today.

### Step D — Deterministic regression

Where practical (workshop or evals):

- Unit test: correlation builder rejects mismatched rollout ids / missing reward / missing frame.  
- Fixture test: golden correlation JSON validates schema.  
- Do **not** replace the live check with fixtures only.

### Step E — Wire Trace V5 when sealing is ready

If not already on the automated path:

1. Seal last policy/scripted rollout (+ frames refs) into Trace V5 via inventory ingest.  
2. Open `trace.rollout_inspector.v1` (or Craftax scrub bound to same digest).  
3. Include `traceDigest` in correlation evidence.  
4. Export/reopen covered by CUA-020; automated gate at least needs digest stability + tool/obs presence.

---

## 5. Touchpoints

| Need | Location |
| --- | --- |
| Hardcoded gate fail | `evals/workshop/runner/live.ts` (~L112) |
| Gate client rollouts / visual | `evals/workshop/runner/client.ts` |
| Eval-driver HTTP | `apps/synth_desktop/src-tauri/src/eval_driver.rs` |
| Policy rollout (actions + eventLog already) | `run_policy_rollout` in same file |
| Driver contract doc | `apps/synth_desktop/EVAL_DRIVER.md` |
| Craftax HTTP | façade `:8080` (see `containers.md` §4) |
| Trace ingest / vault | `inventory.rs`, `trace_ingest.rs`, `synth_trace_import` |
| Visuals IPC / event_log fetch | `visuals_ipc.rs` |
| Templates | `live.container_rollouts.v1`, `craftax.rollout_scrub.v1`, `trace.rollout_inspector.v1` |
| Manual twin | CUA-018 in `evals/workshop/manual/CUA_MANUAL_GATE.md` |

---

## 6. Topology / how to run

Debug Workshop instance with eval-driver enabled (`EVAL_DRIVER.md`: debug build + instance or `SYNTH_DESKTOP_EVAL_DRIVER=1`). Craftax façade healthy on `:8080`. Then:

```bash
# Workshop instance with eval-driver descriptor present
# Craftax:
#   cd ~/Documents/GitHub/evals/containers/images/craftax-gamebench-rust
#   PYTHONPATH=. python -m craftax_gold --port 8080

npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:local -- \
  --slot <slot-id> \
  --synth-dev-root /absolute/path/to/synth-dev \
  --instance <workshop-instance> \
  --frontend-url http://127.0.0.1:<frontend-port> \
  --slot-health-url http://127.0.0.1:<slot-port>/health \
  --mlx-health-url http://127.0.0.1:<mlx-port>/health \
  --craftax-url http://127.0.0.1:8080
```

Expect `LIVE-TRACE-CORRELATION` → **pass** with concrete ids in `detail`. Release builds must **not** expose eval-driver (`EVAL_DRIVER.md`).

---

## 7. Acceptance checklist

- [ ] Eval-driver exposes correlation evidence (route or enriched rollout) with schema version.  
- [ ] Live gate check removed hardcoded fail; grades real evidence.  
- [ ] One pass includes obs + action + reward + frame + model/tool event.  
- [ ] Identities align (seed, rolloutId, and traceDigest if sealed).  
- [ ] Anti–cross-bind: evidence for seed A fails against seed B (or second correlation distinct).  
- [ ] Deterministic unit/schema regression added.  
- [ ] No fixture-only or constant `pass`.  
- [ ] Cleanup still deletes gate containers.  
- [ ] CUA-018 runnable with evidence paths for the 37-item receipt.  
- [ ] `gate:local` receipt attaches rollout ids / digests / visual id.

---

## 8. Non-goals / traps

- Rebuilding the entire Trace V5 vault or visual template system (already largely done).  
- Grading success from `open_visual` UI alone.  
- Pointing Codex/Desktop at Craftax without going through register + driver.  
- Using Harbor DEO trajectories as a substitute for Craftax gold correlation without explicit provenance.  
- Shipping eval-driver in the release artifact.  
- Waiving `LIVE-TRACE-CORRELATION` as infra — it is **product** / blocking.

---

## 9. Suggested first commit slice

1. Document + implement `synth.trace-correlation.v1` on the driver (policy path is richest).  
2. Flip `live.ts` to call it and assert fields.  
3. Add schema/unit test.  
4. Run `gate:local` Craftax segment; paste receipt detail into this handoff under “Evidence”.

---

## 10. Evidence (fill when green)

| Field | Value |
| --- | --- |
| Workshop revision | |
| Evals revision | |
| Craftax revision / port | |
| `rolloutId` / seed | |
| `traceDigest` | |
| `visualId` | |
| Gate run id / receipt path | |
| `LIVE-TRACE-CORRELATION` detail | |
