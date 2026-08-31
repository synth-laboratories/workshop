//! SQLite repository for failures. The only writer of failure_* tables.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::definition::{
    FailureDefinition, FailureDisposition, FailureId, FailureKind, PersistenceFailure,
};
use super::lifecycle::{allowed, FailureLifecycleState, TransitionReason};
use super::occurrence::{FailureCause, OperationalFailure};
use super::redaction::redact_value;
use super::relationship::{FailureRelationship, FailureRelationshipKind};
use crate::platform::operations::{OperationContext, OperationId, OperationKind, OperationPhase};

pub struct FailureRepository;

impl FailureRepository {
    pub fn insert(conn: &Connection, failure: &OperationalFailure, actor: &str) -> Result<()> {
        let facts = redact_value(if failure.safe_facts.is_null() {
            failure.kind.safe_facts()
        } else {
            failure.safe_facts.clone()
        });
        let cause_json = failure
            .cause
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?
            .unwrap_or(serde_json::Value::Null);
        conn.execute(
            "INSERT INTO failure_occurrences(
                failure_id, schema_version, code, domain, category, disposition,
                lifecycle_state, operation_kind, operation_phase, operation_id,
                session_id, turn_id, container_id, evaluation_id, rollout_id, visual_id,
                kind_json, facts_json, cause_json, raised_at, updated_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
            params![
                failure.failure_id.as_str(),
                super::definition::FAILURE_SCHEMA_VERSION,
                failure.kind.code(),
                failure.kind.category().as_str(),
                failure.kind.category().as_str(),
                failure.disposition.as_str(),
                failure.lifecycle_state.as_str(),
                failure.operation.as_str(),
                failure.phase.as_str(),
                failure
                    .context
                    .operation_id
                    .as_ref()
                    .map(|id| id.as_str().to_owned()),
                failure.context.session_id,
                failure.context.turn_id,
                failure.context.container_id,
                failure.context.evaluation_id,
                failure.context.rollout_id,
                failure.context.visual_id,
                serde_json::to_string(&failure.kind)?,
                serde_json::to_string(&facts)?,
                cause_json.to_string(),
                failure.raised_at.to_rfc3339(),
                failure.updated_at.to_rfc3339(),
            ],
        )
        .context("insert failure occurrence")?;
        Self::insert_transition(
            conn,
            &failure.failure_id,
            0,
            None,
            failure.lifecycle_state,
            TransitionReason::Raised,
            actor,
            failure.updated_at,
        )?;
        Ok(())
    }

    pub fn insert_transition(
        conn: &Connection,
        failure_id: &FailureId,
        sequence: i64,
        from: Option<FailureLifecycleState>,
        to: FailureLifecycleState,
        reason: TransitionReason,
        actor: &str,
        at: DateTime<Utc>,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO failure_transitions(
                failure_id, sequence, from_state, to_state, reason, actor, at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                failure_id.as_str(),
                sequence,
                from.map(|s| s.as_str()),
                to.as_str(),
                reason.as_str(),
                actor,
                at.to_rfc3339(),
            ],
        )
        .context("insert failure transition")?;
        Ok(())
    }

    pub fn load(conn: &Connection, failure_id: &str) -> Result<Option<OperationalFailure>> {
        let row = conn
            .query_row(
                "SELECT failure_id, kind_json, facts_json, cause_json, disposition, lifecycle_state,
                        operation_kind, operation_phase, operation_id, session_id, turn_id,
                        container_id, evaluation_id, rollout_id, visual_id, raised_at, updated_at
                 FROM failure_occurrences WHERE failure_id = ?1",
                [failure_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, Option<String>>(10)?,
                        row.get::<_, Option<String>>(11)?,
                        row.get::<_, Option<String>>(12)?,
                        row.get::<_, Option<String>>(13)?,
                        row.get::<_, Option<String>>(14)?,
                        row.get::<_, String>(15)?,
                        row.get::<_, String>(16)?,
                    ))
                },
            )
            .optional()?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(hydrate(row)?))
    }

    pub fn transition(
        conn: &Connection,
        failure_id: &str,
        to: FailureLifecycleState,
        reason: TransitionReason,
        actor: &str,
        at: DateTime<Utc>,
    ) -> Result<OperationalFailure> {
        let mut current = Self::load(conn, failure_id)?
            .with_context(|| format!("failure `{failure_id}` not found"))?;
        allowed(current.lifecycle_state, to).map_err(anyhow::Error::msg)?;
        let sequence: i64 = conn.query_row(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM failure_transitions WHERE failure_id = ?1",
            [failure_id],
            |row| row.get(0),
        )?;
        Self::insert_transition(
            conn,
            &current.failure_id,
            sequence,
            Some(current.lifecycle_state),
            to,
            reason,
            actor,
            at,
        )?;
        conn.execute(
            "UPDATE failure_occurrences SET lifecycle_state = ?1, updated_at = ?2 WHERE failure_id = ?3",
            params![to.as_str(), at.to_rfc3339(), failure_id],
        )?;
        current.lifecycle_state = to;
        current.updated_at = at;
        Ok(current)
    }

    pub fn relate(conn: &Connection, rel: &FailureRelationship) -> Result<()> {
        conn.execute(
            "INSERT INTO failure_relationships(from_failure_id, to_failure_id, kind)
             VALUES (?1,?2,?3)",
            params![rel.from.as_str(), rel.to.as_str(), rel.kind.as_str()],
        )?;
        Ok(())
    }

    pub fn open_for_container(
        conn: &Connection,
        container_id: &str,
    ) -> Result<Option<OperationalFailure>> {
        let id: Option<String> = conn
            .query_row(
                "SELECT failure_id FROM failure_occurrences
                 WHERE container_id = ?1 AND lifecycle_state NOT IN ('resolved','terminalized','superseded')
                 ORDER BY raised_at DESC LIMIT 1",
                [container_id],
                |row| row.get(0),
            )
            .optional()?;
        match id {
            Some(id) => Self::load(conn, &id),
            None => Ok(None),
        }
    }
}

/// One-shot copy of legacy error prose into explicit unclassified failures.
/// Never a runtime reader: after this, old columns are unused.
pub fn migrate_historical_failures(conn: &Connection) -> Result<()> {
    let already: i64 = conn.query_row(
        "SELECT COUNT(*) FROM failure_occurrences WHERE code = 'historical_failure_unclassified'",
        [],
        |row| row.get(0),
    )?;
    if already > 0 {
        return Ok(());
    }
    migrate_table(conn, "optimizer_runs", "id", "error_json", "optimizer_runs")?;
    if table_has_column(conn, "reports", "error")? {
        migrate_table(conn, "reports", "id", "error", "reports")?;
    }
    migrate_container_probe_errors(conn)?;
    Ok(())
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
        params![table, column],
        |row| row.get(0),
    )?)
}

fn migrate_table(
    conn: &Connection,
    table: &str,
    id_col: &str,
    error_col: &str,
    source_table: &str,
) -> Result<()> {
    let sql = format!(
        "SELECT {id_col}, {error_col} FROM {table}
         WHERE {error_col} IS NOT NULL AND TRIM({error_col}) NOT IN ('', '[]', '{{}}', 'null')"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for (source_id, payload) in rows {
        let kind = FailureKind::Persistence(PersistenceFailure::HistoricalUnclassified {
            source_table: source_table.into(),
            source_id: source_id.clone(),
        });
        let mut context = OperationContext::default();
        context.ensure_operation_id();
        let mut failure = OperationalFailure::new(
            kind,
            context,
            OperationKind::Bootstrap,
            OperationPhase::Recover,
            Some(FailureCause::Detail(payload)),
            Utc::now(),
        );
        failure.safe_facts = failure.kind.safe_facts();
        FailureRepository::insert(conn, &failure, "historical_migration")?;
        if table == "optimizer_runs" {
            let _ = conn.execute(
                "UPDATE optimizer_runs SET terminal_failure_id = ?1 WHERE id = ?2 AND terminal_failure_id IS NULL",
                params![failure.failure_id.as_str(), source_id],
            );
        }
    }
    Ok(())
}

fn migrate_container_probe_errors(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id, health_json FROM containers
         WHERE json_extract(health_json, '$.error') IS NOT NULL",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for (container_id, health_json) in rows {
        let kind = FailureKind::Persistence(PersistenceFailure::HistoricalUnclassified {
            source_table: "containers".into(),
            source_id: container_id.clone(),
        });
        let context = OperationContext::bootstrap("historical").for_container(container_id.clone());
        let mut failure = OperationalFailure::new(
            kind,
            context,
            OperationKind::Bootstrap,
            OperationPhase::Recover,
            Some(FailureCause::Detail(health_json)),
            Utc::now(),
        );
        failure.safe_facts = failure.kind.safe_facts();
        FailureRepository::insert(conn, &failure, "historical_migration")?;
        let _ = conn.execute(
            "UPDATE containers SET current_failure_id = ?1 WHERE id = ?2 AND current_failure_id IS NULL",
            params![failure.failure_id.as_str(), container_id],
        );
    }
    Ok(())
}

fn hydrate(
    row: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
    ),
) -> Result<OperationalFailure> {
    let (
        failure_id,
        kind_json,
        facts_json,
        cause_json,
        disposition,
        lifecycle_state,
        operation_kind,
        operation_phase,
        operation_id,
        session_id,
        turn_id,
        container_id,
        evaluation_id,
        rollout_id,
        visual_id,
        raised_at,
        updated_at,
    ) = row;
    let kind: FailureKind = serde_json::from_str(&kind_json)?;
    let disposition = match disposition.as_str() {
        "approval_required" => FailureDisposition::ApprovalRequired,
        "repair_required" => FailureDisposition::RepairRequired,
        "retryable" => FailureDisposition::Retryable,
        "terminal" => FailureDisposition::Terminal,
        "cancelled" => FailureDisposition::Cancelled,
        "programmer_error" => FailureDisposition::ProgrammerError,
        other => anyhow::bail!("unknown stored disposition `{other}`"),
    };
    Ok(OperationalFailure {
        failure_id: FailureId(failure_id),
        kind,
        operation: parse_operation_kind(&operation_kind)?,
        phase: parse_operation_phase(&operation_phase)?,
        disposition,
        lifecycle_state: FailureLifecycleState::parse(&lifecycle_state)
            .map_err(anyhow::Error::msg)?,
        context: OperationContext {
            operation_id: operation_id.map(OperationId),
            session_id,
            turn_id,
            container_id,
            evaluation_id,
            rollout_id,
            visual_id,
            ..OperationContext::default()
        },
        safe_facts: serde_json::from_str(&facts_json).unwrap_or(serde_json::Value::Null),
        cause: serde_json::from_str::<FailureCause>(&cause_json).ok(),
        raised_at: DateTime::parse_from_rfc3339(&raised_at)
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(&updated_at)
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    })
}

fn parse_operation_kind(value: &str) -> Result<OperationKind> {
    Ok(match value {
        "container.probe" => OperationKind::ContainerProbe,
        "container.restart" => OperationKind::ContainerRestart,
        "container.register" => OperationKind::ContainerRegister,
        "container.prepare" => OperationKind::ContainerPrepare,
        "evaluation.admit" => OperationKind::EvaluationAdmit,
        "evaluation.execute" => OperationKind::EvaluationExecute,
        "session.turn" => OperationKind::SessionTurn,
        "session.recover" => OperationKind::SessionRecover,
        "visual.render" => OperationKind::VisualRender,
        "runtime.bootstrap" => OperationKind::Bootstrap,
        "observability.query" => OperationKind::Query,
        other => anyhow::bail!("unknown stored operation `{other}`"),
    })
}

fn parse_operation_phase(value: &str) -> Result<OperationPhase> {
    Ok(match value {
        "start" => OperationPhase::Start,
        "probe" => OperationPhase::Probe,
        "admit" => OperationPhase::Admit,
        "approve" => OperationPhase::Approve,
        "execute" => OperationPhase::Execute,
        "settle" => OperationPhase::Settle,
        "recover" => OperationPhase::Recover,
        "shutdown" => OperationPhase::Shutdown,
        other => anyhow::bail!("unknown stored phase `{other}`"),
    })
}

#[allow(dead_code)]
pub fn parse_relationship(value: &str) -> Result<FailureRelationshipKind> {
    Ok(match value {
        "caused_by" => FailureRelationshipKind::CausedBy,
        "consequence_of" => FailureRelationshipKind::ConsequenceOf,
        "supersedes" => FailureRelationshipKind::Supersedes,
        "repair_of" => FailureRelationshipKind::RepairOf,
        "retry_of" => FailureRelationshipKind::RetryOf,
        other => anyhow::bail!("unknown relationship `{other}`"),
    })
}
