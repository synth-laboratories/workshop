# Handoff — Test Sync Intern in the local frontend

**Date:** 2026-08-06 · **Branch:** `feat/intern-async-24-7-20260805` (backend + frontend)
**Master:** `INTERN_24_7_BURN_HANDOFF_2026-08-06.md` · **Sibling:** `INTERN_ASYNC_EVAL_TEST_HANDOFF_2026-08-06.md`
**Nothing is pushed. Do not push unless Josh asks.**

## Your job

Drive the **Sync Intern desk through the real local frontend** against slot2's live backend, and prove the WP0 Sync chain end to end from the UI: **operator turn → FINAL admitted → work_summary projected → deliverable projected → work product openable.** The eval (`internbenchsync-work-product`) asserts that chain over HTTP; you are proving the *human surface* over the same plane.

This is the `Downstream / FE cockpit` row of `evals/suites/product/intern/ACCEPTANCE_RUNBOOK_ASYNC_24_7.md`, last marked **🟥 red** and untouched since.

---

## Read this before you touch anything

**You are consuming slot2, which is a contended, exclusive resource.** The sync eval rerun (WS2 of the master handoff) and the async eval both drive the same runtime. **Confirm with the lead that slot2 is yours before you start**, and hand it back when you're done. Do not run concurrently with an eval — you will corrupt each other's receipts and neither result will be trustworthy.

Slot2 is the only known-good Intern slot. slot1 is mid-investigation by a private Evals lane; slot3/4 are claimed by others; slot5/6 are `lease_lapsed` and lack the Laguna hot-patch, the OpenRouter key, and the exe.dev override.

---

## Verified state (checked 2026-08-06 ~16:00 ET)

| Thing | State |
|---|---|
| slot2 backend | `http://127.0.0.1:41209`, healthy; MQ `41218` |
| slot2 `hosts.py` hot-patch | present (`_intern_actor_provider_materialization_plan` = `True`) |
| slot2 model | OpenRouter Laguna `openrouter/poolside/laguna-s-2.1`, key len 73 |
| **frontend `.env.local`** | **points at `41609` (slot6) — WRONG for this task** |
| frontend auth | `LOCAL_DEV_AUTH_BYPASS=1` and `NEXT_PUBLIC_LOCAL_DEV_AUTH_BYPASS=1` already set — no Clerk login needed |
| FE routes | `src/app/(pages)/(protected)/smr/intern/page.tsx`, `…/intern/sync/page.tsx`, `…/intern/async/page.tsx`, `…/smr/[projectId]/research-intern/page.tsx` |
| Playwright specs | `tests/e2e/smr/research_intern_live.spec.ts` (Sync, live, no mocks) · `research_intern.spec.ts` (mocked) |

---

## Step 1 — Repoint the frontend at slot2

`frontend/.env.local` has **four** backend URL vars, all currently `41609`. Change every one to `41209`:

```
NEXT_PUBLIC_BACKEND_URL=http://127.0.0.1:41209
SYNTH_BACKEND_URL=http://127.0.0.1:41209
DEV_BACKEND_URL=http://127.0.0.1:41209
BACKEND_URL=http://127.0.0.1:41209
```

Missing one is the classic way to get a UI that renders but talks to a stale slot. `src/lib/config.public.ts:43` reads `NEXT_PUBLIC_BACKEND_URL` and falls back to `http://localhost:8000` — a *silent* wrong-target, not an error.

**Restore `.env.local` to `41609` when you're finished**, or note loudly in your report that you left it on slot2.

## Step 2 — Confirm OpenAPI lockstep

The FE's typed client is generated from the backend spec. If backend HEAD moved past frontend `1f1d27e9`, types are stale:

```bash
cd ~/Documents/GitHub/frontend
bun run generate:smr    # openapi-typescript ../backend/smr_openapi.yaml -> src/lib/generated/smr-openapi.ts
bun run typecheck
```

If `generate:smr` produces a diff, **stop and report it** — that is WS5a of the master handoff and it means the FE and backend contracts have drifted. Don't paper over it to get your test running.

## Step 3 — Mint a disposable org, API key, and fixture chain

Do **not** hand-author factory/project/effort/run ids. The backend refuses tribal ids and there is a provisioning authority for exactly this.

```bash
cd ~/Documents/GitHub/evals
export RESEARCH_INTERN_TARGET_DESCRIPTOR_PATH=/tmp/intern-acceptance-slot2/target.json
uv run python -c '
from suites.product.intern.fixtures.disposable_org import create
org_id, api_key = create()
print("ORG", org_id); print("KEY", api_key)'
```

`/tmp/intern-acceptance-slot2/target.json` already exists and points at slot2 (`backend_url`, `db_container: synth-slot2-db-1`). The provisioner is `DockerPsqlProvisioner` — it works through the slot's DB container.

Then mint the bound chain via the fixture endpoint (`app/api/v1/managed_research/research_intern.py:1234`, request contract `packages/intern/contracts.py:1162`):

```bash
curl -sS -X POST http://127.0.0.1:41209/smr/research-intern/fixtures \
  -H "Authorization: Bearer $SYNTH_API_KEY" -H "Content-Type: application/json" \
  -d '{"idempotency_key":"fe-sync-'"$(uuidgen)"'","task_template":"sync_work_product_chain","include_run":true}' \
  | python3 -m json.tool
```

The receipt is the durable evidence of what exists. Keep it — it carries the factory / project / effort / run ids you need next. Note the endpoint requires an **operator** key (`require_operator`); the disposable-org provisioner mints one.

## Step 4 — Run the frontend

```bash
cd ~/Documents/GitHub/frontend
bash scripts/dev-reset.sh status   # non-destructive; check for an existing Next dev owner
bun run dev                        # bun install + build:content + next dev, PORT=3000
```

If a stale Next dev holds the lock: `bash scripts/dev-reset.sh reset` (kills only repo-local Next dev processes).

## Step 5 — Drive the Sync desk by hand

Open `http://localhost:3000/smr/intern/sync` (auth bypass is on). Navigate to the project from your fixture receipt — `…/smr/<projectId>/research-intern` is the project-scoped desk.

Send one real operator turn. **This spends** — a live Laguna invocation through OpenRouter, roughly the same cost as one eval run.

**Then assert all four links in the UI, in order:**

| Link | What you must actually see |
|---|---|
| `final_admitted` | the turn resolves to an admitted FINAL, not an error toast or a spinner that never lands |
| `work_summary_projected` | a work summary appears on the session/effort |
| `deliverable_projected` | a deliverable/projection row materializes |
| `work_product_openable` | you can **open** the work product and it renders |

Screenshot each. A green HTTP chain with a broken render is still a red FE row — that is the whole point of this task existing separately from the eval.

## Step 6 — Playwright live spec (optional but preferred)

`tests/e2e/smr/research_intern_live.spec.ts` automates the same acceptance. Its env contract (read from the spec header):

```bash
export BASE_URL=http://localhost:3000
export SYNTH_BACKEND_URL=http://127.0.0.1:41209
export SYNTH_API_KEY=<from step 3>
export LOCAL_DEV_AUTH_BYPASS=1
export RESEARCH_INTERN_LIVE_CANDIDATE_ID=slot2-fe-sync
export RESEARCH_INTERN_LIVE_FACTORY_ID=<from fixture receipt>
export RESEARCH_INTERN_LIVE_PROJECT_ID=<…>
export RESEARCH_INTERN_LIVE_EFFORT_ID=<…>
export RESEARCH_INTERN_LIVE_RUN_ID=<…>
export RESEARCH_INTERN_LIVE_REPLY_TIMEOUT_MS=300000   # default 5 min
cd ~/Documents/GitHub/frontend
bunx playwright test tests/e2e/smr/research_intern_live.spec.ts
```

Notes: `playwright.config.ts` loads `.env.local` with `override:false`, so real exports win. `workers: 1`, `fullyParallel: false`, `retries: 0`, global timeout 60 s per test (the spec raises its own). Set `RUN_PLAYWRIGHT=1` if you want Playwright to start the dev server itself instead of step 4.

---

## Known failure modes and how to read them

| Symptom | Read it as |
|---|---|
| Turn dies with `intern_actor_result_invalid_json` | **the known Laguna blocker**, not an FE bug. Root cause is a stray trailing `}` in Laguna's FINAL rejected by strict `json.loads` — see master handoff §1. If WS1's parser fix has landed, re-apply the hot-patch (below) before concluding anything. |
| Turn ends with prose, no FINAL | the *other* Laguna failure mode (no-FINAL turn). Also not an FE bug. Report it — it is direct evidence for WS3. |
| `intern_effect_attempts_exhausted` | infra, not contract. Usually usage limits or mid-run 503s. |
| `control_plane_snapshot_missing` / 503 on poll while `/health` is 200 | known standing noise around restarts. Re-latch a healthy stack, re-run. |
| UI renders but data is stale/absent | check you changed **all four** URL vars in `.env.local`. |
| Type errors after `generate:smr` | contract drift — escalate, don't patch types. |

**If any container was recreated or restarted, the hot-patch is gone** and Sync will fail in ways that look like product bugs:

```bash
HOSTS=~/Documents/GitHub/backend/services/intern/runners/hosts.py
for c in synth-slot2-smr-runtime-1 synth-slot2-backend-api-1 synth-slot2-temporal-worker-1; do
  docker cp "$HOSTS" "$c:/app/services/intern/runners/hosts.py"
done
docker restart synth-slot2-smr-runtime-1 synth-slot2-backend-api-1 synth-slot2-temporal-worker-1
# prove:
docker exec synth-slot2-smr-runtime-1 python -c \
  'from services.intern.runners import hosts; print(hasattr(hosts,"_intern_actor_provider_materialization_plan"))'
```

After a restart the slot reads `observed_healthy_unlatched` — re-latch: `./local_dev/scripts/slotctl latch slot2 --reason "post-hotpatch restart"`.

---

## Report back with

1. Green/red per chain link, with a screenshot each.
2. The fixture receipt (factory/project/effort/run ids) and the org id, so the result is reproducible.
3. Whether `bun run generate:smr` produced a diff.
4. Any Laguna FINAL-contract failure you saw, **with the rollout path** — that is WS3 evidence and it is more valuable than your pass/fail.
5. Whether you restored `.env.local` to `41609`.
6. Confirmation you released slot2.

## Do not

- Run while an eval is driving slot2.
- Push, or commit frontend changes beyond `.env.local` (and don't commit that).
- Touch slot1 (private Evals lane) or slot4 (env-authority burn history).
- Hand-author factory/project/effort/run ids.
- `source` the full `compose.env` into your shell — it poisons the env and a shell `OPENROUTER_API_KEY` will override compose's on the next recreate. Export only what you need.
