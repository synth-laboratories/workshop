# Handoff — Harbor GameBench code-policy DEO, Codex Luna med, Workshop v0.2

**Goal:** Run a Harbor-packaged GameBench **code-policy DEO** with **Codex Luna medium**, watch it live in Workshop, and leave a durable visual. Ideally a **custom Harbor-rollout visual** (not only the trial/verifier card).

**Not:** public dig.bench, native Craftax 10× (A1), GEPA/SFT, v0.3 E4 three-arm DEO matrix (Terra V2 / Luna V1 / Luna forced V2). This is the **v0.2 A2 shape**: one Harbor fold, Luna med, visual first.

Canonical product clock: [`v0.2-launch.md`](./v0.2-launch.md). Skill: `apps/synth_desktop/skills/run-live-container-evals/SKILL.md`. Systems: [`v0p2_systems.md`](../v0p2_systems.md), [`container_compat.md`](../container_compat.md) § Harbor GameBench / code-policy DEO.

---

## What this job actually is

Two policies, one Harbor trial. Do not collapse them.

```text
Harbor trial  (OUTER — live.harbor_eval.v1)
  Policy  harness+config fused: harbor_fused + luna_med   AUTHOR
  World   Harbor env image (GameBench code_policy_opt workspace)
  Eval    verifier script → reward.txt | json   SCRIPT NODE after exit
  Stream  trial.planned / trial.launched / tools / stdout / verifier / status
  live_frames = unsupported   (advertising native frames on the fold is a stop)

Child Craftax  (INNER — code-policy player)
  Policy  IsolatedPolicyProcess + heuristic_policy.py     PLAYER
  World   same Craftax engine as A1
  Eval    child env-sum → candidate vs baseline → held-out GATE
  Stream  Craftax NEV / frames / policy spans on the CHILD descriptor
  Codex (Harbor) authors files, then PUT /policy + POST /rollouts on the child
        — not python run_hillclimb.py
```

GameBench is **content** (task/dataset). Harbor is the **fold**. Parent `/reward` is the hillclimb **gate**, not a copy of child env-sum. Missing `reward.txt` stays **null**, never `0`. A verifier `0.0` with exit 1 is a **task** score, not infra failure (A2 Luna/Sol both scored `0.0` honestly).

---

## Proven floor (do not re-claim from this machine)

A2 **PASS** on an **isolated Docker machine**, not this laptop’s OrbStack.

| Pin | Value |
| --- | --- |
| Image | `127.0.0.1:5000/gamebench-harbor@sha256:9b626225449960c4b99566abab1d709065e6bcc64e6ade3e68804c6f0ef7cdef` |
| Bundle | `sha256:3865089b42185a6b0bdaeff3925fef8fe64ed74eae2dbf8c7c9ec08200e83918` |
| Policies | `harbor_fused` + `luna_med`, and `harbor_fused` + `sol_med` (this job: **Luna med only**) |
| Workshop visual | `live.harbor_eval.v1`, reviewed before start, `qualityGate.ready` |
| Example rollout | `a2_workshop_luna2` — subscribe-before-start, idempotent start, terminal `completed`, Trace V5, `POST /reward` scored |
| Receipt | `/Users/joshuapurtell/Documents/Codex/2026-08-12/let/receipts/external-acceptance/a2-final/` (`FINAL.md`, `workshop-luna2/receipt.json`) |

**This OrbStack still hangs** on `docker run` (sticks at Created). Do not restart OrbStack while unrelated jobs exist. Do not re-claim A2 from `docs/receipts/2026-08-12/harbor_docker.json`. External Harbor bundles fail if the Docker **build context** omits repo-root paths the Dockerfile needs.

Harbor A2 / V5 / V6 / O-gate Workshop code that the receipt used lives on modern-stack `agent/aug12-modern-stack-completion` (tip moving; last noted `8a6aca6`). Cherry-pick with G2 before treating the main `josh/aug12-optimizers-workshop-visuals` tip as the Harbor runtime.

---

## Operator clock (Workshop)

Isolated named Desktop instance. Not the prod profile. Independent tester for a GO-quality pass.

1. **Discover.** `container_list` → `container_probe`. Read `runtime_family`, transports, `metadata.liveEval`. Harbor register should advertise template `live.harbor_eval.v1`, slot **`stream`**, `liveFrames: unsupported`, `policy_ref`s `harbor_fused`+`luna_med` (and Sol if present). Never guess `http://127.0.0.1:…/events` or `/rollouts/{id}/stream`.
2. **Refuse** if `live_frames=native` on the Harbor fold, if slot is `live`/`jobs`, or if probe is GameBench `:8098` health-only (that is the v0.1 gold HTTP story, not this job).
3. **Prepare, do not start.** `container_prepare_rollout` with explicit:
   ```json
   {
     "policy_ref": { "harness": "harbor_fused", "config": "luna_med" },
     "telemetry": { "enabled": true, "transport": "sse" }
   }
   ```
   Host does **not** default `luna_med`. Keep returned `rollout_id`, `stream_id`, declared SSE + poll URLs, pins.
4. **Visual first.** Create `live.harbor_eval.v1`, bind slot `stream` as `live_sse` to the **declared** SSE URL (and poll beside it). `show`. Wait for control envelope `stream.subscribed` with `ready: true`. HTTP 200 and heartbeats are not ready.
5. **Review** at least twice (wide + compact). `mark_ready`. Then start with `container_start_prepared_rollout` using the exact prepared identity + `visual_id` + `policy_ref`.
6. **Watch the outer visual** through terminal status: trials, tool/stdout, verifier, `reward.txt` present vs missing. Seal Trace V5. Reopen after Harbor agent/verifier containers are gone.
7. **If you want the inner play** (code-policy rollouts, frames, `heuristic_policy.py`): bind a **second** visual to the **child** Craftax stream (see below). Do not hang child frames on the Harbor fold.

`container_run_rollouts` is engine acceptance only — never the ReAct/Harbor eval path.

---

## Visuals

### What ships today — `live.harbor_eval.v1`

Path: `visuals/templates/live.harbor_eval.v1/`.

Shows the **outer trial**: trial cards (planned / launched / verified), metric strip (trial count, reward with missing≠0, `reward.txt` present/not yet, status), bounded tool/stdout stream. ATIF is a projection, not the log. No Craftax map. Slot **`stream` only**.

That is enough to **run and visualize the Harbor attempt**. It is **not** a Harbor-rollout / hillclimb / child-Craftax visual.

### Ideal — custom Harbor-rollout visual (do this if you have time)

Keep `live.harbor_eval.v1` id stable (receipts/MCP). Add a **nested** view for the child code-policy play. Suggested id (new, do not reuse Craftax live as the Harbor fold):

`live.harbor_code_policy.v1` or a child pane inside the Harbor chrome that **binds a second slot** only when the child descriptor exists.

Must show:

| Surface | Source |
| --- | --- |
| Author (Luna med) | Harbor trial stream — files written, `heuristic_policy.py`, tool calls |
| Player | Child Craftax: `PUT /policy` digest, `POST /policy/restart`, episode seeds |
| Candidates | baseline vs authored policy, search vs held-out |
| Gate | parent `/reward` as gate; child env-sum never copied onto parent |
| Child frames | **child** `live.craftax.v1` or nested scrub; Harbor fold stays `live_frames=unsupported` |
| Honesty | missing reward/usage `—`; no guessed `/events`; two visuals do not mix run_ids |

Pass when: one Workshop session shows Luna authoring, a named child rollout playing the written policy, and a gate score, without advertising frames on the Harbor trial stream.

Do **not** mix this into SYN-3202 `git add -A`. New template id; do not rewrite `live.harbor_eval.v1` receipts. Family modularization (`families/first_class_example_containers/harbor`) is parked — not required to run the job.

Until that template exists, the honest combo is:

1. `live.harbor_eval.v1` on the Harbor trial stream.
2. `live.craftax.v1` (or `craftax.rollout_scrub.v1` after seal) on the **child** stream if prepare returned one.
3. Trace V5 inspector for sealed evidence.

---

## Pass / fail for this E2E

**Pass**

- Isolated machine (or a Docker that actually `docker run`s). Visual open and `stream.subscribed` **before** first paid Luna call.
- `policy_ref` is `harbor_fused` + `luna_med` (named by the operator, not a host default).
- Distinct agent vs verifier executions.
- Missing ≠ 0. `reward.txt` missing is null. Honest `0.0` from the verifier is allowed and must look like a score, not a hole filled with zero.
- Terminal status, Trace V5, reopen after Harbor processes are gone.
- No guessed URLs. No child frames on the Harbor fold.
- Secret-free receipt (image digest, bundle digest, Workshop SHA, visual id, rollout id).

**Fail / stop**

- This laptop’s OrbStack hang used as “Harbor doesn’t work.”
- Fixture / `harbor_docker.json` claimed as A2.
- `live_frames=native` on Harbor.
- Mixing Sol and Luna into one visual.
- Starting before `visual.ready` / `stream.subscribed`.
- Treating hillclimb shell (`run_hillclimb.py`) as the protocol.

---

## Suggested first hour

1. Read this file + `run-live-container-evals` + A2 `FINAL.md`. Confirm you are **not** on the hanging OrbStack (use `a2-harbor-slot` or equivalent isolated Docker).
2. Confirm modern-stack Harbor Workshop commits are on the Desktop you will click (or cherry-pick `8a6aca6` lineage first).
3. Register the pinned Harbor image; probe; confirm `metadata.liveEval`.
4. Run **one** Luna med trial on `live.harbor_eval.v1` (not ten).
5. If the child Craftax descriptor is present, bind a second visual for the rollout. If not, file that as the custom-visual gap — still a valid Harbor E2E without it.
6. Seal, screenshot, receipt. Stop Harbor agent/verifier. Reopen the visual from the spool.

Do not spend A1 10×. Do not obtain a dig.bench token. Do not staff Intern.
