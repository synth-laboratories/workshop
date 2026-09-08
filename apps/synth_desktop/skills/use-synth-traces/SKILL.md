---
name: use-synth-traces
description: Query existing eval jobs and sealed V5 traces, page immutable results, read exact source evidence, and optionally prepare annotations or visuals.
---

Use `trace_manage` for core trace research. No annotation, Jesterky install,
visual, or paid provider is required.

1. Resolve an existing optimizer/eval job ID. Query with:
   `{"schemaVersion":"synth.trace-query.v2","evalJobIds":["job"],"grain":"episodes"}`.
   `includeChildEvals:true` follows explicit optimizer child references.
2. Filters are arrays of `{field,op,value}`. `where` filters the result grain;
   `entityWhere`, `annotationWhere`, and `rewardWhere` are existence filters.
   Operators: eq, ne, in, gte, lte, contains, missing. Use `contains` for labels.
   Grains: episodes, entities, annotations, rewards. Never submit SQL or paths.
3. Query configuration fields, reward/valid/status, actor/session/event/tool/text,
   or annotation label/score/confidence/reviewState/current. `annotation_target`
   correlates findings with entities; `recorded_link` uses recorded links;
   `repeated_failed_action` finds an actor's next recorded repeat after an error.
   Do not infer causal order or semantic intent from timestamps alone.
4. `aggregate:reward` counts each episode once and reports measured/missing
   denominators, grouped by compatible reward semantics. `count` counts rows.
   `paired_reward` compares two job IDs by environment/version/task/seed/repeat
   and reward definition, reporting missing/ambiguous matches explicitly.
5. Save the returned snapshotId/resultDigest. `page` takes snapshot_id, offset,
   limit (1–200); follow nextOffset until null. Results never silently truncate.
   Source/review changes require a fresh query; old snapshots remain pinned.
6. Read exact evidence with operation `source`: snapshot_id, result_id, optional
   selector copied from the result, offset, source_limit (1–64000 characters).
   Follow nextOffset for long source text. Query previews may be clipped.
7. Optionally call `prepare_annotations` with snapshot_id and explicit result_ids
   (1–200) to obtain deduplicated container/trace targets. This is free; then use
   annotation_manage to discover compatible definitions and estimate/start via
   existing reservations. Jesterky is an optional alternative preparation tool.
8. Optionally call open_query or open to inspect saved results or trace replay.
   A visual is never evidence that missing source capture succeeded.

Missing reward is not zero; absent annotations are not a clean finding. Report
traceAvailability, captureStatus and analysisState. Refine a query that exceeds
100,000 results or 64 MiB; do not summarize a partial aggregate as complete.
