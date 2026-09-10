//! Transport-independent visual operations, incrementally migrated from IPC.

use crate::contract::capabilities::PublicOperation;
use crate::visuals::{
    TemplateMeta, VisualCreateRequest, VisualQuery, VisualRecord, VisualRegistry,
    VisualUpdateRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListVisualTemplatesRequest {
    /// Optional case-insensitive genre or template-ID substring filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
pub struct ListVisualTemplatesResult {
    pub templates: Vec<TemplateMeta>,
}

pub struct ListVisualTemplates;

impl PublicOperation for ListVisualTemplates {
    type Request = ListVisualTemplatesRequest;
    type Response = ListVisualTemplatesResult;

    const ID: &'static str = "visuals.templates.list.v1";
    const MCP_NAME: &'static str = "visual_list_templates";
    const DESCRIPTION: &'static str = "List registered Workshop visual templates. Optionally filter by case-insensitive genre or template-ID substring. Returns the actual template contracts, input bindings, and observation requirements.";
    const READ_ONLY: bool = true;
    const DESTRUCTIVE: bool = false;
    const IDEMPOTENT: bool = true;
}

impl ListVisualTemplates {
    /// The catalogue belongs to the connected instance. Existing authenticated
    /// IPC / desktop entry points retain their access checks during migration.
    pub(crate) fn execute(
        registry: &VisualRegistry,
        request: ListVisualTemplatesRequest,
    ) -> anyhow::Result<ListVisualTemplatesResult> {
        Ok(ListVisualTemplatesResult {
            templates: registry.list_templates(request.genre.as_deref())?,
        })
    }
}


#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListRequest {
    pub search: Option<String>,
    pub offset: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListResult {
    pub visuals: Vec<VisualRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetRequest {
    pub visual_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VisualResult {
    pub visual: VisualRecord,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub template_id: String,
    pub title: String,
    /// Object envelope {"schemaVersion":"synth.visual-bindings.v1","inputs":[{"input":"stream","kind":"live_sse","source":"...","poll_url":"..."}]}. Never pass a bare array. Consult visual_list_templates for input names.
    #[schemars(with = "Option<serde_json::Map<String, Value>>")]
    pub bindings: Option<Value>,
    /// Source for source-authored templates, validated by the visual registry.
    pub content: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    pub visual_id: String,
    pub expected_revision: i64,
    pub title: Option<String>,
    pub bindings: Option<Value>,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CaptureResult {
    pub visual_id: String,
    pub revision: i64,
    /// Native host capture receipt: path, digest, viewport, and observed app state.
    pub receipt: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresentRequest {
    pub visual_id: String,
    /// Expand the native Workshop window to full screen. False exits full screen.
    #[serde(default)]
    pub fullscreen: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PresentResult {
    pub visual_id: String,
    pub revision: i64,
    /// Presentation was requested; use visual_observe to inspect renderer evidence.
    pub requested: bool,
    /// The window's own fullscreen state once the transition settled, not the
    /// state that was asked for. Compare it with `app_capture`'s
    /// `windowFullscreen`; they are read the same way.
    pub fullscreen: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObserveResult {
    pub visual_id: String,
    pub revision: i64,
    /// Null means the renderer has not published an observation for this visual.
    pub observation: Option<Value>,
}

macro_rules! operation {
    ($name:ident, $req:ty, $res:ty, $id:literal, $tool:literal, $desc:literal, $read:expr, $idem:expr) => {
        pub struct $name;
        impl PublicOperation for $name {
            type Request = $req;
            type Response = $res;
            const ID: &'static str = $id;
            const MCP_NAME: &'static str = $tool;
            const DESCRIPTION: &'static str = $desc;
            const READ_ONLY: bool = $read;
            const DESTRUCTIVE: bool = false;
            const IDEMPOTENT: bool = $idem;
        }
    };
}

operation!(ListVisuals, ListRequest, ListResult, "visuals.list.v1", "visual_list", "List visuals in the explicitly connected local Workshop instance. Includes shared library and conversation visuals. Results are bounded to 100; paginate with offset.", true, true);
operation!(
    GetVisual,
    GetRequest,
    VisualResult,
    "visuals.get.v1",
    "visual_get",
    "Read a Workshop visual, including its current revision, template, bindings and provenance.",
    true,
    true
);
operation!(CreateVisual, CreateRequest, VisualResult, "visuals.create_shared.v1", "visual_create", "Create a shared visual in the connected Workshop instance using a registered template. No chat is created. Read the template contract first. Source and binding validation are enforced by Workshop. Creation is not idempotent: inspect existing results before retrying an uncertain response.", false, false);
operation!(PresentVisual, PresentRequest, PresentResult, "visuals.present.v1", "visual_present", "Open a visual in Workshop's visual library and focus its preview, optionally full screen. This requests presentation; it does not certify that rendering completed. It may navigate the Workshop window.", false, true);
operation!(ObserveVisual, GetRequest, ObserveResult, "visuals.observe.v1", "visual_observe", "Read the renderer's observation of a visual. Null means no observation; compare any observed revision with the current revision. An observation is not a screenshot and does not prove visual quality.", true, true);
operation!(UpdateVisual, UpdateRequest, VisualResult, "visuals.update.v1", "visual_update", "Update a visual's title, bindings or source at an expected revision. Conflicts reject the entire write; read current state and reconcile before retrying. Ownership and provenance cannot be reassigned.", false, false);
operation!(CaptureVisual, GetRequest, CaptureResult, "visuals.capture.v1", "visual_capture", "Capture the actual visual through Workshop's native WebView snapshot. Returns a PNG image and native receipt. Temporarily focuses the visual; fails if the revision changes during capture. Requires macOS and a running renderer.", false, false);

impl ListVisuals {
    pub async fn execute(
        registry: &VisualRegistry,
        request: ListRequest,
    ) -> anyhow::Result<ListResult> {
        let limit = request.limit.unwrap_or(50);
        anyhow::ensure!(
            (1..=100).contains(&limit),
            "limit must be between 1 and 100"
        );
        Ok(ListResult {
            visuals: registry
                .list(VisualQuery {
                    search: request.search,
                    limit: Some(i64::from(limit)),
                    offset: Some(i64::from(request.offset.unwrap_or(0))),
                    ..Default::default()
                })
                .await?,
        })
    }
}

impl GetVisual {
    pub async fn execute(
        registry: &VisualRegistry,
        request: GetRequest,
    ) -> anyhow::Result<VisualResult> {
        Ok(VisualResult {
            visual: registry.get(request.visual_id).await?,
        })
    }
}

impl CreateVisual {
    pub async fn execute(
        registry: &VisualRegistry,
        request: CreateRequest,
    ) -> anyhow::Result<(VisualResult, Value)> {
        anyhow::ensure!(!request.title.trim().is_empty(), "title must not be empty");
        let (visual, event) = registry
            .create_shared(VisualCreateRequest {
                template_id: request.template_id,
                title: Some(request.title),
                bindings: request.bindings,
                content: request.content,
                source_agent_id: Some("workshop-mcp".into()),
                id: None,
                status: None,
                renderer_kind: None,
                session_id: None,
                message_id: None,
                run_id: None,
                trace_id: None,
                parent_visual_id: None,
                source_model: None,
                metadata: None,
            })
            .await?;
        Ok((VisualResult { visual }, event))
    }
}

impl UpdateVisual {
    pub async fn execute(
        registry: &VisualRegistry,
        request: UpdateRequest,
    ) -> anyhow::Result<(VisualResult, Value)> {
        anyhow::ensure!(
            request.expected_revision > 0,
            "expected_revision must be positive"
        );
        anyhow::ensure!(
            request.title.is_some() || request.bindings.is_some() || request.content.is_some(),
            "provide title, bindings, or content"
        );
        if let Some(title) = &request.title {
            anyhow::ensure!(!title.trim().is_empty(), "title must not be empty");
        }
        let (visual, event) = registry
            .update_at_revision(
                request.visual_id,
                VisualUpdateRequest {
                    title: request.title,
                    bindings: request.bindings,
                    content: request.content,
                    status: None,
                    renderer_kind: None,
                    message_id: None,
                    run_id: None,
                    trace_id: None,
                    metadata: None,
                    bump_revision: Some(true),
                },
                Some(request.expected_revision),
            )
            .await?;
        Ok((VisualResult { visual }, event))
    }
}

