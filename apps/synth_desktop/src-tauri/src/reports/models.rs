use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const REPORT_SCHEMA_VERSION: &str = "synth.desktop-report.v1";
pub const REPORT_REVISION_SCHEMA: &str = "synth.report-revision.v1";
pub const REPORT_BUNDLE_SCHEMA: &str = "synth.report-bundle.v1";

pub const BLOCK_OUTLINE: &str = "report.outline.v1";
pub const BLOCK_PROSE: &str = "report.prose.v1";
pub const BLOCK_RESULT: &str = "report.result.v1";
pub const BLOCK_VISUAL: &str = "report.visual.v1";
pub const BLOCK_DIAGRAM: &str = "report.diagram.v1";
pub const BLOCK_TRACE: &str = "report.trace-v5.v1";
pub const BLOCK_CLAIM: &str = "report.claim.v1";
pub const BLOCK_ATTACHMENT: &str = "report.attachment.v1";
pub const BLOCK_EXPERIMENT_RECORDS: &str = "report.experiment-records.v1";
pub const BLOCK_RESEARCH_LOG: &str = "report.research-log.v1";

pub const KNOWN_BLOCK_KINDS: &[&str] = &[
    BLOCK_OUTLINE,
    BLOCK_PROSE,
    BLOCK_RESULT,
    BLOCK_VISUAL,
    BLOCK_DIAGRAM,
    BLOCK_TRACE,
    BLOCK_CLAIM,
    BLOCK_ATTACHMENT,
    BLOCK_EXPERIMENT_RECORDS,
    BLOCK_RESEARCH_LOG,
];

pub const APPENDIX_ANCHORS: &[(&str, &str)] = &[
    ("findings", "Findings"),
    ("methods", "Methods"),
    ("results", "Results and visuals"),
    ("traces", "Trace evidence"),
    ("limitations", "Limitations"),
    ("experiment-records", "Experiment Records"),
    ("research-log", "Research Log"),
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ReportStatus {
    Draft,
    Sealed,
}

impl ReportStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Sealed => "sealed",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "sealed" => Self::Sealed,
            _ => Self::Draft,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ExperimentStatus {
    Planned,
    Running,
    Completed,
    Failed,
    Aborted,
    Superseded,
    Excluded,
}

impl ExperimentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
            Self::Superseded => "superseded",
            Self::Excluded => "excluded",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "aborted" => Self::Aborted,
            "superseded" => Self::Superseded,
            "excluded" => Self::Excluded,
            _ => Self::Planned,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportRecord {
    pub schema_version: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_ref: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub current_revision: i64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub authors: Vec<String>,
    pub status: ReportStatus,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportRevision {
    pub schema_version: String,
    pub report_id: String,
    #[specta(type = specta_typescript::Number)]
    pub revision: i64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub authors: Vec<String>,
    pub status: ReportStatus,
    pub blocks: Vec<ReportBlock>,
    pub sources: Vec<ReportSource>,
    pub claims: Vec<ReportClaim>,
    pub limitations: Vec<ReportLimitation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_version: Option<String>,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportBlock {
    pub block_id: String,
    pub kind: String,
    pub anchor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_digest: Option<String>,
    #[serde(default = "default_reference_mode")]
    pub reference_mode: String,
    pub access_state: String,
    pub integrity_state: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportSource {
    pub source_id: String,
    pub resource_kind: String,
    pub resource_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_digest: Option<String>,
    #[serde(default = "default_reference_mode")]
    pub reference_mode: String,
    pub relation: String,
    pub access_state: String,
    pub integrity_state: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportClaim {
    pub claim_id: String,
    pub statement: String,
    pub status: String,
    #[serde(default = "default_claim_confidence")]
    pub confidence: String,
    #[serde(default = "default_claim_why")]
    pub why: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportValidationFinding {
    pub code: String,
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportValidationResult {
    pub report_id: String,
    #[specta(type = specta_typescript::Number)]
    pub revision: i64,
    pub sealable: bool,
    pub findings: Vec<ReportValidationFinding>,
}

fn default_reference_mode() -> String {
    "live".into()
}
fn default_claim_confidence() -> String {
    "low".into()
}
fn default_claim_why() -> String {
    "Migrated legacy claim; rationale was not recorded.".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportLimitation {
    pub limitation_id: String,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentRecord {
    pub experiment_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    #[specta(type = specta_typescript::Number)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypothesis: Option<String>,
    pub status: ExperimentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_digest: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub arms: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub runs: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub results: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub evaluator_refs: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub trace_collection_refs: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub claim_refs: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub research_log_refs: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub limitations: Value,
    pub created_at: String,
    pub created_by: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResearchLogEntry {
    pub entry_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub sequence: i64,
    pub occurred_at: String,
    pub recorded_at: String,
    pub author: String,
    pub actor_kind: String,
    pub entry_kind: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub links: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_effect: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes_entry_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_digest: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportSeal {
    pub receipt_digest: String,
    pub report_id: String,
    #[specta(type = specta_typescript::Number)]
    pub report_revision: i64,
    pub schema_version: String,
    pub compiler_name: String,
    pub compiler_version: String,
    pub runtime_digest: String,
    pub index_digest: String,
    pub data_digest: String,
    #[specta(type = specta_typescript::Number)]
    pub receipt_size_bytes: i64,
    #[specta(type = specta_typescript::Number)]
    pub total_size_bytes: i64,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportSealBundle {
    pub seal: ReportSeal,
    pub index_html: String,
    #[specta(type = specta_typescript::Unknown)]
    pub data: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub receipt: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportRevisionCompare {
    pub left: ReportSealBundle,
    pub right: ReportSealBundle,
    pub same_digest: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportUpload {
    pub receipt_digest: String,
    pub collection_id: Option<String>,
    pub publication_id: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub publication_revision: Option<i64>,
    pub state: String,
    pub committed_url: Option<String>,
    pub error: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportPromotion {
    #[serde(alias = "publication_id")]
    pub publication_id: String,
    pub slug: String,
    pub status: String,
    #[serde(alias = "public_url")]
    pub public_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReportAudience {
    Private,
    Workspace {
        #[serde(rename = "workspace_id", alias = "workspaceId")]
        workspace_id: String,
    },
    Members {
        #[serde(rename = "member_ids", alias = "memberIds")]
        member_ids: Vec<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportAudienceRequest {
    #[serde(rename = "receipt_digest", alias = "receiptDigest")]
    pub receipt_digest: String,
    pub audience: ReportAudience,
    #[serde(rename = "redaction_policy_version", alias = "redactionPolicyVersion")]
    pub redaction_policy_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportAudienceState {
    #[serde(alias = "publication_id")]
    pub publication_id: String,
    pub audience: ReportAudience,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportComment {
    pub comment_id: String,
    pub report_id: String,
    #[specta(type = specta_typescript::Number)]
    pub report_revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    pub body: String,
    pub author_id: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportCommentCreate {
    pub body: String,
    pub anchor: Option<String>,
    pub author_id: Option<String>,
    pub receipt_digest: Option<String>,
    pub publication_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportAttachTrace {
    #[serde(alias = "trace_digest")]
    pub trace_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "trace_id")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "collection_id"
    )]
    pub collection_id: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportCreateRequest {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub authors: Option<Vec<String>>,
    pub project_ref: Option<String>,
    pub id: Option<String>,
    pub created_by: Option<String>,
    pub blocks: Option<Vec<ReportBlock>>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportUpdateRequest {
    #[specta(type = specta_typescript::Number)]
    #[serde(default, alias = "expected_revision")]
    pub expected_revision: Option<i64>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub authors: Option<Vec<String>>,
    pub project_ref: Option<String>,
    pub blocks: Option<Vec<ReportBlock>>,
    pub sources: Option<Vec<ReportSource>>,
    pub claims: Option<Vec<ReportClaim>>,
    pub limitations: Option<Vec<ReportLimitation>>,
}

#[derive(Clone, Debug, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportQuery {
    pub status: Option<String>,
    pub search: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportVisibilityRequest {
    pub request_id: String,
    pub report_id: String,
    #[specta(type = specta_typescript::Number)]
    pub report_revision: i64,
    pub receipt_digest: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub requested_by: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportVisibilityRequestCreate {
    #[serde(alias = "receipt_digest")]
    pub receipt_digest: String,
    pub target: String,
    pub slug: Option<String>,
    pub reason: Option<String>,
    pub requested_by: Option<String>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentRecordUpsert {
    pub experiment_id: Option<String>,
    pub title: String,
    pub hypothesis: Option<String>,
    pub status: Option<String>,
    pub protocol_digest: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub arms: Option<Value>,
    #[specta(type = specta_typescript::Unknown)]
    pub runs: Option<Value>,
    #[specta(type = specta_typescript::Unknown)]
    pub results: Option<Value>,
    #[specta(type = specta_typescript::Unknown)]
    pub evaluator_refs: Option<Value>,
    #[specta(type = specta_typescript::Unknown)]
    pub trace_collection_refs: Option<Value>,
    #[specta(type = specta_typescript::Unknown)]
    pub claim_refs: Option<Value>,
    #[specta(type = specta_typescript::Unknown)]
    pub research_log_refs: Option<Value>,
    #[specta(type = specta_typescript::Unknown)]
    pub limitations: Option<Value>,
    pub created_by: Option<String>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResearchLogAppend {
    pub occurred_at: Option<String>,
    pub author: Option<String>,
    pub actor_kind: Option<String>,
    pub entry_kind: String,
    pub title: String,
    pub body: String,
    pub tags: Option<Vec<String>>,
    #[specta(type = specta_typescript::Unknown)]
    pub links: Option<Value>,
    pub claim_effect: Option<String>,
    pub supersedes_entry_id: Option<String>,
}

pub fn is_evidence_kind(kind: &str) -> bool {
    matches!(
        kind,
        BLOCK_RESULT
            | BLOCK_VISUAL
            | BLOCK_DIAGRAM
            | BLOCK_TRACE
            | BLOCK_CLAIM
            | BLOCK_ATTACHMENT
            | BLOCK_EXPERIMENT_RECORDS
            | BLOCK_RESEARCH_LOG
    )
}

pub fn validate_block(block: &ReportBlock) -> anyhow::Result<()> {
    if !KNOWN_BLOCK_KINDS.contains(&block.kind.as_str()) {
        anyhow::bail!("unsupported report block kind {}", block.kind);
    }
    if block.block_id.trim().is_empty() || block.anchor.trim().is_empty() {
        anyhow::bail!("report block requires blockId and anchor");
    }
    if !matches!(block.reference_mode.as_str(), "live" | "pinned") {
        anyhow::bail!("report block referenceMode must be live or pinned");
    }
    if !matches!(
        block.access_state.as_str(),
        "available" | "redacted" | "forbidden" | "missing" | "accessible"
    ) {
        anyhow::bail!("invalid report block accessState {}", block.access_state);
    }
    if !matches!(
        block.integrity_state.as_str(),
        "verified"
            | "digest_mismatch"
            | "unresolved"
            | "unsupported"
            | "source_changed"
            | "unknown"
    ) {
        anyhow::bail!(
            "invalid report block integrityState {}",
            block.integrity_state
        );
    }
    if block.reference_mode == "pinned"
        && (block.source_revision.is_none() || block.source_digest.is_none())
    {
        anyhow::bail!(
            "pinned evidence block {} requires both revision and digest",
            block.kind
        );
    }
    if is_evidence_kind(&block.kind)
        && block.source_revision.is_none()
        && block.source_digest.is_none()
        && block.access_state != "missing"
    {
        anyhow::bail!(
            "evidence block {} requires an exact revision, digest, or explicit missing access state",
            block.kind
        );
    }
    Ok(())
}

pub fn default_blocks() -> Vec<ReportBlock> {
    vec![
        ReportBlock {
            block_id: "blk_findings".into(),
            kind: BLOCK_PROSE.into(),
            anchor: "findings".into(),
            title: Some("Findings".into()),
            payload: serde_json::json!({"markdown": ""}),
            source_revision: None,
            source_digest: None,
            reference_mode: "live".into(),
            access_state: "available".into(),
            integrity_state: "verified".into(),
        },
        ReportBlock {
            block_id: "blk_methods".into(),
            kind: BLOCK_PROSE.into(),
            anchor: "methods".into(),
            title: Some("Methods".into()),
            payload: serde_json::json!({"markdown": ""}),
            source_revision: None,
            source_digest: None,
            reference_mode: "live".into(),
            access_state: "available".into(),
            integrity_state: "verified".into(),
        },
        ReportBlock {
            block_id: "blk_experiment_records".into(),
            kind: BLOCK_EXPERIMENT_RECORDS.into(),
            anchor: "experiment-records".into(),
            title: Some("Experiment Records".into()),
            payload: serde_json::json!({"experimentIds": []}),
            source_revision: Some("working".into()),
            source_digest: None,
            reference_mode: "live".into(),
            access_state: "available".into(),
            integrity_state: "unresolved".into(),
        },
        ReportBlock {
            block_id: "blk_research_log".into(),
            kind: BLOCK_RESEARCH_LOG.into(),
            anchor: "research-log".into(),
            title: Some("Research Log".into()),
            payload: serde_json::json!({"entryIds": []}),
            source_revision: Some("working".into()),
            source_digest: None,
            reference_mode: "live".into(),
            access_state: "available".into(),
            integrity_state: "unresolved".into(),
        },
    ]
}

pub fn generated_outline(blocks: &[ReportBlock]) -> ReportBlock {
    let items = APPENDIX_ANCHORS
        .iter()
        .filter_map(|(anchor, title)| {
            blocks
                .iter()
                .find(|block| block.anchor == *anchor)
                .map(|_| {
                    serde_json::json!({
                        "anchor": anchor,
                        "title": title,
                    })
                })
        })
        .chain(blocks.iter().filter_map(|block| {
            if APPENDIX_ANCHORS
                .iter()
                .any(|(anchor, _)| *anchor == block.anchor)
                || block.kind == BLOCK_OUTLINE
            {
                None
            } else {
                Some(serde_json::json!({
                    "anchor": block.anchor,
                    "title": block.title.clone().unwrap_or_else(|| block.kind.clone()),
                }))
            }
        }))
        .collect::<Vec<_>>();
    ReportBlock {
        block_id: "blk_outline".into(),
        kind: BLOCK_OUTLINE.into(),
        anchor: "outline".into(),
        title: Some("Outline".into()),
        payload: serde_json::json!({ "items": items }),
        source_revision: None,
        source_digest: None,
        reference_mode: "live".into(),
        access_state: "available".into(),
        integrity_state: "verified".into(),
    }
}
