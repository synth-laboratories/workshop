//! The typed, bounded query contract.
//!
//! The agent never writes LogsQL, SQL, or a path. It fills in this struct, and
//! every backend (journal SQL, VictoriaLogs LogsQL) compiles *from* it. Unknown
//! fields, oversized ranges, oversized limits, and unknown enum members are
//! refused here — once, at the edge — so no backend has to be trusted to
//! enforce them.

use super::event::{
    is_known_component, scope_components, Correlation, Severity, CORRELATION_FIELDS,
};
use anyhow::{bail, Result};
use serde_json::Value;
use std::time::Duration;

/// Hard ceilings. These are the contract, not defaults a caller can raise.
pub const MAX_LIMIT: usize = 500;
pub const DEFAULT_LIMIT: usize = 100;
pub const MAX_RANGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub const DEFAULT_RANGE: Duration = Duration::from_secs(60 * 60);
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_FILTER_VALUES: usize = 32;

/// Field names accepted in a query object. Anything else is an error, so a
/// hallucinated `sql`, `logsql`, `path`, or `url` field fails loudly instead of
/// being silently ignored.
pub const QUERY_FIELDS: &[&str] = &[
    "scope",
    "component",
    "severity",
    "code",
    "event",
    "since",
    "until",
    "limit",
    "cursor",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticQuery {
    pub components: Vec<String>,
    pub severities: Vec<Severity>,
    pub codes: Vec<String>,
    pub events: Vec<String>,
    pub correlation: Correlation,
    pub since: Duration,
    pub until: Option<Duration>,
    pub limit: usize,
    /// Page backwards from this journal sequence (exclusive).
    pub cursor: Option<i64>,
}

impl Default for DiagnosticQuery {
    fn default() -> Self {
        Self {
            components: Vec::new(),
            severities: Vec::new(),
            codes: Vec::new(),
            events: Vec::new(),
            correlation: Correlation::default(),
            since: DEFAULT_RANGE,
            until: None,
            limit: DEFAULT_LIMIT,
            cursor: None,
        }
    }
}

impl DiagnosticQuery {
    /// Absolute lower bound of the query window.
    pub fn start_timestamp(&self, now: chrono::DateTime<chrono::Utc>) -> String {
        (now - chrono::Duration::from_std(self.since)
            .unwrap_or_else(|_| chrono::Duration::hours(1)))
        .to_rfc3339()
    }

    pub fn end_timestamp(&self, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
        self.until
            .map(|until| (now - chrono::Duration::from_std(until).unwrap_or_default()).to_rfc3339())
    }
}

/// Parse an agent- or renderer-supplied query object.
pub fn parse(value: &Value) -> Result<DiagnosticQuery> {
    let object = match value {
        Value::Null => return Ok(DiagnosticQuery::default()),
        Value::Object(object) => object,
        _ => bail!("diagnostic query must be an object"),
    };

    for key in object.keys() {
        if !QUERY_FIELDS.contains(&key.as_str()) && !CORRELATION_FIELDS.contains(&key.as_str()) {
            bail!("diagnostic query rejects unknown field `{key}`");
        }
    }

    let mut query = DiagnosticQuery::default();

    let mut components: Vec<String> = Vec::new();
    for scope in string_list(object.get("scope"), "scope")? {
        let expanded = scope_components(&scope)
            .ok_or_else(|| anyhow::anyhow!("unknown diagnostic scope `{scope}`"))?;
        components.extend(expanded.iter().map(|value| (*value).to_owned()));
    }
    for component in string_list(object.get("component"), "component")? {
        if !is_known_component(&component) {
            bail!("unknown diagnostic component `{component}`");
        }
        components.push(component);
    }
    components.sort();
    components.dedup();
    query.components = components;

    for severity in string_list(object.get("severity"), "severity")? {
        let parsed = Severity::parse(&severity)
            .ok_or_else(|| anyhow::anyhow!("unknown diagnostic severity `{severity}`"))?;
        if !query.severities.contains(&parsed) {
            query.severities.push(parsed);
        }
    }

    query.codes = identifier_list(object.get("code"), "code")?;
    query.events = identifier_list(object.get("event"), "event")?;

    for field in CORRELATION_FIELDS {
        if let Some(value) = object.get(*field) {
            let identity = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("diagnostic query field `{field}` must be a non-empty string")
                })?;
            if identity.len() > super::event::MAX_IDENTIFIER_CHARS {
                bail!("diagnostic query field `{field}` is too long");
            }
            query.correlation.set(field, Some(identity.to_owned()));
        }
    }

    if let Some(since) = object.get("since") {
        query.since = parse_duration(since).map_err(|error| anyhow::anyhow!("`since`: {error}"))?;
        if query.since > MAX_RANGE {
            bail!(
                "diagnostic query `since` exceeds the {} day maximum",
                MAX_RANGE.as_secs() / 86_400
            );
        }
    }
    if let Some(until) = object.get("until") {
        let parsed = parse_duration(until).map_err(|error| anyhow::anyhow!("`until`: {error}"))?;
        if parsed >= query.since {
            bail!("diagnostic query `until` must be more recent than `since`");
        }
        query.until = Some(parsed);
    }

    if let Some(limit) = object.get("limit") {
        let limit = limit
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("diagnostic query `limit` must be a number"))?;
        if limit == 0 {
            bail!("diagnostic query `limit` must be at least 1");
        }
        if limit as usize > MAX_LIMIT {
            bail!("diagnostic query `limit` exceeds the maximum of {MAX_LIMIT}");
        }
        query.limit = limit as usize;
    }

    if let Some(cursor) = object.get("cursor") {
        if !cursor.is_null() {
            let cursor = cursor
                .as_i64()
                .or_else(|| cursor.as_str().and_then(|value| value.parse().ok()))
                .ok_or_else(|| anyhow::anyhow!("diagnostic query `cursor` must be a sequence"))?;
            if cursor < 0 {
                bail!("diagnostic query `cursor` must not be negative");
            }
            query.cursor = Some(cursor);
        }
    }

    Ok(query)
}

fn string_list(value: Option<&Value>, field: &str) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = match value {
        Value::Null => return Ok(Vec::new()),
        Value::String(single) => vec![single.clone()],
        Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str().map(|value| value.to_owned()).ok_or_else(|| {
                    anyhow::anyhow!("diagnostic query `{field}` must contain strings")
                })
            })
            .collect::<Result<Vec<_>>>()?,
        _ => bail!("diagnostic query `{field}` must be a string or array of strings"),
    };
    if items.len() > MAX_FILTER_VALUES {
        bail!("diagnostic query `{field}` accepts at most {MAX_FILTER_VALUES} values");
    }
    Ok(items
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect())
}

/// Codes and event names are indexed labels. Accept only the identifier shape
/// they are emitted with — this is what keeps a filter value from becoming an
/// injection vector in either backend.
fn identifier_list(value: Option<&Value>, field: &str) -> Result<Vec<String>> {
    let items = string_list(value, field)?;
    for item in &items {
        if !item.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        }) {
            bail!("diagnostic query `{field}` value `{item}` is not an identifier");
        }
    }
    Ok(items)
}

/// `"20m"`, `"2h"`, `"7d"`, `"90s"`. Bare numbers are seconds.
pub fn parse_duration(value: &Value) -> Result<Duration> {
    let text = match value {
        Value::Number(number) => {
            let seconds = number
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("must be a positive number of seconds"))?;
            return Ok(Duration::from_secs(seconds));
        }
        Value::String(text) => text.trim().to_ascii_lowercase(),
        _ => bail!("must be a duration such as `20m`, `2h`, or `7d`"),
    };
    if text.is_empty() {
        bail!("must not be empty");
    }
    let (digits, unit) = text.split_at(
        text.find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len()),
    );
    let amount: u64 = digits
        .parse()
        .map_err(|_| anyhow::anyhow!("must start with a number"))?;
    let seconds = match unit {
        "" | "s" | "sec" | "secs" => amount,
        "m" | "min" | "mins" => amount * 60,
        "h" | "hr" | "hrs" => amount * 3_600,
        "d" | "day" | "days" => amount * 86_400,
        other => bail!("unknown duration unit `{other}`"),
    };
    Ok(Duration::from_secs(seconds))
}

