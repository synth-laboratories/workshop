//! Deriving chart panels from bound evidence.
//!
//! A panel either carries literal values or a `from` block naming a bound slot,
//! a path into that slot's document, a transform pipeline, and a mapping from
//! the resulting columns onto the panel's channels. Resolution turns the second
//! into the first, so `charts.rs` only ever renders literal values and the
//! renderer needs no knowledge of traces, fixtures, or snapshots.
//!
//! Absence survives the pipeline. A missing field is `None`, an aggregate over
//! no values is `None`, and `None` reaches the renderer as a gap, a hatched
//! cell, or an em dash. Only `count` — which is defined over zero rows — yields
//! a zero.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Rows are JSON objects; every transform is a total function from rows to rows.
pub type Row = Map<String, Value>;

pub const MAX_ROWS: usize = 200_000;
pub const MAX_GROUPS: usize = 5_000;

// ── The `from` block ────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataSource {
    /// Canonical bind-point name. `slot` is the one-release alias.
    #[serde(alias = "slot")]
    pub input: String,
    /// Dotted path into the resolved document; `steps`, `visual.items`,
    /// `rollouts.0.actions`. Omitted means the document itself.
    #[serde(default)]
    pub path: Option<String>,
    /// Trace V5 projection kind. Only meaningful for `trace_v5` slots.
    #[serde(default)]
    pub projection: Option<String>,
    #[serde(default)]
    pub transform: Vec<Op>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    Filter(FilterOp),
    Sort(SortOp),
    Limit(LimitOp),
    Select(SelectOp),
    Unwind(UnwindOp),
    Derive(DeriveOp),
    GroupAggregate(GroupAggregateOp),
    Bin(BinOp),
    Unpivot(UnpivotOp),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilterOp {
    pub field: String,
    #[serde(rename = "is")]
    pub predicate: Predicate,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Predicate {
    #[serde(default)]
    pub eq: Option<Value>,
    #[serde(default)]
    pub ne: Option<Value>,
    #[serde(default)]
    pub gt: Option<f64>,
    #[serde(default)]
    pub gte: Option<f64>,
    #[serde(default)]
    pub lt: Option<f64>,
    #[serde(default)]
    pub lte: Option<f64>,
    #[serde(default)]
    pub any_of: Option<Vec<Value>>,
    /// `true` keeps rows where the field is present and non-null.
    #[serde(default)]
    pub present: Option<bool>,
    /// Case-sensitive substring match on a string field.
    #[serde(default)]
    pub contains: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SortOp {
    pub by: String,
    #[serde(default = "default_order")]
    pub order: String,
}

fn default_order() -> String {
    "asc".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LimitOp {
    pub count: usize,
}

/// Project and rename. Values are pulled by dotted path, so nested evidence
/// becomes flat columns the mappings can name.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectOp {
    pub fields: BTreeMap<String, String>,
}

/// Explode an array-valued field into one row per element, keeping the parent
/// columns. `achievements` per step becomes one row per achievement.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnwindOp {
    pub field: String,
    /// Column the element lands in; defaults to the field name.
    #[serde(default)]
    pub r#as: Option<String>,
    /// Keep parent rows whose field is empty or absent, with the column null.
    #[serde(default)]
    pub keep_empty: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeriveOp {
    pub field: String,
    pub from: DeriveExpr,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeriveExpr {
    /// Running total of a numeric column, in current row order.
    #[serde(default)]
    pub cumulative: Option<String>,
    /// Difference from the previous row's value; the first row is null.
    #[serde(default)]
    pub delta: Option<String>,
    /// `[numerator, denominator]`; a zero or absent denominator is null.
    #[serde(default)]
    pub ratio: Option<Vec<String>>,
    /// Zero-based position in the current row order.
    #[serde(default)]
    pub row_index: Option<bool>,
    /// `true` when the named field is present and non-null.
    #[serde(default)]
    pub present: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupAggregateOp {
    #[serde(default)]
    pub by: Vec<String>,
    pub aggregate: BTreeMap<String, Aggregate>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Aggregate {
    pub func: String,
    #[serde(default)]
    pub field: Option<String>,
}

/// Turn several columns of one row into several rows — the shape a metric
/// heatmap needs, where the metric name is an axis rather than a column.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnpivotOp {
    pub fields: Vec<String>,
    /// Column the field's name lands in; defaults to `name`.
    #[serde(default)]
    pub name_as: Option<String>,
    /// Column the field's value lands in; defaults to `value`.
    #[serde(default)]
    pub value_as: Option<String>,
    /// Emit a row for a field the source row does not carry, with a null value.
    /// On by default: a metric nobody recorded is a visible gap, not a silence.
    #[serde(default = "default_true")]
    pub keep_absent: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BinOp {
    pub field: String,
    pub bins: usize,
    /// Column the bin's lower edge lands in; defaults to `bin`.
    #[serde(default)]
    pub r#as: Option<String>,
}

// ── Path access ─────────────────────────────────────────────────────────────

/// Dotted path with numeric array indices: `visual.items.0.detail.reward`.
pub fn pluck<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cursor = value;
    for segment in path.split('.') {
        cursor = match cursor {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cursor)
}

fn field(row: &Row, path: &str) -> Option<Value> {
    if let Some(direct) = row.get(path) {
        return Some(direct.clone());
    }
    pluck(&Value::Object(row.clone()), path).cloned()
}

/// A number, or nothing. Booleans count as 1/0 so `rate` over a flag column is
/// the share of true — the shape every unlock-rate chart needs.
pub fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::Bool(true) => Some(1.0),
        Value::Bool(false) => Some(0.0),
        _ => None,
    }
}

fn numeric_field(row: &Row, path: &str) -> Option<f64> {
    number(field(row, path).as_ref())
}

fn label_field(row: &Row, path: &str) -> Option<String> {
    match field(row, path)? {
        Value::String(text) => Some(text),
        Value::Null => None,
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        other => Some(other.to_string()),
    }
}

// ── The pipeline ────────────────────────────────────────────────────────────

/// Turn a resolved document into rows, then run the transform pipeline.
pub fn table(document: &Value, source: &DataSource) -> Result<Vec<Row>> {
    let selected = match source.path.as_deref() {
        Some(path) => pluck(document, path).with_context(|| {
            format!(
                "input {} has no value at path {path}; the document's top-level keys are {}",
                source.input,
                top_level_keys(document)
            )
        })?,
        None => document,
    };
    let mut rows = rows_from(selected).with_context(|| {
        format!(
            "input {}{} must resolve to an array of objects",
            source.input,
            source
                .path
                .as_deref()
                .map(|path| format!(" at path {path}"))
                .unwrap_or_default()
        )
    })?;
    if rows.len() > MAX_ROWS {
        bail!("input {} resolved {} rows, above the {MAX_ROWS} ceiling; add a filter or limit transform", source.input, rows.len());
    }
    for op in &source.transform {
        rows = apply(rows, op)?;
        if rows.len() > MAX_ROWS {
            bail!(
                "a transform on input {} produced {} rows, above the {MAX_ROWS} ceiling",
                source.input,
                rows.len()
            );
        }
    }
    Ok(rows)
}

fn top_level_keys(document: &Value) -> String {
    match document {
        Value::Object(map) => {
            let names: Vec<&str> = map.keys().map(String::as_str).take(12).collect();
            names.join(", ")
        }
        Value::Array(items) => format!("an array of {} items", items.len()),
        other => format!("a bare {}", kind_of(other)),
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn rows_from(value: &Value) -> Result<Vec<Row>> {
    match value {
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Object(map) => Ok(map.clone()),
                // An array of scalars is a legitimate column; name it `value`
                // so a mapping has something to point at.
                scalar => {
                    let mut row = Map::new();
                    row.insert("value".into(), scalar.clone());
                    Ok(row)
                }
            })
            .collect(),
        Value::Object(map) => Ok(vec![map.clone()]),
        other => bail!("expected an array or object, found {}", kind_of(other)),
    }
}

fn apply(rows: Vec<Row>, op: &Op) -> Result<Vec<Row>> {
    Ok(match op {
        Op::Filter(filter) => rows
            .into_iter()
            .filter(|row| matches(&field(row, &filter.field), &filter.predicate))
            .collect(),
        Op::Sort(sort) => {
            let descending = match sort.order.as_str() {
                "asc" => false,
                "desc" => true,
                other => bail!("sort order must be asc or desc, found {other}"),
            };
            let mut rows = rows;
            rows.sort_by(|a, b| {
                let ordering = compare(&field(a, &sort.by), &field(b, &sort.by));
                if descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
            rows
        }
        Op::Limit(limit) => rows.into_iter().take(limit.count).collect(),
        Op::Select(select) => rows
            .into_iter()
            .map(|row| {
                let mut out = Map::new();
                for (alias, path) in &select.fields {
                    out.insert(alias.clone(), field(&row, path).unwrap_or(Value::Null));
                }
                out
            })
            .collect(),
        Op::Unwind(unwind) => {
            let target = unwind.r#as.clone().unwrap_or_else(|| unwind.field.clone());
            let mut out = Vec::new();
            for row in rows {
                match field(&row, &unwind.field) {
                    Some(Value::Array(items)) if !items.is_empty() => {
                        for item in items {
                            let mut child = row.clone();
                            match item {
                                // An object element merges its keys in, so
                                // `achievements.name` is addressable as `name`.
                                Value::Object(map) => {
                                    for (key, value) in map {
                                        child.insert(key, value);
                                    }
                                    child.remove(&unwind.field);
                                }
                                scalar => {
                                    child.insert(target.clone(), scalar);
                                }
                            }
                            out.push(child);
                        }
                    }
                    _ if unwind.keep_empty => {
                        let mut child = row.clone();
                        child.insert(target.clone(), Value::Null);
                        out.push(child);
                    }
                    _ => {}
                }
            }
            out
        }
        Op::Derive(derive) => derive_column(rows, derive)?,
        Op::GroupAggregate(group) => group_aggregate(rows, group)?,
        Op::Bin(bin) => bin_column(rows, bin)?,
        Op::Unpivot(unpivot) => {
            if unpivot.fields.is_empty() {
                bail!("unpivot needs at least one field");
            }
            let name_column = unpivot.name_as.clone().unwrap_or_else(|| "name".into());
            let value_column = unpivot.value_as.clone().unwrap_or_else(|| "value".into());
            let mut out = Vec::with_capacity(rows.len() * unpivot.fields.len());
            for row in rows {
                for path in &unpivot.fields {
                    let found = field(&row, path);
                    if found.is_none() && !unpivot.keep_absent {
                        continue;
                    }
                    let mut child = row.clone();
                    for path in &unpivot.fields {
                        child.remove(path);
                    }
                    // The label is the last path segment, so `metrics.health`
                    // reads as `health` on the axis.
                    let label = path.rsplit('.').next().unwrap_or(path);
                    child.insert(name_column.clone(), Value::String(label.to_string()));
                    child.insert(value_column.clone(), found.unwrap_or(Value::Null));
                    out.push(child);
                }
            }
            out
        }
    })
}

fn matches(value: &Option<Value>, predicate: &Predicate) -> bool {
    let present = !matches!(value, None | Some(Value::Null));
    if let Some(required) = predicate.present {
        if present != required {
            return false;
        }
    }
    if let Some(expected) = &predicate.eq {
        if value.as_ref() != Some(expected) {
            return false;
        }
    }
    if let Some(expected) = &predicate.ne {
        if value.as_ref() == Some(expected) {
            return false;
        }
    }
    if let Some(options) = &predicate.any_of {
        match value {
            Some(found) if options.contains(found) => {}
            _ => return false,
        }
    }
    if let Some(needle) = &predicate.contains {
        match value {
            Some(Value::String(text)) if text.contains(needle.as_str()) => {}
            _ => return false,
        }
    }
    for (bound, keep) in [
        (predicate.gt, 1_i8),
        (predicate.gte, 2),
        (predicate.lt, 3),
        (predicate.lte, 4),
    ] {
        let Some(bound) = bound else { continue };
        // An absent value satisfies no numeric comparison. Treating it as zero
        // is how "unmeasured" silently becomes "below threshold".
        let Some(found) = number(value.as_ref()) else {
            return false;
        };
        let ok = match keep {
            1 => found > bound,
            2 => found >= bound,
            3 => found < bound,
            _ => found <= bound,
        };
        if !ok {
            return false;
        }
    }
    true
}

fn compare(a: &Option<Value>, b: &Option<Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (number(a.as_ref()), number(b.as_ref())) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        _ => match (a, b) {
            (Some(Value::String(left)), Some(Value::String(right))) => left.cmp(right),
            // Absent sorts last in ascending order, so a gap never leads.
            (None | Some(Value::Null), None | Some(Value::Null)) => Ordering::Equal,
            (None | Some(Value::Null), _) => Ordering::Greater,
            (_, None | Some(Value::Null)) => Ordering::Less,
            _ => Ordering::Equal,
        },
    }
}

fn derive_column(rows: Vec<Row>, derive: &DeriveOp) -> Result<Vec<Row>> {
    let expression = &derive.from;
    let declared = [
        expression.cumulative.is_some(),
        expression.delta.is_some(),
        expression.ratio.is_some(),
        expression.row_index.is_some(),
        expression.present.is_some(),
    ]
    .iter()
    .filter(|set| **set)
    .count();
    if declared != 1 {
        bail!("derive.from must name exactly one expression");
    }
    let mut out = Vec::with_capacity(rows.len());
    let mut running: Option<f64> = None;
    let mut previous: Option<f64> = None;
    for (index, row) in rows.into_iter().enumerate() {
        let mut row = row;
        let value = if let Some(source) = &expression.cumulative {
            match numeric_field(&row, source) {
                Some(found) => {
                    let total = running.unwrap_or(0.0) + found;
                    running = Some(total);
                    json_number(total)
                }
                // A gap does not reset the total, and it does not invent one.
                None => running.map(json_number).unwrap_or(Value::Null),
            }
        } else if let Some(source) = &expression.delta {
            let found = numeric_field(&row, source);
            let value = match (previous, found) {
                (Some(before), Some(now)) => json_number(now - before),
                _ => Value::Null,
            };
            if found.is_some() {
                previous = found;
            }
            value
        } else if let Some(pair) = &expression.ratio {
            if pair.len() != 2 {
                bail!("derive ratio takes exactly two field names");
            }
            match (numeric_field(&row, &pair[0]), numeric_field(&row, &pair[1])) {
                (Some(numerator), Some(denominator)) if denominator != 0.0 => {
                    json_number(numerator / denominator)
                }
                _ => Value::Null,
            }
        } else if expression.row_index.is_some() {
            json_number(index as f64)
        } else {
            let source = expression.present.as_ref().expect("checked above");
            Value::Bool(!matches!(field(&row, source), None | Some(Value::Null)))
        };
        row.insert(derive.field.clone(), value);
        out.push(row);
    }
    Ok(out)
}

fn json_number(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn group_aggregate(rows: Vec<Row>, group: &GroupAggregateOp) -> Result<Vec<Row>> {
    if group.aggregate.is_empty() {
        bail!("groupAggregate needs at least one aggregate");
    }
    if rows.is_empty() && group.by.is_empty() {
        // A grouped aggregate over nothing has no groups, but an ungrouped one
        // still has an answer: zero rows, and no measurement of anything else.
        // Returning no row at all would render as "not asked" rather than
        // "asked, and nothing was there".
        let mut row = Row::new();
        for (alias, aggregate) in &group.aggregate {
            row.insert(alias.clone(), reduce(&[], aggregate)?);
        }
        return Ok(vec![row]);
    }
    // Insertion order is the first appearance of each key, so the result is
    // stable without imposing an alphabetical order the author did not ask for.
    let mut order: Vec<String> = Vec::new();
    let mut buckets: BTreeMap<String, (Row, Vec<Row>)> = BTreeMap::new();
    for row in rows {
        let mut key = String::new();
        let mut carried = Map::new();
        for path in &group.by {
            let value = field(&row, path).unwrap_or(Value::Null);
            key.push_str(&value.to_string());
            key.push('\u{1f}');
            carried.insert(path.clone(), value);
        }
        if !buckets.contains_key(&key) {
            if buckets.len() >= MAX_GROUPS {
                bail!("groupAggregate produced more than {MAX_GROUPS} groups");
            }
            order.push(key.clone());
            buckets.insert(key.clone(), (carried, Vec::new()));
        }
        buckets.get_mut(&key).expect("just inserted").1.push(row);
    }
    let mut out = Vec::with_capacity(order.len());
    for key in order {
        let (carried, members) = buckets.remove(&key).expect("ordered key exists");
        let mut row = carried;
        for (alias, aggregate) in &group.aggregate {
            row.insert(alias.clone(), reduce(&members, aggregate)?);
        }
        out.push(row);
    }
    Ok(out)
}

fn reduce(rows: &[Row], aggregate: &Aggregate) -> Result<Value> {
    let func = aggregate.func.as_str();
    if func == "count" {
        // A count is defined over zero rows. Every other aggregate is not.
        return Ok(json_number(rows.len() as f64));
    }
    let path = aggregate
        .field
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("aggregate {func} needs a field"))?;
    if func == "countDistinct" {
        let distinct: BTreeSet<String> = rows
            .iter()
            .filter_map(|row| field(row, path))
            .filter(|value| !value.is_null())
            .map(|value| value.to_string())
            .collect();
        return Ok(json_number(distinct.len() as f64));
    }
    if func == "first" || func == "last" {
        let mut found = rows.iter().filter_map(|row| field(row, path));
        return Ok(if func == "first" {
            found.next().unwrap_or(Value::Null)
        } else {
            found.last().unwrap_or(Value::Null)
        });
    }
    let mut values: Vec<f64> = rows
        .iter()
        .filter_map(|row| numeric_field(row, path))
        .collect();
    if values.is_empty() {
        // Nothing was measured. Absence is the honest answer; zero would be a
        // measurement nobody took.
        return Ok(Value::Null);
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let result = match func {
        "sum" => values.iter().sum::<f64>(),
        "mean" | "rate" => values.iter().sum::<f64>() / values.len() as f64,
        "min" => values[0],
        "max" => values[values.len() - 1],
        "median" => percentile(&values, 0.5),
        "p25" => percentile(&values, 0.25),
        "p75" => percentile(&values, 0.75),
        "p90" => percentile(&values, 0.90),
        other => bail!("unknown aggregate {other}"),
    };
    Ok(json_number(result))
}

/// Linear interpolation between order statistics, matching the frontend
/// Craftax page so a derived percentile and the published one agree.
pub fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let position = (sorted.len() - 1) as f64 * quantile.clamp(0.0, 1.0);
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        return sorted[lower];
    }
    sorted[lower] + (sorted[upper] - sorted[lower]) * (position - lower as f64)
}

fn bin_column(rows: Vec<Row>, bin: &BinOp) -> Result<Vec<Row>> {
    if bin.bins < 2 || bin.bins > 200 {
        bail!("bin count must be 2..200");
    }
    let target = bin.r#as.clone().unwrap_or_else(|| "bin".into());
    let values: Vec<f64> = rows
        .iter()
        .filter_map(|row| numeric_field(row, &bin.field))
        .collect();
    if values.is_empty() {
        return Ok(rows);
    }
    let low = values.iter().copied().fold(f64::INFINITY, f64::min);
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let width = if (high - low).abs() < f64::EPSILON {
        1.0
    } else {
        (high - low) / bin.bins as f64
    };
    Ok(rows
        .into_iter()
        .map(|mut row| {
            let edge = numeric_field(&row, &bin.field).map(|value| {
                let index = (((value - low) / width).floor() as usize).min(bin.bins - 1);
                low + index as f64 * width
            });
            row.insert(target.clone(), edge.map(json_number).unwrap_or(Value::Null));
            row
        })
        .collect())
}

// ── Channel mappings ────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeriesFrom {
    pub source: DataSource,
    pub series: Vec<SeriesMap>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeriesMap {
    #[serde(default)]
    pub name: Option<String>,
    /// Split the rows into one series per distinct value of this column.
    #[serde(default)]
    pub name_field: Option<String>,
    pub x: String,
    pub y: String,
    #[serde(default)]
    pub band: Option<BandMap>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BandMap {
    pub lo: String,
    pub hi: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BarsFrom {
    pub source: DataSource,
    pub category: String,
    pub series: Vec<BarMap>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BarMap {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub name_field: Option<String>,
    pub value: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScatterFrom {
    pub source: DataSource,
    pub x: String,
    pub y: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistogramFrom {
    pub source: DataSource,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeatmapFrom {
    pub source: DataSource,
    pub row: String,
    pub column: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableFrom {
    pub source: DataSource,
    pub columns: Vec<ColumnMap>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColumnMap {
    pub header: String,
    pub field: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricsFrom {
    pub source: DataSource,
    /// Per-row form: every row becomes one metric, with the label read from a
    /// column.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub tone: Option<String>,
    /// Single-row form: one aggregate row becomes several metrics, each with a
    /// written label and a column to read. This is the shape a KPI strip over
    /// an aggregate actually has — the label is language, not data.
    #[serde(default)]
    pub items: Vec<MetricMap>,
    /// Which row the single-row form reads; defaults to the first.
    #[serde(default)]
    pub row: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricMap {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub tone: Option<String>,
}

// ── Derivation ──────────────────────────────────────────────────────────────

use super::charts::{
    Axis, BarSeries, BarsPanel, Cell, ChartSpec, HeatmapPanel, HistogramPanel, MetricItem,
    MetricsPanel, Panel, Point, ScatterPanel, ScatterPoint, Series, SeriesPanel, TablePanel,
};

/// Slots the spec's `from` blocks name, so the caller resolves only the
/// evidence this chart actually reads.
pub fn required_slots(spec: &ChartSpec) -> BTreeMap<String, Option<String>> {
    let mut out = BTreeMap::new();
    for panel in &spec.panels {
        if let Some(source) = panel_source(panel) {
            out.entry(source.input.clone())
                .or_insert_with(|| source.projection.clone());
        }
    }
    out
}

fn panel_source(panel: &Panel) -> Option<&DataSource> {
    match panel {
        Panel::Metrics(p) => p.from.as_ref().map(|from| &from.source),
        Panel::Series(p) => p.from.as_ref().map(|from| &from.source),
        Panel::Bars(p) => p.from.as_ref().map(|from| &from.source),
        Panel::Scatter(p) => p.from.as_ref().map(|from| &from.source),
        Panel::Histogram(p) => p.from.as_ref().map(|from| &from.source),
        Panel::Heatmap(p) => p.from.as_ref().map(|from| &from.source),
        Panel::Table(p) => p.from.as_ref().map(|from| &from.source),
        Panel::Note(_) => None,
    }
}

/// Replace every `from` block with the values it derives. The result is a
/// literal spec — the only thing the renderer ever sees.
pub fn resolve(spec: &ChartSpec, documents: &BTreeMap<String, Value>) -> Result<ChartSpec> {
    let mut resolved = spec.clone();
    for panel in &mut resolved.panels {
        let Some(source) = panel_source(panel).cloned() else {
            continue;
        };
        let document = documents.get(&source.input).ok_or_else(|| {
            anyhow::anyhow!(
                "panel reads input {} but the visual has no binding on that input",
                source.input
            )
        })?;
        let rows = table(document, &source)?;
        derive_panel(panel, &rows)
            .with_context(|| format!("deriving a panel from input {}", source.input))?;
    }
    Ok(resolved)
}

fn derive_panel(panel: &mut Panel, rows: &[Row]) -> Result<()> {
    match panel {
        Panel::Metrics(target) => {
            let from = target.from.take().expect("source implies from");
            if !from.items.is_empty() {
                if from.label.is_some() || from.value.is_some() {
                    bail!("metrics `from` takes either items or label/value, not both");
                }
                let index = from.row.unwrap_or(0);
                target.items = match rows.get(index) {
                    Some(row) => from
                        .items
                        .iter()
                        .map(|map| MetricItem {
                            label: map.label.clone(),
                            value: display(field(row, &map.value).as_ref()),
                            detail: map
                                .detail
                                .as_deref()
                                .and_then(|path| label_field(row, path)),
                            tone: map.tone.clone(),
                        })
                        .collect(),
                    // The row the author asked for is not there. Say so on every
                    // tile rather than dropping the strip.
                    None => from
                        .items
                        .iter()
                        .map(|map| MetricItem {
                            label: map.label.clone(),
                            value: "—".into(),
                            detail: None,
                            tone: map.tone.clone(),
                        })
                        .collect(),
                };
            } else {
                let label = from
                    .label
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("metrics `from` needs items or label/value"))?;
                let value = from
                    .value
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("metrics `from` needs items or label/value"))?;
                target.items = rows
                    .iter()
                    .map(|row| MetricItem {
                        label: label_field(row, label).unwrap_or_else(|| "—".into()),
                        value: display(field(row, value).as_ref()),
                        detail: from
                            .detail
                            .as_deref()
                            .and_then(|path| label_field(row, path)),
                        tone: from.tone.as_deref().and_then(|path| label_field(row, path)),
                    })
                    .collect();
            }
        }
        Panel::Series(target) => {
            let from = target.from.take().expect("source implies from");
            let mut out = Vec::new();
            for map in &from.series {
                for (name, group) in split(rows, map.name_field.as_deref()) {
                    let points: Vec<Point> = group
                        .iter()
                        .filter_map(|row| {
                            numeric_field(row, &map.x).map(|x| Point {
                                x,
                                y: numeric_field(row, &map.y),
                            })
                        })
                        .collect();
                    let band = match &map.band {
                        Some(band) => group
                            .iter()
                            .filter_map(|row| {
                                let x = numeric_field(row, &map.x)?;
                                let lo = numeric_field(row, &band.lo)?;
                                let hi = numeric_field(row, &band.hi)?;
                                Some(super::charts::Band { x, lo, hi })
                            })
                            .collect(),
                        None => Vec::new(),
                    };
                    out.push(Series {
                        name: name
                            .or_else(|| map.name.clone())
                            .unwrap_or_else(|| map.y.clone()),
                        color: map.color.clone(),
                        style: map.style.clone().unwrap_or_else(|| "line".into()),
                        points,
                        band,
                    });
                }
            }
            target.series = out;
        }
        Panel::Bars(target) => {
            let from = target.from.take().expect("source implies from");
            let categories = distinct(rows, &from.category);
            let mut out = Vec::new();
            for map in &from.series {
                for (name, group) in split(rows, map.name_field.as_deref()) {
                    let mut values = vec![None; categories.len()];
                    for row in &group {
                        let Some(category) = label_field(row, &from.category) else {
                            continue;
                        };
                        let Some(index) = categories.iter().position(|item| *item == category)
                        else {
                            continue;
                        };
                        values[index] = numeric_field(row, &map.value);
                    }
                    out.push(BarSeries {
                        name: name
                            .or_else(|| map.name.clone())
                            .unwrap_or_else(|| map.value.clone()),
                        color: map.color.clone(),
                        values,
                    });
                }
            }
            target.categories = categories;
            target.series = out;
        }
        Panel::Scatter(target) => {
            let from = target.from.take().expect("source implies from");
            target.points = rows
                .iter()
                .filter_map(|row| {
                    Some(ScatterPoint {
                        x: numeric_field(row, &from.x)?,
                        y: numeric_field(row, &from.y)?,
                        label: from
                            .label
                            .as_deref()
                            .and_then(|path| label_field(row, path)),
                        color: None,
                        group: from
                            .group
                            .as_deref()
                            .and_then(|path| label_field(row, path)),
                    })
                })
                .collect();
        }
        Panel::Histogram(target) => {
            let from = target.from.take().expect("source implies from");
            target.values = rows
                .iter()
                .filter_map(|row| numeric_field(row, &from.value))
                .collect();
        }
        Panel::Heatmap(target) => {
            let from = target.from.take().expect("source implies from");
            let row_labels = distinct(rows, &from.row);
            let column_labels = distinct(rows, &from.column);
            let mut values = vec![vec![None; column_labels.len()]; row_labels.len()];
            let mut written = vec![vec![false; column_labels.len()]; row_labels.len()];
            for row in rows {
                let (Some(row_label), Some(column_label)) =
                    (label_field(row, &from.row), label_field(row, &from.column))
                else {
                    continue;
                };
                let (Some(r), Some(c)) = (
                    row_labels.iter().position(|item| *item == row_label),
                    column_labels.iter().position(|item| *item == column_label),
                ) else {
                    continue;
                };
                if written[r][c] {
                    // Two rows for one cell means the caller has not decided
                    // what the cell is. Overwriting silently would publish
                    // whichever row happened to come last.
                    bail!(
                        "heatmap has more than one row for cell ({row_label}, {column_label}); aggregate before mapping"
                    );
                }
                written[r][c] = true;
                values[r][c] = numeric_field(row, &from.value);
            }
            target.rows = row_labels;
            target.columns = column_labels;
            target.values = values;
        }
        Panel::Table(target) => {
            let from = target.from.take().expect("source implies from");
            target.columns = from.columns.iter().map(|map| map.header.clone()).collect();
            target.rows = rows
                .iter()
                .map(|row| {
                    from.columns
                        .iter()
                        .map(|map| match field(row, &map.field) {
                            None | Some(Value::Null) => Cell::Null,
                            Some(Value::Number(number)) => number
                                .as_f64()
                                .filter(|value| value.is_finite())
                                .map(Cell::Number)
                                .unwrap_or(Cell::Null),
                            Some(Value::String(text)) => Cell::Text(text),
                            Some(other) => Cell::Text(other.to_string()),
                        })
                        .collect()
                })
                .collect();
        }
        Panel::Note(_) => {}
    }
    Ok(())
}

/// One group per distinct value of `name_field`, in first-appearance order; a
/// single unnamed group when no split column is given.
fn split(rows: &[Row], name_field: Option<&str>) -> Vec<(Option<String>, Vec<Row>)> {
    let Some(path) = name_field else {
        return vec![(None, rows.to_vec())];
    };
    let mut order: Vec<String> = Vec::new();
    let mut groups: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for row in rows {
        let name = label_field(row, path).unwrap_or_else(|| "—".into());
        if !groups.contains_key(&name) {
            order.push(name.clone());
            groups.insert(name.clone(), Vec::new());
        }
        groups
            .get_mut(&name)
            .expect("just inserted")
            .push(row.clone());
    }
    order
        .into_iter()
        .map(|name| {
            let rows = groups.remove(&name).expect("ordered key exists");
            (Some(name), rows)
        })
        .collect()
}

fn distinct(rows: &[Row], path: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for row in rows {
        if let Some(value) = label_field(row, path) {
            if !out.contains(&value) {
                out.push(value);
            }
        }
    }
    out
}

/// Metric values are display strings. Numbers get a compact, stable rendering;
/// an absent value says so rather than showing a zero.
fn display(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "—".into(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => {
            let Some(found) = number.as_f64() else {
                return number.to_string();
            };
            if found.fract().abs() < 1e-9 && found.abs() < 1e15 {
                format!("{}", found as i64)
            } else if found.abs() >= 100.0 {
                format!("{found:.1}")
            } else if found.abs() >= 1.0 {
                format!("{found:.2}")
            } else {
                format!("{found:.3}")
            }
        }
        Some(other) => other.to_string(),
    }
}

/// Axis captions default to the mapped column when the author did not write
/// one, so a derived panel is never unlabelled.
pub fn default_axis(axis: &Axis, field_name: &str) -> Axis {
    if axis.label.is_some() {
        return axis.clone();
    }
    Axis {
        label: Some(field_name.to_string()),
        ..axis.clone()
    }
}

