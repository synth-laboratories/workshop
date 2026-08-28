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
    Chart,
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
            Self::Chart => "chart",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "tsx" => Self::Tsx,
            "html" => Self::Html,
            "mermaid" => Self::Mermaid,
            "systems" => Self::Systems,
            "systems-dynamic" => Self::SystemsDynamic,
            "chart" => Self::Chart,
            _ => Self::Template,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualRecord {
    pub schema_version: String,
    pub id: String,
    #[specta(type = specta_typescript::Number)]
    pub current_revision: i64,
    pub title: String,
    pub template_id: String,
    pub status: VisualStatus,
    pub renderer_kind: RendererKind,
    #[specta(type = specta_typescript::Unknown)]
    pub bindings: Value,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub parent_visual_id: Option<String>,
    #[serde(default)]
    pub source_agent_id: Option<String>,
    #[serde(default)]
    pub source_model: Option<String>,
    #[serde(default)]
    pub content_digest: Option<String>,
    #[serde(default)]
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
    #[specta(type = specta_typescript::Number)]
    pub revision: i64,
    pub template_id: String,
    pub renderer_kind: RendererKind,
    #[serde(default)]
    pub content_digest: Option<String>,
    #[serde(default)]
    pub bindings_digest: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub bindings: Option<Value>,
    #[serde(default)]
    pub preview_digest: Option<String>,
    #[serde(default)]
    pub author_agent_id: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub parent_revision: Option<i64>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualAnnotation {
    pub id: String,
    pub visual_id: String,
    #[specta(type = specta_typescript::Number)]
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
    #[specta(type = specta_typescript::Number)]
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
    #[specta(type = specta_typescript::Number)]
    pub visual_revision: i64,
    pub artifact_id: String,
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
    #[specta(type = specta_typescript::Number)]
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
    #[specta(type = specta_typescript::Number)]
    pub limit: Option<i64>,
    #[specta(type = specta_typescript::Number)]
    pub offset: Option<i64>,
}

/// Binding kinds a visual may declare.
///
/// `query_snapshot` is an immutable result set addressed by snapshot id. A
/// visual must never bind to a live query: it would return different rows on
/// every render and the page could not state what the reader is looking at.
pub const VISUAL_BINDING_KINDS: &[&str] = &[
    "inline",
    "trace_v5",
    "local_cas",
    "run_ref",
    "live_sse",
    "fixture",
    "optimizer_run",
    "query_snapshot",
    // The document pane's grant: the one workspace path a visual declares. It
    // is resolved by the host on every read through the conversation's session
    // roots, never by `bindTemplateSlots`, which is why the TypeScript arm
    // throws rather than fetching.
    "workspace_file",
];

/// How an authored bindings value reached the canonical envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingsForm {
    /// Already `synth.visual-bindings.v1`, or an empty authoring default.
    Canonical,
    /// COMPAT: upgraded from a slot-keyed descriptor map, e.g.
    /// `{"stream": [{"kind": "live_sse", ...}]}`. Remove once no writer emits
    /// this shape; the canonical envelope is the only documented contract.
    UpgradedSlotMap,
    /// COMPAT: upgraded from a legacy inline prop bag, e.g. `{"matrix": []}`.
    /// Remove alongside `UpgradedSlotMap`.
    UpgradedPropBag,
}

impl BindingsForm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::UpgradedSlotMap => "upgraded_slot_map",
            Self::UpgradedPropBag => "upgraded_prop_bag",
        }
    }

    pub fn is_upgrade(&self) -> bool {
        !matches!(self, Self::Canonical)
    }
}

/// Canonical bindings plus the record of how they got there.
#[derive(Clone, Debug)]
pub struct CanonicalBindings {
    pub value: Value,
    pub form: BindingsForm,
    /// Slot names touched by an upgrade. Empty when already canonical.
    pub upgraded_slots: Vec<String>,
}

/// Canonical bind-point name. `input` is the wire field; `slot` still binds
/// on stored envelopes. Both present and unequal fails closed.
pub fn descriptor_input_name(descriptor: &Value) -> anyhow::Result<String> {
    let object = descriptor
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("visual binding descriptors must be objects"))?;
    let input = object.get("input").and_then(Value::as_str);
    let slot = object.get("slot").and_then(Value::as_str);
    match (input, slot) {
        (Some(a), Some(b)) if a == b => Ok(a.to_string()),
        (Some(_), Some(_)) => {
            anyhow::bail!("visual binding input and slot disagree; send one name")
        }
        (Some(a), None) | (None, Some(a)) => Ok(a.to_string()),
        (None, None) => anyhow::bail!("visual binding requires an input name"),
    }
}

pub fn stamp_binding_input(descriptor: &mut Value, name: &str) {
    if let Some(object) = descriptor.as_object_mut() {
        object.insert("input".into(), json!(name));
        object.remove("slot");
    }
}

fn stamp_descriptors(items: &[Value]) -> anyhow::Result<Vec<Value>> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let mut descriptor = item.clone();
        let name = descriptor_input_name(&descriptor)?;
        stamp_binding_input(&mut descriptor, &name);
        out.push(descriptor);
    }
    Ok(out)
}

/// Envelope array: canonical `inputs`; `slots` still binds on stored envelopes.
pub fn binding_descriptors(bindings: &Value) -> anyhow::Result<Vec<Value>> {
    let object = bindings
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("visual bindings must be a JSON object"))?;
    envelope_arrays(object)
}

fn envelope_arrays(object: &serde_json::Map<String, Value>) -> anyhow::Result<Vec<Value>> {
    let inputs = object.get("inputs").and_then(Value::as_array);
    let slots = object.get("slots").and_then(Value::as_array);
    match (inputs, slots) {
        (None, None) => {
            anyhow::bail!("canonical visual bindings require an inputs array")
        }
        (Some(a), None) | (None, Some(a)) => stamp_descriptors(a),
        (Some(a), Some(b)) => {
            let left = stamp_descriptors(a)?;
            let right = stamp_descriptors(b)?;
            if left != right {
                anyhow::bail!("visual bindings inputs and slots disagree; send one array");
            }
            Ok(left)
        }
    }
}

/// Decide whether one authored value is a binding descriptor rather than
/// inline input data.
///
/// This is a **heuristic** and it is deliberately the only one: a legacy prop
/// bag and a slot-keyed descriptor map are both bare JSON objects, so the shape
/// alone has to tell them apart. A value counts as a descriptor when it names a
/// `kind` from `VISUAL_BINDING_KINDS` *and* carries at least one field only a
/// descriptor has (`input`, `slot`, `source`, `data`, `poll_url`). Inline chart data such
/// as `{"kind": "bar"}` therefore stays inline data.
///
/// It exists because writers were allowed to persist un-canonical bindings for
/// several releases. It is removed once `BindingsForm::Upgraded*` stops firing
/// in the field. See: docs/contracts/visual_bindings.md.
fn is_binding_descriptor(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(kind) = object.get("kind").and_then(Value::as_str) else {
        return false;
    };
    if !VISUAL_BINDING_KINDS.contains(&kind) {
        return false;
    }
    ["input", "slot", "source", "data", "poll_url"]
        .iter()
        .any(|field| object.contains_key(*field))
}

fn is_descriptor_entry(value: &Value) -> bool {
    match value {
        Value::Array(items) => !items.is_empty() && items.iter().all(is_binding_descriptor),
        other => is_binding_descriptor(other),
    }
}

/// The one authority for the shape of visual bindings.
///
/// Every write path — HTTP, MCP, import, migration — goes through this
/// function, so a visual can only ever persist bindings the renderer can read.
/// Three outcomes and no fourth: canonical passes through, a recognised legacy
/// shape is upgraded and reported, anything else is refused. Nothing is
/// accepted silently, because a binding shape the renderer cannot read renders
/// an empty pane with no error — which is indistinguishable from a stream that
/// produced nothing.
///
/// See: docs/contracts/visual_bindings.md.
pub fn canonicalize_bindings(bindings: &Value) -> anyhow::Result<CanonicalBindings> {
    let Some(object) = bindings.as_object() else {
        anyhow::bail!("visual bindings must be a JSON object");
    };
    if let Some(schema_version) = object.get("schemaVersion") {
        if schema_version.as_str() != Some(VISUAL_BINDINGS_SCHEMA_VERSION) {
            anyhow::bail!(
                "unsupported visual bindings schemaVersion {}; this build writes {}",
                schema_version,
                VISUAL_BINDINGS_SCHEMA_VERSION
            );
        }
        let descriptors = envelope_arrays(object)?;
        validate_bindings(&descriptors)?;
        return Ok(CanonicalBindings {
            value: canonical_envelope(descriptors),
            form: BindingsForm::Canonical,
            upgraded_slots: Vec::new(),
        });
    }
    // An authoring default. Stamping it canonical here keeps every persisted
    // visual on one contract without reporting an upgrade nobody performed.
    if object.is_empty() {
        return Ok(CanonicalBindings {
            value: canonical_envelope(Vec::new()),
            form: BindingsForm::Canonical,
            upgraded_slots: Vec::new(),
        });
    }

    let descriptor_entries = object
        .iter()
        .filter(|(_, value)| is_descriptor_entry(value))
        .count();
    let form = if descriptor_entries > 0 {
        BindingsForm::UpgradedSlotMap
    } else {
        BindingsForm::UpgradedPropBag
    };

    let mut slots = Vec::new();
    let mut upgraded_slots = Vec::new();
    for (name, value) in object {
        super::live_eval::assert_live_eval_slot(name)?;
        upgraded_slots.push(name.clone());
        if form == BindingsForm::UpgradedPropBag {
            slots.push(json!({"input": name, "kind": "inline", "data": value}));
            continue;
        }
        // A slot map that mixes descriptors and raw data cannot be read either
        // way round. Refusing it is the whole point: guessing is what produced
        // an empty pane with no error.
        if !is_descriptor_entry(value) {
            anyhow::bail!(
                "visual binding slot {name:?} mixes descriptors and inline data; \
                 send {VISUAL_BINDINGS_SCHEMA_VERSION} bindings with an explicit inputs array"
            );
        }
        let descriptors = match value {
            Value::Array(items) => items.clone(),
            other => vec![other.clone()],
        };
        for descriptor in descriptors {
            let mut descriptor = descriptor;
            // The map key is authoritative: a descriptor filed under
            // "stream" is a stream binding whatever its own field claims.
            stamp_binding_input(&mut descriptor, name);
            slots.push(descriptor);
        }
    }
    validate_bindings(&slots)?;
    Ok(CanonicalBindings {
        value: canonical_envelope(slots),
        form,
        upgraded_slots,
    })
}

fn canonical_envelope(slots: Vec<Value>) -> Value {
    let stamped = stamp_descriptors(&slots).unwrap_or(slots);
    json!({
        "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
        "inputs": stamped,
    })
}

fn validate_bindings(slots: &[Value]) -> anyhow::Result<()> {
    for slot in slots {
        let slot_name = descriptor_input_name(slot)?;
        super::live_eval::assert_live_eval_slot(&slot_name)?;
        let kind = slot.get("kind").and_then(Value::as_str).unwrap_or_default();
        if !VISUAL_BINDING_KINDS.contains(&kind) {
            anyhow::bail!("unsupported visual binding kind: {kind}");
        }
        if kind == "inline"
            && !slot
                .as_object()
                .is_some_and(|object| object.contains_key("data"))
        {
            anyhow::bail!(
                "{}",
                json!({
                    "code": "visual_binding_invalid",
                    "input": slot_name,
                    "slot": slot_name,
                    "kind": "inline",
                    "expected_source_kind": "inline",
                    "missing_field": "data",
                    "remediation": "Inline bindings require a data object for this input."
                })
            );
        }
        if kind != "inline" && slot.get("source").and_then(Value::as_str).is_none() {
            anyhow::bail!(
                "{}",
                json!({
                    "code": "visual_binding_invalid",
                    "input": slot_name,
                    "slot": slot_name,
                    "kind": kind,
                    "expected_source_kind": kind,
                    "missing_field": "source",
                    "remediation": format!("{kind} visual binding requires source")
                })
            );
        }
        if kind == "live_sse" {
            if let Some(source) = slot.get("source").and_then(Value::as_str) {
                super::live_eval::assert_declared_stream_source(source)?;
            }
        }
    }
    Ok(())
}

/// Declared live-stream poll URLs, in slot order.
///
/// The renderer's native poll command allowlists against this. It reads the
/// canonical envelope through the same function that writes it, so a visual
/// cannot declare a stream the poll command will not recognise.
pub fn declared_poll_urls(bindings: &Value) -> Vec<String> {
    let Ok(canonical) = canonicalize_bindings(bindings) else {
        return Vec::new();
    };
    canonical
        .value
        .get("inputs")
        .or_else(|| canonical.value.get("slots"))
        .and_then(Value::as_array)
        .map(|slots| {
            slots
                .iter()
                .filter(|slot| slot.get("kind").and_then(Value::as_str) == Some("live_sse"))
                .filter_map(|slot| {
                    slot.get("poll_url")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The optimizer runs this visual actually declares, in binding order.
///
/// The media bridge needs to know which run a visual may read frames from, and
/// "the run it happens to be showing" is not something a renderer gets to
/// assert. This reads the same canonical bindings the rest of the host reads,
/// so a visual bound to no run is granted nothing rather than defaulting to
/// whichever run last wrote to it.
pub fn declared_optimizer_run_ids(bindings: &Value) -> Vec<String> {
    let Ok(canonical) = canonicalize_bindings(bindings) else {
        return Vec::new();
    };
    canonical
        .value
        .get("inputs")
        .or_else(|| canonical.value.get("slots"))
        .and_then(Value::as_array)
        .map(|slots| {
            slots
                .iter()
                .filter(|slot| slot.get("kind").and_then(Value::as_str) == Some("optimizer_run"))
                .filter_map(|slot| {
                    slot.get("source")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
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

