//! Typed, read-only trace queries and the immutable snapshots they produce.
//!
//! Agents never receive SQL. They send a structured query that compiles to a
//! parameterized statement over `trace_index` — the projection index, not the
//! sealed archives, which are never re-parsed to render a filtered list.
//!
//! Every field below is an allow-listed column. An unknown field is an error
//! rather than a passthrough, so storage can evolve without an agent's habits
//! becoming a compatibility constraint.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const TRACE_QUERY_SCHEMA: &str = "synth.trace-query.v1";
pub const TRACE_QUERY_RESULT_SCHEMA: &str = "synth.trace-query-result.v1";

/// Hard ceiling on rows a single query may return, whatever it asks for.
pub const MAX_LIMIT: i64 = 200;
/// Text search is bounded so a query cannot become a table scan over free text.
pub const MAX_TEXT_LEN: usize = 200;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TraceQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#where: Option<TraceWhere>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_by: Vec<TraceOrder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TraceWhere {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace_digests: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harness: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub benchmark: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_id: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lifecycle_status: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capture_status: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_evidence: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_media: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reward: Option<RangeF64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<TimeRange>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RangeF64 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gte: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lte: Option<f64>,
}

/// RFC3339 bounds. `trace_index` has no `created_at`; `started_at` is the
/// column that exists, and naming it here keeps the AST honest about that.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimeRange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TraceOrder {
    pub field: String,
    #[serde(default = "default_direction")]
    pub direction: String,
}

fn default_direction() -> String {
    "desc".into()
}

/// Sortable columns. Anything absent here cannot be named in `order_by`, so no
/// caller-supplied string ever reaches the SQL text.
const ORDERABLE: [&str; 7] = [
    "started_at",
    "reward",
    "duration_ms",
    "event_count",
    "tool_call_count",
    "error_count",
    "cost_usd",
];

/// A compiled query: SQL text plus its bound parameters, never interpolated.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledQuery {
    pub sql: String,
    pub params: Vec<Value>,
    pub limit: i64,
}

impl TraceQuery {
    pub fn compile(&self) -> Result<CompiledQuery> {
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<Value> = Vec::new();

        if let Some(text) = self.text.as_deref() {
            let text = text.trim();
            if text.len() > MAX_TEXT_LEN {
                bail!("text search is limited to {MAX_TEXT_LEN} characters");
            }
            if !text.is_empty() {
                clauses.push("search_text LIKE ?".into());
                params.push(json!(format!("%{text}%")));
            }
        }

        if let Some(filter) = &self.r#where {
            let mut push_in = |column: &str, values: &Vec<String>| {
                if values.is_empty() {
                    return;
                }
                let holes = vec!["?"; values.len()].join(", ");
                clauses.push(format!("{column} IN ({holes})"));
                params.extend(values.iter().map(|value| json!(value)));
            };
            push_in("trace_digest", &filter.trace_digests);
            push_in("model", &filter.model);
            push_in("provider", &filter.provider);
            push_in("harness", &filter.harness);
            push_in("benchmark", &filter.benchmark);
            push_in("task_id", &filter.task_id);
            push_in("lifecycle_status", &filter.lifecycle_status);
            push_in("capture_status", &filter.capture_status);

            if let Some(has_evidence) = filter.has_evidence {
                clauses.push("has_evidence = ?".into());
                params.push(json!(i64::from(has_evidence)));
            }
            if let Some(has_media) = filter.has_media {
                clauses.push("has_media = ?".into());
                params.push(json!(i64::from(has_media)));
            }
            if let Some(reward) = &filter.reward {
                if let Some(gte) = reward.gte {
                    clauses.push("reward >= ?".into());
                    params.push(json!(gte));
                }
                if let Some(lte) = reward.lte {
                    clauses.push("reward <= ?".into());
                    params.push(json!(lte));
                }
            }
            if let Some(started) = &filter.started {
                if let Some(after) = &started.after {
                    clauses.push("started_at >= ?".into());
                    params.push(json!(after));
                }
                if let Some(before) = &started.before {
                    clauses.push("started_at <= ?".into());
                    params.push(json!(before));
                }
            }
        }

        let mut order = Vec::new();
        for entry in &self.order_by {
            if !ORDERABLE.contains(&entry.field.as_str()) {
                bail!("`{}` is not an orderable trace field", entry.field);
            }
            let direction = match entry.direction.to_ascii_lowercase().as_str() {
                "asc" => "ASC",
                "desc" => "DESC",
                other => bail!("`{other}` is not a sort direction"),
            };
            order.push(format!("{} {direction}", entry.field));
        }
        if order.is_empty() {
            order.push("started_at DESC".into());
        }
        // Ties must not reorder between runs, or the same query would produce a
        // different snapshot each time it is taken.
        order.push("trace_digest ASC".into());

        let limit = self.limit.unwrap_or(MAX_LIMIT).clamp(1, MAX_LIMIT);
        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };
        let sql = format!(
            "SELECT trace_digest, model, provider, benchmark, task_id, lifecycle_status, \
             capture_status, reward, cost_usd, event_count, tool_call_count, error_count, \
             duration_ms, started_at, has_media, has_evidence FROM trace_index{where_sql} \
             ORDER BY {} LIMIT ?",
            order.join(", ")
        );
        params.push(json!(limit));

        Ok(CompiledQuery { sql, params, limit })
    }
}

/// An immutable result set, addressable by id and reproducible from its AST.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuerySnapshot {
    pub schema_version: String,
    pub snapshot_id: String,
    pub domain: String,
    pub query_schema_version: String,
    pub query_ast: Value,
    pub result_ids: Vec<String>,
    pub result_count: usize,
    pub facets: Value,
    pub result_digest: String,
    pub queried_at: String,
    /// True when the hard cap cut the result set — never reported as complete.
    pub truncated: bool,
}

/// Stable identity for a result set: the same rows in the same order under the
/// same query always digest alike, so a re-run that changed nothing is visible
/// as such.
pub fn result_digest(query_ast: &Value, result_ids: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(TRACE_QUERY_RESULT_SCHEMA.as_bytes());
    hasher.update(
        serde_json::to_vec(query_ast)
            .unwrap_or_default()
            .as_slice(),
    );
    for id in result_ids {
        hasher.update(b"\0");
        hasher.update(id.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

pub fn snapshot_id(result_digest: &str) -> String {
    let short = result_digest.trim_start_matches("sha256:");
    format!("trace_query_{}", &short[..short.len().min(32)])
}

/// Parse an agent-supplied query, rejecting unknown fields outright.
pub fn parse_query(value: &Value) -> Result<TraceQuery> {
    serde_json::from_value(value.clone()).context("query rejected")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_is_bounded_and_deterministically_ordered() {
        let compiled = TraceQuery::default().compile().unwrap();
        assert!(compiled.sql.contains("FROM trace_index"));
        assert!(compiled.sql.ends_with("LIMIT ?"));
        assert!(compiled.sql.contains("ORDER BY started_at DESC, trace_digest ASC"));
        assert_eq!(compiled.limit, MAX_LIMIT);
        assert_eq!(compiled.params, vec![json!(MAX_LIMIT)]);
    }

    #[test]
    fn every_value_is_bound_and_never_interpolated() {
        let query = TraceQuery {
            text: Some("reward chart".into()),
            r#where: Some(TraceWhere {
                benchmark: vec!["craftax".into()],
                lifecycle_status: vec!["failed".into(), "error".into()],
                reward: Some(RangeF64 { gte: Some(0.5), lte: None }),
                started: Some(TimeRange { after: Some("2026-08-14T00:00:00Z".into()), before: None }),
                ..TraceWhere::default()
            }),
            order_by: vec![TraceOrder { field: "reward".into(), direction: "asc".into() }],
            limit: Some(25),
        };
        let compiled = query.compile().unwrap();
        assert!(compiled.sql.contains("benchmark IN (?)"));
        assert!(compiled.sql.contains("lifecycle_status IN (?, ?)"));
        assert!(compiled.sql.contains("ORDER BY reward ASC, trace_digest ASC"));
        // No caller-supplied value appears in the statement text.
        for needle in ["craftax", "failed", "0.5", "2026-08-14", "reward chart", "25"] {
            assert!(!compiled.sql.contains(needle), "{needle} leaked into SQL");
        }
        assert_eq!(compiled.params.len(), 7);
        assert_eq!(compiled.limit, 25);
    }

    #[test]
    fn the_row_cap_cannot_be_raised_by_asking() {
        assert_eq!(TraceQuery { limit: Some(100_000), ..Default::default() }.compile().unwrap().limit, MAX_LIMIT);
        assert_eq!(TraceQuery { limit: Some(0), ..Default::default() }.compile().unwrap().limit, 1);
        assert_eq!(TraceQuery { limit: Some(-5), ..Default::default() }.compile().unwrap().limit, 1);
    }

    #[test]
    fn only_allow_listed_fields_can_be_named() {
        let bad_order = TraceQuery {
            order_by: vec![TraceOrder { field: "trace_digest; DROP TABLE trace_index".into(), direction: "asc".into() }],
            ..Default::default()
        };
        assert!(bad_order.compile().is_err());

        let bad_direction = TraceQuery {
            order_by: vec![TraceOrder { field: "reward".into(), direction: "asc; DELETE FROM traces".into() }],
            ..Default::default()
        };
        assert!(bad_direction.compile().is_err());

        // An unknown filter is refused at parse time rather than ignored.
        assert!(parse_query(&json!({"where": {"path": "/etc/passwd"}})).is_err());
        assert!(parse_query(&json!({"sql": "SELECT 1"})).is_err());
        assert!(parse_query(&json!({"limit": 10})).is_ok());
    }

    #[test]
    fn oversized_text_search_is_refused_rather_than_truncated() {
        let query = TraceQuery { text: Some("x".repeat(MAX_TEXT_LEN + 1)), ..Default::default() };
        assert!(query.compile().is_err());
    }

    #[test]
    fn identity_covers_the_query_and_the_rows_in_order() {
        let ast = json!({"limit": 10});
        let a = result_digest(&ast, &["d1".into(), "d2".into()]);
        assert_eq!(a, result_digest(&ast, &["d1".into(), "d2".into()]));
        // Reordering is a different result set, not the same one.
        assert_ne!(a, result_digest(&ast, &["d2".into(), "d1".into()]));
        // Same rows from a different question is also a different snapshot.
        assert_ne!(a, result_digest(&json!({"limit": 11}), &["d1".into(), "d2".into()]));
        assert!(snapshot_id(&a).starts_with("trace_query_"));
    }
}
