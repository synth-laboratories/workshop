# Handoff: annotations across Containers, Evals, and Workshop

**Date:** 2026-09-01  
**Status:** the post-hoc annotation path is implemented and proven on a real Craftax rollout, including a paid rubric verifier and the Workshop Rubric UI.  
**Scope:** current architecture, repository ownership, proof receipts, and the decision boundary for any future online annotation mode.

---

## Product sentence

Annotations are a separately sealed behavioral-evidence layer over an already completed Trace V5 rollout. They explain how and why a policy succeeded or failed without changing the benchmark's authoritative reward, achievements, tests, environment state, or terminal result.

The current trustworthy path is intentionally **post-hoc**:

1. Run the benchmark normally.
2. Determine objective reward and terminal state.
3. Complete and seal the Trace V5 rollout.
4. Run deterministic and/or agentic annotators over that immutable trace.
5. Validate selectors and payloads against the sealed trace.
6. Seal annotations and rubric results into an independently versioned evidence head.
7. Reconcile that evidence into Workshop for findings, rubric scorecards, audit, and trace drill-down.
8. Use the evidence to design the next policy or harness experiment.

Annotation job events may stream while an annotation job is running, but the job is still reading a completed rollout. The current system does not use annotations to steer the rollout that produced the trace.

---

## Causal boundary

The non-negotiable invariant is:

> Annotation truth is downstream of eval truth.

Annotations and rubric judgments may explain performance. They must never:

- alter environment reward;
- create or remove achievements;
- change test or verifier outcomes;
- change objective terminal status or eval validity;
- rewrite the original trace;
- turn missing evidence into a score of zero;
- expose hidden chain of thought.

Missing verifier evidence is **Unavailable**, not `0`. A rubric score is available only when a non-empty sealed verifier result is attached to the evidence head.

This boundary is enforced in the Workshop post-terminal annotation stage and in the container evidence-validation path.

---

## Repository ownership

### Containers: trustworthy execution substrate

Repository: `/Users/joshuapurtell/GitHub/containers`

Primary implementation:

- `src/synth_containers/tracing/annotation/`
- `src/synth_containers/rubrics/v1.py`
- `src/synth_containers/annotations.py`
- `openapi/container-contract-v1.yaml`

Containers owns:

- annotation definitions and registries;
- deterministic, model API, Codex app-server, and Jesterky runners;
- job states, workers, schedulers, streams, campaigns, retries, and caching;
- bounded read-only trace-inspection tools;
- selector and proposal validation;
- immutable evidence bundles and evidence-head extension rules;
- consensus and adjudication;
- paid-compute reservations, signed brokers, pricing, and reconciliation ledgers;
- durable JSON authority with a rebuildable SQLite index;
- container mounting and `/annotation/*` operations;
- rubric and verifier-result contracts.

Important files:

- `tracing/annotation/service.py` — job execution and sealing
- `tracing/annotation/jobs.py` — request, state, usage, estimate, and error contracts
- `tracing/annotation/validation.py` — selector and proposal validation
- `tracing/annotation/tools.py` — bounded inspection tool surface
- `tracing/annotation/persistence.py` — durable job and evidence authority
- `tracing/annotation/campaign.py` — campaign planning and execution
- `tracing/annotation/broker.py` and `signed_broker.py` — paid-compute authorization
- `rubrics/v1.py` — rubric criteria, evidence references, and verifier results

The container is authoritative for the sealed trace and annotation evidence. Workshop stores a durable local projection, not a competing source of truth.

### Evals: domain meaning

Repository: `/Users/joshuapurtell/GitHub/evals`

Domain packs:

- `domains/craftax/annotations/`
- `domains/banking77/annotations/`
- `domains/healthbench/annotations/`
- `domains/generic/annotations/`
- `domains/deepswe/annotations/`
- `domains/code_policy/annotations/`

Evals owns:

- taxonomies and output contracts;
- deterministic fact extractors;
- registered agentic annotators and prompts;
- milestone graphs;
- anchored rubric definitions;
- aggregation and calibration logic;
- golden fixtures and human-audit helpers;
- domain image registration;
- campaign and acceptance-run scripts.

Operational scripts live in `scripts/annotation/`. Five annotated recipes exist for Banking77, Craftax gold, HealthBench, DeepSWE, and code-policy.

The best conceptual overview is:

`docs/handoffs/HANDOFF_AGENTIC_ANNOTATIONS_RUBRICS_DEEPSWE_CODE_POLICY.md`

The durable evidence collection is:

`artifacts/annotation/`

At the time of this handoff it contains 1,963 files, 94 materialized annotator workspaces, and five top-level acceptance receipts.

### Workshop: orchestration and presentation

Repository: `/Users/joshuapurtell/GitHub/workshop-v08-e2e-refactor`  
Branch: `codex/v0.9.0`

Workshop owns:

- discovery of annotation-capable registered loopback containers;
- trace-to-container ownership checks;
- campaign estimation and bounded paid-compute approval;
- one exact signed reservation per paid job;
- proxying annotation operations to the owning container;
- durable reservation settlement and campaign reconciliation;
- local projections of campaigns, jobs, evidence heads, findings, and rubrics;
- Analysis workbench, Rubric, Trace, Audit, and inspector presentation;
- local human review revisions;
- agent-facing annotation MCP operations and skills.

Important backend files:

- `apps/synth_desktop/src-tauri/src/annotations_ipc.rs`
- `apps/synth_desktop/src-tauri/src/session/annotation_projection.rs`
- `apps/synth_desktop/src-tauri/src/session/annotation_reservation.rs`
- `apps/synth_desktop/src-tauri/src/optimizers/annotation_stage.rs`
- `apps/synth_desktop/src-tauri/src/optimizers/container_eval.rs`
- `apps/synth_desktop/src-tauri/src/bin/synth_annotations_mcp.rs`

Product surfaces:

- `visuals/families/analysis/analysis.annotation_workbench.v1/`
- `visuals/families/analysis/annotation.overlay.v1/`
- `visuals/tests/annotation_workbench.test.mjs`
- `apps/synth_desktop/tests/playwright/annotation-paid-card.spec.ts`

Agent instructions:

- `apps/synth_desktop/skills/trace-v5-annotate/SKILL.md`
- `apps/synth_desktop/skills/trace-v5-verify/SKILL.md`
- `apps/synth_desktop/skills/annotation-review/SKILL.md`
- `apps/synth_desktop/skills/craftax-trace-analysis/SKILL.md`

---

## What is proven now

The real Craftax target was reused; the rollout was not rerun.

| Item | Proven value |
|---|---|
| Rollout | `roll_ab9de205861d` |
| Environment | Rust GameBench gold, `env:craftax_gold` |
| Workshop local trace | `tracev5_d90a66ed94d8b65fdf6df8a3` |
| Raw trace digest | `sha256:d90a66ed94d8b65fdf6df8a306912a5e26eea07f5f17965a4e8fc987bd40ed21` |
| Normalized trace digest | `sha256:6e47e52c30126d3f360d74576569a7358230b0255d646e8465a57693fdb48d69` |
| Paid verifier job | `ajob_a0d742cc7ce34e3a` |
| Verifier result | `vres_2a294bcd197fd15c` |
| Annotator | `craftax.rubric_verifier` |
| Rubric | `craftax.execution_quality` |
| Result digest | `sha256:f3b1f77bfb50067ac860cf551277fd038c73ac9fa3d8379ea6b57dddea3fe56d` |
| Evidence head | `sha256:668f85a4825bf96cb25476eecd96736e5f6cb430948fac18134b5207b780db7e` |
| Score | `0.4722222222222222` / **47.2%** |
| Threshold | `0.5` |
| Verdict | **fail** |
| Usage | 25 tool calls, 122,713 tokens |
| Cost | maximum approved `$2`; actual cost unavailable |

The sealed criterion result says the policy was grounded and safe but ineffective at converting understanding into progress:

- passed state grounding, belief calibration, plan quality, plan/action fidelity, tool reliability, and safety/survival;
- failed feedback incorporation, progress efficiency, and strategic prioritization;
- context robustness was not applicable because the trace contains one model call and no compaction transition.

The Workshop workbench is proven at revision 2 with:

- `1/1 jobs sealed`;
- `10 selectors resolved`;
- the updated evidence head;
- the rubric ID, overall fail verdict, and 47.2% score;
- all ten criterion names, judgments, scores, and rationales;
- Overview, Findings, Milestones, Rubric, Trace, and Audit views.

Durable receipt:

`/Users/joshuapurtell/GitHub/evals/artifacts/annotation/craftax-gold-rubric-verifier-20260901/receipt.json`

Latest Workshop UI commit:

`3ab12fbc fix: render sealed rubric criteria`

---

## Why the current system is post-hoc

Post-hoc execution provides several trust properties cheaply:

- the evidence target is immutable;
- annotators cannot influence the policy being judged;
- every finding can be validated against stable selectors;
- retries and alternative annotators compare against the same trace;
- reward and annotation truth cannot become causally entangled;
- evidence heads can be cached, versioned, superseded, and audited independently;
- paid-compute reservations can be bounded before work begins;
- Workshop can reload summaries without replaying the provider or the trace journal.

This is the right default for evaluation, behavioral diagnosis, rubric scoring, regression comparison, and experiment design.

---

## What an online mode would mean

An online annotator would inspect a growing trace prefix while the rollout is still running. That is not merely a faster form of the current system; it creates a different product and trust contract.

There are at least three possible online modes:

1. **Observe only** — emit provisional findings during execution, but never expose them to the acting policy.
2. **Human/operator alerting** — surface provisional safety or quality signals to an operator, who may independently stop or intervene.
3. **Policy steering** — feed annotation output back into the policy or harness during the same rollout.

Only the first mode preserves most of the current causal separation. The third mode turns the annotator into part of the policy/harness and must be evaluated, versioned, and scored as such.

Any online design must introduce explicit contracts for:

- trace-prefix identity and monotonic cursors;
- provisional versus sealed findings;
- retraction and supersession when later evidence contradicts an early judgment;
- bounded latency and backpressure;
- whether the acting policy can see the signal;
- intervention authority and audit receipts;
- reward-contamination prevention;
- final replay over the sealed trace;
- separate versioning of observer, alerting, and steering modes.

Do not reuse `sealed`, `applied`, or final rubric terminology for provisional online output without these distinctions.

---

## Recommended next architectural decision

> **2026-09-01 follow-up:** the observe-only provisional lane below is now implemented across Containers, Evals, and Workshop. See `docs/HANDOFF_LIVE_ANNOTATION_PROTOCOLS_2026-09-01.md`.

Keep the current post-hoc path as the authoritative evaluation layer.

If live behavior visibility is wanted, add an **observe-only provisional annotation lane** first:

1. Subscribe to monotonic Trace V5 prefixes.
2. Emit `provisional` findings into a separate stream and namespace.
3. Do not expose those findings to the policy or reward calculation.
4. Permit retraction and supersession as the trace grows.
5. After terminal seal, rerun or reconcile against the complete trace.
6. Promote only validated final findings into the existing evidence-head system.

This gives live monitoring without weakening the post-hoc evidence model. Policy steering should be treated later as a distinct harness feature, not as an annotation-system extension.

---

## Documentation corrections

Several existing handoffs are now historical:

- `docs/HANDOFF_ANNOTATION_WORKBENCH_CUA_2026-09-01.md` says the Workshop UI is unproven. It is now proven.
- `evals/docs/handoffs/PLAN_ANNOTATIONS_REMAINING_WORK_2026-09-01.md` lists the real verifier scorecard path as pending. It is complete for the Craftax proof.
- Earlier material says the packaged/native paid approval path is missing. A native `$2`-bounded approval and sealed Luna verifier have now completed.
- Earlier material says annotated recipes exist only as fixtures. Five recipes now exist under `apps/synth_desktop/src-tauri/recipes/annotation_eval/`.

Preserve those documents as history, but use this handoff plus the durable Craftax receipt for the current state.

---

## Immediate follow-up work

1. Add a link to this handoff from the older cross-repository completion handoff.
2. Refresh the Workshop annotation skills with the current terminal state names and real rubric-result behavior.
3. Add restart-survival coverage for a running annotation campaign and container restart.
4. Close exact paid-cost reconciliation; the proven job has `cost_status: unavailable`.
5. Run clean-checkout acceptance for the five packaged annotated recipes.
6. Decide whether live annotation means observe-only monitoring, operator alerting, or policy steering before adding streaming semantics.

---

## Definition of done for the current post-hoc product

- Objective rollout evidence seals before annotation starts.
- Annotation is causally downstream and cannot change objective truth.
- Every accepted finding cites selectors that resolve against the sealed trace.
- Missing verifier evidence remains Unavailable.
- Paid jobs have one bounded approval and one exact signed reservation each.
- Jobs reconcile durably to terminal states.
- Evidence heads and verifier results attach idempotently.
- Workshop reloads the local projection without rerunning the provider.
- The Rubric view renders criterion-level judgments and rationale.
- Trace and Audit views retain drill-down and human-review provenance.
- A durable receipt records job, evidence, verifier, usage, and failure-regression information.

Craftax now satisfies this path except for exact actual-cost accounting and broader restart/clean-checkout coverage.
