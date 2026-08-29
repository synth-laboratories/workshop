# NanoHorizon Craftax acceptance report — c0c01101 / c683876b

Date: 2026-08-28 (run finished 2026-08-29T00:43:35Z)  
Verdict: **accepted** against the **new** closure, not the retired gold pin.

This supersedes
`docs/NANOHORIZON_CRAFTAX_ACCEPTANCE_REPORT_2026-08-28.md` for release
identity. That document remains the record of the two overlapping gold-image
failures. Do not relabel those runs as this pass.

## Closure that was accepted

| Component | Revision / digest |
| --- | --- |
| Containers | `c0c01101f9f1d5ff02a2678be052bcc4b44eb550` (`fix(contract): echo admitted task instance limits`) |
| NanoHorizon | `df06fce73ef0f1dcccb42cbc404ae567e939740f` (`fix(eval): pin task-limit-compatible container runtime`) |
| Evals | `43ec21b8a73f87a72fae982f5bb614245ea1f106` |
| GameBench | `3d35f379a6d3f951720bfcc04d0f05518d9b8034` |
| Source-manifest digest (include list, recomputed) | `sha256:6b9586d74ea2c8b9848954bdc6ac164fa334864324754fdc8b3ebecef1aa2016` (unchanged; `workshop.containers.toml` is not an included input) |
| Live OCI / `/info.imageDigest` | `sha256:c683876b30f228c682e66c91c5aedbaa60386bf806de35132c2bdf9fffbb9c31` |
| `/info.producerSourceRevision` | `c0c01101f9f1d5ff02a2678be052bcc4b44eb550` |
| Environment | `env:craftax_gold` (`gold_ok: true`) |
| `declarationOrigin.sourceRoot` | `/Users/joshuapurtell/GitHub/nanohorizon-e2e-final` |
| `declarationOrigin.sourceRevision` | `df06fce73ef0f1dcccb42cbc404ae567e939740f` (agrees with producer) |

Workshop instance `j` at execution:

| Surface | Value |
| --- | --- |
| Compiled / health | `364d2978b1fb` |
| Git tip at build | `364d2978` (`feat(eval): forward admitted limits…`) |
| PID | `79173` |
| Executable digest | `sha256:14c7ebc3e533d698c0d35b03dfd3656bb2f3eacafa15b62382e240d3a46e5aea` |
| Signing | ad-hoc |
| Eval driver | `http://127.0.0.1:65211` |
| Visuals IPC | `http://127.0.0.1:65210` |

`364d2978` is required: `c0c01101` rejects `/task_instances/materialize` without
`limits`. Workshop `a6d4c71e` failed closed with `limits_required` against this
image. The fix forwards admitted `max_calls` / `max_steps` / `max_cost_usd`.

Preflight (`scripts/preflight-nanohorizon-e2e.sh`) now pins this Containers /
NanoHorizon pair. The four external repos were exact and clean at run time.
Workshop was clean at `cua-build`.

## The run

| Field | Value |
| --- | --- |
| Optimizer run | `opt_eval_craftax_b130a1d92a02` |
| Session | `acceptance_craftax_20260828_c683b` |
| Container | `ctr_c694dbc5a60f45069f82d7f06edd4530` @ `http://127.0.0.1:18091` |
| Policy | `nanohorizon/glm-5.3-flash` · `src/challenge/policy.py` |
| Provider / model | `openrouter` / `z-ai/glm-5.3-flash` |
| Seeds | `780005, 780006, 780007, 780008, 780009` |
| Limits | 5 rollouts · 10 calls · 2000 steps · `$2.45` |
| Concurrency | 5 |
| Started | `2026-08-29T00:41:27.893802Z` |
| Finished | `2026-08-29T00:43:35.166002Z` |
| Status | `completed` / `evalStatus: completed` |
| Inline execution spec | `sha256:0b9cdc27389a4990d050233bbde42fd6941dcc8ee8b22d5118ccff73377882f7` |
| Primary visual | `vis_14bcb337cdf54e87b2a9be98d109359a` (`live.craftax.v1`) |
| Trace workstation | `vis_751c28b657e14ddea6386d99cdc4dd73` |
| Mean reward | `1.8` |
| `GET …/result` | HTTP 200 |

One admission. No second orchestrator.

## Trials

| Seed | Rollout | Steps | Reward | Evidence |
| --- | --- | --- | --- | --- |
| 780005 | `roll_craftax_train_780005_8d6c9e2d` | 34 | 1.0 | `sealed_complete` |
| 780006 | `roll_craftax_train_780006_b5e01e21` | 53 | 3.0 | `sealed_complete` |
| 780007 | `roll_craftax_train_780007_5f7c2a7b` | 31 | 1.0 | `sealed_complete` |
| 780008 | `roll_craftax_train_780008_1275cdc8` | 39 | 2.0 | `sealed_complete` |
| 780009 | `roll_craftax_train_780009_7c6bfb41` | 79 | 2.0 | `sealed_complete` |

Evaluator outcome per trial: `scored`. Journals closed; none rejected
(`journalRejected` unset, not `true`). Chain heads present; high-water acked.

## Trace V5 (five sealed)

| Trace id | Digest |
| --- | --- |
| `tracev5_6d945064cfedbb58b804c58f` | `sha256:6d945064cfedbb58b804c58f2ae87576f3022a584c51f9b2d30746b61c63f7cb` |
| `tracev5_b7fe2aa13a9eee6e97878160` | `sha256:b7fe2aa13a9eee6e97878160f27fd3adf9da60383583906120378be703ca297f` |
| `tracev5_ed877dd94dc0d2605e637f59` | `sha256:ed877dd94dc0d2605e637f597b2e69d87920a2c58210bcde39f565a23f7f9473` |
| `tracev5_f1131e5311f726135b4134b5` | `sha256:f1131e5311f726135b4134b56a7f6cb2e6157ba3f90e34d8cd3cc791049ae96f` |
| `tracev5_0282a38b82106510528c66bd` | `sha256:0282a38b82106510528c66bda49d31825d7b2962677b1cf4dd0fb8c9fac108ca` |

Evidence ledger: 5/5 `sealed_complete`. Completeness: `complete`. Ten refs
(five evaluator results + five traces).

## Spend, consent, credentials

| Field | Value |
| --- | --- |
| Paid approval | `approval-auto-1d723651e63f44119330262981751728` (conversation policy auto-approve, not an operator click) |
| Cap | `{maxCostUsdMicros: 2450000, maxRollouts: 5}` |
| `receiptViolation` | false |
| Provider receipt | `workshop.secrets_proxy` · 50 calls · `$0.016544` · digest `sha256:39af573b6abf9cee0bb8811416438772e7ce1323d4e39845ee554db61ddb07a7` |
| Capability | `cap_38f3601415484cb08c9aa916deb10325` · **exhausted** (50/50 budgeted calls) · not `revoked` |
| Source | `configured_env_file` · `container_proxy` · still registered |
| Keychain | not used |

Exhausted vs revoked: this grant was consumed by completing the admitted
envelope, not torn down after a producer failure. The file-backed source
remains. That is the success path for a fully used lease.

## Scorecard

| Criterion | This run |
| --- | --- |
| Seeds `780005..780009` under pinned runtime/policy/model | yes |
| No journal digest / chain-head mismatch | yes |
| Five stable rollout IDs before settlement | yes |
| Five rollouts terminalize | yes, all `completed` |
| Five sealed Trace V5 | yes |
| Numeric evaluator reward per rollout | yes (1, 3, 1, 2, 2) |
| Provider receipt, run usage, terminal agree | `$0.016544` / 50 calls |
| Paid approval `$2.45` / 5-rollout envelope | yes |
| Capability settled; file-backed source remains | exhausted; `configured_env_file` |
| `GET …/result` not `evidence_missing` | HTTP 200, `meanReward` 1.8 |
| Gold Craftax only | `env:craftax_gold` |
| Image digest = producer checkout | `c683876b` / `c0c01101` |
| sourceRoot + sourceRevision agree with image | yes |
| No Keychain | yes |

## Re-check (2026-08-28, after restart)

Restarted instance `j` (pid `79173` → `4108`). Same compiled binary
`364d2978b1fb`, same executable digest `sha256:14c7ebc3…`. New ports
`:65132` (eval driver) / `:65131` (visuals IPC). `GET /v1/health` and
`GET /v1/preflight` agree (`buildRevision` / `sourceRevision`
`364d2978b1fb`). `paidCompute` still `{requiresBoundedCap: true}` only.

Then `GET` run `opt_eval_craftax_b130a1d92a02`, `GET` primary visual
`vis_14bcb337cdf54e87b2a9be98d109359a`, and `POST …/open_visual` twice.

| Check | Result |
| --- | --- |
| Authoritative cost | `$0.016544` / 50 calls before, after boot, after both opens |
| Visual cost pointer `/inputs/0/data/progress/cost` | `$0.016544 / $2.45` |
| Visual revision | stayed `14` (projection refresh was a no-op; cost already current) |
| Trace V5 ids + digests | identical to the sealed set above |
| Evaluator result refs + ledger | identical; 5/5 `sealed_complete` |
| Provider receipt digest | `sha256:39af573b6abf9cee0bb8811416438772e7ce1323d4e39845ee554db61ddb07a7` |
| Second `open_visual` | no revision bump, no extra refs beyond first open |

First `open_visual` attached a presentation visual
`vis_9a528e2746d444af93d0f90a7335c093` (`role: trace`) and emitted
`visual.show`. That is a show-path attachment, not a rewrite of the
terminal manifest, traces, or cost. The second open did not add another.

Unit test `reopening_a_terminal_inline_eval_refreshes_a_stale_cost_projection_exactly_once`
(with `--features eval-driver`): **pass**.

Playwright, exact line gates, no weakening:

| Spec | Result |
| --- | --- |
| `visual-responsive-gate.spec.ts:242` Craftax semantic viewer | **pass** |
| `optimizer-banking77.spec.ts:262` unresolved live binding | **fail** — `optimizer-run-unavailable` never appears after `synthOptimizers.get` throws `"run is offline"`; still no GEPA demo candidates assertion reached |

Still not claimed: chain-fold golden fixture.

## What had to change to admit this closure

1. Operator chose the new pin over restoring `bc4bbeab` / `92bb5b36`.
2. Workshop `364d2978` forwards admitted limits into materialize (same shape
   already on `workshop-v08-freeze`).
3. `j` rebuilt ad-hoc and relaunched; health matches `364d2978`.
4. Single `evaluations/start` on the visuals IPC.

Retired gold image `sha256:bc4bbeab…` is no longer the acceptance identity.
