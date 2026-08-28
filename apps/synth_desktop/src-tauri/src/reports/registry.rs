use super::models::{
    default_blocks, generated_outline, is_evidence_kind, validate_block, ExperimentRecord,
    ExperimentRecordUpsert, ExperimentStatus, ReportAttachTrace, ReportBlock, ReportClaim,
    ReportCreateRequest, ReportLimitation, ReportQuery, ReportRecord, ReportRevision, ReportSeal,
    ReportSealBundle, ReportSource, ReportStatus, ReportUpdateRequest, ReportValidationFinding,
    ReportValidationResult, ReportVisibilityRequest, ReportVisibilityRequestCreate,
    ResearchLogAppend, ResearchLogEntry, BLOCK_DIAGRAM, BLOCK_EXPERIMENT_RECORDS, BLOCK_OUTLINE,
    BLOCK_PROSE, BLOCK_RESEARCH_LOG, BLOCK_TRACE, BLOCK_VISUAL, REPORT_REVISION_SCHEMA,
    REPORT_SCHEMA_VERSION,
};
use crate::storage::{ContentStore, Database, EventAppend, EventJournal, EventSource};
use crate::visuals::{VisualRegistry, VisualSeal};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

const COMPILER_NAME: &str = "workshop";
const UNRESOLVED_VISUAL_EVIDENCE: &str = "unresolved_visual_evidence";
const EMPTY_REPORT: &str = "empty_report";
const BUNDLE_SCHEMA: &str = super::models::REPORT_BUNDLE_SCHEMA;
const FROZEN_RUNTIME: &str = concat!(
    include_str!("rollout_inspector.js"),
    "\n",
    include_str!("compare_story.js"),
    "\n",
    include_str!("reader.js")
);
const REPORT_READER_CSS: &str = concat!(
    include_str!("reader.css"),
    "\n",
    include_str!("rollout_inspector.css"),
    "\n",
    include_str!("compare_story.css")
);

#[derive(Clone)]
pub struct ReportRegistry {
    pub(crate) db: Arc<Database>,
    pub(crate) content: ContentStore,
    visuals: VisualRegistry,
}

impl ReportRegistry {
    pub fn new(
        db: Arc<Database>,
        _journal: EventJournal,
        content: ContentStore,
        visuals: VisualRegistry,
    ) -> Self {
        Self {
            db,
            content,
            visuals,
        }
    }

    pub async fn list(&self, query: ReportQuery) -> Result<Vec<ReportRecord>> {
        let db = self.db.clone();
        db.run(move |conn| list_reports(conn, &query)).await
    }

    pub async fn get(&self, report_id: String) -> Result<ReportRecord> {
        let db = self.db.clone();
        db.run(move |conn| load_report(conn, &report_id)).await
    }

    pub async fn get_revision(
        &self,
        report_id: String,
        revision: Option<i64>,
    ) -> Result<ReportRevision> {
        let db = self.db.clone();
        db.run(move |conn| {
            let report = load_report(conn, &report_id)?;
            let rev = revision.unwrap_or(report.current_revision);
            load_revision(conn, &report_id, rev)
        })
        .await
    }

    pub async fn validate(
        &self,
        report_id: String,
        revision: Option<i64>,
    ) -> Result<ReportValidationResult> {
        let mut snapshot = self.get_revision(report_id, revision).await?;
        for block in &mut snapshot.blocks {
            if is_visual_or_diagram(&block.kind) {
                resolve_evidence_state(block, &self.visuals).await?;
                normalize_block(block);
            }
        }
        Ok(validate_revision(&snapshot))
    }

    pub async fn pin_all(&self, report_id: String) -> Result<(ReportRecord, Value)> {
        let revision = self.get_revision(report_id.clone(), None).await?;
        if revision.status == ReportStatus::Sealed {
            bail!("sealed report revisions are immutable; create a new draft before pinning");
        }
        let experiments = self.list_experiments(report_id.clone()).await?;
        let log = self.list_research_log(report_id.clone()).await?;
        let mut blocks = freeze_blocks(&revision.blocks, &experiments, &log)?;
        let mut unresolved = Vec::new();
        let mut visual_unresolved: Vec<ReportValidationFinding> = Vec::new();
        for block in &mut blocks {
            if !is_evidence_kind(&block.kind) {
                continue;
            }
            resolve_evidence_state(block, &self.visuals).await?;
            normalize_block(block);
            if is_visual_or_diagram(&block.kind) {
                let (visual_id, target_revision) = visual_block_target(block).unwrap_or_default();
                let seals = if visual_id.is_empty() {
                    Vec::new()
                } else {
                    self.visuals.list_seals(Some(visual_id.clone())).await?
                };
                let admission = admit_visual_evidence(
                    &visual_id,
                    target_revision,
                    None,
                    &seals,
                    Some(block.block_id.clone()),
                );
                if admission.ok {
                    block.reference_mode = "pinned".into();
                } else {
                    visual_unresolved.extend(admission.reasons);
                }
                continue;
            }
            if block.access_state == "available"
                && block.integrity_state == "verified"
                && block.source_revision.is_some()
                && block.source_digest.is_some()
            {
                block.reference_mode = "pinned".into();
            } else if block.access_state != "missing" && block.access_state != "redacted" {
                unresolved.push(block.anchor.clone());
            }
        }
        if let Some(finding) = visual_unresolved.first() {
            bail!("{}: {}", finding.code, finding.message);
        }
        if !unresolved.is_empty() {
            bail!(
                "cannot pin unresolved evidence blocks: {}",
                unresolved.join(", ")
            );
        }
        self.update(
            report_id,
            ReportUpdateRequest {
                expected_revision: Some(revision.revision),
                title: None,
                summary: None,
                authors: None,
                project_ref: None,
                blocks: Some(blocks),
                sources: None,
                claims: None,
                limitations: None,
            },
        )
        .await
    }

    pub async fn create(&self, request: ReportCreateRequest) -> Result<(ReportRecord, Value)> {
        let now = Utc::now().to_rfc3339();
        let id = request
            .id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("rep_{}", Uuid::new_v4().simple()));
        if !id.starts_with("rep_") {
            bail!("report id must start with rep_");
        }
        let mut blocks = request.blocks.unwrap_or_else(default_blocks);
        ensure_appendix_blocks(&mut blocks);
        for block in &blocks {
            validate_block(block)?;
        }
        let record = ReportRecord {
            schema_version: REPORT_SCHEMA_VERSION.into(),
            id: id.clone(),
            project_ref: request.project_ref,
            current_revision: 1,
            title: request
                .title
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "Untitled report".into()),
            summary: request.summary,
            authors: request.authors.unwrap_or_else(|| vec!["user".into()]),
            status: ReportStatus::Draft,
            created_by: request.created_by.unwrap_or_else(|| "user".into()),
            created_at: now.clone(),
            updated_at: now.clone(),
            archived_at: None,
        };
        let revision = ReportRevision {
            schema_version: REPORT_REVISION_SCHEMA.into(),
            report_id: id,
            revision: 1,
            title: record.title.clone(),
            summary: record.summary.clone(),
            authors: record.authors.clone(),
            status: ReportStatus::Draft,
            blocks,
            sources: Vec::new(),
            claims: Vec::new(),
            limitations: Vec::new(),
            content_digest: None,
            compiler_name: None,
            compiler_version: None,
            created_by: record.created_by.clone(),
            created_at: now,
        };
        let db = self.db.clone();
        let stored = record.clone();
        let (stored, event) = db
            .run_transaction(move |conn| {
                insert_report(conn, &stored)?;
                insert_revision(conn, &revision)?;
                let event = crate::storage::append_event(
                    conn,
                    EventAppend {
                        event_id: None,
                        session_id: None,
                        run_id: None,
                        source: EventSource::Report,
                        kind: "report.created".into(),
                        payload: json!({
                            "reportId": stored.id,
                            "revision": stored.current_revision,
                        }),
                        remote_sequence: None,
                        command_id: None,
                        created_at: None,
                    },
                )?;
                Ok((stored, serde_json::to_value(event)?))
            })
            .await?;
        Ok((stored, event))
    }

    pub async fn update(
        &self,
        report_id: String,
        request: ReportUpdateRequest,
    ) -> Result<(ReportRecord, Value)> {
        let current_record = self.get(report_id.clone()).await?;
        if current_record.archived_at.is_some() {
            bail!("archived reports must be restored before editing");
        }
        if let Some(expected) = request.expected_revision {
            if expected != current_record.current_revision {
                bail!(
                    "report revision conflict: expected {expected}, current {}",
                    current_record.current_revision
                );
            }
        }
        let current = self
            .get_revision(report_id.clone(), Some(current_record.current_revision))
            .await?;
        let now = Utc::now().to_rfc3339();
        let start_from = if current.status == ReportStatus::Sealed {
            let mut next = current.clone();
            next.revision += 1;
            next.status = ReportStatus::Draft;
            next.content_digest = None;
            next.compiler_name = None;
            next.compiler_version = None;
            next.created_at = now.clone();
            next
        } else {
            current
        };
        let mut next = start_from;
        if let Some(title) = request.title {
            next.title = title;
        }
        if request.summary.is_some() {
            next.summary = request.summary;
        }
        if let Some(authors) = request.authors {
            next.authors = authors;
        }
        if let Some(blocks) = request.blocks {
            let mut blocks = blocks;
            for block in &mut blocks {
                normalize_block(block);
                validate_block(block)?;
            }
            next.blocks = blocks;
        }
        if let Some(sources) = request.sources {
            let mut sources = sources;
            for source in &mut sources {
                normalize_source(source);
            }
            next.sources = sources;
        }
        if let Some(claims) = request.claims {
            next.claims = claims;
        }
        if let Some(limitations) = request.limitations {
            next.limitations = limitations;
        }
        ensure_appendix_blocks(&mut next.blocks);
        let validation = validate_revision(&next);
        if let Some(finding) = validation
            .findings
            .iter()
            .find(|row| row.severity == "error" && row.code != EMPTY_REPORT)
        {
            bail!("report validation {}: {}", finding.code, finding.message);
        }
        let db = self.db.clone();
        let project_ref = request.project_ref.clone();
        let (stored, event) = db
            .run_transaction(move |conn| {
                if let Ok(status) = revision_status(conn, &report_id, next.revision) {
                    if status == "sealed" {
                        bail!("sealed report revisions are immutable");
                    }
                }
                if load_report(conn, &report_id)?.current_revision != next.revision {
                    insert_revision(conn, &next)?;
                } else {
                    replace_revision(conn, &next)?;
                }
                conn.execute(
                    "UPDATE reports SET title = ?1, summary = ?2, authors_json = ?3, current_revision = ?4,
                            status = 'draft', project_ref = COALESCE(?5, project_ref), updated_at = ?6
                     WHERE id = ?7",
                    params![
                        next.title,
                        next.summary,
                        serde_json::to_string(&next.authors)?,
                        next.revision,
                        project_ref,
                        now,
                        report_id,
                    ],
                )?;
                let stored = load_report(conn, &report_id)?;
                let event = crate::storage::append_event(
                    conn,
                    EventAppend {
                        event_id: None,
                        session_id: None,
                        run_id: None,
                        source: EventSource::Report,
                        kind: "report.updated".into(),
                        payload: json!({
                            "reportId": stored.id,
                            "revision": stored.current_revision,
                        }),
                        remote_sequence: None,
                        command_id: None,
                        created_at: None,
                    },
                )?;
                Ok((stored, serde_json::to_value(event)?))
            })
            .await?;
        Ok((stored, event))
    }

    pub async fn set_archived(&self, report_id: String, archived: bool) -> Result<ReportRecord> {
        let db = self.db.clone();
        let now = Utc::now().to_rfc3339();
        db.run_transaction(move |conn| {
            load_report(conn, &report_id)?;
            conn.execute(
                "UPDATE reports SET archived_at = ?1, updated_at = ?2 WHERE id = ?3",
                params![archived.then_some(now.clone()), now, report_id],
            )?;
            crate::storage::append_event(
                conn,
                EventAppend {
                    event_id: None,
                    session_id: None,
                    run_id: None,
                    source: EventSource::Report,
                    kind: if archived {
                        "report.archived".into()
                    } else {
                        "report.restored".into()
                    },
                    payload: json!({"reportId": report_id}),
                    remote_sequence: None,
                    command_id: None,
                    created_at: None,
                },
            )?;
            load_report(conn, &report_id)
        })
        .await
    }

    pub async fn request_visibility(
        &self,
        report_id: String,
        request: ReportVisibilityRequestCreate,
    ) -> Result<ReportVisibilityRequest> {
        let seal = self.get_seal(request.receipt_digest.clone()).await?.seal;
        let report = self.get(report_id.clone()).await?;
        if seal.report_id != report_id || seal.report_revision != report.current_revision {
            bail!("visibility approval requires the current sealed Report revision");
        }
        if report.archived_at.is_some() {
            bail!("archived reports cannot change visibility");
        }
        if !matches!(
            request.target.as_str(),
            "private" | "public" | "unpublished"
        ) {
            bail!("visibility target must be private, public, or unpublished");
        }
        let slug = request.slug.map(|value| value.trim().to_owned());
        if request.target == "public"
            && !slug
                .as_deref()
                .is_some_and(|value| valid_report_slug(value))
        {
            bail!("public visibility requires a lowercase kebab-case slug");
        }
        let now = Utc::now();
        let record = ReportVisibilityRequest {
            request_id: format!("rvr_{}", Uuid::new_v4().simple()),
            report_id,
            report_revision: seal.report_revision,
            receipt_digest: seal.receipt_digest,
            target: request.target,
            slug,
            reason: request.reason,
            requested_by: request.requested_by.unwrap_or_else(|| "agent".into()),
            status: "pending".into(),
            decision_by: None,
            error: None,
            created_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
            expires_at: (now + Duration::hours(24)).to_rfc3339(),
        };
        let db = self.db.clone();
        let stored = record.clone();
        db.run_transaction(move |conn| {
            conn.execute(
                "INSERT INTO report_visibility_requests(
                    request_id, report_id, report_revision, receipt_digest, target, slug, reason,
                    requested_by, status, decision_by, error, created_at, updated_at, expires_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                params![
                    stored.request_id,
                    stored.report_id,
                    stored.report_revision,
                    stored.receipt_digest,
                    stored.target,
                    stored.slug,
                    stored.reason,
                    stored.requested_by,
                    stored.status,
                    stored.decision_by,
                    stored.error,
                    stored.created_at,
                    stored.updated_at,
                    stored.expires_at,
                ],
            )?;
            Ok(stored)
        })
        .await
    }

    pub async fn list_visibility_requests(
        &self,
        report_id: Option<String>,
    ) -> Result<Vec<ReportVisibilityRequest>> {
        let db = self.db.clone();
        db.run(move |conn| {
            let sql = if report_id.is_some() {
                "SELECT request_id, report_id, report_revision, receipt_digest, target, slug, reason, requested_by, status, decision_by, error, created_at, updated_at, expires_at FROM report_visibility_requests WHERE report_id = ?1 ORDER BY created_at DESC"
            } else {
                "SELECT request_id, report_id, report_revision, receipt_digest, target, slug, reason, requested_by, status, decision_by, error, created_at, updated_at, expires_at FROM report_visibility_requests ORDER BY created_at DESC"
            };
            let mut statement = conn.prepare(sql)?;
            let rows = if let Some(id) = report_id {
                statement
                    .query_map([id], visibility_request_from_row)?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            } else {
                statement
                    .query_map([], visibility_request_from_row)?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            Ok(rows)
        })
        .await
    }

    pub async fn decide_visibility(
        &self,
        request_id: String,
        approved: bool,
        decision_by: String,
    ) -> Result<ReportVisibilityRequest> {
        let db = self.db.clone();
        let now = Utc::now();
        db.run_transaction(move |conn| {
            let current = load_visibility_request(conn, &request_id)?;
            if current.status != "pending" {
                bail!("visibility request is no longer pending");
            }
            if current.expires_at < now.to_rfc3339() {
                conn.execute(
                    "UPDATE report_visibility_requests SET status='expired', updated_at=?1 WHERE request_id=?2",
                    params![now.to_rfc3339(), request_id],
                )?;
                bail!("visibility request expired");
            }
            let report = load_report(conn, &current.report_id)?;
            if report.current_revision != current.report_revision || report.archived_at.is_some() {
                bail!("visibility request no longer matches the current Report revision");
            }
            conn.execute(
                "UPDATE report_visibility_requests SET status=?1, decision_by=?2, updated_at=?3 WHERE request_id=?4",
                params![if approved { "approved" } else { "denied" }, decision_by, now.to_rfc3339(), request_id],
            )?;
            load_visibility_request(conn, &request_id)
        })
        .await
    }

    pub async fn finish_visibility(
        &self,
        request_id: String,
        error: Option<String>,
    ) -> Result<ReportVisibilityRequest> {
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let current = load_visibility_request(conn, &request_id)?;
            if current.status != "approved" {
                bail!("only an approved visibility request can execute");
            }
            conn.execute(
                "UPDATE report_visibility_requests SET status=?1, error=?2, updated_at=?3 WHERE request_id=?4",
                params![if error.is_some() { "failed" } else { "executed" }, error, Utc::now().to_rfc3339(), request_id],
            )?;
            load_visibility_request(conn, &request_id)
        })
        .await
    }

    pub async fn list_experiments(&self, report_id: String) -> Result<Vec<ExperimentRecord>> {
        let db = self.db.clone();
        db.run(move |conn| list_experiments(conn, &report_id)).await
    }

    pub async fn upsert_experiment(
        &self,
        report_id: String,
        request: ExperimentRecordUpsert,
    ) -> Result<ExperimentRecord> {
        let now = Utc::now().to_rfc3339();
        let group_id = request
            .experiment_group_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let record = ExperimentRecord {
            experiment_id: request
                .experiment_id
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| format!("exp_{}", Uuid::new_v4().simple())),
            report_id: Some(report_id.clone()),
            revision: None,
            title: request.title,
            hypothesis: request.hypothesis,
            status: ExperimentStatus::parse(request.status.as_deref().unwrap_or("planned")),
            protocol_digest: request.protocol_digest,
            arms: request.arms.unwrap_or_else(|| json!([])),
            runs: request.runs.unwrap_or_else(|| json!([])),
            results: request.results.unwrap_or_else(|| json!([])),
            evaluator_refs: request.evaluator_refs.unwrap_or_else(|| json!([])),
            trace_collection_refs: request.trace_collection_refs.unwrap_or_else(|| json!([])),
            claim_refs: request.claim_refs.unwrap_or_else(|| json!([])),
            research_log_refs: request.research_log_refs.unwrap_or_else(|| json!([])),
            limitations: request.limitations.unwrap_or_else(|| json!([])),
            created_at: now,
            created_by: request.created_by.unwrap_or_else(|| "user".into()),
            experiment_group_id: group_id,
        };
        let db = self.db.clone();
        let stored = record.clone();
        let provided_status = request.status.clone();
        db.run(move |conn| {
            load_report(conn, &report_id)?;
            let mut stored = stored;
            if let Some(group_id) = stored.experiment_group_id.as_deref() {
                let group = load_experiment_group(conn, group_id)?.ok_or_else(|| {
                    anyhow::anyhow!("unknown experimentGroupId `{group_id}`")
                })?;
                stored.title = group.0;
                if provided_status.is_none() {
                    stored.status = ExperimentStatus::parse(&group.1);
                }
            }
            conn.execute(
                "INSERT INTO experiment_records(
                    experiment_id, report_id, revision, title, hypothesis, status, protocol_digest,
                    arms_json, runs_json, results_json, evaluator_refs_json, trace_collection_refs_json,
                    claim_refs_json, research_log_refs_json, limitations_json, created_at, created_by,
                    experiment_group_id
                 ) VALUES (?1,?2,NULL,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
                 ON CONFLICT(experiment_id) DO UPDATE SET
                    title=excluded.title, hypothesis=excluded.hypothesis, status=excluded.status,
                    protocol_digest=excluded.protocol_digest, arms_json=excluded.arms_json,
                    runs_json=excluded.runs_json, results_json=excluded.results_json,
                    evaluator_refs_json=excluded.evaluator_refs_json,
                    trace_collection_refs_json=excluded.trace_collection_refs_json,
                    claim_refs_json=excluded.claim_refs_json,
                    research_log_refs_json=excluded.research_log_refs_json,
                    limitations_json=excluded.limitations_json,
                    experiment_group_id=excluded.experiment_group_id",
                params![
                    stored.experiment_id,
                    stored.report_id,
                    stored.title,
                    stored.hypothesis,
                    stored.status.as_str(),
                    stored.protocol_digest,
                    serde_json::to_string(&stored.arms)?,
                    serde_json::to_string(&stored.runs)?,
                    serde_json::to_string(&stored.results)?,
                    serde_json::to_string(&stored.evaluator_refs)?,
                    serde_json::to_string(&stored.trace_collection_refs)?,
                    serde_json::to_string(&stored.claim_refs)?,
                    serde_json::to_string(&stored.research_log_refs)?,
                    serde_json::to_string(&stored.limitations)?,
                    stored.created_at,
                    stored.created_by,
                    stored.experiment_group_id,
                ],
            )?;
            Ok(stored)
        })
        .await
    }

    pub async fn list_research_log(&self, report_id: String) -> Result<Vec<ResearchLogEntry>> {
        let db = self.db.clone();
        db.run(move |conn| list_log(conn, &report_id)).await
    }

    pub async fn append_research_log(
        &self,
        report_id: String,
        request: ResearchLogAppend,
    ) -> Result<ResearchLogEntry> {
        validate_log_kind(&request.entry_kind)?;
        if request.body.trim().is_empty() {
            bail!("research log entry requires a body");
        }
        let actor_kind = request.actor_kind.unwrap_or_else(|| "human".into());
        if actor_kind != "human" && actor_kind != "agent" {
            bail!("research log actorKind must be human or agent");
        }
        let now = Utc::now().to_rfc3339();
        let db = self.db.clone();
        db.run(move |conn| {
            load_report(conn, &report_id)?;
            if let Some(parent) = request.supersedes_entry_id.as_deref() {
                let exists: Option<String> = conn
                    .query_row(
                        "SELECT entry_id FROM research_log_entries WHERE entry_id = ?1 AND report_id = ?2",
                        params![parent, report_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if exists.is_none() {
                    bail!("research log supersession target is missing");
                }
            }
            let sequence: i64 = conn.query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM research_log_entries WHERE report_id = ?1",
                [&report_id],
                |row| row.get(0),
            )?;
            let entry = ResearchLogEntry {
                entry_id: format!("log_{}", Uuid::new_v4().simple()),
                report_id: Some(report_id.clone()),
                sequence,
                occurred_at: request.occurred_at.unwrap_or_else(|| now.clone()),
                recorded_at: now,
                author: request.author.unwrap_or_else(|| "user".into()),
                actor_kind,
                entry_kind: request.entry_kind,
                title: request.title,
                body: request.body,
                tags: request.tags.unwrap_or_default(),
                links: request.links.unwrap_or_else(|| json!([])),
                claim_effect: request.claim_effect,
                supersedes_entry_id: request.supersedes_entry_id,
                source_digest: None,
            };
            conn.execute(
                "INSERT INTO research_log_entries(
                    entry_id, report_id, sequence, occurred_at, recorded_at, author, actor_kind,
                    entry_kind, title, body, tags_json, links_json, claim_effect, supersedes_entry_id, source_digest
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,NULL)",
                params![
                    entry.entry_id,
                    entry.report_id,
                    entry.sequence,
                    entry.occurred_at,
                    entry.recorded_at,
                    entry.author,
                    entry.actor_kind,
                    entry.entry_kind,
                    entry.title,
                    entry.body,
                    serde_json::to_string(&entry.tags)?,
                    serde_json::to_string(&entry.links)?,
                    entry.claim_effect,
                    entry.supersedes_entry_id,
                ],
            )?;
            Ok(entry)
        })
        .await
    }

    pub async fn attach_trace(
        &self,
        report_id: String,
        request: ReportAttachTrace,
        projection_verified: bool,
    ) -> Result<(ReportRecord, Value)> {
        const PROJECTION_SCHEMA: &str = "synth.trace-projection.rollout-inspector.v1";
        if request.trace_digest.trim().is_empty() {
            bail!("trace digest is required");
        }
        if let Some(projection) = &request.projection {
            let schema = projection
                .get("schema_version")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if schema != PROJECTION_SCHEMA {
                bail!("trace projection must be {PROJECTION_SCHEMA}");
            }
        }
        let revision = self.get_revision(report_id.clone(), None).await?;
        let mut blocks = revision.blocks.clone();
        if !blocks.iter().any(|block| block.kind == BLOCK_TRACE) {
            blocks.push(ReportBlock {
                block_id: "blk_traces".into(),
                kind: BLOCK_TRACE.into(),
                anchor: "traces".into(),
                title: Some("Trace evidence".into()),
                payload: json!({
                    "projectionKind": "rollout-inspector",
                    "traces": []
                }),
                source_revision: None,
                source_digest: None,
                reference_mode: "live".into(),
                access_state: "missing".into(),
                integrity_state: "unresolved".into(),
            });
        }
        for block in &mut blocks {
            if block.kind != BLOCK_TRACE {
                continue;
            }
            let mut payload = if block.payload.is_object() {
                block.payload.clone()
            } else {
                json!({})
            };
            if let Some(collection_id) = &request.collection_id {
                payload["collectionId"] = json!(collection_id);
            }
            payload["projectionKind"] = json!("rollout-inspector");
            let mut traces = payload
                .get("traces")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let entry = json!({
                "traceDigest": request.trace_digest,
                "traceId": request.trace_id,
                "label": request.label,
                "projection": request.projection,
            });
            if let Some(existing) = traces.iter_mut().find(|row| {
                row.get("traceDigest")
                    .or_else(|| row.get("trace_digest"))
                    .and_then(Value::as_str)
                    == Some(request.trace_digest.as_str())
            }) {
                *existing = entry;
            } else {
                traces.push(entry);
            }
            payload["traces"] = Value::Array(traces);
            let has_projection = payload["traces"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .any(|row| row.get("projection").is_some_and(|value| !value.is_null()))
                })
                .unwrap_or(false);
            block.payload = payload;
            block.source_digest = Some(request.trace_digest.clone());
            if has_projection && projection_verified {
                block.access_state = "accessible".into();
                block.integrity_state = "verified".into();
            } else if has_projection {
                block.access_state = "accessible".into();
                block.integrity_state = "unknown".into();
            } else {
                block.access_state = "missing".into();
                block.integrity_state = "unknown".into();
            }
        }
        self.update(
            report_id,
            ReportUpdateRequest {
                expected_revision: Some(revision.revision),
                title: None,
                summary: None,
                authors: None,
                project_ref: None,
                blocks: Some(blocks),
                sources: None,
                claims: None,
                limitations: None,
            },
        )
        .await
    }

    pub async fn seal(&self, report_id: String, revision: i64) -> Result<(ReportSeal, Value)> {
        let mut snapshot = self.get_revision(report_id.clone(), Some(revision)).await?;
        if snapshot.status == ReportStatus::Sealed {
            if let Some(existing) = self.seal_for_revision(&report_id, revision).await? {
                return Ok((existing, json!({})));
            }
        }
        let experiments = self.list_experiments(report_id.clone()).await?;
        let log = self.list_research_log(report_id.clone()).await?;
        snapshot.blocks = freeze_blocks(&snapshot.blocks, &experiments, &log)?;
        ensure_appendix_blocks(&mut snapshot.blocks);
        snapshot.blocks.retain(|block| block.kind != BLOCK_OUTLINE);
        snapshot
            .blocks
            .insert(0, generated_outline(&snapshot.blocks));
        for block in &mut snapshot.blocks {
            resolve_evidence_state(block, &self.visuals).await?;
        }
        let validation = validate_revision(&snapshot);
        if !validation.sealable {
            let details = validation
                .findings
                .iter()
                .filter(|row| row.severity == "error")
                .map(|row| format!("{}: {}", row.code, row.message))
                .collect::<Vec<_>>()
                .join("; ");
            bail!("report validation failed: {details}");
        }
        scan_forbidden(&serde_json::to_value(&snapshot.blocks)?)?;
        let missing_limitations = missing_limitations(&snapshot.blocks);
        for body in missing_limitations {
            if !snapshot.limitations.iter().any(|item| item.body == body) {
                snapshot.limitations.push(ReportLimitation {
                    limitation_id: format!("lim_{}", Uuid::new_v4().simple()),
                    body,
                });
            }
        }
        snapshot.schema_version = REPORT_REVISION_SCHEMA.into();
        snapshot.status = ReportStatus::Sealed;
        snapshot.compiler_name = Some(COMPILER_NAME.into());
        snapshot.compiler_version = Some(env!("CARGO_PKG_VERSION").into());
        let canonical = canonical_revision(&snapshot)?;
        let digest = hex_sha256(&canonical_json(&canonical)?);
        snapshot.content_digest = Some(digest.clone());
        let data = json!({
            "schema_version": BUNDLE_SCHEMA,
            "revision": canonical_revision(&snapshot)?,
            "outline": generated_outline(&snapshot.blocks).payload["items"],
            "experiments": experiments,
            "research_log": log,
            "compiler": {
                "name": COMPILER_NAME,
                "version": env!("CARGO_PKG_VERSION"),
            },
            "validation": validation,
        });
        scan_forbidden(&data)?;
        let runtime_digest = hex_sha256(FROZEN_RUNTIME.as_bytes());
        let compile = || -> Result<(Vec<u8>, String)> {
            let data_bytes = canonical_json(&data)?;
            let index_html = build_index_html(&data, &runtime_digest)?;
            refuse_network_html(&index_html)?;
            Ok((data_bytes, index_html))
        };
        let (data_bytes, index_html) = compile()?;
        let (repeat_data_bytes, repeat_index_html) = compile()?;
        if data_bytes != repeat_data_bytes || index_html != repeat_index_html {
            bail!("nondeterministic report compilation produced different bundle members");
        }
        let index_bytes = index_html.as_bytes();
        let data_digest = hex_sha256(&data_bytes);
        let index_digest = hex_sha256(index_bytes);
        let validation_digest = hex_sha256(&canonical_json(&json!(validation))?);
        let receipt = json!({
            "schema_version": BUNDLE_SCHEMA,
            "report_id": snapshot.report_id,
            "revision": snapshot.revision,
            "content_digest": digest,
            "validation_digest": validation_digest,
            "compiler": {
                "name": COMPILER_NAME,
                "version": env!("CARGO_PKG_VERSION"),
                "runtime_digest": runtime_digest,
            },
            "members": [
                {"logical_path":"data.json","digest_sha256":data_digest,"size_bytes":data_bytes.len(),"media_type":"application/vnd.synth.report-bundle-data+json"},
                {"logical_path":"index.html","digest_sha256":index_digest,"size_bytes":index_bytes.len(),"media_type":"text/html; charset=utf-8"}
            ]
        });
        let receipt_bytes = canonical_json(&receipt)?;
        let receipt_digest = hex_sha256(&receipt_bytes);
        let stored_index = self.content.put_bytes("report_bundles", index_bytes)?;
        let stored_data = self.content.put_bytes("report_bundles", &data_bytes)?;
        let stored_receipt = self.content.put_bytes("report_bundles", &receipt_bytes)?;
        if stored_index != index_digest
            || stored_data != data_digest
            || stored_receipt != receipt_digest
        {
            bail!("local report CAS digest verification failed");
        }
        let now = Utc::now().to_rfc3339();
        let seal = ReportSeal {
            receipt_digest: receipt_digest.clone(),
            report_id: report_id.clone(),
            report_revision: revision,
            schema_version: BUNDLE_SCHEMA.into(),
            compiler_name: COMPILER_NAME.into(),
            compiler_version: env!("CARGO_PKG_VERSION").into(),
            runtime_digest,
            index_digest,
            data_digest,
            receipt_size_bytes: receipt_bytes.len() as i64,
            total_size_bytes: (index_bytes.len() + data_bytes.len() + receipt_bytes.len()) as i64,
            created_at: now.clone(),
        };
        let db = self.db.clone();
        let stored_seal = seal.clone();
        let frozen = snapshot.clone();
        let (stored_seal, event) = db
            .run_transaction(move |conn| {
                replace_revision(conn, &frozen)?;
                conn.execute(
                    "UPDATE reports SET status = 'sealed', current_revision = ?1, title = ?2, summary = ?3, updated_at = ?4 WHERE id = ?5",
                    params![frozen.revision, frozen.title, frozen.summary, now, report_id],
                )?;
                conn.execute(
                    "INSERT OR IGNORE INTO report_seals(
                        receipt_digest, report_id, report_revision, schema_version, compiler_name,
                        compiler_version, runtime_digest, index_digest, data_digest, receipt_size_bytes,
                        total_size_bytes, created_at
                     ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    params![
                        stored_seal.receipt_digest,
                        stored_seal.report_id,
                        stored_seal.report_revision,
                        stored_seal.schema_version,
                        stored_seal.compiler_name,
                        stored_seal.compiler_version,
                        stored_seal.runtime_digest,
                        stored_seal.index_digest,
                        stored_seal.data_digest,
                        stored_seal.receipt_size_bytes,
                        stored_seal.total_size_bytes,
                        stored_seal.created_at,
                    ],
                )?;
                let event = crate::storage::append_event(
                    conn,
                    EventAppend {
                        event_id: None,
                        session_id: None,
                        run_id: None,
                        source: EventSource::Report,
                        kind: "report.sealed".into(),
                        payload: json!({
                            "reportId": stored_seal.report_id,
                            "revision": stored_seal.report_revision,
                            "receiptDigest": stored_seal.receipt_digest,
                        }),
                        remote_sequence: None,
                        command_id: None,
                        created_at: None,
                    },
                )?;
                Ok((stored_seal, serde_json::to_value(event)?))
            })
            .await?;
        Ok((stored_seal, event))
    }

    pub async fn list_seals(&self, report_id: Option<String>) -> Result<Vec<ReportSeal>> {
        let db = self.db.clone();
        db.run(move |conn| {
            let sql = if report_id.is_some() {
                "SELECT receipt_digest, report_id, report_revision, schema_version, compiler_name, compiler_version, runtime_digest, index_digest, data_digest, receipt_size_bytes, total_size_bytes, created_at FROM report_seals WHERE report_id = ?1 ORDER BY report_revision DESC, created_at DESC"
            } else {
                "SELECT receipt_digest, report_id, report_revision, schema_version, compiler_name, compiler_version, runtime_digest, index_digest, data_digest, receipt_size_bytes, total_size_bytes, created_at FROM report_seals ORDER BY created_at DESC"
            };
            let mut statement = conn.prepare(sql)?;
            let rows = if let Some(id) = report_id {
                statement
                    .query_map([id], seal_from_row)?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            } else {
                statement
                    .query_map([], seal_from_row)?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            Ok(rows)
        })
        .await
    }

    pub async fn get_seal(&self, receipt_digest: String) -> Result<ReportSealBundle> {
        let db = self.db.clone();
        let lookup = receipt_digest.clone();
        let seal = db
            .run(move |conn| {
                conn.query_row(
                    "SELECT receipt_digest, report_id, report_revision, schema_version, compiler_name, compiler_version, runtime_digest, index_digest, data_digest, receipt_size_bytes, total_size_bytes, created_at FROM report_seals WHERE receipt_digest = ?1",
                    [lookup],
                    seal_from_row,
                )
                .optional()?
                .ok_or_else(|| anyhow!("report seal does not exist"))
            })
            .await?;
        let index_html = String::from_utf8(
            self.content
                .get_bytes("report_bundles", &seal.index_digest)?,
        )
        .context("sealed report index.html must be UTF-8")?;
        let data: Value = serde_json::from_slice(
            &self
                .content
                .get_bytes("report_bundles", &seal.data_digest)?,
        )?;
        let receipt: Value = serde_json::from_slice(
            &self
                .content
                .get_bytes("report_bundles", &seal.receipt_digest)?,
        )?;
        Ok(ReportSealBundle {
            seal,
            index_html,
            data,
            receipt,
        })
    }

    pub async fn compare_seals(
        &self,
        left_digest: String,
        right_digest: String,
    ) -> Result<super::models::ReportRevisionCompare> {
        let left = self.get_seal(left_digest).await?;
        let right = self.get_seal(right_digest).await?;
        Ok(super::models::ReportRevisionCompare {
            same_digest: left.seal.receipt_digest == right.seal.receipt_digest,
            left,
            right,
        })
    }

    async fn seal_for_revision(
        &self,
        report_id: &str,
        revision: i64,
    ) -> Result<Option<ReportSeal>> {
        let db = self.db.clone();
        let id = report_id.to_string();
        db.run(move |conn| {
            conn.query_row(
                "SELECT receipt_digest, report_id, report_revision, schema_version, compiler_name, compiler_version, runtime_digest, index_digest, data_digest, receipt_size_bytes, total_size_bytes, created_at FROM report_seals WHERE report_id = ?1 AND report_revision = ?2 ORDER BY created_at DESC LIMIT 1",
                params![id, revision],
                seal_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
        .await
    }
}

fn ensure_appendix_blocks(blocks: &mut Vec<ReportBlock>) {
    if !blocks
        .iter()
        .any(|block| block.kind == BLOCK_EXPERIMENT_RECORDS)
    {
        blocks.push(
            default_blocks()
                .into_iter()
                .find(|block| block.kind == BLOCK_EXPERIMENT_RECORDS)
                .expect("default experiment records block"),
        );
    }
    if !blocks.iter().any(|block| block.kind == BLOCK_RESEARCH_LOG) {
        blocks.push(
            default_blocks()
                .into_iter()
                .find(|block| block.kind == BLOCK_RESEARCH_LOG)
                .expect("default research log block"),
        );
    }
}

fn freeze_blocks(
    blocks: &[ReportBlock],
    experiments: &[ExperimentRecord],
    log: &[ResearchLogEntry],
) -> Result<Vec<ReportBlock>> {
    let mut frozen = Vec::new();
    for mut block in blocks.iter().cloned() {
        if block.kind == BLOCK_EXPERIMENT_RECORDS {
            let ids = experiment_ids(&block);
            let selected = experiments
                .iter()
                .filter(|row| ids.is_empty() || ids.iter().any(|id| id == &row.experiment_id))
                .cloned()
                .collect::<Vec<_>>();
            block.payload = json!({
                "experimentIds": selected.iter().map(|row| row.experiment_id.clone()).collect::<Vec<_>>(),
                "records": selected,
            });
            block.source_revision = Some("frozen".into());
            block.source_digest = Some(hex_sha256(&canonical_json(&block.payload)?));
            block.integrity_state = "verified".into();
        }
        if block.kind == BLOCK_RESEARCH_LOG {
            let ids = log_ids(&block);
            let selected = log
                .iter()
                .filter(|row| ids.is_empty() || ids.iter().any(|id| id == &row.entry_id))
                .cloned()
                .collect::<Vec<_>>();
            block.payload = json!({
                "entryIds": selected.iter().map(|row| row.entry_id.clone()).collect::<Vec<_>>(),
                "entries": selected,
            });
            block.source_revision = Some("frozen".into());
            block.source_digest = Some(hex_sha256(&canonical_json(&block.payload)?));
            block.integrity_state = "verified".into();
        }
        frozen.push(block);
    }
    Ok(frozen)
}

fn experiment_ids(block: &ReportBlock) -> Vec<String> {
    block
        .payload
        .get("experimentIds")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn log_ids(block: &ReportBlock) -> Vec<String> {
    block
        .payload
        .get("entryIds")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn is_visual_or_diagram(kind: &str) -> bool {
    matches!(kind, BLOCK_VISUAL | BLOCK_DIAGRAM)
}

fn visual_block_target(block: &ReportBlock) -> Option<(String, i64)> {
    let visual_id = block
        .payload
        .get("visualId")
        .or_else(|| block.payload.get("visual_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?
        .to_string();
    let revision = block
        .payload
        .get("visualRevision")
        .or_else(|| block.payload.get("visual_revision"))
        .and_then(Value::as_i64)
        .or_else(|| {
            block
                .source_revision
                .as_deref()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(0);
    Some((visual_id, revision))
}

struct EvidenceAdmission {
    ok: bool,
    reasons: Vec<ReportValidationFinding>,
}

fn admit_visual_evidence(
    visual_id: &str,
    revision: i64,
    content_digest: Option<String>,
    seals: &[VisualSeal],
    block_id: Option<String>,
) -> EvidenceAdmission {
    let _ = content_digest;
    if visual_id.is_empty() {
        return EvidenceAdmission {
            ok: false,
            reasons: vec![unresolved_visual_finding(
                None,
                block_id,
                "visual/diagram block is missing visualId".into(),
                None,
            )],
        };
    }
    let receipt_digest = seals
        .iter()
        .find(|seal| seal.visual_id == visual_id && seal.visual_revision == revision)
        .map(|seal| seal.receipt_digest.clone());
    if receipt_digest.is_some() {
        return EvidenceAdmission {
            ok: true,
            reasons: vec![],
        };
    }
    EvidenceAdmission {
        ok: false,
        reasons: vec![unresolved_visual_finding(
            Some(visual_id.to_string()),
            block_id,
            format!(
                "visual {visual_id} rev {revision} has no seal receipt; pin and seal require a VisualSeal"
            ),
            None,
        )],
    }
}

fn unresolved_visual_finding(
    visual_id: Option<String>,
    block_id: Option<String>,
    message: String,
    receipt_digest: Option<String>,
) -> ReportValidationFinding {
    ReportValidationFinding {
        code: UNRESOLVED_VISUAL_EVIDENCE.into(),
        severity: "error".into(),
        block_id,
        claim_id: None,
        message,
        remediation: Some("Seal that exact visual revision, then pin this evidence.".into()),
        visual_id,
        receipt_digest,
    }
}

async fn resolve_evidence_state(block: &mut ReportBlock, visuals: &VisualRegistry) -> Result<()> {
    if block.access_state == "missing" {
        block.integrity_state = "unknown".into();
        return Ok(());
    }
    match block.kind.as_str() {
        "report.visual.v1" | "report.diagram.v1" => {
            let visual_id = block
                .payload
                .get("visualId")
                .or_else(|| block.payload.get("visual_id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let revision = block
                .payload
                .get("visualRevision")
                .or_else(|| block.payload.get("visual_revision"))
                .and_then(Value::as_i64);
            let Some(visual_id) = visual_id else {
                block.access_state = "missing".into();
                block.integrity_state = "unknown".into();
                return Ok(());
            };
            match visuals.get(visual_id.clone()).await {
                Ok(visual) => {
                    let target = revision.unwrap_or(visual.current_revision);
                    let seals = visuals.list_seals(Some(visual_id.clone())).await?;
                    if let Some(seal) = seals.into_iter().find(|row| row.visual_revision == target)
                    {
                        let bundle = visuals.get_seal(seal.receipt_digest.clone()).await?;
                        block.source_revision = Some(target.to_string());
                        block.source_digest = Some(seal.receipt_digest);
                        block.payload["sealedHtml"] = Value::String(bundle.index_html);
                        block.payload["sealedMediaType"] =
                            Value::String("text/html; charset=utf-8".into());
                        block.integrity_state = "verified".into();
                    } else {
                        block.source_revision = Some(target.to_string());
                        block.integrity_state = "unknown".into();
                    }
                }
                Err(_) => {
                    block.access_state = "missing".into();
                    block.integrity_state = "unknown".into();
                }
            }
        }
        kind if is_evidence_kind(kind) => {
            if block.source_revision.is_none() && block.source_digest.is_none() {
                block.access_state = "missing".into();
                block.integrity_state = "unknown".into();
            }
        }
        _ => {}
    }
    Ok(())
}

fn missing_limitations(blocks: &[ReportBlock]) -> Vec<String> {
    blocks
        .iter()
        .filter(|block| block.access_state == "missing" || block.integrity_state != "verified")
        .map(|block| {
            format!(
                "Block {} ({}) is {} / {}",
                block.anchor, block.kind, block.access_state, block.integrity_state
            )
        })
        .collect()
}

fn normalize_block(block: &mut ReportBlock) {
    block.access_state = canonical_access_state(&block.access_state);
    block.integrity_state = canonical_integrity_state(&block.integrity_state);
    if block.reference_mode.is_empty() {
        block.reference_mode = if block.source_revision.is_some() && block.source_digest.is_some() {
            "pinned".into()
        } else {
            "live".into()
        };
    }
}

fn normalize_source(source: &mut ReportSource) {
    source.access_state = canonical_access_state(&source.access_state);
    source.integrity_state = canonical_integrity_state(&source.integrity_state);
    if source.reference_mode.is_empty() {
        source.reference_mode =
            if source.resource_revision.is_some() && source.resource_digest.is_some() {
                "pinned".into()
            } else {
                "live".into()
            };
    }
}

fn block_markdown(block: &ReportBlock) -> &str {
    block
        .payload
        .get("markdown")
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn revision_has_substantive_content(revision: &ReportRevision) -> bool {
    if revision
        .summary
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        return true;
    }
    revision.blocks.iter().any(|block| {
        if matches!(
            block.kind.as_str(),
            BLOCK_OUTLINE | BLOCK_EXPERIMENT_RECORDS | BLOCK_RESEARCH_LOG
        ) {
            return false;
        }
        if block.kind == BLOCK_PROSE {
            return !block_markdown(block).trim().is_empty();
        }
        is_evidence_kind(&block.kind)
    })
}

fn validation_finding(
    code: &str,
    block_id: Option<String>,
    claim_id: Option<String>,
    message: String,
    remediation: &str,
) -> ReportValidationFinding {
    ReportValidationFinding {
        code: code.into(),
        severity: "error".into(),
        block_id,
        claim_id,
        message,
        remediation: Some(remediation.into()),
        visual_id: None,
        receipt_digest: None,
    }
}

fn validate_revision(revision: &ReportRevision) -> ReportValidationResult {
    let mut findings = Vec::new();
    let mut block_ids = HashSet::new();
    let mut anchors = HashSet::new();
    let mut source_ids = HashSet::new();
    let mut claim_ids = HashSet::new();

    for block in &revision.blocks {
        if !block_ids.insert(block.block_id.clone()) {
            findings.push(validation_finding(
                "duplicate_block_id",
                Some(block.block_id.clone()),
                None,
                format!("block id {} is duplicated", block.block_id),
                "Assign every block a stable unique id.",
            ));
        }
        if !anchors.insert(block.anchor.clone()) {
            findings.push(validation_finding(
                "duplicate_block_anchor",
                Some(block.block_id.clone()),
                None,
                format!("anchor {} is duplicated", block.anchor),
                "Assign every block a stable unique anchor.",
            ));
        }
        if block.reference_mode == "pinned"
            && (block.source_revision.is_none() || block.source_digest.is_none())
        {
            findings.push(validation_finding(
                "incomplete_pinned_block",
                Some(block.block_id.clone()),
                None,
                "pinned evidence requires both source revision and digest".into(),
                "Resolve and pin the exact source revision and digest.",
            ));
        }
        if matches!(
            block.integrity_state.as_str(),
            "digest_mismatch" | "unsupported"
        ) {
            findings.push(validation_finding(
                "unsafe_block_integrity",
                Some(block.block_id.clone()),
                None,
                format!("block integrity is {}", block.integrity_state),
                "Replace or re-resolve the evidence before sealing.",
            ));
        } else if is_visual_or_diagram(&block.kind) {
            let integrity = block.integrity_state.as_str();
            let missing_digest = block.source_digest.as_deref().unwrap_or("").is_empty();
            if matches!(integrity, "unresolved" | "unknown") || missing_digest {
                let (visual_id, revision) = visual_block_target(block).unwrap_or_default();
                findings.push(unresolved_visual_finding(
                    (!visual_id.is_empty()).then_some(visual_id.clone()),
                    Some(block.block_id.clone()),
                    if visual_id.is_empty() {
                        "visual/diagram block is missing visualId".into()
                    } else {
                        format!(
                            "visual {visual_id} rev {revision} has no seal receipt; pin and seal require a VisualSeal"
                        )
                    },
                    None,
                ));
            }
        }
    }
    for source in &revision.sources {
        if !source_ids.insert(source.source_id.clone()) {
            findings.push(validation_finding(
                "duplicate_source_id",
                None,
                None,
                format!("source id {} is duplicated", source.source_id),
                "Assign every source a stable unique id.",
            ));
        }
        if source.reference_mode == "pinned"
            && (source.resource_revision.is_none() || source.resource_digest.is_none())
        {
            findings.push(validation_finding(
                "incomplete_pinned_source",
                None,
                None,
                format!(
                    "pinned source {} lacks a revision or digest",
                    source.source_id
                ),
                "Resolve and pin the exact source revision and digest.",
            ));
        }
    }
    let evidence_ids = block_ids
        .union(&source_ids)
        .cloned()
        .collect::<HashSet<_>>();
    for claim in &revision.claims {
        if !claim_ids.insert(claim.claim_id.clone()) {
            findings.push(validation_finding(
                "duplicate_claim_id",
                None,
                Some(claim.claim_id.clone()),
                format!("claim id {} is duplicated", claim.claim_id),
                "Assign every claim a stable unique id.",
            ));
        }
        if !matches!(
            claim.status.as_str(),
            "true" | "false" | "needs_more_analysis" | "unresolved"
        ) {
            findings.push(validation_finding(
                "invalid_claim_status",
                None,
                Some(claim.claim_id.clone()),
                format!("claim status {} is not supported", claim.status),
                "Use true, false, needs_more_analysis, or unresolved.",
            ));
        }
        if !matches!(
            claim.confidence.as_str(),
            "low" | "medium" | "high" | "overwhelming"
        ) {
            findings.push(validation_finding(
                "invalid_claim_confidence",
                None,
                Some(claim.claim_id.clone()),
                format!("claim confidence {} is not supported", claim.confidence),
                "Use low, medium, high, or overwhelming.",
            ));
        }
        if claim.why.trim().is_empty() {
            findings.push(validation_finding(
                "missing_claim_why",
                None,
                Some(claim.claim_id.clone()),
                "claim rationale is empty".into(),
                "Explain why the evidence supports the verdict.",
            ));
        }
        for evidence_ref in &claim.evidence_refs {
            if !evidence_ids.contains(evidence_ref) {
                findings.push(validation_finding(
                    "dangling_claim_evidence",
                    None,
                    Some(claim.claim_id.clone()),
                    format!("claim references missing evidence {}", evidence_ref),
                    "Attach the evidence or remove the reference.",
                ));
            }
        }
    }
    if !revision_has_substantive_content(revision) {
        findings.push(validation_finding(
            EMPTY_REPORT,
            None,
            None,
            "Add a narrative or sealed evidence before sealing".into(),
            "Write findings or methods, or pin sealed visual evidence.",
        ));
    }
    ReportValidationResult {
        report_id: revision.report_id.clone(),
        revision: revision.revision,
        sealable: !findings.iter().any(|row| row.severity == "error"),
        findings,
    }
}

fn canonical_revision(revision: &ReportRevision) -> Result<Value> {
    Ok(json!({
        "schema_version": REPORT_REVISION_SCHEMA,
        "report_id": revision.report_id,
        "revision": revision.revision,
        "title": revision.title,
        "summary": revision.summary,
        "authors": revision.authors,
        "blocks": revision.blocks,
        "sources": revision.sources,
        "claims": revision.claims,
        "limitations": revision.limitations,
        "content_digest": revision.content_digest,
        "compiler_name": revision.compiler_name,
        "compiler_version": revision.compiler_version,
        "created_by": revision.created_by,
    }))
}

fn list_reports(conn: &Connection, query: &ReportQuery) -> Result<Vec<ReportRecord>> {
    let mut statement = conn.prepare(
        "SELECT id, project_ref, current_revision, title, summary, authors_json, status, created_by, created_at, updated_at, archived_at FROM reports ORDER BY updated_at DESC",
    )?;
    let rows = statement.query_map([], report_from_row)?;
    let mut reports = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    if !query.include_archived {
        reports.retain(|row| row.archived_at.is_none());
    }
    if let Some(status) = query.status.as_deref() {
        if status == "archived" {
            reports.retain(|row| row.archived_at.is_some());
        } else {
            reports.retain(|row| row.status.as_str() == status);
        }
    }
    if let Some(search) = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let needle = search.to_ascii_lowercase();
        reports.retain(|row| {
            row.title.to_ascii_lowercase().contains(&needle)
                || row
                    .summary
                    .as_deref()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains(&needle)
        });
    }
    if let Some(limit) = query.limit {
        reports.truncate(limit.max(0) as usize);
    }
    Ok(reports)
}

fn load_report(conn: &Connection, report_id: &str) -> Result<ReportRecord> {
    conn.query_row(
        "SELECT id, project_ref, current_revision, title, summary, authors_json, status, created_by, created_at, updated_at, archived_at FROM reports WHERE id = ?1",
        [report_id],
        report_from_row,
    )
    .optional()?
    .ok_or_else(|| anyhow!("report does not exist"))
}

fn load_revision(conn: &Connection, report_id: &str, revision: i64) -> Result<ReportRevision> {
    let mut revision_row = conn
        .query_row(
            "SELECT report_id, revision, title, summary, authors_json, status, content_digest, compiler_name, compiler_version, created_by, created_at FROM report_revisions WHERE report_id = ?1 AND revision = ?2",
            params![report_id, revision],
            |row| {
                Ok(ReportRevision {
                    schema_version: REPORT_REVISION_SCHEMA.into(),
                    report_id: row.get(0)?,
                    revision: row.get(1)?,
                    title: row.get(2)?,
                    summary: row.get(3)?,
                    authors: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                    status: ReportStatus::parse(&row.get::<_, String>(5)?),
                    blocks: Vec::new(),
                    sources: Vec::new(),
                    claims: Vec::new(),
                    limitations: Vec::new(),
                    content_digest: row.get(6)?,
                    compiler_name: row.get(7)?,
                    compiler_version: row.get(8)?,
                    created_by: row.get(9)?,
                    created_at: row.get(10)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| anyhow!("report revision does not exist"))?;
    revision_row.blocks = list_blocks(conn, report_id, revision)?;
    revision_row.sources = list_sources(conn, report_id, revision)?;
    revision_row.claims = list_claims(conn, report_id, revision)?;
    revision_row.limitations = list_limitations(conn, report_id, revision)?;
    Ok(revision_row)
}

fn revision_status(conn: &Connection, report_id: &str, revision: i64) -> Result<String> {
    conn.query_row(
        "SELECT status FROM report_revisions WHERE report_id = ?1 AND revision = ?2",
        params![report_id, revision],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn insert_report(conn: &Connection, report: &ReportRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO reports(id, project_ref, current_revision, title, summary, authors_json, status, created_by, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            report.id,
            report.project_ref,
            report.current_revision,
            report.title,
            report.summary,
            serde_json::to_string(&report.authors)?,
            report.status.as_str(),
            report.created_by,
            report.created_at,
            report.updated_at,
        ],
    )?;
    Ok(())
}

fn insert_revision(conn: &Connection, revision: &ReportRevision) -> Result<()> {
    conn.execute(
        "INSERT INTO report_revisions(report_id, revision, title, summary, authors_json, status, content_digest, compiler_name, compiler_version, created_by, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            revision.report_id,
            revision.revision,
            revision.title,
            revision.summary,
            serde_json::to_string(&revision.authors)?,
            revision.status.as_str(),
            revision.content_digest,
            revision.compiler_name,
            revision.compiler_version,
            revision.created_by,
            revision.created_at,
        ],
    )?;
    replace_revision_children(conn, revision)?;
    Ok(())
}

fn replace_revision(conn: &Connection, revision: &ReportRevision) -> Result<()> {
    conn.execute(
        "UPDATE report_revisions SET title = ?1, summary = ?2, authors_json = ?3, status = ?4, content_digest = ?5, compiler_name = ?6, compiler_version = ?7, created_by = ?8, created_at = ?9 WHERE report_id = ?10 AND revision = ?11",
        params![
            revision.title,
            revision.summary,
            serde_json::to_string(&revision.authors)?,
            revision.status.as_str(),
            revision.content_digest,
            revision.compiler_name,
            revision.compiler_version,
            revision.created_by,
            revision.created_at,
            revision.report_id,
            revision.revision,
        ],
    )?;
    replace_revision_children(conn, revision)?;
    Ok(())
}

fn replace_revision_children(conn: &Connection, revision: &ReportRevision) -> Result<()> {
    conn.execute(
        "DELETE FROM report_revision_blocks WHERE report_id = ?1 AND revision = ?2",
        params![revision.report_id, revision.revision],
    )?;
    conn.execute(
        "DELETE FROM report_sources WHERE report_id = ?1 AND revision = ?2",
        params![revision.report_id, revision.revision],
    )?;
    conn.execute(
        "DELETE FROM report_claims WHERE report_id = ?1 AND revision = ?2",
        params![revision.report_id, revision.revision],
    )?;
    conn.execute(
        "DELETE FROM report_limitations WHERE report_id = ?1 AND revision = ?2",
        params![revision.report_id, revision.revision],
    )?;
    for (position, block) in revision.blocks.iter().enumerate() {
        conn.execute(
            "INSERT INTO report_revision_blocks(report_id, revision, position, block_id, kind, anchor, title, payload_json, source_revision, source_digest, access_state, integrity_state, reference_mode)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                revision.report_id,
                revision.revision,
                position as i64,
                block.block_id,
                block.kind,
                block.anchor,
                block.title,
                serde_json::to_string(&block.payload)?,
                block.source_revision,
                block.source_digest,
                block.access_state,
                block.integrity_state,
                block.reference_mode,
            ],
        )?;
    }
    for source in &revision.sources {
        conn.execute(
            "INSERT INTO report_sources(report_id, revision, source_id, resource_kind, resource_id, resource_revision, resource_digest, relation, access_state, integrity_state, reference_mode)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                revision.report_id,
                revision.revision,
                source.source_id,
                source.resource_kind,
                source.resource_id,
                source.resource_revision,
                source.resource_digest,
                source.relation,
                source.access_state,
                source.integrity_state,
                source.reference_mode,
            ],
        )?;
    }
    for claim in &revision.claims {
        conn.execute(
            "INSERT INTO report_claims(report_id, revision, claim_id, statement, status, evidence_json, confidence, why)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                revision.report_id,
                revision.revision,
                claim.claim_id,
                claim.statement,
                claim.status,
                serde_json::to_string(&claim.evidence_refs)?,
                claim.confidence,
                claim.why,
            ],
        )?;
    }
    for limitation in &revision.limitations {
        conn.execute(
            "INSERT INTO report_limitations(report_id, revision, limitation_id, body)
             VALUES (?1,?2,?3,?4)",
            params![
                revision.report_id,
                revision.revision,
                limitation.limitation_id,
                limitation.body,
            ],
        )?;
    }
    Ok(())
}

fn list_blocks(conn: &Connection, report_id: &str, revision: i64) -> Result<Vec<ReportBlock>> {
    let mut statement = conn.prepare(
        "SELECT block_id, kind, anchor, title, payload_json, source_revision, source_digest, access_state, integrity_state, reference_mode FROM report_revision_blocks WHERE report_id = ?1 AND revision = ?2 ORDER BY position ASC",
    )?;
    let rows = statement.query_map(params![report_id, revision], |row| {
        Ok(ReportBlock {
            block_id: row.get(0)?,
            kind: row.get(1)?,
            anchor: row.get(2)?,
            title: row.get(3)?,
            payload: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or(json!({})),
            source_revision: row.get(5)?,
            source_digest: row.get(6)?,
            access_state: canonical_access_state(&row.get::<_, String>(7)?),
            integrity_state: canonical_integrity_state(&row.get::<_, String>(8)?),
            reference_mode: row.get(9)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn canonical_access_state(value: &str) -> String {
    match value {
        "accessible" => "available".into(),
        other => other.into(),
    }
}

fn canonical_integrity_state(value: &str) -> String {
    match value {
        "unknown" => "unresolved".into(),
        other => other.into(),
    }
}

fn list_sources(conn: &Connection, report_id: &str, revision: i64) -> Result<Vec<ReportSource>> {
    let mut statement = conn.prepare(
        "SELECT source_id, resource_kind, resource_id, resource_revision, resource_digest, relation, access_state, integrity_state, reference_mode FROM report_sources WHERE report_id = ?1 AND revision = ?2",
    )?;
    let rows = statement.query_map(params![report_id, revision], |row| {
        Ok(ReportSource {
            source_id: row.get(0)?,
            resource_kind: row.get(1)?,
            resource_id: row.get(2)?,
            resource_revision: row.get(3)?,
            resource_digest: row.get(4)?,
            relation: row.get(5)?,
            access_state: canonical_access_state(&row.get::<_, String>(6)?),
            integrity_state: canonical_integrity_state(&row.get::<_, String>(7)?),
            reference_mode: row.get(8)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn list_claims(conn: &Connection, report_id: &str, revision: i64) -> Result<Vec<ReportClaim>> {
    let mut statement = conn.prepare(
        "SELECT claim_id, statement, status, evidence_json, confidence, why FROM report_claims WHERE report_id = ?1 AND revision = ?2",
    )?;
    let rows = statement.query_map(params![report_id, revision], |row| {
        Ok(ReportClaim {
            claim_id: row.get(0)?,
            statement: row.get(1)?,
            status: row.get(2)?,
            evidence_refs: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
            confidence: row.get(4)?,
            why: row.get(5)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn list_limitations(
    conn: &Connection,
    report_id: &str,
    revision: i64,
) -> Result<Vec<ReportLimitation>> {
    let mut statement = conn.prepare(
        "SELECT limitation_id, body FROM report_limitations WHERE report_id = ?1 AND revision = ?2",
    )?;
    let rows = statement.query_map(params![report_id, revision], |row| {
        Ok(ReportLimitation {
            limitation_id: row.get(0)?,
            body: row.get(1)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn list_experiments(conn: &Connection, report_id: &str) -> Result<Vec<ExperimentRecord>> {
    let mut statement = conn.prepare(
        "SELECT r.experiment_id, r.report_id, r.revision, r.title, r.hypothesis, r.status,
                r.protocol_digest, r.arms_json, r.runs_json, r.results_json, r.evaluator_refs_json,
                r.trace_collection_refs_json, r.claim_refs_json, r.research_log_refs_json,
                r.limitations_json, r.created_at, r.created_by, r.experiment_group_id,
                g.title
           FROM experiment_records r
           LEFT JOIN experiment_groups g ON g.id = r.experiment_group_id
          WHERE r.report_id = ?1
          ORDER BY r.created_at ASC",
    )?;
    let rows = statement.query_map([report_id], |row| {
        let group_title: Option<String> = row.get(18)?;
        Ok(ExperimentRecord {
            experiment_id: row.get(0)?,
            report_id: row.get(1)?,
            revision: row.get(2)?,
            title: {
                let stored: String = row.get(3)?;
                group_title.unwrap_or(stored)
            },
            hypothesis: row.get(4)?,
            status: ExperimentStatus::parse(&row.get::<_, String>(5)?),
            protocol_digest: row.get(6)?,
            arms: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or(json!([])),
            runs: serde_json::from_str(&row.get::<_, String>(8)?).unwrap_or(json!([])),
            results: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or(json!([])),
            evaluator_refs: serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or(json!([])),
            trace_collection_refs: serde_json::from_str(&row.get::<_, String>(11)?)
                .unwrap_or(json!([])),
            claim_refs: serde_json::from_str(&row.get::<_, String>(12)?).unwrap_or(json!([])),
            research_log_refs: serde_json::from_str(&row.get::<_, String>(13)?)
                .unwrap_or(json!([])),
            limitations: serde_json::from_str(&row.get::<_, String>(14)?).unwrap_or(json!([])),
            created_at: row.get(15)?,
            created_by: row.get(16)?,
            experiment_group_id: row.get(17)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn load_experiment_group(conn: &Connection, id: &str) -> Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT title, status FROM experiment_groups WHERE id=?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(Into::into)
}

fn list_log(conn: &Connection, report_id: &str) -> Result<Vec<ResearchLogEntry>> {
    let mut statement = conn.prepare(
        "SELECT entry_id, report_id, sequence, occurred_at, recorded_at, author, actor_kind, entry_kind, title, body, tags_json, links_json, claim_effect, supersedes_entry_id, source_digest FROM research_log_entries WHERE report_id = ?1 ORDER BY sequence ASC",
    )?;
    let rows = statement.query_map([report_id], |row| {
        Ok(ResearchLogEntry {
            entry_id: row.get(0)?,
            report_id: row.get(1)?,
            sequence: row.get(2)?,
            occurred_at: row.get(3)?,
            recorded_at: row.get(4)?,
            author: row.get(5)?,
            actor_kind: row.get(6)?,
            entry_kind: row.get(7)?,
            title: row.get(8)?,
            body: row.get(9)?,
            tags: serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or_default(),
            links: serde_json::from_str(&row.get::<_, String>(11)?).unwrap_or(json!([])),
            claim_effect: row.get(12)?,
            supersedes_entry_id: row.get(13)?,
            source_digest: row.get(14)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn report_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReportRecord> {
    Ok(ReportRecord {
        schema_version: REPORT_SCHEMA_VERSION.into(),
        id: row.get(0)?,
        project_ref: row.get(1)?,
        current_revision: row.get(2)?,
        title: row.get(3)?,
        summary: row.get(4)?,
        authors: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
        status: ReportStatus::parse(&row.get::<_, String>(6)?),
        created_by: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        archived_at: row.get(10)?,
    })
}

fn visibility_request_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ReportVisibilityRequest> {
    Ok(ReportVisibilityRequest {
        request_id: row.get(0)?,
        report_id: row.get(1)?,
        report_revision: row.get(2)?,
        receipt_digest: row.get(3)?,
        target: row.get(4)?,
        slug: row.get(5)?,
        reason: row.get(6)?,
        requested_by: row.get(7)?,
        status: row.get(8)?,
        decision_by: row.get(9)?,
        error: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        expires_at: row.get(13)?,
    })
}

fn load_visibility_request(conn: &Connection, request_id: &str) -> Result<ReportVisibilityRequest> {
    conn.query_row(
        "SELECT request_id, report_id, report_revision, receipt_digest, target, slug, reason, requested_by, status, decision_by, error, created_at, updated_at, expires_at FROM report_visibility_requests WHERE request_id = ?1",
        [request_id],
        visibility_request_from_row,
    )
    .optional()?
    .ok_or_else(|| anyhow!("visibility request does not exist"))
}

fn valid_report_slug(value: &str) -> bool {
    value.len() <= 96
        && !value.is_empty()
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.contains("--")
}

fn seal_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReportSeal> {
    Ok(ReportSeal {
        receipt_digest: row.get(0)?,
        report_id: row.get(1)?,
        report_revision: row.get(2)?,
        schema_version: row.get(3)?,
        compiler_name: row.get(4)?,
        compiler_version: row.get(5)?,
        runtime_digest: row.get(6)?,
        index_digest: row.get(7)?,
        data_digest: row.get(8)?,
        receipt_size_bytes: row.get(9)?,
        total_size_bytes: row.get(10)?,
        created_at: row.get(11)?,
    })
}

fn validate_log_kind(kind: &str) -> Result<()> {
    if !matches!(
        kind,
        "hypothesis"
            | "decision"
            | "observation"
            | "anomaly"
            | "protocol_change"
            | "rerun"
            | "interpretation"
            | "limitation"
            | "claim_decision"
            | "correction"
            | "idea"
            | "run_event"
            | "result"
    ) {
        bail!("unsupported research log entry kind");
    }
    Ok(())
}

fn canonical_json(value: &Value) -> Result<Vec<u8>> {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(object) => {
                let mut keys = object.keys().collect::<Vec<_>>();
                keys.sort();
                let mut result = Map::new();
                for key in keys {
                    result.insert(key.clone(), sorted(&object[key]));
                }
                Value::Object(result)
            }
            Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
            _ => value.clone(),
        }
    }
    let mut bytes = serde_json::to_vec(&sorted(value))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn is_live_stream_locator(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("eventsource")
        || lower.contains("live_sse")
        || lower.contains("/events")
        || ((lower.contains("http://") || lower.contains("https://"))
            && (lower.contains("stream") || lower.contains("/sse")))
}

fn scan_forbidden(value: &Value) -> Result<()> {
    fn walk(value: &Value, path: &str) -> Result<()> {
        match value {
            Value::Object(object) => {
                for (key, child) in object {
                    let normalized = key.to_ascii_lowercase().replace('-', "_");
                    if [
                        "api_key",
                        "access_token",
                        "authorization",
                        "chain_of_thought",
                        "credential",
                        "hidden_reasoning",
                        "password",
                        "refresh_token",
                        "secret",
                    ]
                    .iter()
                    .any(|needle| normalized.contains(needle))
                    {
                        bail!("seal policy forbids {path}.{key}");
                    }
                    if normalized.contains("stream_url") || normalized == "live_sse" {
                        bail!("seal policy forbids live stream binding at {path}.{key}");
                    }
                    walk(child, &format!("{path}.{key}"))?;
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    walk(child, &format!("{path}[{index}]"))?;
                }
            }
            Value::String(text)
                if text.contains("s3://")
                    || text.contains("gs://")
                    || is_live_stream_locator(text) =>
            {
                bail!("seal policy forbids storage locator or live stream URL at {path}");
            }
            _ => {}
        }
        Ok(())
    }
    walk(value, "$")
}

fn build_index_html(data: &Value, runtime_digest: &str) -> Result<String> {
    // HTML recognizes script end tags case-insensitively, even for
    // application/json. Escaping every '<' as a JSON Unicode escape prevents
    // any report-authored string from becoming markup or a script terminator.
    let inline = serde_json::to_string(data)?.replace('<', "\\u003c");
    let css = REPORT_READER_CSS.replace("</style", "<\\/style");
    let runtime = FROZEN_RUNTIME.replace("</script", "<\\/script");
    Ok(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; font-src 'none'; connect-src 'none'; frame-src 'self';"><title>Sealed Report</title><style>{css}</style></head><body><main id="app"></main><script id="synth-report-data" type="application/json">{inline}</script><script data-runtime-digest="{runtime_digest}">{runtime}</script></body></html>"#
    ))
}

fn refuse_network_html(html: &str) -> Result<()> {
    for token in [
        "fetch(",
        "XMLHttpRequest",
        "EventSource",
        "WebSocket",
        "import(",
        "src=\"http",
        "href=\"http",
        "@import",
    ] {
        if html.contains(token) {
            bail!("sealed report index.html contains forbidden network capability: {token}");
        }
    }
    Ok(())
}

