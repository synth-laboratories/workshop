# Root-cause analysis and fix plan: Craftax GLM 5.3 release-acceptance failures

Date: 2026-08-28
Companion to: `docs/HANDOFF_CRAFTAX_GLM53_RELEASE_ACCEPTANCE_FAILURES_2026-08-28.md`
Run: `opt_eval_craftax_899e1fe95813` · Workshop `j` at `7d7783b66e51` · producer revision `04e0a94aa333`

All seven investigation tracks from the failure handoff are root-caused below. Every claim
was verified against the code and the run's local evidence (`synth.sqlite3` event log);
none are hypotheses unless explicitly marked. File references are repo-relative to
`workshop-v08-e2e-refactor` unless another repo is named.

**Repos involved**

| Repo | Role | Local checkout |
| --- | --- | --- |
| `workshop-v08-e2e-refactor` | Workshop desktop (relay, admission, secrets, renderer, visuals) | `~/GitHub/workshop-v08-e2e-refactor` (branch `codex/finish-inline-eval-refactor`) |
| synth-containers | Journal producer (`event_log.py`, nanohorizon runner) | `~/GitHub/containers-nanohorizon-e2e-final` — **checked out at exactly the pinned producer revision `04e0a94a`** |
| nanohorizon | Pinned policy source (`src/challenge/policy.py`) | `~/GitHub/nanohorizon` |
| evals | Craftax image manifest / Dockerfile | `~/GitHub/evals-craftax-live-context` |

---

## 1. Journal digest mismatch at sequence 10 — ROOT-CAUSED, byte-level proof

**One sentence:** the producer hashes ASCII-escaped JSON (`—`) while Workshop hashes
raw UTF-8 (`—`), and sequence 10 is the first event whose payload contains a non-ASCII
character — em-dashes baked into the nanohorizon system prompt.

### Mechanism

- Producer digest — synth-containers `src/synth_containers/event_log.py:103-113`:

  ```python
  blob = json.dumps({"kind": kind, "sequence": sequence, "payload": payload},
                    sort_keys=True, separators=(",", ":"), default=str)
  # json.dumps defaults to ensure_ascii=True → non-ASCII becomes \uXXXX in the hashed bytes
  return hashlib.sha256(blob.encode("utf-8")).hexdigest()[:16]
  ```

- Workshop recompute — `apps/synth_desktop/src-tauri/src/optimizers/eval_relay.rs:747-783`
  (`verify_envelope_digest`):

  ```rust
  let canonical = serde_json::to_vec(&json!({ "kind": kind, "sequence": sequence, "payload": payload }))?;
  // serde_json emits raw UTF-8
  ```

- Key ordering and separators coincidentally agree (verified: `preserve_order` is NOT
  enabled on serde_json, so its BTreeMap sorts keys exactly like `sort_keys=True`, and
  both are compact). That is why the two implementations agree on any pure-ASCII payload
  and disagree the moment one non-ASCII character appears.

### Why deterministically sequence 10, every seed

From the run's own event log (`~/.synth-desktop/instances/v08/j/data/synth.sqlite3`,
table `optimizer_events`), sequences 1–9 of every rollout are the same nine structural,
pure-ASCII events:

`trace.opened, env.episode.opened, task_resolved, observation, frame, artifact.declared,
artifact.available, policy.session.opened, span.policy.opened`

Sequence 10 is the first `span.policy.data` — emitted at synth-containers
`src/synth_containers/policies/nanohorizon.py:791` — and the first payload to embed the
full `messages` array. The pinned policy source (run spec `sourceDigest` →
`nanohorizon/src/challenge/policy.py`) contains **em-dashes (U+2014) inside
`MECHANICS_GUIDE` and `SYSTEM_PROMPT`**:

> "…use \`do\` immediately **—** do not repeat the direction action…"
> "Assistant content is thinking, not the action list **—** never write the actions as JSON…"

Every rollout's first model call carries that prompt, so every seed fails at exactly
sequence 10; the digest pairs differ per seed because the observation text inside the
message varies.

### Proof

Reproduced both canonicalizations locally: identical 16-hex digests for an ASCII payload;
divergent digests for a payload containing one em-dash. Producer preimage contains
`immediately — do not`, Workshop preimage contains `immediately — do not`. Recorded
mismatches (e.g. seed 780008: computed `e81f320f624572a6`, declared `d0feb9797d8176e9`)
are consistent with this: same sequence, different payloads, different digest pairs.

This also closes failure-handoff question 6: the "verbatim provenance / never SHA-256"
comment at `eval_relay.rs:21-26` refers only to frame/PNG provenance digests and is not
implicated in envelope verification.

### Second latent divergence — fix in the same change

Float exponent formatting differs: Python emits `1e-05` / `1e+16`; Rust (ryu) emits
`1e-5` / `1e16`. Any payload float with |x| < 1e-4 or ≥ 1e16 breaks the digest even after
the unicode fix. This is not theoretical: OpenRouter per-call `cost` inside `usage` for
cheap glm-5.3-flash calls sits right at that boundary (this run averaged ~$0.0005/call).

### Fix

Preferred (kills the whole bug class): **verify over the producer's exact bytes instead
of re-serializing.** Capture each event's `payload` with `serde_json::value::RawValue`
in the page decode and hash the bytes the producer actually served, with the producer
contract being "the page embeds each event's canonical encoding verbatim". No
cross-language canonicalization to maintain, ever.

If recompute-and-compare is kept instead, the journal-v2 contract must pin ONE encoding —
either both sides escape non-ASCII (`ensure_ascii`-style; needs a custom escaper in Rust)
or both emit raw UTF-8 (`ensure_ascii=False` in `_digest`, `_persist`, and the page
serializer in `event_log.py`) — **plus** a shared float-formatting rule (hardest part;
this is exactly why the raw-bytes option is preferred).

Either way:

- Add the cross-language golden vector the failure handoff requires, exercised by both
  the Python producer tests and a Rust unit test in `eval_relay.rs`. Minimum cases: an
  em-dash string, CJK text, `1e-05`, `5e-5`, `1e+16`, `-0.0`, a nested map with mixed
  key insertion order, and an empty object.
- Do **not** weaken or bypass validation — it failed closed correctly here.
- Rerun seeds `780005..780009` per the reproduction script in the failure handoff; the
  acceptance bar is five sealed Trace V5 traces.

---

## 2. Terminal record missing `rolloutId` — ROOT-CAUSED: not a race

**One sentence:** ordering is correct and the ID is durably committed before dispatch;
the integrity-failure path then *rebuilds* the terminal record from scratch without it.

### Mechanism

- ID minted at `optimizers/container_eval.rs:3699`; `eval.trial.started` carrying
  `rollout_id`/`trial_id` is durably committed (`container_eval.rs:3743` →
  `eval_relay.rs:1222-1253`, transactional via `service.rs:1617-1666`) **before** the
  rollout POST is dispatched. The "settlement visible before identity commits" hypothesis
  is refuted.
- On `RelayIntegrityError`, `container_eval.rs:3796-3798` does a bare `return Err(error)`
  — dropping the in-scope identity. (Contrast the `CancelledError` branch at 3810+,
  which carefully builds a record *with* `rolloutId`/`trialId`.)
- The parent settles via `settled_child_error_record` → `failed_record`
  (`container_eval.rs:2512-2543`), which constructs the terminal record with **no
  `rolloutId`/`trialId` keys**, then writes it to both `eval.trial.terminal` (explicit
  null at `container_eval.rs:1904`) and `optimizer_runs.summary`.
- Reconciliation reads `summary.records` and refuses at `container_eval.rs:2689-2691`
  ("terminal record for seed 780008 has no rolloutId" — 780008 was merely first; all
  five lacked it).
- The final projection showed all five IDs anyway because the kernel eval algorithm
  self-heals: on a terminal event with null rolloutId it falls back to the ID remembered
  from `eval.trial.started` (`optimizers/kernel/algorithms/eval.rs:310-317`). So the two
  read authorities genuinely disagree — `summary.records` is the degraded copy.

### Fix

1. Primary: build the failed terminal record **inside `run_one_example`** where the
   identity is in scope (mirror the cancellation branch), or wrap the error in a typed
   struct carrying `rollout_id`/`trial_id` so `failed_record` can populate them. Keep
   `failed_record` bare only for genuinely pre-identity failures (never-dispatched rows).
2. Defense in depth: when a summary record lacks `rolloutId`, let `reconcile_evidence`
   resolve it from the kernel state it already loads (`container_eval.rs:2630`) instead
   of refusing on the weaker copy.

### Test (required by the failure handoff)

The mock harness already forces integrity failures: `CraftaxMockOptions { journal_v2:
true, skip_sequence: … }` (`container_eval.rs:6858-6875`; see existing tests at 7437 and
7561). Add: run an inline eval with a forced integrity failure on every seed, assert
(a) every `summary.records` row and every `eval.trial.terminal` event carries a non-null
`rolloutId` equal to its `eval.trial.started` ID, and (b) `reconcile_evidence` never
fails on identity grounds.

---

## 3. Visual stuck on "streaming" after terminal failure — ROOT-CAUSED

**One sentence:** the visual derives liveness purely from transport and trace vocabulary,
and it has **no optimizer-lifecycle binding at all**, so a run that fails without closing
the container stream can never look terminal.

### Mechanism

- `visuals/families/first_class_example_containers/live.craftax.v1/shell.tsx:292-294`:
  `visualLive = state === "live" && terminalLanes < lanes.length`. `state` goes terminal
  only when every stream's poll cursor reports `closed`
  (`visuals/chrome/useLiveEvalStreams.ts:152-153`); lanes go terminal only on kinds
  `eval.run.terminal` / `trace.reconciled` / `status∈{completed,finished,failed,cancelled}`
  (`shell.tsx:204-213`, same vocabulary in `projectCraftax.ts:550-555`).
- On evidence rejection the relay fails the trial and returns
  (`eval_relay.rs:429-447`); nothing closes the polled container stream and nothing
  appends any terminal marker the visual recognizes.
- The terminal fact exists only in the optimizer journal
  (`optimizers/eval_recipes.rs:1602-1632` maps `eval.run.terminal{failed}` →
  `optimizer.run.failed` — why the optimizer card was right). The visual could receive it
  through an `optimizer_run` binding (`VisualHost.tsx:664-668`, `subscribeToRun` at
  938-946), but **`live.craftax.v1/template.json` declares only a `stream` input** — the
  subscription early-returns. Green dot at `shell.tsx:411`, "Follow live" at
  `shell.tsx:469`.

### Fix

1. Add an optional `optimizer_run` input to `live.craftax.v1/template.json` and bind it
   at run attach; pass a derived `runLifecycle: { status, reason, evidence: { valid,
   rejected, sealedTraces } }` into the shell.
2. In the shell, lifecycle is the senior authority over transport: run failed ⇒ status
   "Failed" (never "streaming"), dot not green, "Jump to end" or disabled instead of
   "Follow live", replay controls disabled when there are no trustworthy replay events.
3. Server-side belt-and-suspenders: when settlement fails a run, append one durable
   terminal `status`/`eval.run.terminal` row per bound rollout stream (or revise the
   binding), so even a lifecycle-blind viewer's existing vocabulary flips terminal and
   the poll cursor can report `closed`.

---

## 4. Empty visual doesn't say why — ROOT-CAUSED (same missing edge as #3)

- "5 / 5 moments": `replayMomentIndexes` (`projectCraftax.ts:463-476`) deliberately
  counts every non-delta event as a moment, so five lifecycle/terminal markers render as
  five "moments". The honest `environmentStepCount` (0 for this run) is computed at
  `shell.tsx:275` and never rendered in the timeline heading.
- "No policy.call has been emitted at this temporal cutoff" (`shell.tsx:492`) cannot
  distinguish *rejected* from *absent* evidence because rejection is invisible to the
  visual (see #3).
- Seal disabled: `sealEligible` at `VisualHost.tsx:1493-1494`; the only explanation is a
  hover `title` on the button (`VisualHost.tsx:1694-1700`). Digest `—` comes from
  `formatVisualAdmissionIdentity` (`types/landing.ts:184-201`) with no seal receipt.

### Fix

1. Extend the evidence-state vocabulary with `rejected` (distinct from `missing`), fed by
   the relay's integrity detail; empty state becomes: *"37 provider calls occurred; all
   five journals failed digest verification at sequence 10 — trace evidence was rejected,
   not missing"* with failure code, failing sequence, and affected rollouts/seeds.
2. Timeline honesty: render `environmentStepCount` beside moments; when all moments are
   lifecycle markers, label them ("5 run markers · 0 environment steps") and disable Play.
3. Seal: render the disable reason visibly — "Seal unavailable — run failed with 0 sealed
   traces (evidence rejected)".
4. Tests: existing moment-vs-step tests are at `visuals/tests/craftax_semantic.test.mjs:101-126`
   and `craftax_viewer.test.mjs:259-270`; add the failed-evidence fixture (stream of only
   lifecycle markers) the failure handoff calls for.

---

## 5. Credential lease vs execution envelope — ROOT-CAUSED

**One sentence:** the agent-facing credential route always issues a hard-coded 40-call /
$0.60 policy, and no preflight compares any capability against the admitted envelope.

### Mechanism

- `/v1/secrets/use` (`visuals_ipc.rs:3905`, policy built at 4006-4018) always returns
  `ProviderUsePolicy::default()` — `max_calls: 40`, `max_cost_usd: 0.60` hard-coded at
  `secrets/capability.rs:119,122`. By design the agent cannot widen it; the operator
  approval for authority #1 always says 40/$0.60.
- The paid-compute modal (authority #2) is built independently from the recipe envelope
  in `authorize_inline_evaluation_start` (`lib.rs:1296-1323`) and never reconciles
  against existing capabilities.
- `request_use` (`secrets/mod.rs:826-828`) silently returns an existing **narrower**
  capability for the same `(secret_id, run_id)` without policy comparison.
  `ProviderUsePolicy::intersect` (`capability.rs:129-140`) exists and is unused.
- Enforcement itself is sound and fail-closed: call 41 → `exhausted` → HTTP 429
  `budget_exhausted` (`capability.rs:317-350`, `proxy.rs:503-517`); cost is debited
  post-response so an over-spend is caught at the next reserve. Meaning: had the run
  exceeded 40 calls through the agent-issued capability, it would have died mid-run —
  the operator's $2.45 consent was unfulfillable through that lease.
- Mitigating detail: when Workshop's own runner wires the proxy, it derives the policy
  from the admitted envelope (`container_eval.rs:2167-2182` →
  `admission/spec.rs:453-494`) — the undersized lease bites specifically when the agent
  wires its own capability via `/v1/secrets/use`.

### Fix

1. In `authorize_inline_evaluation_start`, between admission and the PaidCompute
   approval: compute the envelope policy (`spec.provider_use_policy()`, already
   implemented) and **refuse to start** if any capability the run will ride has
   `max_calls < rollouts × callsPerRollout` or `max_cost_usd < hard ceiling`. Emit a
   structured `capability_underscoped` error naming both sets of numbers.
2. In `request_use`: when an active capability exists, compare policies and return a
   conflict instead of silently handing back the narrower grant.
3. Let `/v1/secrets/use` accept a run-envelope reference and size the issued policy as
   `envelope.intersect(product_ceiling)` so the displayed/approved numbers match one
   authority; when both modals must appear, state the relationship explicitly.

---

## 6. "Revoke" unregistered the file-backed source — ROOT-CAUSED

**One sentence:** the clean revoke primitive exists but is not exposed to the agent, so
the agent's only available cleanup verb (`source_remove`) unregisters the reusable
source as a side effect — and the manual cleanup was redundant anyway.

### Mechanism

- `revoke_capability` (`secrets/mod.rs:1096-1099` → `capability.rs:483-510`) only flips
  status to `revoked` and never touches `secret_refs`, locators, or loaded material. It
  is exposed only as the settings-UI command `secrets_revoke_capability`.
- The agent's `secrets_manage` MCP tool (`bin/synth_secrets_mcp.rs:96-122`) has **no
  revoke operation**. Its `source_remove` → `remove_locator_source`
  (`secrets/mod.rs:635-697`) deletes the `secret_refs` registration, unloads material,
  revokes the source's capabilities — and returns `"status": "unloaded"`, a name that
  hides the unregistration. `registered=false, loaded=false` is exactly its post-state.
- Run capabilities auto-revoke at terminal state (`optimizers/service.rs:1952`), so no
  manual cleanup was needed at all.

### Fix

1. Add a `use_revoke` (or `capability_revoke`) operation to `secrets_manage` and a
   `/v1/secrets/use_revoke` route taking `capabilityId`/`runId`, calling only
   `revoke_capability`/`revoke_run`.
2. Split `source_remove` semantics: "unload material" (clear memory, keep registration)
   vs "unregister source" (delete `secret_refs`); at minimum stop returning `"unloaded"`
   for an operation that unregisters.
3. Lifecycle test (fits fixtures near `secrets/mod.rs:1907`): register file-backed
   source → issue one-run capability → revoke it → assert capability unusable and absent
   from active leases (`proxy_rejects_disallowed_model_and_revoked_capability` pattern at
   `mod.rs:2288-2325`) → assert the source still reports `registered: true` and reloads
   through a fresh explicit approval. No credential values read; no Keychain.

---

## 7. Contradictory cost telemetry — ROOT-CAUSED

**One sentence:** the usage pipeline only knows producer-emitted cost; the proxy's
metered cost arrives via a separate poll that is bolted onto the projection as display
metadata nothing in the usage code reads — and the terminal receipt is folded back only
on the container-eval path.

### Mechanism

- "Cost unavailable · producer emitted no cost telemetry" is printed by `costSummary`
  (`renderer/src/runtime/runProgress/usage.ts:112-117`) whenever the producer-event-derived
  metric is absent. `CoveredMetricSource` (`types.ts:102`) has no "proxy" variant.
- The live "$x / $2.45" on the same card comes from a 2.5-second capability poll
  (`useRunProgress.ts:90-116` → `providerAccessFromSecrets`) merged as a sibling field
  (`useRunProgress.ts:120-124`); `RunProgressCard.tsx` renders both lines (151 vs
  186-188) with no reconciliation.
- The terminal receipt ($0.018659 / 37 calls; `provider_usage_receipt`,
  `capability.rs:525-608`) is reconciled into the run event log only by
  `append_provider_usage_reconciliation` in `container_eval.rs:1688-1796` — the
  inline/relay paths never call it, so those runs stay "unavailable" forever while the
  durable receipt sits in SQLite. `providerAccessFromSecrets` also deliberately returns
  `undefined` once terminal (`providerAccess.ts:32`), so the proxy line vanishes exactly
  when the receipt matters.

### Fix

1. Add `"proxy"` to `CoveredMetricSource` and `SOURCE_WORDS`.
2. Merge the proxy figure into the cost metric where the two flows already meet
   (`useRunProgress.ts:120-124`): if producer cost is null and `usedCostUsd` is present,
   substitute `{ value, source: "proxy", coverage: 1 }` (the proxy meters every call by
   construction). Keep producer cost when present; show the proxy figure as cross-check.
3. Call the receipt reconciliation from **every** eval settlement path before the
   terminal seal so `optimizer.usage.reconciled` lands inside the terminal cursor; treat
   that event as full-coverage proxy source, not 1-of-N rollout coverage.
4. Labels: "$0.0187 metered by Workshop proxy · $2.45 cap" (live), "$0.0187 · provider
   receipt (37 calls) via Workshop proxy" (terminal). "Cost unavailable" becomes
   reachable only when there is genuinely no proxy capability and no producer telemetry.

---

## Suggested fix order

| Phase | Items | Why |
| --- | --- | --- |
| 1 — release gate | #1 digest contract + golden vectors; #2 identity in failed records | These block the five-seed acceptance rerun. #1 changes producer + relay together; #2 is a contained Rust change with an existing forcing harness. |
| 2 — same seams | #5 envelope preflight; #6 revoke surface | Small Rust changes in admission/secrets; both have crisp tests. |
| 3 — parallelizable UI | #3/#4 lifecycle binding + evidence-rejected UX; #7 cost sources | Renderer/template work, independent of phase 1. |

After phase 1 lands, rerun the reproduction in the failure handoff (fresh instance,
seeds `780005..780009`, 50-call/$2.45 single envelope) and hold it to that document's
acceptance criteria — five sealed Trace V5 traces, consistent lifecycle states, accurate
cost labels, capability revoked with source intact, no Keychain.

## Additional suggestions (beyond the seven tracks)

1. **Sweep for other duplicated canonicalizations.** The digest bug is a class:
   `journal_chain_genesis`/`journal_chain_extend` in `eval_relay.rs:739-745` currently
   match `event_log.py` `chain_genesis`/`chain_extend`, but they are hand-mirrored in two
   languages with no shared vector. Whatever golden-vector harness #1 introduces should
   cover the chain-head fold too, and any future digest added to the contract should be
   required to ship with a vector on both sides (CI on both repos).
2. **Prompt hygiene is not the fix — but note it.** Replacing the em-dashes in
   `nanohorizon/src/challenge/policy.py` would mask the bug for this policy and it would
   return with the first non-ASCII model output or observation. Fix the contract, not the
   prompt. (Model outputs from GLM routinely contain non-ASCII; only the deterministic
   prompt made the failure reproducible at sequence 10.)
3. **Persist the rejected raw page for diagnostics.** The relay currently discards the
   offending event on integrity failure; quarantining the raw page bytes (clearly marked
   unverified, never treated as evidence) would have made this a five-minute diagnosis
   instead of a source-archaeology exercise, and gives the golden-vector work real field
   preimages.
4. **Evals-repo hygiene from the earlier defects.** The `image.toml` gamebench-path fix
   exists in the detached checkout and in `evals-craftax-live-context` (`4726e2bd3` "Add
   isolated Craftax live-eval catalog"); verify it is pushed to the authoritative evals
   remote before release. Also keep the promised regression coverage for
   `container_image_digest_missing` and the manifest-parse diagnostics.
5. **Adopt the transcript contract.** The failure handoff's five-line transcript contract
   (preflight / approval / progress / failure / final) is worth enforcing in the agent
   harness; the #5 preflight refusal and #6 `use_revoke` verb remove the two places the
   previous transcript was forced into misleading improvisation.
6. **The trace workstation visual (`vis_e2760320…`) shares template lineage with
   `live.craftax.v1`** — when fixing #3/#4, check whether other first-class container
   templates also lack an `optimizer_run` input before calling the family done.
