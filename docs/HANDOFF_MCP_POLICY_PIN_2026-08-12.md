# Handoff: MCP coding agent owns the policy pin (no silent recipe)

**For:** the engineer picking this up  
**Date:** 2026-08-12 ~16:11 ET  
**Continues:** [`HANDOFF_FINISH_FLOOR_2026-08-12.md`](./HANDOFF_FINISH_FLOOR_2026-08-12.md)  
**Nothing committed or pushed.** Do not reopen locked decisions. Do not commit unless asked.

Canonical plan: [`PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md)  
Acceptance: [`aug_12_update.md`](./aug_12_update.md) A1–A8  
Containers suite: [`container_compat.md`](./container_compat.md) §12  
Synth Style: parse at the trust boundary, pair assertions, **missing ≠ default**, one umbrella layer.

**Do not use** [`aug_12_notes.md`](./aug_12_notes.md).

---

## What you are finishing

Live policy rollouts are kicked off by the **Workshop coding agent via MCP**. The host does not pick a recipe.

A1 Desktop is **not claimed**. Unit tests and headless C3 do not count. The remaining proof is one paid Luna seed through the in-app agent: visual subscribed → explicit `policy_ref` → container ReAct only → harvest the log.

Dogfood still:

> Find the Craftax Rust GameBench container, run exactly 10 rollouts, collect Trace V5 / rewards, open a visual that compares them.

---

## Locked (do not reopen)

- The coding agent via `synth-containers-mcp` **names** `policy_ref`. Workshop and Containers do **not** fill `luna_med` or `default_policy_harness` on start.
- Policy = `{harness, config, code?}`. `config` is required except `isolated_policy_process` (code policy; config on that harness is still a 403 `bind_policy_config`).
- Live path: `container_prepare_rollout` → bind declared slot `stream` → `visual.ready` → `container_start_prepared_rollout` with `policy_ref` + `task_instance_id` or `seed`.
- `container_run_rollouts` is **scripted engine acceptance only**. Explicit action list. Not ReAct. Not a model eval.
- `eval_driver` `policy_rollouts` is a sibling HTTP route. It also requires the caller pin. It must not grow a Craftax/Luna default. The skill tells the agent to use MCP start, not this route.
- Seed `0` is a valid pin only when the agent sends `seed` or `task_instance_id`. Missing is not seed 0.
- Connect-before-start. Declared SSE URL only. No `/events` guess. `telemetry.transport=auto` forbidden.
- Harbor remains the only first-class fold. Do not Harbor-wrap dig.bench.

---

## Trees (do not mix them)

| Work | Path | Branch | Git |
| --- | --- | --- | --- |
| Workshop MCP + visuals IPC + skills | `/Users/joshuapurtell/Documents/GitHub/workshop` | `josh/aug12-optimizers-workshop-visuals` | **Uncommitted.** Mixed with ChatGPT OAuth / landing / optimizer WIP. Split before commit. |
| Containers façade pin | `/Users/joshuapurtell/Documents/GitHub/containers` | `dev` | **Uncommitted.** Do not force-push `dev`. |

Leave `optimizers`, `optimizers-beta`, G1, and SFT trees alone for this slice.

---

## What landed this session (uncommitted)

### Containers (`containers` / `dev`)

Parse at `POST /rollouts`. Prepare may omit the pin (identity + stream only). Start may not.

| Piece | Where |
| --- | --- |
| Require `policy_ref.harness`; require `config` unless isolated | `src/synth_containers/platform/http_requests.py` `_require_policy_pin` |
| Pair refusal; no `luna_med` / default-harness fill | `src/synth_containers/platform/state.py` `start_rollout` |
| Craftax simulate refuses a missing pin (programmer error if start leaked) | `src/synth_containers/platform/runtimes/craftax.py` |
| `ScriptedReAct` no longer defaults `config_id="luna_med"` | `src/synth_containers/platform/react.py` |
| Conformance `_start` **names** an explicit pin (test helper, not server fill) | `tests/conformance/container_compat/run.py` |
| Examples that start a rollout now pass a pin | `examples/headless_visual_consumer.py`, `deo_nested_reward.py`, `optimizer_child_eval_refs.py` |

`/metadata.policy_ref` may still advertise `default_policy_harness` with `config: null`. That is discovery, not a start fill. Named configs `luna_med` / `sol_med` still exist for callers to **name**.

### Workshop (`josh/aug12-optimizers-workshop-visuals`)

One owned check: `require_caller_policy_ref` / `require_task_instance` in `apps/synth_desktop/src-tauri/src/container_stream.rs`. Eval driver and visuals IPC both use it.

| Piece | Where |
| --- | --- |
| MCP start requires `policy_ref`; prepare may carry the pin | `apps/synth_desktop/src-tauri/src/bin/synth_containers_mcp.rs` |
| IPC start forwards the pin; refuses missing task instance; scripted path requires `actions` | `apps/synth_desktop/src-tauri/src/visuals_ipc.rs` |
| `policy_rollouts` uses the same pin check | `apps/synth_desktop/src-tauri/src/eval_driver.rs` |
| Agent workflow | `apps/synth_desktop/skills/run-live-container-evals/SKILL.md` |
| `container_run_rollouts` labeled scripted-only | `apps/synth_desktop/skills/use-synth-containers/SKILL.md` |
| Visual-ready start must include the pin | `apps/synth_desktop/skills/use-synth-visuals/SKILL.md` |

`luna_med` in skill JSON is an **example the agent may send**, not a host default.

---

## Agent start shape (copy this)

```text
container_list / container_probe
container_prepare_rollout          # identity + declared stream; does not start
synth_visuals: bind slot stream as live_sse to declared SSE URL
visual.ready + stream.subscribed   # ACK is not evidence
container_start_prepared_rollout   # MUST include policy_ref + task_instance_id|seed
```

```json
{
  "container_id": "…",
  "rollout_id": "roll_…",
  "stream": { "id": "stream:…", "transports": { "poll": { "url": "…" }, "sse": { "url": "…" } } },
  "visual_id": "…",
  "task_instance_id": "seed:0",
  "policy_ref": { "harness": "react", "config": "luna_med" }
}
```

Omit `policy_ref` → MCP schema + visuals IPC + Containers HTTP all refuse.  
Omit `config` on `react` → refuse (`does not default luna_med`).  
`isolated_policy_process` omits `config`; sending one is still 403 `bind_policy_config`.

`container_run_rollouts` requires `actions`. Do not use it for A1.

---

## What is still open

1. **One live seed through the Workshop coding agent.** Not fixtures. Not `eval_driver` unit tests. Visual bound and subscribed before the first paid call. Container owns ReAct. Harvest the declared log. Then 10× is A1.
2. Renderer EventSource is still not waited on; driver/IPC wait on the poll ACK from prepare. SSE replays from 0, so a late pane still gets history — but Data page attach without `visualId` still opens empty inline.
3. Host stepper leftovers in `eval_driver.rs` (`policy_actions_from_model`, `DEFAULT_POLICY_ACTIONS`) stay dead. Do not revive them as the live path.
4. A2–A8 unchanged (Harbor Docker, Banking77 GEPA, hosted SFT, dig.bench Desktop).

Do not put Craftax/Luna defaults back in `eval_driver.rs` or `visuals_ipc.rs`.

---

## Verify (this slice)

```bash
# containers — pin refusal + façade
cd /Users/joshuapurtell/Documents/GitHub/containers
uv run --with pytest pytest tests/test_http_requests.py \
  tests/test_platform_leftovers.py tests/test_platform_event_journal.py \
  tests/test_craftax_eval_examples.py -q
uv run python -m tests.conformance.container_compat.run --target craftax_engine
uv run python -m tests.conformance.container_compat.run --target craftax_code_policy
uv run python -m tests.conformance.container_compat.run --target harbor_public

# workshop — pin helpers + MCP schema + IPC scripted path
cd /Users/joshuapurtell/Documents/GitHub/workshop/apps/synth_desktop/src-tauri
cargo test --lib container_stream
cargo test --lib eval_driver::tests::policy_rollouts_require_caller_policy_ref
cargo test --lib visuals_ipc
cargo test --bin synth-containers-mcp
```

This session: containers pin tests + craftax_engine / code_policy / harbor / deo_nested / digbench_mock conformance `failed: []`. Workshop `container_stream` 6, `policy_rollouts_require_caller_policy_ref`, `visuals_ipc` 7, MCP schema test.

---

## If you get stuck

- Paid run with no `policy_ref` in the log → you used `container_run_rollouts` or an old start path. Stop. Use MCP start.
- Log says `luna_med` but the agent did not send it → a silent fill leaked. That is a bug; do not paper over it in the viewer.
- Agent cannot pass `policy_ref` on prepare → schema used to be `additionalProperties: false` with only `telemetry`. Prepare now accepts the pin; start still **requires** it.
- Visual empty at first Luna call → skipped `stream.subscribed` / opened inline without the declared SSE bind.
- Reward `0` on incomplete → you filled missing. Return `null`.

Ask Containers only if `POST /rollouts` accepts a body without `policy_ref.harness`. Workshop should already fail closed at MCP + IPC.

---

## 16:58 takeover receipt

The attempted isolated `livecraftax-codex` acceptance was stopped before rollout start. It prepared container `ctr_397f065f44944f8183da23e166330942`, rollout `roll_fa381b6e5a1a`, and observed `stream.subscribed`, but never called start. Therefore it produced no rollout reward and incurred no container policy-call cost. Several paid Luna coding-agent turns did occur; that isolated instance did not journal their exact cost, so A1 remains unclaimed.

The attempt exposed and fixed one Workshop source bug: generated Codex MCP config used `SYNTH_DESKTOP_IPC_FILE` for every adapter, while `synth-visuals-mcp` owns `SYNTH_VISUALS_IPC_FILE`. `session/codex/home.rs` now selects the adapter-owned variable and `session/codex/tests.rs` locks the mapping.

Post-fix verification:

- generated MCP IPC-variable regression: 1 passed
- `container_stream`: 6 passed
- `eval_driver::tests::policy_rollouts_require_caller_policy_ref`: 1 passed
- `visuals_ipc`: 8 passed
- `synth-containers-mcp`: 1 passed

The isolated `livecraftax-codex` app and its three MCP child processes were stopped. The existing `livecraftax` app was not rebuilt, relaunched, or touched. Do not resume UI automation from the interrupted task; the remaining paid acceptance should start from the corrected source in a controlled instance and must still prove visual-ready + subscribed before start.

---

## 17:03 A1 one-seed live receipt

A controlled headless Sol/high/fast coding harness completed one real bounded Craftax seed through Workshop MCP. The existing `livecraftax` app remained running and was not rebuilt or relaunched.

- container: `ctr_0a507aa7a8634a12b02a01fee55521d5` (`http://127.0.0.1:8100`, 12-step cap)
- rollout: `roll_254960257ee6`; prepared once; started once; `task_instance_id=seed:0`
- explicit policy: `{ "harness": "react", "config": "luna_med" }`
- visual: `vis_4e6581574c8447e69b99af69371ffd09`, `live.craftax.v1`, revision 2
- pre-start receipts: current revision had two reviews and `qualityGate.ready=true`; declared poll log began with `stream.subscribed { ready:true }`; only then was start invoked
- terminal stream: 186 events, 12 steps, 13 real PNG frames, 32 `span.policy.data` partials, `capture.closed` at high-water 183
- result: reward `1.0` from `eval:craftax.env_sum`; achievement `collect_sapling`
- policy usage: four real OpenRouter `gpt-5.6-luna` medium calls, 3,158 prompt + 419 completion tokens, observed provider cost `$0.00051283`

This proves the one-seed live path. It does not claim the 10-seed A1 matrix.

Two runtime defects were found and fixed during this receipt:

1. A missing provider key previously left the durable stream at `span.policy.opened` forever after HTTP 500. Containers now persists a secret-free failed status, closes policy/session/environment and capture, marks the pin terminal, and seals normally. Regression: `tests/test_platform_leftovers.py`, 10 passed.
2. Workshop applied the 10-second generic rollout timeout to a live policy execution. The run completed, but MCP reported a transport error. Live policy start now uses the named 900-second `CONTAINER_POLICY_ROLLOUT_TIMEOUT`; prepare and scripted/data paths keep their shorter budgets. `visuals_ipc` remains green (8 passed).
