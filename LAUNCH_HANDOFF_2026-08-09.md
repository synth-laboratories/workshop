# Workshop v0.1 launch handoff — closing the gate

**Date:** 2026-08-09, late evening. **Audience:** whoever drives the launch to done tonight.
**Definition of done:** a `GREEN` receipt from `npm run gate:release` in `evals/workshop`, re-checked with `gate:verify`. A non-green receipt is not a waiver, and receipts are honor-system (unsigned) — the verifier catches tampering with summaries, source revisions, artifact hashes, and evidence files, but the contract is that nobody edits a receipt by hand. Don't.

## 1. Where things stand right now

- **Deterministic PR gate** (`results/pr-hardened-v1/` in `evals/workshop`): `RED_PRODUCT`, **2 blockers** — down from 4 this afternoon. The Playwright "Set up agent" failure and the Bombadil composer-clearance violation were fixed on the workshop side this evening; 85 Playwright tests now pass.
- **Remaining deterministic blockers:**
  - 12 `test.fail` markers: `apps/synth_desktop/tests/playwright/design-debt.spec.ts` (8), `gaps.spec.ts` (3), `poolside-polish.spec.ts` (1). Each encodes a real product defect. Fix the behavior — converting to `test.skip`/`fixme` is now detected and still red.
  - 4 renderer launch-debt findings: stub copy in `App.tsx`, `SettingsPage.tsx`, `runtime/sessionView.ts`; fixture/demo state in `components/DemoFixturesBar.tsx`.
- **`LIVE-TRACE-CORRELATION` is a hardcoded blocker** (`evals/workshop/runner/live.ts:66`): the eval driver exposes rollouts and visuals but no route proving observation/action/reward/frame/model-event Trace V5 correlation. This blocks `gate:local` and `gate:release` regardless of everything else. Either implement the route or de-scope explicitly (see §6).
- **Live topology is down** (last preflight: `RED_INFRA`, 5 failures): local frontend absent, eval driver unreachable, MLX bearer unset, slot1 actively claimed + dirty + contract-degraded, device-init route unavailable.
- **All 37 CUA/manual checks pending.** (The checklist grew from 32 to 37 tonight: web funnel, GEPA, Intern [alpha], signing, performance.)
- **The gates themselves were hardened tonight** — grading, floors, signature checks, secret scanning, evidence hashing, dirty-tree blocking. Full change list: "Hardening pass — 2026-08-09 evening" in `launch_gate_implementation_plan.md`. Harness unit tests 14/14; negative control detects 13/13 secret pattern classes.

## 2. Burn-down order

1. **Fix the 12 xfails and 4 debt findings** in the Workshop renderer. This is the bulk of remaining product work.
2. **Decide trace correlation**: implement the driver route, or de-scope per §6.
3. **Commit everything.** The **entire gate harness is untracked** (`evals` repo shows `?? workshop/` — it has never been committed; `.gitignore` already excludes `node_modules/`, `results/`, logs). The workshop tree has ~116 dirty files. The release lane **blocks on dirty trees** (`SOURCE-CLEAN-*`), so both repos must be committed before the release run. Caution: both trees have live concurrent agents — check file mtimes and don't sweep unrelated WIP into your commits (the evals repo has unrelated dirty files under `core/`, `scripts/`, `suites/`, `tests/`).
4. **Stand up the topology** and iterate `gate:preflight` until infra checks pass: dev instance with eval driver, local frontend (device-init must return JSON, no redirect), a slot that is *unclaimed and contract-clean* (slot1 was claimed+dirty — free it or pick another), MLX sidecar **with bearer configured**, Craftax container.
5. **Build the signed, notarized artifact.** The gate now runs `codesign --verify --deep --strict`, `spctl`, and `xcrun stapler validate` against it — fail-closed, first time this will run for real.
6. **Manual matrix**: `gate:manual:init` with the exact artifact SHA-256 and workshop revision, then execute all 37 CUA items. Every pass needs tester identity, ISO timestamp, and evidence that is an **existing absolute path or http(s) URL** (files are sha256-hashed at grade time; free text no longer counts). CUA-032 requires a non-implementer. The receipt expires 24 h after init.
7. **`gate:release`**, then **`gate:verify`** on the resulting receipt.

## 3. Commands

All from `/Users/joshuapurtell/Documents/GitHub/evals/workshop`:

```bash
npm run gate:pr                      # deterministic gate (no artifact/topology needed)
npm run gate:preflight -- \
  --instance <name> \
  --frontend-url http://127.0.0.1:3000 \
  --slot-health-url http://127.0.0.1:41109/health \
  --mlx-health-url http://127.0.0.1:7333/health \
  --craftax-url http://127.0.0.1:18098/health \
  --synth-dev-root <synth-dev checkout> --slot <slotN>

npm run gate:manual:init -- --out manual/manual-gate.json \
  --artifact-sha <sha256> --workshop-revision <git-sha>

npm run gate:release -- \
  --workshop /Users/joshuapurtell/Documents/GitHub/workshop \
  --artifact /absolute/path/to/SynthDesktop.zip \
  --manual manual/manual-gate.json \
  --instance <name> \
  --frontend-url ... --slot-health-url ... --mlx-health-url ... --craftax-url ... \
  --synth-dev-root ... --slot <slotN>

npm run gate:verify -- --receipt results/<run-id>/gate-receipt.json
npm run gate:negative-control        # scanner self-test; see trap below
```

Exit codes: `0` GREEN · `2` red (product/manual/harness) · `3` RED_INFRA · `5` RED_SECURITY · `4` crash — a crash still writes an `INTERRUPTED` receipt; treat it as DO NOT SHIP, fix, rerun.

## 4. Traps that will bite you tonight

- **Manual receipt binding**: it binds to the exact artifact SHA and workshop revision. Rebuild the artifact or land another commit ⇒ regenerate the receipt and redo affected checks. It also expires 24 h after init — don't init it before the artifact is final.
- **Suite floors**: Playwright ≥ 75, Rust ≥ 160, static ≥ 40, harness unit ≥ 8. If a suite legitimately shrinks, adjust the floor in `runner/suites.ts` in the same commit with the reason — don't bypass. Bombadil has no floor (its runner emits no count); don't assume it's floor-protected.
- **Negative control semantics changed**: `NEGATIVE-CONTROL-SECRET` must **pass** (all 13 pattern classes detected). `RED_SECURITY` on that lane now means the scanner itself is broken — the old "red means it worked" reading is obsolete.
- **MLX auth**: the gate requires the bearer (`WORKSHOP_GATE_MLX_API_KEY` or flag) *and* that unauthenticated requests get 401/403. An open loopback endpoint is a blocking fail.
- **Craftax rollouts**: two seeds must produce substantively different payloads with per-rollout step/reward data. If the driver returns id-only records, the check fails by design.
- **Verify timing**: `gate:verify` re-reads source revisions and re-hashes the artifact and evidence *at verify time*, and enforces a 24 h receipt age. Verify against the same checkouts the receipt was cut from, before landing anything new — revision drift reds it (that's the point; it caught real drift tonight).
- **Live lanes create real driver sessions and register containers.** Cleanup is attempted and honestly reported in `LIVE-CLEANUP-*` checks; if those fail, expect leaked sessions/containers on the instance.

## 5. What is *not* covered by automation

Signing UX, the usesynth.ai web funnel, GEPA visualization, Intern [alpha] isolation, and performance/memory are **manual-only** (CUA-033…037). Bombadil's invariant set is what it is — more Bombadil coverage is post-launch polish, not tonight's gap. Receipts are unsigned: the trusted-operator model is the last line.

## 6. De-scope protocol

If something can't be fixed tonight: move the claim to `[alpha]`/Deferred in `launch_v0p1.md` **and** change the corresponding gate check in the same commit, stating the reason. The gate and the launch contract must agree. Never green a receipt by deleting a check silently or editing receipt JSON.

## 7. Pointers

- Gate harness + receipts: `evals/workshop/` (`README.md` has the full command reference)
- Gate design + tonight's hardening log: `workshop/launch_gate_implementation_plan.md`
- Launch contract: `workshop/launch_v0p1.md` · QA catalog: `workshop/qa_cua_end_to_end.md`
- Manual runbook: `evals/workshop/manual/CUA_MANUAL_GATE.md`
- Reference receipts: `evals/workshop/results/pr-hardened-v1/` (current, 2 blockers), `pr-current-v2/` (this afternoon, 4 blockers, pre-hardening)
