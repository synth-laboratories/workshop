use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const VISUAL_SCHEMA_VERSION: &str = "synth.desktop-visual.v1";
pub const VISUAL_BINDINGS_SCHEMA_VERSION: &str = "synth.visual-bindings.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum VisualStatus {
    Draft,
    Live,
    Saved,
    Failed,
    Archived,
}

impl VisualStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Live => "live",
            Self::Saved => "saved",
            Self::Failed => "failed",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "live" => Self::Live,
            "saved" => Self::Saved,
            "failed" => Self::Failed,
            "archived" => Self::Archived,
            _ => Self::Draft,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum RendererKind {
    Template,
    Tsx,
    Html,
    Mermaid,
    Systems,
    SystemsDynamic,
}

impl RendererKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Template => "template",
            Self::Tsx => "tsx",
            Self::Html => "html",
            Self::Mermaid => "mermaid",
            Self::Systems => "systems",
            Self::SystemsDynamic => "systems-dynamic",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "tsx" => Self::Tsx,
            "html" => Self::Html,
            "mermaid" => Self::Mermaid,
            "systems" => Self::Systems,
            "systems-dynamic" => Self::SystemsDynamic,
            _ => Self::Template,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualRecord {
    pub schema_version: String,
    pub id: String,
    #[specta(type = specta_typescript::Unknown)]
    pub current_revision: i64,
    pub title: String,
    pub template_id: String,
    pub status: VisualStatus,
    pub renderer_kind: RendererKind,
    #[specta(type = specta_typescript::Unknown)]
    pub bindings: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_visual_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_digest: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualRevision {
    pub visual_id: String,
    #[specta(type = specta_typescript::Unknown)]
    pub revision: i64,
    pub template_id: String,
    pub renderer_kind: RendererKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bindings_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = specta_typescript::Unknown)]
    pub bindings: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = specta_typescript::Unknown)]
    pub parent_revision: Option<i64>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualAnnotation {
    pub id: String,
    pub visual_id: String,
    #[specta(type = specta_typescript::Unknown)]
    pub visual_revision: i64,
    pub source_digest: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub selector: Value,
    pub kind: String,
    pub body: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
    pub author_id: String,
    pub supersedes_id: Option<String>,
    pub tombstoned: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualAnnotationCreate {
    #[specta(type = specta_typescript::Unknown)]
    pub visual_revision: i64,
    pub source_digest: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub selector: Value,
    pub kind: String,
    pub body: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Option<Value>,
    pub author_id: Option<String>,
    pub supersedes_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualSeal {
    pub receipt_digest: String,
    pub visual_id: String,
    #[specta(type = specta_typescript::Unknown)]
    pub visual_revision: i64,
    pub artifact_id: String,
    pub schema_version: String,
    pub compiler_name: String,
    pub compiler_version: String,
    pub runtime_digest: String,
    pub index_digest: String,
    pub data_digest: String,
    #[specta(type = specta_typescript::Unknown)]
    pub receipt_size_bytes: i64,
    #[specta(type = specta_typescript::Unknown)]
    pub total_size_bytes: i64,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualSealBundle {
    pub seal: VisualSeal,
    pub index_html: String,
    #[specta(type = specta_typescript::Unknown)]
    pub data: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub receipt: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualUpload {
    pub receipt_digest: String,
    pub collection_id: Option<String>,
    pub publication_id: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub publication_revision: Option<i64>,
    pub state: String,
    pub committed_url: Option<String>,
    pub error: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualCreateRequest {
    pub template_id: String,
    pub title: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub bindings: Option<Value>,
    pub id: Option<String>,
    pub status: Option<VisualStatus>,
    pub renderer_kind: Option<RendererKind>,
    pub session_id: Option<String>,
    pub message_id: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: Option<String>,
    pub parent_visual_id: Option<String>,
    pub source_agent_id: Option<String>,
    pub source_model: Option<String>,
    pub content: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualUpdateRequest {
    pub title: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub bindings: Option<Value>,
    pub status: Option<VisualStatus>,
    pub renderer_kind: Option<RendererKind>,
    pub message_id: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: Option<String>,
    pub content: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Option<Value>,
    /// When true, content/bindings changes create a new revision.
    pub bump_revision: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualQuery {
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub template_id: Option<String>,
    pub search: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub limit: Option<i64>,
    #[specta(type = specta_typescript::Unknown)]
    pub offset: Option<i64>,
}

pub fn validate_bindings(bindings: &Value) -> anyhow::Result<()> {
    let Some(object) = bindings.as_object() else {
        anyhow::bail!("visual bindings must be a JSON object");
    };
    let Some(schema_version) = object.get("schemaVersion") else {
        return Ok(()); // legacy prop bag
    };
    if schema_version.as_str() != Some(VISUAL_BINDINGS_SCHEMA_VERSION) {
        anyhow::bail!("unsupported visual bindings schemaVersion");
    }
    let slots = object
        .get("slots")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("canonical visual bindings require a slots array"))?;
    for slot in slots {
        let slot = slot
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("visual binding slots must be objects"))?;
        let slot_name = slot
            .get("slot")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("visual binding slot requires a slot name"))?;
        super::live_eval::assert_live_eval_slot(slot_name)?;
        let kind = slot.get("kind").and_then(Value::as_str).unwrap_or_default();
        if !matches!(
            kind,
            "inline"
                | "trace_v5"
                | "local_cas"
                | "run_ref"
                | "live_sse"
                | "fixture"
                | "optimizer_run"
                // An immutable result set, addressed by snapshot id. A visual
                // must never bind to a live query: it would return different
                // rows on every render and the page could not state what the
                // reader is looking at.
                | "query_snapshot"
        ) {
            anyhow::bail!("unsupported visual binding kind: {kind}");
        }
        if kind == "inline" && !slot.contains_key("data") {
            anyhow::bail!("inline visual binding requires data");
        }
        if kind != "inline" && slot.get("source").and_then(Value::as_str).is_none() {
            anyhow::bail!("{kind} visual binding requires source");
        }
        if kind == "live_sse" {
            if let Some(source) = slot.get("source").and_then(Value::as_str) {
                super::live_eval::assert_declared_stream_source(source)?;
            }
        }
    }
    Ok(())
}

impl VisualRecord {
    pub fn to_legacy_instance(&self) -> Value {
        json!({
            "id": self.id,
            "templateId": self.template_id,
            "title": self.title,
            "bindings": self.bindings,
            "tsxPath": self.metadata.get("tsxPath").cloned().unwrap_or(Value::Null),
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
            "metadata": {
                "status": self.status.as_str(),
                "rendererKind": self.renderer_kind.as_str(),
                "currentRevision": self.current_revision,
                "sessionId": self.session_id,
                "contentDigest": self.content_digest,
                "previewDigest": self.preview_digest,
                "parentVisualId": self.parent_visual_id,
                "source": self.metadata,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_canonical_bindings_and_keeps_legacy_readable() {
        validate_bindings(&json!({"matrix": []})).unwrap();
        validate_bindings(&json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{"slot":"matrix", "kind":"inline", "data":[]}]
        }))
        .unwrap();
        assert!(validate_bindings(&json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{"slot":"matrix", "kind":"local_cas"}]
        }))
        .is_err());
        assert!(validate_bindings(&json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{"slot":"jobs", "kind":"live_sse", "source":"http://127.0.0.1:8098/declared"}]
        }))
        .is_err());
        assert!(validate_bindings(&json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{"slot":"stream", "kind":"live_sse", "source":"http://127.0.0.1:8098/events"}]
        }))
        .is_err());
        validate_bindings(&json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{"slot":"stream", "kind":"live_sse", "source":"http://127.0.0.1:8098/rollouts/r1/stream"}]
        }))
        .unwrap();
    }
}
