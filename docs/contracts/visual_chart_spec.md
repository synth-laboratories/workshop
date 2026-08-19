# Contract: `synth.visual.chart-spec.v1`

Governs `visuals/charts.rs` (parser, validator, SVG renderer), the
`analysis.chart.v1` template, the `visual_chart` tool in
`synth_visuals_mcp.rs`, and `ChartVisual.tsx` in the renderer.

## The rule

**One spec, one renderer, one image.** A chart's canonical source is a bounded
JSON document. The host parses it, renders it to SVG in-process, and stores
that SVG as the visual's rendition. The pane displays that rendition; a review
capture photographs that rendition. There is no second implementation for an
agent and a human to disagree about.

This is the `diagram.systems.v1` contract applied to data: explicit source in,
deterministic SVG out. It is *not* the React-template contract, where the
pixels only exist once a Desktop pane has mounted the shell.

## What follows from it

- **Capture needs no window.** `capture_review` takes the `deterministic-svg`
  branch for `rendererKind: "chart"`, so an agent iterates without opening,
  showing, or settling a pane, and without a rendered-observation handshake.
- **Byte-identical review.** The digest an agent reviewed is the digest the
  pane shows for that revision.
- **Theme is declared, never requested.** `theme` lives in the spec; the
  rendition is keyed on it. The MCP omits a theme override and the pane passes
  `null`, so neither can photograph a variant the other never sees.

## Absence is not zero

Every value channel is nullable and every null survives to the image:

| Channel | `null` renders as |
| --- | --- |
| `series[].points[].y` | a gap — the line breaks and resumes |
| `bars.series[].values[]` | a hatched stub at the baseline, no bar |
| `heatmap.values[][]` | a hatched cell |
| `table.rows[][]` | an em dash |

An unmeasured point plotted at zero is a false measurement, and this is the
one thing the renderer will not do. Regressing it reopens the `CoveredMetric`
bug class.

## Bounds

Declared once in `charts.rs`, enforced at parse:

```
source ≤ 512 KiB      panels ≤ 16        series ≤ 12 per panel
points ≤ 20 000       categories ≤ 200   heatmap cells ≤ 4 000
table ≤ 400 × 12      width 480…2000     rendered height ≤ 16 384 px
SVG output ≤ 4 MiB
```

`deny_unknown_fields` everywhere: a misspelled key is a refusal with the key
named, not a silently dropped panel. Colors must be `#rgb`/`#rrggbb`
literals — a paint attribute is a script vector, so `url(...)` never reaches
the SVG. All author text is XML-escaped.

## Three outcomes, never silent-nothing

Per the `visual_bindings.md` model, a spec is **rendered**, **refused** with
the offending field named (`renderStatus: "failed"` plus `renderError`), or —
for schema-valid but illegible compositions — rendered *and* reported through
`authoring_findings`: too many series for one legend, vertical bars past the
label-collision count, a heatmap wider than its labels, a table longer than a
reviewable pane. Findings are feedback, not failure; `mark_ready` refuses
while any remain, exactly as it does for systems maps.

## Deriving panels from bound evidence

A panel carries literal values **or** a `from` block naming a bound slot —
never both, never neither. The host resolves the slot, walks the path, runs the
transform pipeline, and maps the resulting columns onto the panel's channels;
`charts.rs` only ever sees literal values.

```json
{"kind": "bars", "title": "Actions taken", "from": {
  "source": {"slot": "rollout", "path": "steps", "transform": [
    {"op": "groupAggregate", "by": ["action"], "aggregate": {"count": {"func": "count"}}},
    {"op": "sort", "by": "count", "order": "desc"}]},
  "category": "action",
  "series": [{"name": "steps", "value": "count"}]}}
```

**Slots** resolve one document each — two bindings on one slot is a refusal,
because a still image of "the trace" cannot come from two of them:

| Kind | Document |
| --- | --- |
| `inline` | the descriptor's `data` |
| `fixture` | JSON under the repo's `visuals/` root (path-checked against it) |
| `local_cas` | a JSON blob by digest |
| `trace_v5` | the trace's projection payload; `projection` picks the kind, default `rollout-inspector` |
| `query_snapshot` | the frozen snapshot; address rows at `path: "facets.rows"` |
| `optimizer_run` | the run's typed result from the optimizer service; the per-trial ledger is at `path: "summary.records"` |

An `optimizer_run` may be read before the run seals. That reading is a snapshot,
and it says so: the receipt carries `sealed: false`, `snapshotOfLiveRun: true`,
the `cursor` it was taken at, and a digest of exactly what was taken, so two
charts drawn from one moving run can be told apart instead of silently
disagreeing. A sealed run records `sealed: true` and the manifest's terminal
cursor.

`live_sse` is refused by name — a stream has no single value to draw — and so is
`run_ref`, which names a session run for which no projection is defined yet.

**Transforms** are total functions from rows to rows, applied in order:
`filter`, `sort`, `limit`, `select` (project and rename by dotted path),
`unwind` (a list field becomes rows), `unpivot` (several columns become rows —
what a metric-by-turn heatmap needs), `derive` (`cumulative`, `delta`, `ratio`,
`rowIndex`, `present`), `groupAggregate`
(`count`, `countDistinct`, `sum`, `mean`, `rate`, `median`, `p25`, `p75`,
`p90`, `min`, `max`, `first`, `last`), and `bin`.

Absence survives the pipeline, and this is the whole point:

- a numeric filter never admits an absent value — treating it as `0` is how
  "unmeasured" silently becomes "below threshold";
- `mean`/`sum`/`max` over no values are `null`, while `count` is `0`, because a
  count is defined over zero rows and a mean is not;
- `cumulative` carries its running total across a gap rather than resetting it
  or adding zero;
- an ungrouped aggregate over an empty table still emits its row, so "asked,
  and nothing was there" stays distinguishable from "never asked".

**Mappings** name which column becomes which channel: `series[].{x,y,band}`
with `nameField` to split one table into several series, `bars.{category,
series[].value}`, `scatter.{x,y,label,group}`, `histogram.value`,
`heatmap.{row,column,value}`, `table.columns[].{header,field}`, and metrics in
either form — one metric per row (`label`/`value` as columns) or one aggregate
row as a KPI strip (`items[].{label,value}`, where the label is written text
because a label is language, not data).

Two source rows for one heatmap cell is a refusal, not a silent overwrite.

Resolution is recorded on the render: `dataProvenance` holds each slot's kind,
source, and digest, and `authoringFindings` holds the diagnostics for the
*derived* shape — a chart's real width is only known after its bindings
resolve.

## Panels

`metrics`, `series` (line/stepped/area, optional band), `bars` (grouped or
stacked, vertical or horizontal), `scatter` (optional Pareto frontier with
per-axis `min`/`max` preference), `histogram`, `heatmap`, `table`, `note`.
The palette is the `visuals/chrome/tokens.css` vocabulary; the dark set
mirrors the systems-map technical dark.

## Capture geometry

A chart is a document, not a poster. `capture_review` keeps the requested
viewport **width** and derives the height from the chart's own aspect, so a
tall stack of panels is photographed whole at reading size instead of being
scaled into a fixed box — the failure mode review exists to catch. The
response reports both `viewport` (what was photographed) and
`requested_viewport` (what was asked for).
