# Handoff — finish clean-tip live jobs (A1 resumed 2026-08-12 ~22:00 ET)

**For:** the engineer picking up A1–A8 after this session.
**Do not push** unless Josh asks. Nothing here is on a remote except the
unrelated `origin/dev` bases for G1/SFT.

This file is the authority for *live* status. It supersedes:

- [`receipts/2026-08-12/README.md`](./receipts/2026-08-12/README.md) (dirty tip `b087418-dirty`)
- [`HANDOFF_VISUALS_PUSH_REMAINING_2026-08-12.md`](./HANDOFF_VISUALS_PUSH_REMAINING_2026-08-12.md) A1/A3 **PASS** rows (those receipts are not this commit)

Contracts: [`v0p2_systems.md`](./v0p2_systems.md) → [`aug_12_update.md`](./aug_12_update.md).
Do not use `aug_12_notes.md` (chat dump).

---

## 1. Trees (do not mix)

| Repo | Path | Branch | Tip | Notes |
| --- | --- | --- | --- | --- |
| workshop | `~/Documents/GitHub/workshop` | `josh/aug12-optimizers-workshop-visuals` | `7804a3d` | **no upstream.** Working tree is dirty from a *concurrent* visuals/composer lane — do not `git add -A`. |
| containers | `~/Documents/GitHub/containers` | `josh/aug12-containers-platform` | `a20f994` | rebased onto current `origin/dev`. Clean. |
| G1 GEPA | `~/Documents/GitHub/optimizers-g1` | `josh/aug12-g1-gepa` | `c13d146` | ahead of `origin/dev` by 1. |
| SFT | `~/Documents/GitHub/optimizers-beta-sft` | `josh/aug12-sft` | `11238d1` | ahead of `origin/dev` by 1. |
| **leave alone** | `optimizers` (`main`), `optimizers-beta` (MAPO), backend, synth-dev | | | |

Core code for the AFTER bind/parent floor is in those four tips. Live jobs from
the dirty tip do **not** count.

Uncommitted on workshop that *this* lane owns: `docs/receipts/2026-08-12-clean/`
and this handoff. Leave the Composer/specta/css dirty files to the other lane.

---

## 2. Scoreboard (clean tip only)

Receipts: [`receipts/2026-08-12-clean/`](./receipts/2026-08-12-clean/).

| ID | Result | What actually happened |
| --- | --- | --- |
| **A1** | **PASS** | Re-run from exact Workshop `7804a3d` and Containers `a20f994` in isolated Desktop `a1final`: **10/10** paid seeds, 113 calls, 391,458 tokens, $0.031043, one `live.craftax.v1` visual, `policyAuthority: container`, `recovered: false`, and ten unique spool digests. The earlier 4/10 `a1clean` batch is superseded. |
| **A5** | **PASS** | 8/8 on paid seed 0 (+ WS probe seed 11): poll≡SSE≡WS, control has no sequence, reconnect, `auto` 422, missing `/reward` null, spool reopen after façade kill. |
| **A2** | **blocked** | Cached pinned Harbor image now contains the full Craftax task tree, runner, verifier, and scoring scripts. OrbStack still hangs on actual `docker run`, even `/bin/echo hello`; no verifier score or Desktop proof exists. Restart was not attempted because many unrelated long-lived Docker jobs are active. **NOT A2.** |
| **A3** | **PASS** | Patched dual run on `a3retry`: Luna `banking77_gepa_luna_med_cf98a77a` completed with IPC high-water 846, 232 `proposer.delta`, and 140 typed child refs; Sol `banking77_gepa_sol_med_34883bde` completed with high-water 861, 247 deltas, and 140 refs. Alternating reads proved the unfocused Sol lane advanced while Luna completed. Distinct spools and `optimizer.gepa.live.v1` visuals. |
| **A4** | **PASS** | Root cause of the earlier FAIL was a stale `target/debug` binary built before `11238d1`'s occupancy implementation. The exact rebuilt one-process service typed the overlapping second live Tinker request `sft.training.queued` / `accelerator_busy`; it did not start early, kept a distinct spool, and emitted `sft.training.started` after run A released the slot. A compiled-service HTTP acceptance proof covers the same transition without paid training. |
| **A6** | **PASS** | Fresh bounded Tinker run `sft_banking77_a6pass_223234`: checkpoint `ckpt-10`, one Banking77 child with declared `stream_id` + `reward_url`, numeric environment reward **0.0** (`status=scored`, campaign `scored=1/1`), then `ckpt-10` promoted. Root cause of prior nulls was an unexported `TINKER_API_KEY` in the Banking77 child plus missing `jinja2`; zero here is a real wrong-classification score, not missing coerced to zero. |
| **A7** | **out of cut** | Unmodified Echo image. |
| **A8** | **blocked** | No `DIGBENCH_API_TOKEN`. Headless mock 18/18. **NOT A8.** |
| **W1–W3** | **W1/W2 pass; W3 partial** | Fresh isolated `w1final` CUA with legitimate ChatGPT GPT-5.6 Sol completed W2 before paid work, then exactly 10 real `react/muse_spark_medium` Craftax rollouts through façade `:8298`: 10/10 completed, rewards sum 18.6, no retries or fabricated evidence, and every stream/frame was reviewed. W3 missing-Visuals-MCP safe-stop passed. The remaining scoped poll-503/frame-404/policy-pin drills did not run because a new required Sol task hit the explicit ChatGPT usage limit before tool use (reset shown as Aug 18, 2026 9:45 PM); proxy state was restored and the proxy stopped, with zero extra paid rollouts. Receipt: [`w1-w3-cua.json`](./receipts/2026-08-12/w1-w3-cua.json). |

---

## 3. Code bugs to fix *before* re-running paid jobs

### A3 — `~/.gepa/index.jsonl` append race (G1) — fixed locally, PASS

Two `gepa run` processes `writeln!` the same file with no lock
(`optimizers-g1/rust/crates/synth_gepa/src/lib.rs` `append_global_gepa_run_index`).
One JSONL line contained **both** run ids. Sidecar
`indexed_run_paths` (`service.rs`) does `serde_json::from_str` per line and
skips extra data → `run_not_found` → Workshop treats 404 as an empty page.

Local fix: an exclusive `fs2` file lock now covers read, dedupe, append, flush,
and sync, with a concurrent-append test. Workshop returns a typed not-indexed
error for optimizer-events 404; the recipe loop retries it only while the child
is alive and the post-exit drain still fails closed. Focused tests pass. The
The first bounded window ended before Sol returned; artifacts later showed it
had completed normally after roughly three minutes. A direct ChatGPT-auth
`gpt-5.6-sol` route probe passed. Workshop now pins proposer timeout to 300s
and message-stall timeout to 120s. The `a3retry` retained pair both completed
with proposer deltas and typed child refs over Workshop IPC; A3 is closed.

### A4 — resolved; stale compiled binary caused the failed proof

Commit `11238d1` already contained shared hosted occupancy, but the original
receipt launched `target/debug/optimizers-beta` compiled at 19:21, before the
20:56 occupancy sources. Rebuilding fixed the proof. Added
`acceptance/hosted_sft_occupancy_acceptance.py`: it starts one compiled service,
submits two overlapping HTTP runs, observes typed `accelerator_busy`, verifies
distinct spools / preserved first prefix, and waits for the second start after
release. The live `:8881` probe repeated the typed queue and post-release start.

### A6 — resolved; export the sampling environment

The old null rewards were not classifier results. Banking77 needs `tinker`,
`transformers`, `jinja2`, and an **exported** `TINKER_API_KEY` in its own
process. The env file had been sourced without `set -a`, so the sidecar trained
but the classifier SDK raised the generic Tinker missing-key error. Containers
now emits secret-free typed error codes (`tinker_api_key_missing`, otherwise
class/status only). With the corrected launch, the fresh optimizer campaign
scored numeric 0.0 and promoted its only checkpoint. Null remains null.

### A1 — resolved by isolated exact-tip re-run

The `a1final` batch completed all ten paid seeds without a hot reload. The
receipt excludes an initial zero-call probe where the façade lacked
`OPENROUTER_API_KEY`; those lanes closed `policy_error` before any provider
call and are not counted as A1. The receipt also records that rust gold ran
from GameBench `0230d41` with `--allow-drift` because the evals checkout pins
`ef6bb06`; Workshop and Containers remained on the exact clean-cut tips.

### Harbor live docker

OrbStack 29.4.0 answers `docker info`/`ps` but `docker run --rm alpine:3.20`
sticks at `Created`. Restart OrbStack (or Docker Desktop) before any A2
prep. Leftover `harbor-agent-roll_*` / `harbor-probe-ok` may still be
`Created`; `docker rm -f` hung last time.

---

## 4. How to finish each remaining job

**Ports / instances used tonight** (do not collide):

| Name | Role |
| --- | --- |
| Desktop `a1clean` | A1 eval-driver; data `~/.synth-desktop/instances/v02/a1clean/` |
| Façade `127.0.0.1:8297` | `craftax_react` (may still be up) |
| Gold Craftax `127.0.0.1:18100` | rust gold (`SYNTH_CRAFTAX_URL`) |
| Desktop `a3gepa` | A3; likely dead |
| Banking77 `:8110` | SFT checkpoint evals (A4/A6). Do not steal for A3. |
| SFT sidecar `:8881` | `OPTIMIZERS_BETA_SERVICE_TOKEN` + `SYNTH_OPTIMIZERS_BETA_URL` |
| A3 owned containers | `:51725` / `:51729` (ephemeral cookbook) |

### A1 (no CUA)

```text
./scripts/desktop-instance.sh dev a1clean   # or a fresh name if a1clean is wedged
# containers: uv run python examples/serve_craftax_react.py --port 8297
# SYNTH_CRAFTAX_URL=http://127.0.0.1:18100
```

Eval-driver: `$HOME/.synth-desktop/instances/v02/<name>/data/eval-driver.json`.
Headers: `Authorization: Bearer <token>`, `X-Synth-Eval-Driver: synth.eval-driver.v1`.
`POST /v1/policy_preflight` then register container, `open_visual` once, loop
seeds 0–9 `POST /v1/containers/{id}/policy_rollouts` with
`policyRef: {harness: react, config: luna_med}`, `provider=openrouter`,
`model=openai/gpt-5.6-luna`, `telemetry.transport=sse`, `slot=stream`.
Host does not default `luna_med`. Never guess `/events`. Drain poll via
`cursor.next` until `has_more` is false.

A5 already passed on seed 0 of this incomplete batch. Re-run A5 only if you
replace the stream.

### A3 (no CUA, after G1 index fix)

- Instance `a3gepa` (or new name). Visuals IPC, **not** eval-driver
  (`POST /v1/optimizers/recipes/run`).
- `SYNTH_OPTIMIZER_PROJECT_ROOT` → `optimizers-g1`.
- `SYNTH_BANKING77_GEPA_COOKBOOK_ROOT` → dir with `gepa.toml` **and**
  `synth_service_app.py`. Do not default secrets to `/tmp/synth-ai/.env`
  (worktree `ROOT` trampoline). Stage `OPENAI_API_KEY` into the *instance*
  env file the running Desktop already reads.
- Recipes: `gepa.banking77.luna.v1` then `gepa.banking77.sol.v1`,
  `openVisual: true`. Flip-read IPC high-water on the unfocused lane.
- Pass only if Workshop ingested `proposer.delta` and child refs — disk
  jsonl alone is not A3.
- Do not bind `:8110` or `:8297`.

### A4 then A6 (no CUA, after occupancy fix)

- Banking77 façade `:8110`. Sidecar from `optimizers-beta-sft` `:8881`.
- `TINKER_API_KEY` required. **No OpenAI Fine-tuning.** No `goex.sft.v1`.
- Base model from `docs/sft_tinker_base_models.toml` (Nemotron Lightning).
- A4 pass: second job typed `queued` / `accelerator_busy`; first log not
  rewritten; then it starts when the slot frees.
- A6 pass: checkpoint `/reward` is a real number or honest null *and* at
  least one scored campaign. Null-only is PARTIAL.

### A2 / A8 / W1–W3 (CUA)

- A2: register Harbor-packaged GameBench in Desktop, `live.harbor_eval.v1`
  before trial start, pins `luna_med` + `sol_med`. Restart OrbStack first.
- A8: `DIGBENCH_API_TOKEN`, visual `live.digbench.v1` before `start_session`,
  both harnesses, no frames.
- W1–W3: fresh-workspace Sol, skills refuse guessed `/events`, stop on tool fail.

A7 stays out.

---

## 5. Honesty rules (do not weaken)

- Two envelopes: `synth.trace-stream-event.v1` (eval) and `optimizer_event.v1`
  (campaign). Child evals = `synth.resource-ref.v1` `{kind: container_rollout,
  attributes: {stream_id, reward_url}}`.
- Connect-before-start: `stream.subscribed` (sequence null) before
  `POST /rollouts` / first paid call. Slot `stream` for live eval,
  `optimizer_run` for optimizer visuals. Fail `live` / `jobs`. Never guess
  `/events`.
- Missing ≠ 0. Harbor `reward.txt` missing stays null.
- Harbor is the only fold. GameBench / dig.bench / Banking77 are content.
- GELO `goex` / JSONL SFT / DualGepaHub tests are not A1–A8.
- Dirty-tip receipts are not this cut.

Linear (Backlog, Josh): parent [SYN-3201](https://linear.app/synth-ai/issue/SYN-3201)
on project [Workshop v0.2 Release](https://linear.app/synth-ai/project/workshop-v02-release-6a70edbc8773).
A1+A5 SYN-3203 · A2 SYN-3204 · A3 SYN-3205 · A4→A6 SYN-3206 · A8 SYN-3207.

---

## 6. Suggested order

1. **G1 index flock** + Workshop 404-while-running (unblock A3).
2. ~~**SFT shared occupancy** on hosted `POST /v1/runs`~~ — complete; A4 PASS.
3. ~~Isolated **A1 10/10**~~ — complete on `a1final`.
4. Re-run **A3**. ~~Fresh scored **A6** campaign~~ — complete.
5. OrbStack restart, then CUA **A2**; token then **A8**; **W1–W3**.

Do not start more core bind/parent work. The floor is in the four tips above.
The remainder is these defects plus paid/CUA proofs.
