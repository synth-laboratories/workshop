---
name: annotation-review
description: Review, accept, reject, dispute, or adjudicate existing Trace V5 annotations with annotation_get_evidence, annotation_review, and annotation_consensus. Use after annotation jobs have sealed results and a person or agent needs to record a verdict or resolve disagreement. Do not use to create new findings — that stays on trace-v5-annotate.
---


# Annotation Review

Reviews are append-only. A review creates a new revision that supersedes the original annotation (`supersedes_id`); the original record, its evidence, and its digests are never modified or deleted.

## Procedure

1. `annotation_get_evidence(annotation_id)` — read the finding, the resolved target text, and every cited selector's text. Judge from that evidence only.
2. `annotation_review(annotation_id, decision, reviewer, rationale, evidence?)` with `decision` in `accepted | rejected | disputed | needs_review`. Optional extra evidence selectors are resolved before the revision is sealed.
3. A second review of the same original is refused (`revision_conflict`): review the latest revision instead.
4. Disagreement between repeated annotations: `annotation_consensus(trace_id, annotator_id)` reports inter-annotator agreement and appends majority consensus records that name every source and each dissenting id. For a decision rather than a vote, start an adjudication job (`mode: adjudicate`, `source_annotation_ids`) with `craftax.adjudicator`; its record cites the sources and never edits them.

## When reporting

State the original `annotation_id`, the new revision id, the decision, and the bundle digest of the evidence head after the review. Never describe a rejected annotation as removed; it is superseded and still visible with `include_superseded`.

Every `annotation_manage` call names the immutable `container_id` of the registered container that sealed the trace (from `container_list`); Workshop resolves its loopback URL from the registry and never accepts a URL from you.
