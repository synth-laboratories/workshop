# Pinned acceptance manifest — v0.5 packaged workflows

Produced at the close of the integration assignment. Assignment 2 (packaged
CUA acceptance) runs against exactly these pins.

**Status: two lanes are ready to run, one is blocked.** Read "Blocked" before
starting.

---

## Pins

| Component | Pin | Pushed |
|---|---|---|
| Workshop | `a96bf17c` on `v0.5/banking77-healthbench-cua-fix` | yes — `origin/v0.5/banking77-healthbench-cua-fix` |
| Containers (pinned/consumed) | `ac43172`, version `0.4.1.dev20260817` (`origin/dev`) | yes |
| Containers (required candidate) | `fd876ca` on `candidate/dev-eval-readiness` | **no — push to `dev` blocked, see Blocked #1** |
| Optimizers sidecar | `0.2.14` (wheel `synth_optimizers-0.2.14-cp311-abi3-macosx_11_0_arm64.whl`) | published wheel; source not in the local checkout |
| Optimizers floor in Workshop | `contract/runtimes.rs`: `official = min_supported = "0.2.14"` | n/a |
| Cookbook | `9d82a6a` ("Merge pull request #9 from synth-laboratories/dev"), detached at `/Users/joshuapurtell/Documents/Codex/2026-08-16/imp/work/cookbooks-banking77-v04-pinned` | on `synth-laboratories` dev |
| Craftax eval image (published) | `ghcr.io/synth-laboratories/craftax-eval-target@sha256:a3b6ee4047585d9e79bb3eea785fcbac4488ae71e950928d1d35aceba6143499` — arm64/linux | yes, but **private** |
| Craftax eval image (currently pinned) | `craftax-eval-target@sha256:d1b3eaccfd833f0f67eaf682be0ea162e93ddacb71db944be9b3e03c82cd09bd` — **local-only, never pushed** | no |

### Packaged app build identity

No packaged release build of `a96bf17c` exists yet. Build it as the first step
of Assignment 2:

```
cd <workshop>/  &&  ./scripts/workshop-qa prepare <name>
```

- Instance root: `$SYNTH_DESKTOP_INSTANCES_ROOT:-~/.synth-desktop/instances}/v04/<name>`
  (note: the script says `v04` while existing instances live under `v05`; it is
  self-consistent — `prepare` creates the path `run` uses — but do not expect to
  reuse a `v05` instance).
- Driver descriptor: `<instance>/data/eval-driver.json`, schema
  `synth.eval-driver.v1`.
- Record from `<instance>/instance.json`: `appVersion`, `bundleId`,
  `executableDigest`, `provenance.sourceRevision`. `sourceRevision` must read
  `a96bf17c` with **no** `-dirty` suffix. All pre-existing instances are stale
  debug builds at `31a2246438b5-dirty`; none of them is this candidate.
- `./scripts/workshop-qa preflight <name>` must exit 0 before any run.

---

## Expected recipes and limits

### `gepa.banking77.luna.v1` — Banking77 GEPA
Proposer `gpt-5.6-luna`, reasoning effort medium, `auth_mode = chatgpt`.
Policy `gpt-4.1-nano`.

| Bound | Value |
|---|---|
| `max_generations` | 1 |
| `proposals_per_generation` | 10 |
| `minibatch_size` | 20 |
| minibatch **pool** | 50 (`train:0..49`) — must exceed `minibatch_size` |
| train rows / heldout rows | 50 / 50 |
| `max_train_rollouts` | 750 |
| `max_heldout_rollouts` | 100 |
| `max_total_rollouts` | 850 |
| `max_cost_usd` | 9.00 |
| proposer timeout / stall timeout | 300s / 300s |

Expected train spend: 50 seed + 10 × (20 candidate-minibatch + 20
parent-reference) = 450, leaving room for six full-train promotions at 50 each.

### `eval.healthbench.smoke.v1` — HealthBench eval
2 train seeds `[0, 1]`, 2 heldout seeds `[100, 101]`. Separate `policy` and
`scorer` (grader) usage lanes. Admission refuses before dispatch when a
declared lane reports `credential_present: false`, **and** when the container
advertises no `metadata.model_roles` at all.

### `eval.craftax.llm-policy.smoke.v1` — Craftax eval
This is the exact recipe. `substitutionAllowed: false`; it must never become
`gepa.craftax.*`. Currently `availability: unavailable`
("target image is not published and pinned yet").
`eval.craftax.code-policy.smoke.v1` runs 10 trials per candidate
(`CRAFTAX_CODE_SMOKE_TRIALS_PER_CANDIDATE = 10`).

### `eval.banking77.baseline.v1` — Banking77 eval (already green)
10 train seeds `[0..9]`, concurrency 10, harness `desktop_eval`, policy config
`banking77_gpt_4_1_nano` — verified against the container's advertised
immutable policy, not registered via a POST.

### `gepa.craftax.smoke.v1` — Craftax GEPA (follow-on)
1 proposal/generation, minibatch 1, train 1 / heldout 1, `max_total_rollouts`
6, `max_cost_usd` 1.50, `CRAFTER_MAX_TURNS` 8. Deliberately **not** Banking77's
contract.

---

## Static suites at `a96bf17c`

| Suite | Result |
|---|---|
| `cargo test --lib` | 824 passed, 0 failed, 3 ignored |
| `node --test tests/*.test.mjs` | 406 passed, 0 failed |
| `npx tsc --noEmit` | clean |
| Containers `pytest tests/` at `fd876ca` | 336 passed, 8 skipped |

`cargo fmt --check` is dirty across ~24 files, nearly all pre-existing at the
branch point. No file this work touched added new formatting debt.

---

## Blocked — do not start the affected lane

### 1. Containers `dev` does not carry the credential-readiness contract
`origin/dev` is `ac43172`. Five commits it lacks are on
`josh/annotations-list-achievements`, merged and tested locally as `fd876ca`:

- `0bb91f9` execute the registered Banking77 OpenAI classification policy
- `4b81852` allow the pinned Banking77 OpenRouter chat policy
- `e141545` **advertise HealthBench credential readiness by lane**
- `2f2399b` prepare the 0.4.1 dev eval runtime
- `f558140` **`_simulate_or_fail`: a raising rollout must terminalize its pin**

`e141545` is what emits `metadata.model_roles[*].credential_present`. Without
it Workshop's HealthBench admission now refuses with
`credential_readiness_unavailable` — correct fail-closed behaviour, but the
lane cannot pass until the merge lands and a new dev wheel is published and
repinned.

`f558140` is the `_simulate_or_fail` regression fix. **It is not at the pinned
commit** — it is on the feature branch only.

Merge `fd876ca` is clean (one conflict in `tests/test_platform_leftovers.py`,
resolved keeping dev's `SYNTH_CRAFTAX_URL` pin) and the full suite is green.
The push to `origin/dev` was refused by this session's permission policy and
needs Josh.

Publishing the resulting dev wheel is a `workflow_dispatch` on
`.github/workflows/publish-pypi.yml` (channel `dev`). It does **not** fire on a
branch push, only on dispatch or a `v*` tag.

### 2. Craftax eval cannot be unblocked from this session
Two independent credential/source blockers:

- The `gh` token has scopes `admin:public_key, gist, read:org, repo` — no
  `read:packages` or `write:packages`. It cannot read the GHCR package, push an
  image, or change visibility. `orgs/synth-laboratories/packages/...` returns
  403.
- The eval catalog that names the target ships **inside the pinned optimizers
  wheel**. The local `optimizers` checkout is `0.2.6.dev20260626` and contains
  no eval catalog at all (no `craftax-eval-target` reference, no `eval`
  module). The catalog change cannot be authored against the right baseline
  here.

So renaming the bare `craftax-eval-target` to
`ghcr.io/synth-laboratories/craftax-eval-target` and pinning
`sha256:a3b6ee40…` is an **Optimizers release**, not a Workshop change, and
needs the 0.2.14+ source plus package write access.

Until then Workshop refuses the lane deliberately: admission rejects
registry-less names, mutable tags, local-checkout paths, malformed digests, and
reference/digest disagreement. **This is a behaviour change** —
`eval.craftax.code-policy.smoke.v1` previously reported `available` and ran off
the local-only `d1b3eacc…` pin, producing a benchmark number reproducible on
one machine.

---

## Assignment 2 readiness

| Lane | Ready? |
|---|---|
| Banking77 GEPA | **yes** — this is the lane that answers the open empirical question |
| HealthBench eval | no — blocked on #1 |
| Craftax eval | no — blocked on #2 |
| Craftax GEPA, hosted SFT | follow-on only |

The open empirical question is whether the repaired Banking77 configuration
produces varied paired minibatches in a real run. The sealed terminal manifest
answers it directly — check `gepaEvidence.gate`:

- `distinctDraws` must be `> 1` (it was effectively 1 before)
- `allComparisonsPaired` must be `true`
- `gated`, `accepted`, `rejected`, `rejectionReasons` describe the gate
- `proposals.{requested,returned,registered,shortfall}` describe delivery

Positive uplift is not required. `no_measured_improvement` with honest
accounting is a pass.
