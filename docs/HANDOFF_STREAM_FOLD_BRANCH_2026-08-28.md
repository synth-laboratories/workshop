# Handoff: `feat/user-visual-templates`

**Date:** 2026-08-28
**Branch:** `feat/user-visual-templates`, 21 commits on top of `a506e26c`
**Plan it implements:** `docs/HANDOFF_STREAM_FOLD_VISUALS_CISPO_2026-08-28.md` (Parts I–VII, 42 items)

> **Read this first:** the branch was developed in a git worktree under
> `/private/tmp/claude-501/.../scratchpad/wt-user-templates`. **That path is temporary and may be
> cleaned at any time.** The commits are safe — the branch is real in the `workshop` repo — but
> `git worktree list` will show a stale entry once the directory goes. Move it somewhere durable
> (`git worktree add <path> feat/user-visual-templates`) before doing anything else.

---

## State

| Check | Result |
| --- | --- |
| `cargo check -p synth-desktop --lib --tests` | exit 0 |
| `cargo test -p synth-desktop --lib` | 1490 passed, **4 failed** |
| `npm run typecheck` (apps/synth_desktop) | exit 0 |
| `node --test visuals/tests/*.test.mjs` | 238 passed, 0 failed |
| `./scripts/conform-desktop.sh` | see below |

**The four Rust failures are pre-existing and inherited. Do not chase them.**

```
optimizers::eval_recipes::immutable_target_tests::a_mutable_tag_is_refused_before_the_run_is_created
optimizers::manager::tests::installed_service_has_offline_runtime
optimizers::service::tests::absent_capabilities_refuse_paid_start_instead_of_skipping_the_pin
plugins::policy::tests::never_auto_authorizes_risky_actions
```

Each fails in isolation in under a second, and `git diff a506e26c` shows all four files untouched by
this branch. They are red on `eval/inline-first-admission` too.

Conform counters this branch added, all at their floor: `sequence_fold_outside` 16 → **0**,
`sequence_gap_outside` 2 → **0**, `create_table_outside` 2 → **0**, `template_root_join` **0**,
`data_root_env_outside` **0**. `static_once_lock` went 16 → 18 mid-branch and is back to **16**.

---

## What this branch does

**The live-eval fold has one home.** `src/stream_fold.rs` (crate root, not under `visuals/`, because
the optimizer and Intern journals ask the same questions). Identity, scope, dedupe, conflict
detection, gap scanning, and `project_live_eval` were previously in seven places. Equivalence is
pinned by `visuals/fixtures/live_fold_golden.json` — 21 cases captured from the *unmodified*
TypeScript fold before anything changed, asserted from both languages. **If you touch the fold,
regenerate that golden and expect it byte-identical. A diff there is a behavior change, not a diff
to accept.**

**Sealing works.** It did not before, for either major visual class. `freeze_bindings` required a
`snapshot` key nothing wrote, and `trace_v5` bindings froze a CAS pointer the reader would not have.
Both now resolve at seal time from what the host holds.

**User-authored visual templates.** `~/.synth-desktop/visuals/templates/<id>/{template.json,shell.tsx}`
— discovered at runtime, compiled in the pane through the existing sourced path, hot-reloaded, and
sealed with their source embedded. No rebuild, no checkout.

**A document viewer** in the right panel, which is also the second `Pane` provider.

**A readiness gate** on the stream receipt, with five named refusals.

---

## Bugs found that were not in the plan

Every one was live before this branch, and each was found by tooling or verification rather than by
reading.

1. **`build_index_html` escaped `</script` case-sensitively** while HTML end-tag matching is
   case-insensitive. A producer message containing `</SCRIPT>` closed the JSON island in a sealed
   artifact and injected markup. Theoretical until this branch let producer evidence into that
   island, then reachable. Fixed the same day.
2. **`live_spool.rs` persisted one lane in ten.** Its identity rule treated a bare `event_id` as
   globally unique. A 997-envelope verbatim ten-lane capture is now a fixture.
3. **`REQUIRED_TABLES` named the wrong DDL for both LoRA tables**, so `heal_missing_tables` was inert
   for one and created the wrong table for the other — the repair path built to survive a
   migration-version collision, broken in the situation it exists for.
4. **`TerminalPanel.tsx` rendered the tail before the head** — a live event arriving before its
   snapshot was written ahead of the snapshot's earlier bytes.
5. **Hosted CISPO runs starved both Rust readers.** `training_adapter` said `training.metrics`,
   `sidecar_training` said `sft.training.metrics`; TypeScript handled both, Rust did not.
6. **`optimizerSteps` reported 1 for a 50-step run** — a `Math.max(..., 1)` shadowing the real
   fallback.
7. **`MetricStrip` was allowlisted but unreachable** — the specifier list said yes, the module map
   provided only `VisualChrome`.
8. **`live.harbor_eval.v1` could not be certified at all** after an `observationContract` was added
   without the shell publishing transport state. Introduced and fixed within this branch.

---

## Landmines

- **The fold's golden fixture is the safety net.** See above. It is the only thing standing between a
  refactor and silently corrupted sealed artifacts.
- **`retain_events` is off for live streams on purpose.** The reasoning is in `FoldLimits`' doc
  comment: it is the affordance for folding a *finite* log the caller already holds. Turning it on
  for a live stream is a second, unbounded copy of evidence the host already keeps under a bound.
- **`project_sft_result` and `materialize_sft_result` have no production caller.** The reachable SFT
  result path is `optimizers/results.rs:152`. Fixes in `sft_result.rs` are correct but dormant.
- **`scripts/stage-internal-visuals.sh` prefers a root that *holds* a template**, not one that
  exists. The app creates the new root regardless, so an existence test reproduces the regression
  this branch already fixed once.
- **The user tier skips a broken template; bundled tiers still fail loudly.** Different trust levels,
  deliberately different policies. Do not unify them.

---

## What remains

### Blocking a merge

1. **Nothing has run in the actual app.** No template authored → edited → hot-reloaded → sealed. The
   document viewer has never rendered a pixel. The approval card has never been drawn.
   `desktop:verify` has never run on this branch. **This is the highest-value next action** — it is
   the cheapest way to find what 1,490 passing tests did not.
2. **Rebase.** The branch is based on `a506e26c`; `eval/inline-first-admission` has moved since.
3. **Human review.** ~110 files, built by parallel agents with partitioned file ownership. No single
   author saw the whole shape, and neither has anyone else.

### Decisions, not code

- **`chain_of_thought` / `hidden_reasoning` scanning.** They moved to the metadata-only bucket, so
  they are no longer refused inside frozen evidence. An eval's recorded reasoning genuinely *is* the
  evidence — but this is a privacy policy, not a host-config one, and it deserves a second opinion.
- **Lane collapse under a shared `stream_id`.** `envelope_scope` ranks `stream_id` above
  `rollout_id`, so a producer multiplexing rollouts under one stream id shares a sequence *and*
  identity namespace, and the second lane is dropped as a duplicate. Both implementations agree, so
  it is inherited design. It is pinned by a test and undecided.
- **Managed `renderer.html` seals** still carry item 28's defect. Fixing it the obvious way trips
  `refuse_network_html` and turns currently-sealable managed visuals into failed seals. Needs a
  decision about what a managed seal means.

### Known and unfixed

`evidence_refs` still emits a trace *record id* rather than an archive digest — the last `trace_v5`
in `artifacts.rs` naming something a reader cannot resolve. `evaluations_from` matches a narrower
event set than producers emit, but widening it would let a heldout measurement count toward a
selection verdict, which is a semantics call. `import_meta_glob` is still 7 and
`optimizer_template_id` still 9.

### Deferred by the plan, not oversight

Items **18–22** (per-plugin schema boundaries) — "only once a real second plugin exists."
Item **30-finish** (retiring `stage-internal-visuals.sh`) — refused; three prerequisites are
documented in `visuals/templates-internal/README.md`, chiefly that the sourced allowlist has eleven
exact specifiers and no relative-path resolution, which both real internal templates need.
Item **39** tab persistence — tab state is view state; persisting it would have the renderer minting
durable state (§8).

### Cross-repo — order matters

**synth-mlx-rl → optimizers → workshop.** `advantage_mean`, `reward_variance` and `advantage_std`
appear in **zero files** across all three optimizer repos. The Desktop side forwards them and the
CISPO panel shows "Not reported by this runtime" honestly, but nothing emits them. Start at
synth-mlx-rl or you will get em dashes.

Also: `containers` needs one real Harbor run (no code — the Workshop side is complete and enforced);
`optimizers` needs `page`+`cursor` emission so the receipt's gap detection is trustworthy rather than
best-effort; `backend` needs the releases-manifest decision and artifact upload limits;
`optimizers-beta` needs a disposition (zero CISPO files, eight days stale, and an OSS liability).

---

## Verify it yourself

```bash
cd apps/synth_desktop/src-tauri
cargo check -p synth-desktop --lib --tests -j 4
cargo test  -p synth-desktop --lib -j 4          # expect 1490 / 4 pre-existing
cd .. && npm run typecheck                       # expect exit 0
cd ../.. && node --experimental-strip-types --test visuals/tests/*.test.mjs
./scripts/conform-desktop.sh
```

If `protocol.ts` drifts:
`cargo test -p synth-desktop --lib regenerate_protocol_bindings -- --ignored`, then review the diff.
A plain `cargo test` reports drift and never silently repairs it — that is deliberate.
