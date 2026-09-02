# Banking77 real SFT + CISPO Workshop proof

**Date:** 2026-09-02  
**Model:** `openai/gpt-oss-20b` on Tinker  
**Data:** real Banking77 train, selection, and locked heldout splits  
**Fixtures:** disabled  
**Workshop revision:** `33d2fd2466ed`  

## Outcome

Workshop ran both SFT and CISPO against real data and streamed their public
optimizer event pages into live visuals. SFT produced a statistically supported
heldout uplift. CISPO produced material selection uplift, but its locked heldout
uplift remained inconclusive. These claims must not be conflated.

| Run | Selection result | Locked heldout result | Verdict |
| --- | --- | --- | --- |
| SFT, 100 steps | 79.50% → 86.75%, +7.25 pp, 95% CI [+4.50, +10.25] | 81.25% → 87.50%, +6.25 pp, 95% CI [+3.25, +9.25], McNemar p=0.0000413 | Material heldout uplift |
| CISPO, 50 updates | 83.25% → 87.00%, +3.75 pp, 95% CI [+1.50, +6.25] | 85.25% → 86.75%, +1.50 pp, 95% CI [-0.75, +4.00], McNemar p=0.286 | Selection uplift; heldout inconclusive |

## SFT

- Job: `sft_banking77_nanoclassify_reference_ba558ab3`
- 100 training steps
- 400 paired selection examples
- 400 paired locked heldout examples
- Promoted checkpoint: `inference-100-f9856413e3ed`
- Heldout differences: 31 improved, 6 regressed, 363 unchanged
- Valid-label rate: parent 100.00%, trained 99.75%
- Exact promoted parent references are recorded in
  `banking77-sft-promoted-parent.json` beside this receipt.

Selection checkpoints:

| Step | Accuracy | Uplift | 95% paired CI | Verdict |
| ---: | ---: | ---: | ---: | --- |
| 25 | 81.00% | +1.50 pp | [-1.00, +4.00] | Inconclusive |
| 50 | 84.50% | +5.00 pp | [+2.00, +8.00] | Material uplift |
| 75 | 83.00% | +3.50 pp | [+0.75, +6.50] | Material uplift |
| 100 | 86.75% | +7.25 pp | [+4.50, +10.25] | Material uplift; promoted |

## CISPO

- Job: `cispo_hosted_cb015736757f`
- Exact SFT step-100 state used as the training parent
- 50 updates, 3 prompts per update, group size 64
- 9,600 on-policy sampled trajectories
- 150/150 rollout groups completed; none failed
- 49 provider training receipts, 163,609 training tokens
- 400 paired selection examples per checkpoint
- 400 paired locked heldout examples per arm
- Promoted checkpoint: `inference-50-16636b4216b2`
- Heldout differences: 14 improved, 8 regressed, 378 unchanged
- Valid-label rate: parent 94.00%, trained 96.25%
- Materialized `policy_bundle.json` digest:
  `sha256:f6ab3e1faa1c602f2392f8bbcdd3cafd8b00f308a315f1a20a1a3dbf4a6bea4a`

Selection checkpoints:

| Update | Accuracy | Uplift | 95% paired CI | Verdict |
| ---: | ---: | ---: | ---: | --- |
| 10 | 84.50% | +1.25 pp | [-0.75, +3.25] | Inconclusive |
| 20 | 84.50% | +1.25 pp | [-1.00, +3.50] | Inconclusive |
| 30 | 85.50% | +2.25 pp | [+0.25, +4.50] | Material uplift |
| 40 | 85.25% | +2.00 pp | [0.00, +4.25] | Inconclusive |
| 50 | 87.00% | +3.75 pp | [+1.50, +6.25] | Material uplift; promoted |

## Workshop live-visual evidence

- Optimizer run state: `completed`
- Terminal cursor: `3642`
- Training event cursor: `3641`
- Terminal evidence completeness: `complete`
- Live visual: `vis_a6a117bc8b934d6e9678a9e6a57ec369`
- Template: `optimizer.cispo.live.v1`
- Template digest:
  `dbdc55c5d62ca410d43b1a44dbb2767d17fe9a043429a2a85a70e089d4502e35`
- The Workshop read model reports 150 completed rollout groups, 0 running,
  0 queued, 0 failed, and 0 cancelled.
- The stream included 3,200 per-example evaluation events, 150 rollout-group
  completions, 150 group-advantage events, 49 importance-ratio events, 25
  zero-advantage detections, 50 update completions, five checkpoint evaluations,
  the locked heldout result, materialization, and terminal completion.

The live visual was refreshed after terminal completion and re-opened in its
Workshop session. Its subscription-ready receipt binds it to the run and the
template above.

## Runtime findings

Two provider-adapter bugs were fixed before the successful run:

1. Live sampler names are scoped by training step so post-update cache
   invalidation cannot collide with a previously persisted sampler.
2. Live sampler creation is single-flight so concurrent rollout calls cannot
   race to persist the same sampler name.

The successful run also survived a provider connection outage of roughly 36
minutes after checkpoint 20. The job remained non-terminal and resumed from the
same process and training state when provider polling recovered. No result was
fabricated and no partial run was promoted.

## Claim boundary

The correct product statement is:

> Real Banking77 SFT is proven to improve the locked heldout set. Real CISPO is
> proven to run end to end through Workshop and to improve its selection set,
> but this CISPO run does not establish heldout uplift.

Provider cost is not available in the Tinker receipts (`cost_missing=true`), so
the receipt records the approved cap rather than inventing an actual charge.
