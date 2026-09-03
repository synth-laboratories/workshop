# analysis.annotation_workbench.v1

Durable presentation of **machine-generated** Trace V5 analysis. Distinct from human `visual_annotations` and from `annotation.overlay.v1` (marker overlays).

```text
Trace = source evidence
Analysis = derived judgments
Visual = durable presentation
```

The visual is a projection. Container storage remains authoritative for full annotation artifacts. A new evidence-head digest creates a new visual revision.

## Inputs

| Input | Kind | Notes |
| --- | --- | --- |
| `evidence` | `annotation_evidence_head` (or fixture/inline) | Required. Schema `synth.annotation-workbench.v1`. |
| `trace` | `trace_v5` | Optional identity for the sealed archive. |
| `rubric` | `verifier_result_v2` | Optional. Missing evidence shows **Unavailable**, never 0. |

## Views

Overview · Findings · Milestones · Rubric · Trace · Audit
