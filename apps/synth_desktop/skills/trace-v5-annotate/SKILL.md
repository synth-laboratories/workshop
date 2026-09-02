---
name: trace-v5-annotate
description: Launch deterministic and Codex app-server annotation over already-sealed Trace V5 rollouts through annotation_list_definitions, annotation_estimate, annotation_start, annotation_get, annotation_events, annotation_cancel, and annotation_list. Use when asked to find false beliefs, plan/action mismatches, recovery behaviour, or milestone progress in an existing trace. Do not use to run or re-run an evaluation — that stays on run-live-container-evals; do not use to score a rubric — that stays on trace-v5-verify.
---


# Trace V5 Annotate

Annotation reads a sealed trace and appends evidence beside it. It never changes the trace, its reward, its achievements, or its engine state, and you must never say it did. Reward is a **separate catalog** (`GET /reward/catalog`, `synth.container.reward-api.v1`); annotations never rewrite `reward_signal`.

## Before launching

1. Prefer a trace that is already sealed. `trace_manage list` / `trace_manage get` shows sealed traces and their immutable `trace_digest`. Never rerun an evaluation just to annotate it.
2. `annotation_list_definitions(trace_id, domain)` lists compatible annotators and rubrics. Each row carries immutable `annotator_digest` and `program_digest` values, `runner_kind`, and `paid`. Start with the deterministic annotators (`paid: false`); launch a Codex annotator only for the question deterministic facts cannot answer.
3. `annotation_estimate(request)` returns the `idempotency_key`, whether a sealed result is `cached`, the `resolved_model` / `resolved_reasoning_effort` the job will be keyed on, the bounded limits (`max_tool_calls`, `max_total_tokens`, `max_cost_usd`), and `requires_reservation`. A cached estimate means no provider call will be made. Paid jobs must declare `max_total_tokens`; the runner enforces cost as a token ceiling.

## Recipes

A fresh Workshop session lists these annotated eval recipes (paid ones ship `enabled = false`):

| Recipe id | Default |
|---|---|
| `eval.craftax.gold.annotated.v1` | enabled (deterministic Craftax gold) |
| `eval.banking77.annotated.v1` | enabled |
| `eval.deepswe.annotated.v1` | `enabled = false` |
| `eval.code_policy.annotated.v1` | `enabled = false` |
| `eval.healthbench.annotated.v1` | `enabled = false` |

Running an enabled recipe produces `eval-annotation-campaigns`. Do not enable a paid recipe just to demo annotations.

## Approval

Paid annotators run only under a **reservation** issued by the host's paid-compute broker. You never construct one: ask once, with one compact summary, and the host returns an opaque `reservation_id` bound to this trace digest, annotator, model, and session, capped in USD micros, single-use.

```
Annotate <trace_id>@<short digest>
annotators: craftax.belief (codex, gpt-5.6-luna, effort medium) ×1 repeat
limits: 200 tool calls, 400k tokens, max charge $0.50
cached: no
```

Reject leaves nothing changed. Never send an approval decision or a dollar amount as a tool argument; pass only the `reservation_id` the host gave you. A reused, forged, or mis-bound id is refused (`reservation_rejected`) before any task starts.

## Launch and babysit

- `annotation_start(request, reservation_id?, session_id?)` *enqueues* one job and returns immediately (`accepted: true`, state `prepared`). A worker runs it. **Job** states: `prepared → running → validating → sealed | abstained | failed | cancelled`. If the idempotency key matches a sealed job, the existing job is returned with a `cached` receipt and no new task starts.
- **Campaign** states (derived from the job mix, never a separate submit): `submitted` (seeded, none terminal), `running` (in-flight mix), `sealed` (every job sealed), `partially_sealed` (some sealed/abstained, none still running), `failed` (every job failed). A mix of sealed and prepared is `running`, not sealed.
- Poll `annotation_get(job_id)` until `terminal: true`. For the lifecycle log, `annotation_events(job_id, after?, limit?)` pages `GET /annotation-jobs/{id}/events` (SSE sibling `/stream`). Hidden chain-of-thought is never included.
- `annotation_cancel(job_id)` interrupts a running task; sealed results are never removed.
- Distinguish outcomes when you report: `sealed` (applied findings exist), `abstained` (every finding lacked evidence — this is a result, not an error), `failed` (typed `error.code`, e.g. `tool_limit_exceeded`, `malformed_output`, `transport_disconnected`), `cached`.

## Report

Always report exact ids: `trace_id` + `trace_digest`, `annotator_id` + `annotator_digest`, `rubric_id` (if any), `job_id`, `bundle_digest`, `execution_trace_id` (for Codex annotators), and the counts `applied / abstained / rejected`. `annotation_list(trace_id, filters)` reads the sealed head from local storage; `annotation_get_evidence(annotation_id)` resolves every cited selector to the exact text.

## Failure modes

- `reservation_required`: the annotator is paid; request approval with the estimate and pass the returned `reservation_id`.
- `reservation_rejected` (`reason`: `reservation_unknown`, `reservation_consumed`, `reservation_binding_mismatch`, `reservation_expired`, `reservation_broker_unavailable`): the id is not valid for this job; ask for a new one, never retry the same id.
- `source_trace_unavailable`: the sealed trace is not materialized locally; import it with `trace_manage import` first.
- `annotation_trace_unknown` / `annotation_trace_unowned` / `annotation_container_mismatch` (Workshop, before any approval card): paid annotation needs the trace in Workshop's own index with a recorded owning container that equals your `container_id`. A bare file import records no owner; re-import the trace from the container that sealed it (`data_trace_materialize` / the traces container import with the immutable container id) and retry.
- `definition_digest_mismatch`: the registry changed; re-list definitions and rebuild the request.
- Findings with unresolvable selectors become typed abstentions (or fail the job when the annotator says so). Never paraphrase an abstention as a finding.

Fixture: the deterministic smoke fixture (`synth_containers.tracing.annotation.fixtures.build_craftax_smoke_trace`) exercises every state without provider calls.

Every `annotation_manage` call names the immutable `container_id` of the registered container that sealed the trace (from `container_list`); Workshop resolves its loopback URL from the registry and never accepts a URL from you.


## Live lane (provisional, observe-only)

Post-hoc annotation over the sealed trace stays the evidence authority. A recipe may also declare
`[live_annotation]`: a digest-pinned protocol runs beside each rollout inside the container and
streams provisional findings (`annotation.finding`, retractable, superseded as evidence grows) on a
declared sibling stream. Operate it with `annotation_protocol_get` / `annotation_protocol_update` /
`annotation_control_send`, and read the post-seal reconciliation with `annotation_provisional_list`.
Never cite a provisional finding as sealed evidence; cite the sealed finding that corroborates it.
