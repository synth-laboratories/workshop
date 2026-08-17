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

/// Decide whether one authored value is a binding descriptor rather than
/// inline slot data.
///
/// This is a **heuristic** and it is deliberately the only one: a legacy prop
/// bag and a slot-keyed descriptor map are both bare JSON objects, so the shape
/// alone has to tell them apart. A value counts as a descriptor when it names a
/// `kind` from `VISUAL_BINDING_KINDS` *and* carries at least one field only a
/// descriptor has (`slot`, `source`, `data`, `poll_url`). Inline chart data such
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
    ["slot", "source", "data", "poll_url"]
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
        let slots = object
            .get("slots")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("canonical visual bindings require a slots array"))?;
        validate_slots(slots)?;
        return Ok(CanonicalBindings {
            value: bindings.clone(),
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
            slots.push(json!({"slot": name, "kind": "inline", "data": value}));
            continue;
        }
        // A slot map that mixes descriptors and raw data cannot be read either
        // way round. Refusing it is the whole point: guessing is what produced
        // an empty pane with no error.
        if !is_descriptor_entry(value) {
            anyhow::bail!(
                "visual binding slot {name:?} mixes descriptors and inline data; \
                 send {VISUAL_BINDINGS_SCHEMA_VERSION} bindings with an explicit slots array"
            );
        }
        let descriptors = match value {
            Value::Array(items) => items.clone(),
            other => vec![other.clone()],
        };
        for descriptor in descriptors {
            let mut descriptor = descriptor;
            if let Some(entry) = descriptor.as_object_mut() {
                // The slot key is authoritative: a descriptor filed under
                // "stream" is a stream binding whatever its own field claims.
                entry.insert("slot".into(), json!(name));
            }
            slots.push(descriptor);
        }
    }
    validate_slots(&slots)?;
    Ok(CanonicalBindings {
        value: canonical_envelope(slots),
        form,
        upgraded_slots,
    })
}

fn canonical_envelope(slots: Vec<Value>) -> Value {
    json!({
        "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
        "slots": slots,
    })
}

fn validate_slots(slots: &[Value]) -> anyhow::Result<()> {
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
        if !VISUAL_BINDING_KINDS.contains(&kind) {
            anyhow::bail!("unsupported visual binding kind: {kind}");
        }
        if kind == "inline" && !slot.contains_key("data") {
            anyhow::bail!(
                "{}",
                json!({
                    "code": "visual_binding_invalid",
                    "slot": slot_name,
                    "kind": "inline",
                    "expected_source_kind": "inline",
                    "missing_field": "data",
                    "remediation": "Inline bindings require a data object for this slot."
                })
            );
        }
        if kind != "inline" && slot.get("source").and_then(Value::as_str).is_none() {
            anyhow::bail!(
                "{}",
                json!({
                    "code": "visual_binding_invalid",
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
        .get("slots")
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
        canonicalize_bindings(&json!({"matrix": []})).unwrap();
        canonicalize_bindings(&json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{"slot":"matrix", "kind":"inline", "data":[]}]
        }))
        .unwrap();
        assert!(canonicalize_bindings(&json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{"slot":"matrix", "kind":"local_cas"}]
        }))
        .is_err());
        assert!(canonicalize_bindings(&json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{"slot":"jobs", "kind":"live_sse", "source":"http://127.0.0.1:8098/declared"}]
        }))
        .is_err());
        assert!(canonicalize_bindings(&json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{"slot":"stream", "kind":"live_sse", "source":"http://127.0.0.1:8098/events"}]
        }))
        .is_err());
        canonicalize_bindings(&json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{"slot":"stream", "kind":"live_sse", "source":"http://127.0.0.1:8098/rollouts/r1/stream"}]
        }))
        .unwrap();
    }

    /// The shape that rendered an empty pane during the v0.4 CUA acceptance
    /// run: ten correct descriptors filed under a slot key instead of the
    /// canonical envelope. Accepting it silently is what cost that run.
    #[test]
    fn upgrades_the_slot_keyed_map_that_rendered_nothing() {
        let authored = json!({
            "stream": (0..10)
                .map(|index| json!({
                    "slot": "stream",
                    "kind": "live_sse",
                    "source": format!("http://127.0.0.1:8114/rollouts/roll_{index}/stream"),
                    "poll_url": format!("http://127.0.0.1:8114/rollouts/roll_{index}/events"),
                    "schema": "synth.trace-stream-event.v1",
                }))
                .collect::<Vec<_>>()
        });

        let canonical = canonicalize_bindings(&authored).unwrap();

        assert_eq!(canonical.form, BindingsForm::UpgradedSlotMap);
        assert!(canonical.form.is_upgrade());
        assert_eq!(canonical.upgraded_slots, vec!["stream".to_string()]);
        assert_eq!(
            canonical.value["schemaVersion"],
            json!(VISUAL_BINDINGS_SCHEMA_VERSION)
        );
        let slots = canonical.value["slots"].as_array().unwrap();
        assert_eq!(slots.len(), 10);
        assert!(slots
            .iter()
            .all(|slot| slot["slot"] == json!("stream") && slot["kind"] == json!("live_sse")));
        assert_eq!(declared_poll_urls(&authored).len(), 10);
    }

    /// The same shape again, but read verbatim from the failing run rather than
    /// reconstructed here. A hand-written fixture can drift towards the code it
    /// tests; this one cannot.
    #[test]
    fn upgrades_the_recorded_v04_acceptance_bindings() {
        let authored: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/v04_cua_slot_map_bindings.json"
        ))
        .expect("recorded v0.4 bindings fixture");

        let canonical = canonicalize_bindings(&authored).unwrap();

        assert_eq!(canonical.form, BindingsForm::UpgradedSlotMap);
        assert_eq!(canonical.value["slots"].as_array().unwrap().len(), 10);
        assert_eq!(declared_poll_urls(&authored).len(), 10);
        // Every declared poll authority is distinct: one cursor per rollout.
        let urls = declared_poll_urls(&authored);
        assert_eq!(
            urls.iter().collect::<std::collections::BTreeSet<_>>().len(),
            10
        );
    }

    #[test]
    fn upgrades_a_single_descriptor_and_takes_the_slot_key_as_authoritative() {
        let canonical = canonicalize_bindings(&json!({
            "stream": {
                "slot": "primary",
                "kind": "live_sse",
                "source": "http://127.0.0.1:8114/rollouts/r1/stream",
                "poll_url": "http://127.0.0.1:8114/rollouts/r1/events"
            }
        }))
        .unwrap();
        assert_eq!(canonical.form, BindingsForm::UpgradedSlotMap);
        assert_eq!(canonical.value["slots"][0]["slot"], json!("stream"));
    }

    #[test]
    fn wraps_a_legacy_prop_bag_as_inline_slots() {
        let canonical = canonicalize_bindings(&json!({"matrix": [1, 2], "title": "x"})).unwrap();
        assert_eq!(canonical.form, BindingsForm::UpgradedPropBag);
        let slots = canonical.value["slots"].as_array().unwrap();
        assert_eq!(slots.len(), 2);
        assert!(slots.iter().all(|slot| slot["kind"] == json!("inline")));
        assert!(slots
            .iter()
            .any(|slot| slot["slot"] == json!("matrix") && slot["data"] == json!([1, 2])));
    }

    #[test]
    fn keeps_inline_chart_data_inline() {
        // `kind` alone must not read as a binding descriptor, or every chart
        // spec with a `kind` field would be reinterpreted as a transport.
        let canonical = canonicalize_bindings(&json!({"chart": {"kind": "bar"}})).unwrap();
        assert_eq!(canonical.form, BindingsForm::UpgradedPropBag);
        assert_eq!(canonical.value["slots"][0]["kind"], json!("inline"));
    }

    #[test]
    fn refuses_shapes_it_cannot_read_either_way() {
        // Mixed descriptor and inline data: unreadable both ways, so refuse.
        assert!(canonicalize_bindings(&json!({
            "stream": {"kind": "live_sse", "source": "http://127.0.0.1:8114/rollouts/r1/stream"},
            "notes": [1, 2, 3]
        }))
        .is_err());
        assert!(canonicalize_bindings(&json!([])).is_err());
        assert!(
            canonicalize_bindings(&json!({"schemaVersion": "synth.visual-bindings.v2"})).is_err()
        );
        assert!(
            canonicalize_bindings(&json!({"schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION}))
                .is_err()
        );
    }

    #[test]
    fn empty_bindings_are_canonical_not_an_upgrade() {
        let canonical = canonicalize_bindings(&json!({})).unwrap();
        assert_eq!(canonical.form, BindingsForm::Canonical);
        assert_eq!(canonical.value["slots"], json!([]));
    }

    #[test]
    fn declared_poll_urls_reads_canonical_and_upgraded_alike() {
        let canonical = json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{
                "slot": "stream",
                "kind": "live_sse",
                "source": "http://127.0.0.1:8114/rollouts/r1/stream",
                "poll_url": "http://127.0.0.1:8114/rollouts/r1/events"
            }]
        });
        assert_eq!(
            declared_poll_urls(&canonical),
            vec!["http://127.0.0.1:8114/rollouts/r1/events".to_string()]
        );
        assert!(declared_poll_urls(&json!({"matrix": []})).is_empty());
    }
}
