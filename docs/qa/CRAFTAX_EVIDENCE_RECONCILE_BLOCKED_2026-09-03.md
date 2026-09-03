# Every Craftax evaluation is unreconcilable

**Date:** 2026-09-03
**Instance:** `visualqa` (source `495014b58ab0-dirty`)
**Impact:** all Craftax eval cells; no Craftax receipt can reach `complete`.
**Owner:** the in-flight change in `container_eval.rs`, not committed at time of writing.

## Symptom

`optimizer_reconcile_evaluation_evidence` fails on every Craftax run:

```
full_trace_step_count_missing: rollout `roll_craftax_train_780005_e16b54e0`
has no terminal environment-step count
```

Two runs, one healthy and one not, fail identically — so this is not a
consequence of the failed run:

| Run | Frames | Reconcile |
|---|---|---|
| `opt_eval_idem_52c0e5790ce8d8fe339e0dde` (Banking77) | none | **succeeds** |
| `opt_eval_idem_8844dd1fa3d614c54bb99412` (Craftax dev, `completed`) | 82 retained | fails |
| `opt_eval_idem_86bd896933b778a43b9dcff1` (Craftax QA, `failed` 4/5) | retained | fails |

Banking77 is the control: it reconciles, so the path itself works.

## Cause

Two predicates disagree about when frame coverage is checkable.

`requires_native_frame_coverage` turns the check **on** if *any* of three things
is true — a terminal step count, a non-zero imported frame count, or a non-empty
imported frame-step list:

```rust
terminal_record.get("steps").and_then(Value::as_u64).is_some()
    || imported.get("importedFrameCount")…is_some_and(|count| count > 0)
    || imported.get("importedFrameSteps")…is_some_and(|steps| !steps.is_empty())
```

`verify_complete_native_frame_trace` in `SealedComplete` mode can only *do* the
check with the first of those — it needs `steps` to build the expected set
`(0..=steps)` — and hard-errors without it.

Craftax satisfies the second and third disjuncts and not the first: it retains
native frames but reports no environment-step count
(`reportedFacts.steps = { value: null, unavailableReason: "steps_not_reported" }`).
So the guard switches the check on and the verifier immediately aborts.

The guard's own doc comment states the intent — "enforce contiguous frame
coverage only when the terminal record or imported bundle says this rollout is
frame-bearing" — and it achieves that for text-only and rubric-only evaluators,
which is what it was written for. Craftax is the case it does not cover:
frame-bearing *and* step-count-less.

## Why this is one cause, not four

It also explains three things previously logged as separate traps:

- the dev Craftax receipt staying `partial` with no stated reason;
- `requireTerminalDrain` not being inferred despite a sealed terminal trace;
- `steps_not_reported` being treated as a display concern ("do not render a
  false zero"). It is not cosmetic — it is a hard gate on evidence compilation.

## What the fix must not do

The tempting one-line fix is to drop the two frame-based disjuncts so coverage
is required only when `steps` exists. That is wrong: it lets a container skip
the contiguity guarantee by omitting its step count, which removes the check
exactly where frames exist to be dropped.

The honest position is that **without an independently reported step count,
coverage is unverifiable** — the maximum retained frame is trivially covered by
the frames themselves, so the bundle cannot attest to its own completeness.

So a missing step count should mark that rollout's evidence `partial` with
reason `steps_not_reported` and let reconciliation continue, rather than
aborting the whole run's receipt. One unreported counter currently destroys the
evidence for every rollout in the run, including four that sealed cleanly.

The durable fix is on the container side: Craftax already knows its final
environment step and relays it through retained events, so it should report it
in the terminal record. Workshop's job is to degrade honestly until it does.

## Reproduce

```bash
node scratchpad/loop/mcp.mjs visualqa optimizers \
  optimizer_reconcile_evaluation_evidence \
  '{"optimizer_run_id":"opt_eval_idem_8844dd1fa3d614c54bb99412"}'
```

Swap in the Banking77 run id above for the passing control.

## Unrelated, found alongside

`data/optimizers/eval/runs/opt_eval_idem_86bd896933b778a43b9dcff1/PAUSE` still
exists and holds `2026-09-03T13:57:29.622729+00:00`, while the projection
reports the run `failed` and `runState: terminal`. The pause marker is not
cleared when a paused run reaches a terminal state, so anything that reads the
marker rather than the projection will report a dead run as resumable.
