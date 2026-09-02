# Handoff: annotation workbench — CUA dogfood

**For:** Computer Use on Workshop Desktop.  
**Date:** 2026-09-01  
**Status:** container + rust GameBench gold are proven. **Workshop UI is not.** That is your cut.

Do not re-run Craftax HTTP to “make sure gold works.” It already did. Do not invent a fixture world. Do not treat `craftax_engine` as an in-process stub. If gold is down, fail closed and restart rust gold (recipe below). File work only under `/Users/joshuapurtell/GitHub`.

---

## What is already proven (do not redo)

Live façade (still intended to be up):

| | |
|---|---|
| Facade | `http://127.0.0.1:18080` |
| Rust gold child | `http://127.0.0.1:65032` (ephemeral; `/health` on the facade reports `gold_url`) |
| Health contract | `status=ok`, `environment_ref=env:craftax_gold`, `gold_ok=true`, `annotation=mounted` |
| Storage | `/Users/joshuapurtell/GitHub/evals/temp/craftax-gold-react-e2e-20260901` |

Two real episodes on that gold:

| Rollout | Policy | Charge | Reward | Annotations |
|---|---|---|---|---|
| `roll_dfd4454a1ed9` | `scripted_react` / `engine_acceptance` | none | **−0.2** scored `eval:craftax.env_sum` | 9 findings, `plan_action.plan_missing` |
| `roll_ab9de205861d` | `react` / `luna_low` (`openai/gpt-5.6-luna`, effort `low`) | **$0.00045**, 1 call | **0.0** scored | 10 findings, `plan_action.partially_aligned` |

Use **`roll_ab9de205861d`** as the CUA target. Current head:

- `trace_id`: `roll_ab9de205861d`
- catalog digest: `sha256:6e47e52c30126d3f360d74576569a7358230b0255d646e8465a57693fdb48d69`
- evidence-head bundle: `sha256:9ae974a34a6902a0cc384acd30a4efd556b625fb0e80cf3a9f34f7797545b410`
- `annotation_count`: 10
- `verifier_result_count`: **0** → workbench Rubric must say **Unavailable**, never a zero score
- post-rollout jobs: 4 completed, 0 failed (`craftax.belief_facts`, `plan_action_fidelity`, `recovery_facts`, `milestone_progress`)

Receipts: `evals/temp/craftax-gold-react-e2e-20260901/{start,status,reward,evidence_head,annotation_summary}.json`.

Current container broker is **`DenyAllBroker`**. Deterministic annotators still run. Paid Codex annotators (`craftax.belief`, `craftax.rubric_verifier`, …) will refuse until the façade is restarted with a host-signed broker secret. Do not call that a product pass for the paid card.

---

## What you are proving

Workshop Desktop, pointed at **this** loopback façade, must show the analysis as a first-class output — not a Visuals fixture, not a pasted JSON.

| # | Proof | Pass |
|---|---|---|
| **P0** | Analysis rail opens `analysis.annotation_workbench.v1` bound to the Luna evidence-head | Outputs → **Analysis** (`data-testid="analysis-rail"`), not only Visuals. Footer `analysis.annotation_workbench.v1`. Findings cite the real trace. |
| **P0b** | Rubric | Tab **Rubric** shows Unavailable (`data-testid="analysis-rubric-unavailable"`). A `0` score is a fail. |
| **P0c** | Audit | Tab **Audit** → record a local review (`data-testid="analysis-review-form"`). Status is not an error. |
| **P0d** | Inspector | Open the rollout inspector for this trace. Spans cited by findings show `data-testid="trace-item-findings-…"`. |
| **P1** | Eval campaign chip | A Workshop-owned **eval** against this container shows `data-testid="eval-annotation-campaigns"` with `submitted\|running\|sealed\|partially_sealed` and `sealed/jobs`. |
| **P2** | Paid annotation card | Intern asks for a **paid** annotator (`craftax.belief` or campaign). Modal `data-testid="paid-compute-approval-modal"` shows job **count** (array or number) and estimate if present. Approve once. Reject must leave the trace unchanged. |

P0 is the ship gate. P1/P2 are the product close. Do not skip P0 to chase a GEPA run.

---

## Trees (do not mix)

| Repo | Path | Branch | Head that matters |
|---|---|---|---|
| Workshop | `/Users/joshuapurtell/GitHub/workshop-v08-e2e-refactor` | `codex/v0.9.0` | `4914fcda` Analysis rail + paid-card counts; `69b1fd74` projection/reconciler/workbench |
| Containers | `/Users/joshuapurtell/GitHub/containers` | `josh/annotations-list-achievements` | `a559169` annotation router + evidence-head |
| Evals | `/Users/joshuapurtell/GitHub/evals` | `agent/workshop-evals-v04` | `56e7b290c` image registrar wiring |

Dirty rustfmt / unrelated images in those trees are **not** this task. Path-level commits only if you must land a fix; never `git add -A`. Do not push. Do not write under `Documents`.

---

## Desktop

From the Workshop tree:

```bash
cd /Users/joshuapurtell/GitHub/workshop-v08-e2e-refactor
pnpm desktop:dev
# same as: ./scripts/desktop-instance.sh dev codex
```

Before clicking anything:

```bash
curl -sf http://127.0.0.1:18080/health | python3 -m json.tool
```

Need `gold_ok: true` and `environment_ref: env:craftax_gold`. If 503 / missing / `gold_ok: false`, **stop**. Restart gold (below). Do not point the eval at JAX Crafter or a fixture.

### Register the façade

Inventory → register a **local** container:

- `baseUrl`: `http://127.0.0.1:18080`
- location: `local`
- task family: Craftax / `craftax-gamebench-rust` if asked

Workshop only talks to **registered loopback HTTP**. A typed URL from chat is refused. Probe must stay 200 with `gold_ok`.

### Materialize the Luna trace

Import `roll_ab9de205861d` **from that container** so Workshop records `container_id` on the trace. A bare zip import with no owner will fail paid annotation later (`annotation_trace_unowned`).

Then either:

1. Wait up to ~20s for the host reconciler (10s tick) if jobs are already in Workshop SQLite, or
2. In the chat, ask Intern to **annotate the sealed Craftax trace** with the deterministic Craftax pack (`craftax.belief_facts`, `plan_action_fidelity`, `recovery_facts`, `milestone_progress`). Cache hits are a pass — do not re-roll gold to get a new trace.

The workbench visual id is `vis_analysis_{trace_digest}_{campaign}`. New evidence-head digest ⇒ new revision.

---

## If gold is down (fail closed)

Rust binary (already built):

`/Users/joshuapurtell/GitHub/gamebench/tasks/craftax-singleplayer/gold_rust/target/release/craftax_gold`

```bash
cd /Users/joshuapurtell/GitHub/evals/containers/images/craftax-gamebench-rust
set -a; source /Users/joshuapurtell/GitHub/evals/.env; set +a
export SYNTH_CRAFTAX_GOLD_BIN=/Users/joshuapurtell/GitHub/gamebench/tasks/craftax-singleplayer/gold_rust/target/release/craftax_gold
export SYNTH_CRAFTAX_MAX_STEPS=12
export SYNTH_ANNOTATION=on
export SYNTH_ANNOTATION_DOMAINS=domains.craftax.annotations:register_craftax_annotators
export SYNTH_ANNOTATION_PROMOTE=synth_containers.tracing.adapters.craftax_container:promote_container_rollout
export SYNTH_ANNOTATION_POST_ROLLOUT=craftax.belief_facts,craftax.plan_action_fidelity,craftax.recovery_facts,craftax.milestone_progress
export SYNTH_CONTAINER_STORAGE=/Users/joshuapurtell/GitHub/evals/temp/craftax-gold-react-e2e-20260901
export PYTHONPATH=/Users/joshuapurtell/GitHub/evals/containers/images/craftax-gamebench-rust:/Users/joshuapurtell/GitHub/evals:/Users/joshuapurtell/GitHub/containers/src
/Users/joshuapurtell/GitHub/containers/.venv/bin/python -m craftax_gold \
  --host 127.0.0.1 --port 18080 \
  --storage-root "$SYNTH_CONTAINER_STORAGE"
```

Do not set `SYNTH_CRAFTAX_URL` yourself. PID 1 binds rust on an ephemeral port. Do not kill gold after a pass unless asked — CUA needs it listening.

Paid **eval** ReAct (already done) used `OPENROUTER_API_KEY` from `evals/.env` in the façade process. Paid **annotation** still needs a non-deny broker; do not “fix” that by stubbing Craftax.

---

## Invariants (fail the run if broken)

- Annotation never changes env reward, achievements, or engine state. Luna env-sum stays **0.0 scored**.
- Missing verifier evidence is **Unavailable**, not 0.
- Analysis is a projection; container storage is authority.
- Machine analysis is not stored as human `visual_annotations`.
- Craftax is rust GameBench gold only (`env:craftax_gold`).

---

## UI testids

| Surface | Testid |
|---|---|
| Analysis rail | `analysis-rail` |
| Workbench | `visual-annotation-workbench` |
| Rubric missing | `analysis-rubric-unavailable` |
| Audit form | `analysis-review-form`, `analysis-review-submit`, `analysis-review-status` |
| Eval campaigns | `eval-annotation-campaigns` |
| Paid card | `paid-compute-approval-modal` |
| Inspector findings | `trace-item-findings-{item_id}` |

Skills in the Desktop tree: `apps/synth_desktop/skills/trace-v5-annotate/SKILL.md`, `annotation-review/SKILL.md`. Intern must pass `container_id` from `container_list`, never a URL.

---

## Out of scope for this CUA pass

- Baking five-domain Docker images
- Prompt/budget pack edits (`craftax.belief` scope, recovery tool budget, HealthBench grader contradictions)
- Human calibration goldens under `evals/artifacts/`
- GLM 400-step contests
- Harbor DeepSWE / code-policy CUA

Those stay engineering. Your job is the desktop proof on the living gold head.

---

## Report back

Paste: health JSON (`gold_ok`, `environment_ref`), registered `container_id`, `trace_id` + digests, workbench `visualId` + revision, rubric unavailable vs not, whether Audit saved, screenshot or capture path of Analysis rail, and whether P1/P2 ran or were blocked (broker / no eval run). If gold was down, say **fail closed** — do not attach a substitute env.
