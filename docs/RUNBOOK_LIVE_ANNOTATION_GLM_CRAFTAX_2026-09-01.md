# Runbook: GLM 5.3 flash on Craftax with a live code + LLM annotator, judged in a live view

**Goal:** run `z-ai/glm-5.3-flash` as the Craftax policy, with the `craftax.live.v1` protocol streaming deterministic findings and GLM-judged findings beside each rollout, into a page that updates live, so a human can judge whether the annotations are valid.
**Proven:** 2026-09-01 on this machine, two 40-step rollouts, judge 5 of 5 calls parsed, about $0.0004 of OpenRouter spend per rollout. Receipts under the session scratchpad (`live-e2e/glm2/receipt.json`).
**Time to first live view:** about 3 minutes once the engine is up.

This is the host-driven path: the containers façade runs in-process on the host against a live Craftax engine. The Workshop desktop pane (`live.annotated_rollouts.v1`) is built and unit-proven but has not been driven end to end; see the last section.

---

## 0. What you are looking at

Three parties, one durable stream each way:

- **rollout stream** (`/rollouts/{id}/events`, SSE at `/stream`): steps, actions, rewards, observations with inventory and achievements, policy plans and reasoning deltas.
- **annotation stream** (`/rollouts/{id}/annotations/events`, SSE at `/annotations/stream`, WS at `/annotations/ws`): the protocol's provisional findings, metrics, judge calls, control acks. Own sequence space; every row carries `stream_id`.
- **control** (`POST /rollouts/{id}/annotations/control`): consumer → annotator. `message` (note / judge_now / set), `protocol.update` (hot-swap), `stop`. Never reaches the policy.

Findings are `provisional`. They are superseded as evidence grows and retracted when later evidence contradicts them. Nothing here changes reward, achievements, or the sealed trace.

## 1. Prerequisites

Worktrees (all unpushed branches):

| Repo | Path | Branch |
| --- | --- | --- |
| containers | `~/GitHub/containers-live-annotation` | `josh/live-annotation-protocol` |
| evals | `~/GitHub/evals-live-annotation` (protocol also copied into `~/GitHub/evals`) | `josh/craftax-live-protocol` |
| containers `main` image tree | `~/GitHub/wt-codex-stopped-container-recovery-20260831/images/craftax-gamebench-rust` | `main` (read-only use) |

One-time setup:

```bash
cd ~/GitHub/containers-live-annotation
uv sync
# The branch base needs this untracked file from the sibling tree (pre-existing branch defect):
cp ~/GitHub/containers/src/synth_containers/platform/trace_bundle.py src/synth_containers/platform/trace_bundle.py
uv run --with pytest --with httpx --with websockets pytest -q tests/test_live_annotation_*.py   # expect 43 passed
```

OpenRouter key: `~/GitHub/evals/.env` carries `OPENROUTER_API_KEY`. Source it into the shell that runs the façade; the façade reads it from the environment at call time and never persists it.

Craftax engine on `127.0.0.1:18098`. One is usually running (`docker ps | grep pivotrl-engine`). To start one:

```bash
docker run -d --name craftax-engine -p 127.0.0.1:18098:8098 \
  --entrypoint /opt/gamebench/tasks/craftax-singleplayer/gold_rust/target/release/craftax_gold \
  evals-craftax-gamebench-rust:tinker-budget-fix --host 0.0.0.0 --port 8098
curl -s http://127.0.0.1:18098/health   # {"env_family":"craftax-singleplayer","ok":true,...}
```

## 2. Run it

```bash
cd ~/GitHub/containers-live-annotation
set -a; source ~/GitHub/evals/.env; set +a
export SYNTH_CRAFTAX_URL=http://127.0.0.1:18098
export PYTHONPATH=src:$HOME/GitHub/wt-codex-stopped-container-recovery-20260831/images/craftax-gamebench-rust:$HOME/GitHub/evals
.venv/bin/python scripts/live_annotation_glm_craftax.py --out /tmp/glm-live --seeds 0,1,2 --max-steps 60 \
  --judge-every-calls 2 --judge-max-calls 8
```

The script prints `>>> VIEWER: http://127.0.0.1:8765/` (or another free port), waits two seconds, then prepares, subscribes and starts the rollouts concurrently. Open the URL before that. When the rollouts finish it prints every finding per rollout with confidence, step, basis (`engine_event`, `readout`, `inventory`, `model`, `consumer`) and the judge's rationale, writes `/tmp/glm-live/receipt.json`, and keeps the viewer up until Ctrl-C.

Knobs: `--seeds`, `--max-steps`, `--judge-every-calls` (one judge call per N policy plans), `--judge-max-calls` (per rollout), `--effort low|medium` (policy), `--plan-max`, `--policy heuristic` (free, no model: exercises only the deterministic layer). Timings seen: 40 steps ≈ 30–45 s per rollout with GLM at effort low; judge calls 5–20 s each.

## 3. What to check while it runs (validity checklist)

In the viewer, per lane:

1. **Incremental.** The first finding appears within a second or two of the first step, not at the end. The activity feed (tab `annotations`) interleaves with rollout events.
2. **Achievements are engine truth.** Every `achievement` chip (green) has `basis engine_event` or `readout` and matches the achievements the observation shows. There is never an achievement the engine did not report.
3. **Milestones follow the graph.** A `milestone` (blue) appears only after all of its engine achievements, or, for inventory-gated ones, after its measurable prerequisites. `crafting.place_table` never precedes `resources.accumulate_two_wood`.
4. **Deterministic failure modes cite real steps.** `feedback_incorporation.repeated_blocked_action` should coincide with runs of `action_applied` rows whose transition is `noop` (the feed shows `· do` with no effect). Hover a chip: it lists the cited sequences. It escalates at 3, 5, 8, 13 repeats, not every step, and a successful transition ends the streak.
5. **Judge findings quote evidence.** Purple `intent` and red `failure_mode` chips marked `judge` carry a rationale (hover). Judge the rationale against the local map and plan in the reasoning excerpt: the run on 2026-09-01 produced `plan_quality.missing_prerequisite` ("schedules make_wood_pickaxe but inventory shows no wood"), `safety_survival.ignored_threat` naming the hostiles, and `plan_quality.spatial_error`. Confidence is the judge's own.
6. **History is visible, never erased.** Tick `history` to see superseded and retracted chips (struck through, with the reason).
7. **Controls are bidirectional.** In a lane's control box send a `note` (it becomes a grey `note` chip with `basis consumer`), `judge_now` (a judge request appears immediately), `set blocked_streak_threshold=6`, and on one lane `stop` (that lane's annotations seal with `stopped_by_consumer` while the rollout continues). Each is acknowledged in the feed as `annotation.control.received` or `refused`.
8. **Closure.** After the rollout finishes, the annotation stream closes on its own (`annotations completed` in the lane header). Judge calls still in flight are given up to 120 s.

Everything in the page is read from the poll authority (`/events` pages), so what you see is what is durable.

## 4. Change the protocol live

The protocol is one stdlib-only file: `~/GitHub/evals/domains/craftax/annotations/live_protocol.py`.

- Edit it and re-run the script: the façade installs a new `anprev_…` revision (content-addressed) and the next rollouts use it. Nothing is rebuilt.
- To hot-swap rollouts already running: install the new revision, then send `{"op":"protocol.update","protocol_revision_id":"anprev_…"}` to `POST {façade}/rollouts/{id}/annotations/control`. `craftax.live.v1` implements `snapshot`/`restore`, so streaks, milestones and ids carry over; the stream records `annotation.protocol.rebound` with `state_carried: true`.
- Judge model and limits live in the install body (`configuration.model`: `model`, `base_url`, `credential_mode`, `effort`, `max_calls`, `max_output_tokens`, `drain_timeout_seconds`). Standalone runs use `credential_mode=environment` and an `api_key_env`; Workshop replaces recipe routing with `credential_mode=workshop_proxy` and its scoped, container-reachable capability URL. No provider key is ever in the body.

Reproduce the mid-rollout control proof without a model: `scripts/live_annotation_craftax_control_e2e.py <out_dir>`.

## 5. Where evidence lands

- `<out>/receipt.json`: per-rollout HTTP result, usage/cost, every finding with basis and rationale, judge counts.
- `<out>/storage/live_annotation/events/*.jsonl`: the durable annotation journals (one per rollout), recoverable after restart.
- `<out>/storage/seals/*.trace-v5.json`: the sealed rollouts. Provisional citations are rollout-stream sequences; in the seal they are `event_id` / `order.chronological_sequence`.

## 6. The Workshop pane (built, not yet driven)

Workshop has the same lane end to end: `[live_annotation]` in a recipe (`recipes/annotation_eval/eval.craftax.gold.live_annotated.v1.toml`), a per-run protocol pin, relay of the annotation stream into the run journal, an auto-minted `live.annotated_rollouts.v1` pane bound to both streams per rollout, `annotation_manage` operations for control and protocol updates, and post-seal reconciliation. The judge request is executed by the container platform through Workshop's run-scoped provider proxy. The provider key remains in Workshop; the container receives only the capability URL and public `workshop-proxy` sentinel. To drive it you need a running desktop built from `~/GitHub/workshop-live-annotation` and a Craftax image rebuilt from a tree that carries `synth_containers.live_annotation` (the image lives on containers `main`, the lane on the annotation branch). The viewer above remains the standalone fallback surface; it shows the same data from the same streams.

## 7. Known gaps

- The standalone viewer's page has been syntax-checked and its data path proven, but nobody has eyeballed it in a browser yet. If it misrenders, the receipts and the printed findings are the same evidence.
- The judge's `intent` label is the model's guess at the policy's goal; treat it as a prompt for the human, not a finding.
- Corroboration between live labels and sealed post-hoc labels is by name; a dedicated post-hoc confirmer would make it precise.
