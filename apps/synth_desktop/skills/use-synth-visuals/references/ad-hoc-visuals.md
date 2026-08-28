# Ad-hoc visual playbook

Use this guide when the user asks the agent to create a one-off visual from
optimizer, eval, rollout, container, trace, or experiment evidence. The normal
output is a durable `analysis.chart.v1` opened in the originating chat's right
pane—not a prose table and not a parallel product-owned optimizer viewer.

## Default decision

For quantitative comparison, call `visual_manage` with operation `chart`.
This remains the default when one arm is missing, a run is partial, or some
telemetry is unavailable. Use `null` values, visible missing-state labels, and a
note panel instead of changing to the legacy `analysis.visual.v1` grammar.

Use `analysis.visual.v1` only when the main artifact is an ordered narrative
record. Use a task-family live template for streaming gameplay or eval state,
and use the optimizer-owned visual for following one optimizer run. Never
create a second generic live viewer for the same optimizer run.

Instance-scope visual listing is cross-task discovery, not ownership transfer.
When a durable visual from another task contains the required evidence, call
`fork` and take the returned visual ID before revising it with operation
`chart`. The fork is owned by the current chat and records its parent. Never
revise or `show` the discovered original: `show` routes to the owner's session,
so a successful tool response can still leave the current chat's pane closed.

## Compose the comparison

Prefer this order:

1. A title naming the task and arms, plus a subtitle stating `n`, seed matching,
   aggregation, and evidence cutoff.
2. Three or fewer decision metrics above the fold.
3. The primary comparison: paired/grouped bars for matched repeats, a series
   for ordered steps, or points for independent cost/quality observations.
4. A compact exact table only when values or provenance need auditability.
5. One short note containing the conclusion and any missing-evidence caveat.

Use one accent for the focal arm and one neutral comparison color. Keep arm
names identical in cards, marks, tables, and prose. Prefer direct labels;
legends are secondary. Do not use decorative gradients, pseudo-3D marks,
smoothed lines across unordered arms, or a Pareto frontier that was not
computed from the displayed points.

Show units, denominators, `n`, seeds, failure coverage, and uncertainty method
where applicable. Missing is not zero; failed is not scored; an unsealed run is
a snapshot, not a final result. Keep exact micro-costs instead of rounding to
`$0.00`.

## Metric denominators

Name the numerator and denominator before calculating an efficiency metric.
For repeated evals, the default **aggregate score per dollar** is:

`sum(scored rollout scores) / sum(policy cost for those same scored rollouts)`

If every rollout has cost, `mean(score_i / cost_i)` is a different statistic;
label it **mean per-rollout score/$**. Never divide a mean score by total
campaign cost and label it merely `score/$`. Never mix costs from failed or
unscored rollouts into the denominator without saying so. If cost coverage is
partial, show the efficiency value as missing or explicitly bounded—do not
silently extrapolate.

For frequency differences, show numerator, denominator, and signed percentage
points. For continuous metrics, state whether the delta is absolute or
relative. A one-rollout arm is exploratory and gets no confidence interval.

## Evidence and event-source matrix

| Source | Visual binding | Use | Authority and recovery |
| --- | --- | --- | --- |
| Optimizer run snapshot | one named slot per arm, `kind: "optimizer_run"`, `source: run_id` | Ad-hoc completed/running cross-run charts | The optimizer store resolves the document and records a snapshot cursor. An unsealed run must be labeled as a snapshot. If a run is unavailable in task/catalog scope, keep it null; never substitute a similarly named run. |
| Imported optimizer evidence | one named slot per arm, `kind: "optimizer_snapshot"`, `source: snapshot_id` | Cross-instance or portable completed-run comparisons | Export in the owning instance, import in the destination, and bind the immutable snapshot id. Preserve source instance/run identity and digest; never read another instance's database directly. |
| Product optimizer stream | host-owned `optimizer_run` slot created by `open_visual` | Follow one GEPA/GELO/SFT/CISPO/eval run live | The product visual reads the persisted optimizer event cursor. Reopen the same visual ID after restart. Do not manually bind, review, or mark it ready. |
| Prepared container rollout | `kind: "live_sse"`, exact declared `source` and `poll_url`, schema `synth.trace-stream-event.v1` | Live Craftax, Harbor, dig.bench, or another declared task-family view | Prepare returns both URLs. SSE is the live path; bounded polling is replay/recovery authority. Require `stream.subscribed {ready:true}` before start. Never guess routes. |
| Several live rollouts | repeated `live_sse` descriptors on the template's multiple `stream` slot using `mode: "append"` | Multi-lane campaign view | De-duplicate `(stream_id, sequence)`. Resume SSE with `Last-Event-ID`; backfill from the declared poll cursor first after reconnect. |
| Sealed rollout/trace | `kind: "trace_v5"` with the durable trace identity | Completed replay, trace inspection, or post-run analysis | Terminal lifecycle and event sequence come from the sealed artifact. `inspectable:false` is a real limitation, not a retry signal. |
| Durable local evidence | `local_cas`, `query_snapshot`, or approved `fixture` | Reproducible offline charting | Bind the content identity, not a prose label. Record digest/projection in provenance. Fixtures must be labeled as fixtures and cannot satisfy a requested live run. |
| Small authored facts | `inline` | A compact spec or facts that have no stronger durable source | Inline data is authored evidence. Do not use it to launder unavailable run metrics or reconstruct an event stream. |

For container streams, prefer SSE for visual telemetry, WebSocket only for
interactive control or declared binary delivery, and bounded polling only when
declared. The control acknowledgement is not evidence and does not advance the
evidence cursor. Terminal status is authoritative; never infer it from a quiet
stream.

Frames are event evidence, not screenshots. Use immutable emitted frame URLs
such as `/rollouts/{id}/frames/{step}.png`; never replay a mutable latest-frame
URL. Preserve the event timestamp/sequence that associates a frame with its
action, reward, and environment step. If a source declares frames unsupported,
show state/actions without fabricating images.

## Two-run chart skeleton

Use literal panel values only after reading the evidence, or use `from` blocks
to derive the same channels from bound run slots. A minimal matched-seed
comparison looks like this:

```js
await tools.mcp__synth_visuals__visual_manage({
  operation: "chart",
  arguments: {
    title: "RuneBench Woodcutting · Luna high vs low",
    presentation: "pane",
    bindings: {
      schemaVersion: "synth.visual-bindings.v1",
      slots: [
        {slot: "high", kind: "optimizer_run", source: highRunId},
        {slot: "low", kind: "optimizer_run", source: lowRunId}
      ]
    },
    spec: {
      version: 1,
      title: "GPT-5.6 Luna — reasoning effort comparison",
      subtitle: "RuneBench Woodcutting · n=5 matched seeds · aggregate score/$",
      panels: [
        {kind: "metrics", items: [
          {label: "Mean score", value: "416.2 high · 231.2 low", detail: "n=5 each"},
          {label: "Policy cost", value: "$0.921 high · $0.183 low", detail: "sum across scored repeats"},
          {label: "Aggregate score/$", value: "2,261 high · 6,305 low", detail: "sum(score) / sum(cost)"}
        ]},
        {kind: "bars", title: "Matched-repeat scores",
         categories: ["780040", "780041", "780042", "780043", "780044"],
         y: {label: "Woodcutting score"},
         series: [
           {name: "High reasoning", values: [320, 525, 225, 337, 674]},
           {name: "Low reasoning", values: [281, 200, 238, 225, 212]}
         ]},
        {kind: "note", title: "Takeaway",
         body: "Low reasoning is cheaper and more score-efficient; high reasoning achieves the higher mean score."}
      ]
    }
  }
});
```

The numbers above demonstrate shape only. Replace every value with the current
bound evidence; if a requested run cannot be resolved, use `null` in numeric
channels and say exactly which run is unavailable. After creation, call `show`,
inspect wide and compact captures, revise the same visual ID, record both
reviews, and call `mark_ready` only when the current revision passes.
