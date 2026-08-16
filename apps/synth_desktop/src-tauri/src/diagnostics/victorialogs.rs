//! VictoriaLogs client and the typed-query → LogsQL compiler.
//!
//! Two rules hold this file together:
//!
//! 1. **The index returns identities, not records.** Every compiled query ends
//!    in `| fields journal_sequence`, and the caller reads the events back from
//!    the authoritative journal. A wiped, stale, or restarted index therefore
//!    cannot change what a query *answers*, only how fast it finds it.
//! 2. **No caller-supplied text ever reaches LogsQL.** Filter values are
//!    validated against a strict identity charset and refused otherwise. There
//!    is no raw-LogsQL parameter to guard, because there is no raw-LogsQL
//!    parameter.

use super::query::DiagnosticQuery;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::time::Duration;

/// Local ingestion budget. Indexing is background work; it must never hold a
/// connection long enough to matter.
pub const INGEST_TIMEOUT: Duration = Duration::from_secs(10);
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
pub const HEALTH_TIMEOUT: Duration = Duration::from_millis(1_500);

/// Stream fields. Deliberately the low-cardinality labels only: adding a
/// correlation identity here would mint a VictoriaLogs stream per rollout.
const STREAM_FIELDS: &str = "component,severity";

#[derive(Clone)]
pub struct VictoriaLogsClient {
    base_url: String,
    http: reqwest::Client,
}

impl VictoriaLogsClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(INGEST_TIMEOUT)
            // A local sidecar is never reached through a corporate proxy, and
            // inheriting one would send diagnostics off the machine.
            .no_proxy()
            .build()
            .context("build VictoriaLogs client")?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            http,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn healthy(&self) -> bool {
        let Ok(response) = self
            .http
            .get(format!("{}/health", self.base_url))
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
        else {
            return false;
        };
        response.status().is_success()
    }

    /// Index one batch as newline-delimited JSON.
    pub async fn ingest(&self, lines: &[Value]) -> Result<()> {
        if lines.is_empty() {
            return Ok(());
        }
        let mut body = String::with_capacity(lines.len() * 256);
        for line in lines {
            body.push_str(&line.to_string());
            body.push('\n');
        }
        let response = self
            .http
            .post(format!(
                "{}/insert/jsonline?_stream_fields={STREAM_FIELDS}&_msg_field=_msg&_time_field=_time",
                self.base_url
            ))
            .header("content-type", "application/stream+json")
            .body(body)
            .timeout(INGEST_TIMEOUT)
            .send()
            .await
            .context("send diagnostics to VictoriaLogs")?;
        if !response.status().is_success() {
            bail!(
                "VictoriaLogs rejected an ingest batch with {}",
                response.status()
            );
        }
        Ok(())
    }

    /// Run a compiled query and return the matching journal sequences, newest
    /// first, deduplicated.
    pub async fn search_sequences(&self, logsql: &str, limit: usize) -> Result<Vec<i64>> {
        let response = self
            .http
            .post(format!("{}/select/logsql/query", self.base_url))
            .form(&[
                ("query", logsql.to_owned()),
                ("limit", limit.to_string()),
            ])
            .timeout(QUERY_TIMEOUT)
            .send()
            .await
            .context("query VictoriaLogs")?;
        if !response.status().is_success() {
            bail!("VictoriaLogs query failed with {}", response.status());
        }
        let body = response.text().await.context("read VictoriaLogs response")?;
        Ok(parse_sequences(&body))
    }
}

/// VictoriaLogs answers with newline-delimited JSON objects.
pub fn parse_sequences(body: &str) -> Vec<i64> {
    let mut sequences: Vec<i64> = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|row| {
            row.get("journal_sequence").and_then(|value| {
                value
                    .as_i64()
                    .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            })
        })
        .collect();
    sequences.sort_unstable_by(|left, right| right.cmp(left));
    sequences.dedup();
    sequences
}

/// Compile a typed query into bounded LogsQL.
pub fn compile(query: &DiagnosticQuery, now: chrono::DateTime<chrono::Utc>) -> Result<String> {
    let mut filters: Vec<String> = Vec::new();

    let start = query.start_timestamp(now);
    let end = query
        .end_timestamp(now)
        .unwrap_or_else(|| now.to_rfc3339());
    filters.push(format!(
        "_time:[{}, {}]",
        quote_time(&start)?,
        quote_time(&end)?
    ));

    if !query.components.is_empty() {
        filters.push(in_filter("component", &query.components)?);
    }
    if !query.severities.is_empty() {
        let values: Vec<String> = query
            .severities
            .iter()
            .map(|severity| severity.as_str().to_owned())
            .collect();
        filters.push(in_filter("severity", &values)?);
    }
    if !query.codes.is_empty() {
        filters.push(in_filter("code", &query.codes)?);
    }
    if !query.events.is_empty() {
        filters.push(in_filter("event", &query.events)?);
    }
    for field in super::event::CORRELATION_FIELDS {
        if let Some(value) = query.correlation.get(field) {
            filters.push(format!("{field}:={}", quote_identity(value)?));
        }
    }

    // Paging happens on the authoritative sequence, not on a log-index cursor.
    // Over-fetch so the post-filter can still fill a page.
    let fetch = (query.limit * 4).min(super::query::MAX_LIMIT * 4);
    Ok(format!(
        "{} | fields journal_sequence | sort by (journal_sequence) desc | limit {fetch}",
        filters.join(" ")
    ))
}

fn in_filter(field: &str, values: &[String]) -> Result<String> {
    let quoted = values
        .iter()
        .map(|value| quote_identity(value))
        .collect::<Result<Vec<_>>>()?
        .join(",");
    Ok(format!("{field}:in({quoted})"))
}

/// Quote a filter value for LogsQL after proving it is an identity.
///
/// Refusing is correct here: a correlation identity that contains a quote, a
/// backslash, a pipe, or a control character is not an identity this system
/// ever minted, and the only reason to accept one would be to let it change the
/// meaning of the query.
fn quote_identity(value: &str) -> Result<String> {
    if value.is_empty() || value.len() > super::event::MAX_IDENTIFIER_CHARS {
        bail!("diagnostic filter value has an unusable length");
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/' | b'@' | b'+')
    }) {
        bail!("diagnostic filter value `{value}` is not an identity");
    }
    Ok(format!("\"{value}\""))
}

fn quote_time(value: &str) -> Result<String> {
    if !value.bytes().all(|byte| {
        byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b':' | b'.' | b'T' | b'Z')
    }) {
        bail!("diagnostic query window is not a timestamp");
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::event::{Correlation, Severity};

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-08-16T12:00:00Z")
            .unwrap()
            .to_utc()
    }

    #[test]
    fn compiles_labels_identities_and_a_bounded_window() {
        let query = DiagnosticQuery {
            components: vec!["visual-host".into(), "containers".into()],
            severities: vec![Severity::Error, Severity::Warn],
            correlation: Correlation {
                visual_id: Some("vis_9".into()),
                ..Default::default()
            },
            since: Duration::from_secs(1_200),
            limit: 100,
            ..Default::default()
        };
        let logsql = compile(&query, now()).unwrap();
        assert!(logsql.contains("_time:[2026-08-16T11:40:00+00:00, 2026-08-16T12:00:00+00:00]"), "{logsql}");
        assert!(logsql.contains("component:in(\"visual-host\",\"containers\")"), "{logsql}");
        assert!(logsql.contains("severity:in(\"error\",\"warn\")"), "{logsql}");
        assert!(logsql.contains("visual_id:=\"vis_9\""), "{logsql}");
        assert!(logsql.ends_with("| fields journal_sequence | sort by (journal_sequence) desc | limit 400"), "{logsql}");
    }

    #[test]
    fn every_compiled_query_returns_identities_only() {
        let logsql = compile(&DiagnosticQuery::default(), now()).unwrap();
        assert!(logsql.contains("| fields journal_sequence"), "{logsql}");
        assert!(logsql.contains("| limit "), "{logsql}");
    }

    #[test]
    fn refuses_identities_that_could_change_the_query() {
        for hostile in [
            "vis\" or component:=\"renderer",
            "vis_1 | delete",
            "vis_1\nseverity:error",
            "vis_1\\",
        ] {
            let query = DiagnosticQuery {
                correlation: Correlation {
                    visual_id: Some(hostile.into()),
                    ..Default::default()
                },
                ..Default::default()
            };
            assert!(compile(&query, now()).is_err(), "accepted `{hostile}`");
        }
    }

    #[test]
    fn parses_sequences_newest_first_without_duplicates() {
        let body = "{\"journal_sequence\":\"7\"}\n{\"journal_sequence\":\"9\"}\n\n{\"journal_sequence\":\"7\"}\n";
        assert_eq!(parse_sequences(body), vec![9, 7]);
    }

    #[test]
    fn ignores_index_rows_that_carry_no_identity() {
        let body = "{\"_msg\":\"noise\"}\nnot json\n{\"journal_sequence\":\"3\"}";
        assert_eq!(parse_sequences(body), vec![3]);
    }
}
