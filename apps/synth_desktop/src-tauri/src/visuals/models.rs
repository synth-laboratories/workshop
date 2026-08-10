use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const VISUAL_SCHEMA_VERSION: &str = "synth.desktop-visual.v1";
pub const VISUAL_BINDINGS_SCHEMA_VERSION: &str = "synth.visual-bindings.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RendererKind {
    Template,
    Tsx,
    Html,
}

impl RendererKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Template => "template",
            Self::Tsx => "tsx",
            Self::Html => "html",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "tsx" => Self::Tsx,
            "html" => Self::Html,
            _ => Self::Template,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VisualRecord {
    pub schema_version: String,
    pub id: String,
    pub current_revision: i64,
    pub title: String,
    pub template_id: String,
    pub status: VisualStatus,
    pub renderer_kind: RendererKind,
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
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VisualRevision {
    pub visual_id: String,
    pub revision: i64,
    pub template_id: String,
    pub renderer_kind: RendererKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bindings_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bindings: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_revision: Option<i64>,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualCreateRequest {
    pub template_id: String,
    pub title: Option<String>,
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
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualUpdateRequest {
    pub title: Option<String>,
    pub bindings: Option<Value>,
    pub status: Option<VisualStatus>,
    pub renderer_kind: Option<RendererKind>,
    pub message_id: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: Option<String>,
    pub content: Option<String>,
    pub metadata: Option<Value>,
    /// When true, content/bindings changes create a new revision.
    pub bump_revision: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualQuery {
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub template_id: Option<String>,
    pub search: Option<String>,
    pub limit: Option<i64>,
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
        if slot.get("slot").and_then(Value::as_str).is_none() {
            anyhow::bail!("visual binding slot requires a slot name");
        }
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
        ) {
            anyhow::bail!("unsupported visual binding kind: {kind}");
        }
        if kind == "inline" && !slot.contains_key("data") {
            anyhow::bail!("inline visual binding requires data");
        }
        if kind != "inline" && slot.get("source").and_then(Value::as_str).is_none() {
            anyhow::bail!("{kind} visual binding requires source");
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
    }
}
