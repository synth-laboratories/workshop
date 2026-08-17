use super::mermaid::{self, Theme};
use super::models::{
    canonicalize_bindings, BindingsForm, RendererKind, VisualCreateRequest, VisualQuery,
    VisualRecord, VisualRevision, VisualStatus, VisualUpdateRequest, VISUAL_SCHEMA_VERSION,
};
use super::renditions::{self, VisualAsset, VisualRendition};
use super::systems::{self, SystemsKind};
use super::templates::{resolve_template, TemplateMeta};
use crate::storage::{ContentStore, Database, EventAppend, EventJournal, EventSource};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct VisualRegistry {
    pub(super) db: Arc<Database>,
    pub(super) journal: EventJournal,
    pub(super) content: ContentStore,
    /// Attached by the composition root once diagnostics exist.
    ///
    /// `visual.render_failed` already lands in the journal as a domain event,
    /// but a domain event is scoped to the visual. The diagnostic is what puts
    /// that failure next to the rollout, stream, and container it belongs to.
    pub(super) diagnostics: Arc<std::sync::OnceLock<Arc<crate::diagnostics::DiagnosticsService>>>,
}

impl VisualRegistry {
    pub fn new(db: Arc<Database>, journal: EventJournal, content: ContentStore) -> Self {
        Self {
            db,
            journal,
            content,
            diagnostics: Arc::new(std::sync::OnceLock::new()),
        }
    }

    pub(crate) fn content(&self) -> &ContentStore {
        &self.content
    }

    /// Wire diagnostics in after both services exist. Idempotent.
    pub fn attach_diagnostics(&self, service: Arc<crate::diagnostics::DiagnosticsService>) {
        let _ = self.diagnostics.set(service);
    }

    /// Record a visual-surface failure with everything the record already knows.
    fn diagnose_visual(
        &self,
        visual: &VisualRecord,
        event: &str,
        code: &str,
        message: &str,
        details: serde_json::Value,
    ) {
        let Some(service) = self.diagnostics.get() else {
            return;
        };
        let mut input = crate::diagnostics::DiagnosticInput::new(
            crate::diagnostics::Severity::Error,
            "visual-registry",
            event,
            code,
            message,
        );
        input.correlation.visual_id = Some(visual.id.clone());
        input.correlation.visual_revision = Some(visual.current_revision);
        input.correlation.session_id = visual.session_id.clone();
        input.correlation.rollout_id = visual.run_id.clone();
        input.correlation.trace_id = visual.trace_id.clone();
        if let Some(object) = details.as_object() {
            input.details = object.clone();
        }
        input.details.insert(
            "template_id".into(),
            serde_json::json!(visual.template_id.clone()),
        );
        service.emit(input);
    }

    /// Report that a writer sent bindings in a shape this build had to upgrade.
    ///
    /// COMPAT: loud on purpose. A silently accepted legacy shape is how ten
    /// declared streams became an empty pane with no error — the upgrade has to
    /// be visible to an operator long before a rendered acceptance fails.
    fn report_bindings_upgrade(
        &self,
        visual_id: &str,
        revision: i64,
        session_id: Option<&str>,
        template_id: &str,
        form: &BindingsForm,
        upgraded_slots: &[String],
    ) {
        if !form.is_upgrade() {
            return;
        }
        eprintln!(
            "synth-desktop: upgraded {} visual bindings for {visual_id} rev {revision} \
             (template {template_id}, slots {upgraded_slots:?}); writers must send {}",
            form.as_str(),
            super::models::VISUAL_BINDINGS_SCHEMA_VERSION
        );
        let Some(service) = self.diagnostics.get() else {
            return;
        };
        let mut input = crate::diagnostics::DiagnosticInput::new(
            crate::diagnostics::Severity::Warn,
            "visual-registry",
            "visual.bindings.upgraded",
            crate::diagnostics::codes::VISUAL_BINDINGS_UPGRADED,
            format!(
                "upgraded {} visual bindings to {}",
                form.as_str(),
                super::models::VISUAL_BINDINGS_SCHEMA_VERSION
            ),
        );
        input.correlation.visual_id = Some(visual_id.to_string());
        input.correlation.visual_revision = Some(revision);
        input.correlation.session_id = session_id.map(str::to_string);
        input
            .details
            .insert("template_id".into(), json!(template_id));
        input.details.insert("form".into(), json!(form.as_str()));
        // Slot names come from a template contract, not from free text, so the
        // cardinality here is bounded by the template's declared slots.
        input.details.insert("slots".into(), json!(upgraded_slots));
        service.emit(input);
    }

    pub async fn list(&self, query: VisualQuery) -> Result<Vec<VisualRecord>> {
        let db = self.db.clone();
        db.run(move |conn| list_visuals(conn, &query)).await
    }

    pub async fn get(&self, id: String) -> Result<VisualRecord> {
        let db = self.db.clone();
        db.run(move |conn| load_visual(conn, &id)).await
    }

    pub async fn revisions(&self, id: String) -> Result<Vec<VisualRevision>> {
        let db = self.db.clone();
        db.run(move |conn| list_revisions(conn, &id)).await
    }

    pub async fn create(&self, request: VisualCreateRequest) -> Result<(VisualRecord, Value)> {
        let template = resolve_template(&request.template_id)?;
        let title = request
            .title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| template.title.clone());
        let id = request
            .id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("vis_{}", Uuid::new_v4().simple()));
        validate_visual_id(&id)?;
        let authored_bindings = request.bindings.unwrap_or_else(|| json!({}));
        let canonical = canonicalize_bindings(&authored_bindings)?;
        let bindings_form = canonical.form.clone();
        let upgraded_slots = canonical.upgraded_slots.clone();
        let bindings = canonical.value;
        let is_mermaid = mermaid::is_mermaid_template(&template.id);
        let systems_kind = systems::template_kind(&template.id);
        if is_mermaid {
            let source = request
                .content
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("diagram.mermaid.v1 requires content"))?;
            mermaid::validate_source(source)?;
            refuse_mermaid_stream_slot(&bindings)?;
        }
        if let Some(kind) = systems_kind {
            let source = request
                .content
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("{} requires content", template.id))?;
            systems::validate_source(source, kind)?;
            refuse_mermaid_stream_slot(&bindings)?;
        }
        let status = request.status.unwrap_or(VisualStatus::Draft);
        let renderer_kind = if is_mermaid {
            RendererKind::Mermaid
        } else if systems_kind == Some(SystemsKind::Static) {
            RendererKind::Systems
        } else if systems_kind == Some(SystemsKind::Dynamic) {
            RendererKind::SystemsDynamic
        } else {
            request.renderer_kind.unwrap_or(RendererKind::Template)
        };
        let mut metadata = request.metadata.unwrap_or_else(|| json!({}));
        if is_mermaid {
            if let Some(object) = metadata.as_object_mut() {
                object
                    .entry("presentation")
                    .or_insert_with(|| json!("pane"));
                object.insert("mediaType".into(), json!(mermaid::MEDIA_TYPE_SOURCE));
                object.insert("renderStatus".into(), json!("queued"));
                object.insert("rendererVersion".into(), json!(mermaid::RENDERER_VERSION));
            }
        }
        if let Some(kind) = systems_kind {
            let scene =
                systems::parse_and_validate(request.content.as_deref().unwrap_or_default(), kind)?;
            if let Some(object) = metadata.as_object_mut() {
                object
                    .entry("presentation")
                    .or_insert_with(|| json!("pane"));
                object.insert("mediaType".into(), json!(systems::MEDIA_TYPE_SOURCE));
                object.insert("renderStatus".into(), json!("queued"));
                object.insert("rendererVersion".into(), json!(systems::RENDERER_VERSION));
                object.insert("visualKind".into(), json!(kind.as_str()));
                if kind == SystemsKind::Dynamic {
                    object.insert("durationMs".into(), json!(scene.duration_ms));
                    object.insert("posterTimeMs".into(), json!(scene.poster_time_ms));
                    object.insert("beatCount".into(), json!(scene.beats.len()));
                    object.insert("reducedMotion".into(), json!(scene.reduced_motion));
                }
            }
        }
        let content_digest = if let Some(content) = request.content.as_ref() {
            Some(self.content.put_bytes("blobs", content.as_bytes())?)
        } else {
            None
        };
        let bindings_digest = Some(digest_json(&bindings));
        let now = Utc::now().to_rfc3339();
        let record = VisualRecord {
            schema_version: VISUAL_SCHEMA_VERSION.to_string(),
            id: id.clone(),
            current_revision: 1,
            title: title.clone(),
            template_id: template.id.clone(),
            status,
            renderer_kind,
            bindings: bindings.clone(),
            session_id: request.session_id.clone(),
            message_id: request.message_id.clone(),
            run_id: request.run_id.clone(),
            trace_id: request.trace_id.clone(),
            parent_visual_id: request.parent_visual_id.clone(),
            source_agent_id: request.source_agent_id.clone(),
            source_model: request.source_model.clone(),
            content_digest: content_digest.clone(),
            preview_digest: None,
            metadata,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        let db = self.db.clone();
        let inserted = record.clone();
        let (record, event) = db
            .run_transaction(move |conn| {
                if let Some(session_id) = inserted.session_id.as_ref() {
                    ensure_session(conn, session_id)?;
                }
                insert_visual(conn, &inserted)?;
                insert_revision(
                    conn,
                    &VisualRevision {
                        visual_id: inserted.id.clone(),
                        revision: 1,
                        template_id: inserted.template_id.clone(),
                        renderer_kind: inserted.renderer_kind.clone(),
                        content_digest: content_digest.clone(),
                        bindings_digest,
                        bindings: Some(bindings),
                        preview_digest: None,
                        author_agent_id: inserted.source_agent_id.clone(),
                        parent_revision: None,
                        created_at: now,
                    },
                )?;
                let event = crate::storage::append_event(
                    conn,
                    EventAppend {
                        event_id: None,
                        session_id: inserted.session_id.clone(),
                        run_id: inserted.run_id.clone(),
                        source: EventSource::Visual,
                        kind: "visual.created".into(),
                        payload: json!({
                            "visualId": inserted.id,
                            "revision": inserted.current_revision,
                            "title": inserted.title,
                            "templateId": inserted.template_id,
                            "status": inserted.status.as_str(),
                        }),
                        remote_sequence: None,
                        command_id: None,
                        created_at: None,
                    },
                )?;
                Ok((inserted, event))
            })
            .await?;
        self.report_bindings_upgrade(
            &record.id,
            record.current_revision,
            record.session_id.as_deref(),
            &record.template_id,
            &bindings_form,
            &upgraded_slots,
        );
        if mermaid::is_mermaid_template(&record.template_id) {
            let rendered = self.render_mermaid(&record.id).await?;
            return Ok((rendered, serde_json::to_value(event)?));
        }
        if systems::template_kind(&record.template_id).is_some() {
            let rendered = self.render_systems(&record.id).await?;
            return Ok((rendered, serde_json::to_value(event)?));
        }
        Ok((record, serde_json::to_value(event)?))
    }

    pub async fn update(
        &self,
        id: String,
        request: VisualUpdateRequest,
    ) -> Result<(VisualRecord, Value)> {
        validate_visual_id(&id)?;
        let content_changed = request.content.is_some();
        if let Some(source) = request.content.as_deref() {
            let existing = self.get(id.clone()).await?;
            if mermaid::is_mermaid_template(&existing.template_id) {
                mermaid::validate_source(source)?;
            }
            if let Some(kind) = systems::template_kind(&existing.template_id) {
                systems::validate_source(source, kind)?;
            }
        }
        // Canonicalise before anything reads the bindings. The mermaid and
        // systems guards below match on the canonical slots array, so a legacy
        // shape used to walk straight past them.
        let mut request = request;
        let mut bindings_form = BindingsForm::Canonical;
        let mut upgraded_slots = Vec::new();
        if let Some(bindings) = request.bindings.as_ref() {
            let canonical = canonicalize_bindings(bindings)?;
            bindings_form = canonical.form.clone();
            upgraded_slots = canonical.upgraded_slots.clone();
            request.bindings = Some(canonical.value);
        }
        if let Some(bindings) = request.bindings.as_ref() {
            let existing = self.get(id.clone()).await?;
            if mermaid::is_mermaid_template(&existing.template_id) {
                refuse_mermaid_stream_slot(bindings)?;
            }
            if systems::template_kind(&existing.template_id).is_some() {
                refuse_mermaid_stream_slot(bindings)?;
            }
        }
        let db = self.db.clone();
        let content = self.content.clone();
        let (updated, event) = db
            .run_transaction(move |conn| {
                let mut current = load_visual(conn, &id)?;
                let mut bumped = false;
                let bump = request.bump_revision.unwrap_or(true);
                if let Some(title) = request.title {
                    current.title = title;
                }
                if let Some(status) = request.status {
                    current.status = status;
                }
                if let Some(renderer_kind) = request.renderer_kind {
                    current.renderer_kind = renderer_kind;
                }
                if let Some(message_id) = request.message_id {
                    current.message_id = Some(message_id);
                }
                if let Some(run_id) = request.run_id {
                    current.run_id = Some(run_id);
                }
                if let Some(trace_id) = request.trace_id {
                    current.trace_id = Some(trace_id);
                }
                if let Some(metadata) = request.metadata {
                    current.metadata = metadata;
                    bumped = true;
                }
                let mut new_bindings = None;
                if let Some(bindings) = request.bindings {
                    // Already canonical: `update` canonicalises before the
                    // transaction so every guard above reads the same shape.
                    current.bindings = bindings.clone();
                    new_bindings = Some(bindings);
                    bumped = true;
                }
                let mut content_digest = current.content_digest.clone();
                if let Some(body) = request.content {
                    content_digest = Some(content.put_bytes("blobs", body.as_bytes())?);
                    current.content_digest = content_digest.clone();
                    bumped = true;
                }
                current.updated_at = Utc::now().to_rfc3339();
                if bump && bumped {
                    let parent = current.current_revision;
                    current.current_revision += 1;
                    insert_revision(
                        conn,
                        &VisualRevision {
                            visual_id: current.id.clone(),
                            revision: current.current_revision,
                            template_id: current.template_id.clone(),
                            renderer_kind: current.renderer_kind.clone(),
                            content_digest,
                            bindings_digest: Some(digest_json(&current.bindings)),
                            bindings: new_bindings.or_else(|| Some(current.bindings.clone())),
                            preview_digest: current.preview_digest.clone(),
                            author_agent_id: current.source_agent_id.clone(),
                            parent_revision: Some(parent),
                            created_at: current.updated_at.clone(),
                        },
                    )?;
                }
                persist_visual(conn, &current)?;
                let event = crate::storage::append_event(
                    conn,
                    EventAppend {
                        event_id: None,
                        session_id: current.session_id.clone(),
                        run_id: current.run_id.clone(),
                        source: EventSource::Visual,
                        kind: "visual.updated".into(),
                        payload: json!({
                            "visualId": current.id,
                            "revision": current.current_revision,
                            "title": current.title,
                            "status": current.status.as_str(),
                        }),
                        remote_sequence: None,
                        command_id: None,
                        created_at: None,
                    },
                )?;
                Ok((current, event))
            })
            .await?;
        self.report_bindings_upgrade(
            &updated.id,
            updated.current_revision,
            updated.session_id.as_deref(),
            &updated.template_id,
            &bindings_form,
            &upgraded_slots,
        );
        if content_changed && mermaid::is_mermaid_template(&updated.template_id) {
            let rendered = self.render_mermaid(&updated.id).await?;
            return Ok((rendered, serde_json::to_value(event)?));
        }
        if content_changed && systems::template_kind(&updated.template_id).is_some() {
            let rendered = self.render_systems(&updated.id).await?;
            return Ok((rendered, serde_json::to_value(event)?));
        }
        Ok((updated, serde_json::to_value(event)?))
    }

    pub async fn save(&self, id: String, tsx: Option<String>) -> Result<(VisualRecord, Value)> {
        let current = self.get(id.clone()).await?;
        let body = tsx.unwrap_or_else(|| default_tsx_stub(&current));
        let digest = self.content.put_bytes("blobs", body.as_bytes())?;
        let mut metadata = current.metadata.clone();
        if let Some(object) = metadata.as_object_mut() {
            object.insert("tsxDigest".into(), Value::String(digest.clone()));
            object.insert(
                "tsxPath".into(),
                Value::String(format!("store/blobs/{}/{}", &digest[..2], digest)),
            );
        }
        self.update(
            id,
            VisualUpdateRequest {
                title: None,
                bindings: None,
                status: Some(VisualStatus::Saved),
                renderer_kind: Some(RendererKind::Tsx),
                message_id: None,
                run_id: None,
                trace_id: None,
                content: Some(body),
                metadata: Some(metadata),
                bump_revision: Some(true),
            },
        )
        .await
    }

    pub async fn fork(
        &self,
        id: String,
        title: Option<String>,
        session_id: Option<String>,
    ) -> Result<(VisualRecord, Value)> {
        let source = self.get(id.clone()).await?;
        self.create(VisualCreateRequest {
            template_id: source.template_id,
            title: Some(title.unwrap_or_else(|| format!("{} (fork)", source.title))),
            bindings: Some(source.bindings),
            id: None,
            status: Some(VisualStatus::Draft),
            renderer_kind: Some(source.renderer_kind),
            session_id: session_id.or(source.session_id),
            message_id: None,
            run_id: source.run_id,
            trace_id: source.trace_id,
            parent_visual_id: Some(id),
            source_agent_id: source.source_agent_id,
            source_model: source.source_model,
            content: source
                .content_digest
                .as_ref()
                .and_then(|digest| self.content.get_bytes("blobs", digest).ok())
                .and_then(|bytes| String::from_utf8(bytes).ok()),
            metadata: Some(json!({
                "forkedFrom": source.id,
                "forkedRevision": source.current_revision,
            })),
        })
        .await
    }

    pub async fn archive(&self, id: String) -> Result<(VisualRecord, Value)> {
        self.update(
            id,
            VisualUpdateRequest {
                title: None,
                bindings: None,
                status: Some(VisualStatus::Archived),
                renderer_kind: None,
                message_id: None,
                run_id: None,
                trace_id: None,
                content: None,
                metadata: None,
                bump_revision: Some(false),
            },
        )
        .await
    }

    pub async fn show(
        &self,
        id: String,
        session_id: Option<String>,
    ) -> Result<(VisualRecord, Value)> {
        let record = self.get(id).await?;
        let event = self
            .journal
            .append(EventAppend {
                event_id: None,
                session_id: session_id.or_else(|| record.session_id.clone()),
                run_id: record.run_id.clone(),
                source: EventSource::Visual,
                kind: "visual.show".into(),
                payload: json!({
                    "visualId": record.id,
                    "revision": record.current_revision,
                    "title": record.title,
                    "templateId": record.template_id,
                    // Who *owns* this visual, which is not who opened it. The
                    // registry is instance-global: without this, a chat that
                    // displayed another chat's visual could not be told apart
                    // from the chat that authored it.
                    "ownerSessionId": record.session_id,
                }),
                remote_sequence: None,
                command_id: None,
                created_at: None,
            })
            .await?;
        Ok((record, serde_json::to_value(event)?))
    }

    pub fn list_templates(&self, genre: Option<&str>) -> Result<Vec<TemplateMeta>> {
        super::templates::list_templates(genre)
    }

    pub fn get_template(&self, template_id: &str) -> Result<TemplateMeta> {
        resolve_template(template_id)
    }

    pub async fn mermaid_source(&self, id: String) -> Result<VisualAsset> {
        self.visual_source(id).await
    }

    pub async fn visual_source(&self, id: String) -> Result<VisualAsset> {
        let visual = self.get(id).await?;
        let media_type = if mermaid::is_mermaid_template(&visual.template_id) {
            mermaid::MEDIA_TYPE_SOURCE
        } else if systems::template_kind(&visual.template_id).is_some() {
            systems::MEDIA_TYPE_SOURCE
        } else {
            bail!(
                "visual {} does not expose canonical renderer source",
                visual.id
            )
        };
        let digest = visual
            .content_digest
            .clone()
            .ok_or_else(|| anyhow!("visual is missing canonical source"))?;
        let bytes = self.content.get_bytes("blobs", &digest)?;
        Ok(VisualAsset {
            visual_id: visual.id,
            revision: visual.current_revision,
            format: "source".into(),
            media_type: media_type.into(),
            theme: None,
            size_class: None,
            digest,
            base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            width_px: None,
            height_px: None,
        })
    }

    pub async fn list_renditions(&self, id: String) -> Result<Vec<VisualRendition>> {
        let visual = self.get(id.clone()).await?;
        let db = self.db.clone();
        let revision = visual.current_revision;
        db.run(move |conn| renditions::list_renditions(conn, &id, revision))
            .await
    }

    pub async fn mermaid_rendition(
        &self,
        id: String,
        format: Option<String>,
        theme: Option<String>,
        size_class: Option<String>,
    ) -> Result<VisualAsset> {
        self.visual_rendition(id, format, theme, size_class).await
    }

    pub async fn visual_rendition(
        &self,
        id: String,
        format: Option<String>,
        theme: Option<String>,
        size_class: Option<String>,
    ) -> Result<VisualAsset> {
        let visual = self.get(id.clone()).await?;
        let systems_kind = systems::template_kind(&visual.template_id);
        if !mermaid::is_mermaid_template(&visual.template_id) && systems_kind.is_none() {
            bail!("visual {} has no SVG rendition renderer", visual.id);
        }
        let format = if format.as_deref() == Some("png") {
            "svg".to_string()
        } else {
            format.unwrap_or_else(|| "svg".into())
        };
        let theme = match theme {
            Some(value) => value,
            None if systems_kind.is_some() => {
                let bytes = visual
                    .content_digest
                    .as_deref()
                    .and_then(|d| self.content.get_bytes("blobs", d).ok())
                    .unwrap_or_default();
                serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .and_then(|v| {
                        v.get("theme").and_then(Value::as_str).map(|s| {
                            if s == "technical-dark" {
                                "dark".to_string()
                            } else {
                                "light".to_string()
                            }
                        })
                    })
                    .unwrap_or_else(|| "light".into())
            }
            None => "light".into(),
        };
        if !matches!(theme.as_str(), "light" | "dark") {
            bail!("unsupported rendition theme {theme}");
        }
        let renderer_version = if systems_kind.is_some() {
            systems::RENDERER_VERSION
        } else {
            mermaid::RENDERER_VERSION
        };
        let size_class = size_class.unwrap_or_else(|| "pane".into());
        let db = self.db.clone();
        let revision = visual.current_revision;
        let visual_id = visual.id.clone();
        let format_key = format.clone();
        let theme_key = theme.clone();
        let size_key = size_class.clone();
        let rendition = db
            .run(move |conn| {
                renditions::get_rendition_for_renderer(
                    conn,
                    &visual_id,
                    revision,
                    &format_key,
                    &theme_key,
                    &size_key,
                    renderer_version,
                )
            })
            .await;
        let rendition = match rendition {
            Ok(value) => value,
            Err(_) => {
                self.render_visual(&id).await?;
                let visual_id = id.clone();
                let format_key = format.clone();
                let theme_key = theme.clone();
                let size_key = size_class.clone();
                self.db
                    .run(move |conn| {
                        renditions::get_rendition_for_renderer(
                            conn,
                            &visual_id,
                            revision,
                            &format_key,
                            &theme_key,
                            &size_key,
                            renderer_version,
                        )
                    })
                    .await?
            }
        };
        let bytes = self
            .content
            .get_bytes("previews", &rendition.content_digest)?;
        Ok(VisualAsset {
            visual_id: rendition.visual_id,
            revision: rendition.revision,
            format: rendition.format,
            media_type: rendition.media_type,
            theme: Some(rendition.theme),
            size_class: Some(rendition.size_class),
            digest: rendition.content_digest,
            base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            width_px: rendition.width_px,
            height_px: rendition.height_px,
        })
    }

    pub async fn render_visual(&self, id: &str) -> Result<VisualRecord> {
        let visual = self.get(id.to_string()).await?;
        if mermaid::is_mermaid_template(&visual.template_id) {
            self.render_mermaid(id).await
        } else if systems::template_kind(&visual.template_id).is_some() {
            self.render_systems(id).await
        } else {
            // Native rendering is for deterministic source-to-SVG templates.
            // Every other template is a React shell that only exists once the
            // Desktop pane renders it, so "no dedicated renderer" is not a
            // defect to retry around — it is the wrong tool. Name the right one,
            // with a code the tool-loop breaker can tell apart from a fault.
            Err(anyhow!(crate::error::StructuredFailure::new(
                "visual_renderer_not_native",
                format!(
                    "{} renders in the Desktop pane, not through a native renderer",
                    visual.template_id
                ),
                "Show the visual in Desktop and use capture_review to produce review evidence; render is only for mermaid and systems diagrams.",
            )
            .with_details(json!({"visualId": id, "templateId": visual.template_id}))))
        }
    }

    pub async fn render_mermaid(&self, id: &str) -> Result<VisualRecord> {
        let visual = self.get(id.to_string()).await?;
        if !mermaid::is_mermaid_template(&visual.template_id) {
            bail!("visual {id} is not a mermaid diagram");
        }
        let digest = visual
            .content_digest
            .clone()
            .ok_or_else(|| anyhow!("diagram.mermaid.v1 requires content"))?;
        let bytes = self.content.get_bytes("blobs", &digest)?;
        let source = String::from_utf8(bytes).context("mermaid source must be UTF-8")?;
        let kind = mermaid::validate_source(&source)?;
        let _ = self
            .journal
            .append(EventAppend {
                event_id: None,
                session_id: visual.session_id.clone(),
                run_id: visual.run_id.clone(),
                source: EventSource::Visual,
                kind: "visual.render_requested".into(),
                payload: json!({
                    "visualId": visual.id,
                    "revision": visual.current_revision,
                    "diagramKind": kind.as_str(),
                }),
                remote_sequence: None,
                command_id: None,
                created_at: None,
            })
            .await;
        let source_for_render = source.clone();
        let rendered = tokio::task::spawn_blocking(move || {
            mermaid::render_isolated(&source_for_render, Theme::Light)
        })
        .await
        .context("join mermaid render")?;
        match rendered {
            Ok(diagram) => {
                self.commit_mermaid_success(visual, kind.as_str(), diagram)
                    .await
            }
            Err(error) => {
                self.commit_mermaid_failure(visual, kind.as_str(), error)
                    .await
            }
        }
    }

    pub async fn render_systems(&self, id: &str) -> Result<VisualRecord> {
        let visual = self.get(id.to_string()).await?;
        let kind = systems::template_kind(&visual.template_id)
            .ok_or_else(|| anyhow!("visual {id} is not a systems diagram"))?;
        let digest = visual
            .content_digest
            .clone()
            .ok_or_else(|| anyhow!("{} requires content", visual.template_id))?;
        let source = String::from_utf8(self.content.get_bytes("blobs", &digest)?)
            .context("systems source must be UTF-8")?;
        let scene = systems::parse_and_validate(&source, kind)?;
        let _ = self.journal.append(EventAppend {
            event_id: None, session_id: visual.session_id.clone(), run_id: visual.run_id.clone(),
            source: EventSource::Visual, kind: "visual.render_requested".into(),
            payload: json!({"visualId":visual.id,"revision":visual.current_revision,"visualKind":kind.as_str()}),
            remote_sequence: None, command_id: None, created_at: None,
        }).await;
        let source_for_render = source.clone();
        let rendered =
            tokio::task::spawn_blocking(move || systems::render_svg(&source_for_render, kind))
                .await
                .context("join systems render")?;
        match rendered {
            Ok(poster) => {
                self.commit_systems_success(visual, kind, scene, poster)
                    .await
            }
            Err(error) => self.commit_systems_failure(visual, kind, error).await,
        }
    }

    async fn commit_systems_success(
        &self,
        mut visual: VisualRecord,
        kind: SystemsKind,
        scene: systems::Scene,
        poster: systems::RenderedSystems,
    ) -> Result<VisualRecord> {
        validate_svg_bytes(poster.svg.as_bytes())?;
        let preview_digest = self.content.put_bytes("previews", poster.svg.as_bytes())?;
        visual.preview_digest = Some(preview_digest.clone());
        visual.updated_at = Utc::now().to_rfc3339();
        if let Some(object) = visual.metadata.as_object_mut() {
            object.insert("mediaType".into(), json!(systems::MEDIA_TYPE_SOURCE));
            object.insert("visualKind".into(), json!(kind.as_str()));
            object.insert("rendererVersion".into(), json!(systems::RENDERER_VERSION));
            object.insert("renderStatus".into(), json!("ready"));
            object.remove("renderError");
            if kind == SystemsKind::Dynamic {
                object.insert("durationMs".into(), json!(scene.duration_ms));
                object.insert("posterTimeMs".into(), json!(scene.poster_time_ms));
                object.insert("beatCount".into(), json!(scene.beats.len()));
                object.insert("reducedMotion".into(), json!(scene.reduced_motion));
            }
        }
        let stored = visual.clone();
        let preview = preview_digest.clone();
        let theme = if scene.theme == "technical-dark" {
            "dark"
        } else {
            "light"
        }
        .to_string();
        let width = poster.width;
        let height = poster.height;
        self.db.clone().run_transaction(move |conn| {
            persist_visual(conn,&stored)?;
            let empty=systems::RenderedSystems{svg:String::new(),width,height};
            renditions::insert_systems_svg_rendition(conn,&stored.id,stored.current_revision,&preview,&empty,&theme,"pane")?;
            renditions::insert_systems_svg_rendition(conn,&stored.id,stored.current_revision,&preview,&empty,&theme,"thumbnail")?;
            crate::storage::append_event(conn,EventAppend{event_id:None,session_id:stored.session_id.clone(),run_id:stored.run_id.clone(),source:EventSource::Visual,kind:"visual.rendered".into(),payload:json!({"visualId":stored.id,"revision":stored.current_revision,"visualKind":kind.as_str(),"previewDigest":preview}),remote_sequence:None,command_id:None,created_at:None})?;
            Ok(())
        }).await?;
        self.get(visual.id).await
    }

    async fn commit_systems_failure(
        &self,
        mut visual: VisualRecord,
        kind: SystemsKind,
        error: anyhow::Error,
    ) -> Result<VisualRecord> {
        let message = error.to_string();
        visual.updated_at = Utc::now().to_rfc3339();
        if let Some(object) = visual.metadata.as_object_mut() {
            object.insert("visualKind".into(), json!(kind.as_str()));
            object.insert("rendererVersion".into(), json!(systems::RENDERER_VERSION));
            object.insert("renderStatus".into(), json!("failed"));
            object.insert("renderError".into(), json!(message));
        }
        let stored = visual.clone();
        let fail_message = message.clone();
        self.db.clone().run_transaction(move|conn|{persist_visual(conn,&stored)?;crate::storage::append_event(conn,EventAppend{event_id:None,session_id:stored.session_id.clone(),run_id:stored.run_id.clone(),source:EventSource::Visual,kind:"visual.render_failed".into(),payload:json!({"visualId":stored.id,"revision":stored.current_revision,"visualKind":kind.as_str(),"error":fail_message}),remote_sequence:None,command_id:None,created_at:None})?;Ok(())}).await?;
        self.diagnose_visual(
            &visual,
            "visual.render.failed",
            crate::diagnostics::codes::VISUAL_RENDER_FAILED,
            &message,
            json!({ "visual_kind": kind.as_str() }),
        );
        self.get(visual.id).await
    }

    async fn commit_mermaid_success(
        &self,
        mut visual: VisualRecord,
        diagram_kind: &str,
        diagram: mermaid::RenderedDiagram,
    ) -> Result<VisualRecord> {
        validate_svg_bytes(diagram.svg.as_bytes())?;
        let preview_digest = self.content.put_bytes("previews", diagram.svg.as_bytes())?;
        visual.preview_digest = Some(preview_digest.clone());
        visual.updated_at = Utc::now().to_rfc3339();
        if let Some(object) = visual.metadata.as_object_mut() {
            object.insert("mediaType".into(), json!(mermaid::MEDIA_TYPE_SOURCE));
            object.insert("diagramKind".into(), json!(diagram_kind));
            object.insert("rendererVersion".into(), json!(mermaid::RENDERER_VERSION));
            object.insert("renderStatus".into(), json!("ready"));
            object.remove("renderError");
        }
        let db = self.db.clone();
        let stored = visual.clone();
        let preview = preview_digest.clone();
        let width = diagram.width;
        let height = diagram.height;
        let kind = diagram.kind;
        let diagram_kind = diagram_kind.to_string();
        db.run_transaction(move |conn| {
            persist_visual(conn, &stored)?;
            renditions::insert_svg_rendition(
                conn,
                &stored.id,
                stored.current_revision,
                &preview,
                &mermaid::RenderedDiagram {
                    kind,
                    svg: String::new(),
                    width,
                    height,
                },
                Theme::Light,
                "pane",
            )?;
            renditions::insert_svg_rendition(
                conn,
                &stored.id,
                stored.current_revision,
                &preview,
                &mermaid::RenderedDiagram {
                    kind,
                    svg: String::new(),
                    width,
                    height,
                },
                Theme::Light,
                "thumbnail",
            )?;
            crate::storage::append_event(
                conn,
                EventAppend {
                    event_id: None,
                    session_id: stored.session_id.clone(),
                    run_id: stored.run_id.clone(),
                    source: EventSource::Visual,
                    kind: "visual.rendered".into(),
                    payload: json!({
                        "visualId": stored.id,
                        "revision": stored.current_revision,
                        "diagramKind": diagram_kind,
                        "previewDigest": preview,
                    }),
                    remote_sequence: None,
                    command_id: None,
                    created_at: None,
                },
            )?;
            Ok(())
        })
        .await?;
        self.get(visual.id).await
    }

    async fn commit_mermaid_failure(
        &self,
        mut visual: VisualRecord,
        diagram_kind: &str,
        error: anyhow::Error,
    ) -> Result<VisualRecord> {
        let message = error.to_string();
        let diagram_kind = diagram_kind.to_string();
        visual.updated_at = Utc::now().to_rfc3339();
        if let Some(object) = visual.metadata.as_object_mut() {
            object.insert("diagramKind".into(), json!(diagram_kind));
            object.insert("rendererVersion".into(), json!(mermaid::RENDERER_VERSION));
            object.insert("renderStatus".into(), json!("failed"));
            object.insert("renderError".into(), json!(message));
        }
        let db = self.db.clone();
        let stored = visual.clone();
        let fail_kind = diagram_kind.clone();
        let fail_message = message.clone();
        db.run_transaction(move |conn| {
            persist_visual(conn, &stored)?;
            crate::storage::append_event(
                conn,
                EventAppend {
                    event_id: None,
                    session_id: stored.session_id.clone(),
                    run_id: stored.run_id.clone(),
                    source: EventSource::Visual,
                    kind: "visual.render_failed".into(),
                    payload: json!({
                        "visualId": stored.id,
                        "revision": stored.current_revision,
                        "diagramKind": fail_kind,
                        "error": fail_message,
                    }),
                    remote_sequence: None,
                    command_id: None,
                    created_at: None,
                },
            )?;
            Ok(())
        })
        .await?;
        self.diagnose_visual(
            &visual,
            "visual.render.failed",
            crate::diagnostics::codes::VISUAL_RENDER_FAILED,
            &message,
            json!({ "diagram_kind": diagram_kind }),
        );
        self.get(visual.id).await
    }
}

fn refuse_mermaid_stream_slot(bindings: &Value) -> Result<()> {
    let Some(slots) = bindings.get("slots").and_then(Value::as_array) else {
        return Ok(());
    };
    for slot in slots {
        if slot.get("slot").and_then(Value::as_str) == Some("stream") {
            bail!("diagram.mermaid.v1 must not bind slot stream");
        }
    }
    Ok(())
}

fn validate_svg_bytes(bytes: &[u8]) -> Result<()> {
    let svg = std::str::from_utf8(bytes).context("rendition is not UTF-8")?;
    let trimmed = svg.trim_start();
    if !(trimmed.starts_with("<svg") || (trimmed.starts_with("<?xml") && trimmed.contains("<svg")))
    {
        bail!("rendition is not svg");
    }
    let lower = svg.to_ascii_lowercase();
    if lower.contains("<script") || lower.contains("href=\"http") || lower.contains("file:") {
        bail!("rendition svg failed safety checks");
    }
    Ok(())
}

fn validate_visual_id(id: &str) -> Result<()> {
    if id.trim().is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        bail!("invalid visual id");
    }
    Ok(())
}

pub(super) fn digest_json(value: &Value) -> String {
    let encoded = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(encoded);
    format!("{:x}", hasher.finalize())
}

fn default_tsx_stub(visual: &VisualRecord) -> String {
    format!(
        r#"/** Auto-saved Synth visual instance.
 * visualId: {id}
 * templateId: {template_json}
 * title: {title}
 */
import {{ lazy, Suspense }} from "react";
import {{ getShellImporter }} from "@synth/visuals/registry";

export const visualId = {id_json};
export const templateId = {template_json};
export const title = {title_json};
export const bindings = {bindings} as const;

const Shell = lazy(async () => {{
  const importer = getShellImporter(templateId);
  if (!importer) throw new Error(`Template ${{templateId}} has no TSX shell`);
  const module = await importer();
  return {{ default: module.Shell ?? module.default }};
}});

export default function VisualInstance(props: Record<string, unknown>) {{
  return <Suspense fallback={{<div role="status">Loading visual…</div>}}><Shell title={{title}} bindings={{bindings}} {{...props}} /></Suspense>;
}}
"#,
        id = visual.id,
        title = visual.title,
        id_json = serde_json::to_string(&visual.id).unwrap_or_else(|_| "\"\"".into()),
        template_json =
            serde_json::to_string(&visual.template_id).unwrap_or_else(|_| "\"\"".into()),
        title_json = serde_json::to_string(&visual.title).unwrap_or_else(|_| "\"\"".into()),
        bindings = serde_json::to_string_pretty(&visual.bindings).unwrap_or_else(|_| "{}".into()),
    )
}

fn ensure_session(conn: &Connection, session_id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM sessions WHERE id = ?1",
            params![session_id],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if exists {
        return Ok(());
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sessions(id, title, target_json, status, latest_cursor, metadata_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'created', 0, '{}', ?4, ?4)",
        params![
            session_id,
            format!("Session {session_id}"),
            json!({"kind":"codex","model":"laguna-xs-2.1","adapter":null}).to_string(),
            now
        ],
    )?;
    Ok(())
}

fn insert_visual(conn: &Connection, visual: &VisualRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO visuals(
            id, current_revision, title, template_id, status, renderer_kind, bindings_json,
            session_id, message_id, run_id, trace_id, parent_visual_id, source_agent_id,
            source_model, content_digest, preview_digest, metadata_json, created_at, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        params![
            visual.id,
            visual.current_revision,
            visual.title,
            visual.template_id,
            visual.status.as_str(),
            visual.renderer_kind.as_str(),
            visual.bindings.to_string(),
            visual.session_id,
            visual.message_id,
            visual.run_id,
            visual.trace_id,
            visual.parent_visual_id,
            visual.source_agent_id,
            visual.source_model,
            visual.content_digest,
            visual.preview_digest,
            visual.metadata.to_string(),
            visual.created_at,
            visual.updated_at,
        ],
    )
    .context("insert visual")?;
    Ok(())
}

fn persist_visual(conn: &Connection, visual: &VisualRecord) -> Result<()> {
    let changed = conn.execute(
        "UPDATE visuals SET
            current_revision = ?2,
            title = ?3,
            template_id = ?4,
            status = ?5,
            renderer_kind = ?6,
            bindings_json = ?7,
            session_id = ?8,
            message_id = ?9,
            run_id = ?10,
            trace_id = ?11,
            parent_visual_id = ?12,
            source_agent_id = ?13,
            source_model = ?14,
            content_digest = ?15,
            preview_digest = ?16,
            metadata_json = ?17,
            updated_at = ?18
         WHERE id = ?1",
        params![
            visual.id,
            visual.current_revision,
            visual.title,
            visual.template_id,
            visual.status.as_str(),
            visual.renderer_kind.as_str(),
            visual.bindings.to_string(),
            visual.session_id,
            visual.message_id,
            visual.run_id,
            visual.trace_id,
            visual.parent_visual_id,
            visual.source_agent_id,
            visual.source_model,
            visual.content_digest,
            visual.preview_digest,
            visual.metadata.to_string(),
            visual.updated_at,
        ],
    )?;
    if changed == 0 {
        bail!("visual not found: {}", visual.id);
    }
    Ok(())
}

fn insert_revision(conn: &Connection, revision: &VisualRevision) -> Result<()> {
    conn.execute(
        "INSERT INTO visual_revisions(
            visual_id, revision, template_id, renderer_kind, content_digest, bindings_digest,
            bindings_json, preview_digest, author_agent_id, parent_revision, created_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            revision.visual_id,
            revision.revision,
            revision.template_id,
            revision.renderer_kind.as_str(),
            revision.content_digest,
            revision.bindings_digest,
            revision.bindings.as_ref().map(|value| value.to_string()),
            revision.preview_digest,
            revision.author_agent_id,
            revision.parent_revision,
            revision.created_at,
        ],
    )
    .context("insert visual revision")?;
    Ok(())
}

fn load_visual(conn: &Connection, id: &str) -> Result<VisualRecord> {
    conn.query_row(
        "SELECT id, current_revision, title, template_id, status, renderer_kind, bindings_json,
                session_id, message_id, run_id, trace_id, parent_visual_id, source_agent_id,
                source_model, content_digest, preview_digest, metadata_json, created_at, updated_at
         FROM visuals WHERE id = ?1",
        params![id],
        |row| {
            Ok(VisualRecord {
                schema_version: VISUAL_SCHEMA_VERSION.to_string(),
                id: row.get(0)?,
                current_revision: row.get(1)?,
                title: row.get(2)?,
                template_id: row.get(3)?,
                status: VisualStatus::parse(&row.get::<_, String>(4)?),
                renderer_kind: RendererKind::parse(&row.get::<_, String>(5)?),
                bindings: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or(json!({})),
                session_id: row.get(7)?,
                message_id: row.get(8)?,
                run_id: row.get(9)?,
                trace_id: row.get(10)?,
                parent_visual_id: row.get(11)?,
                source_agent_id: row.get(12)?,
                source_model: row.get(13)?,
                content_digest: row.get(14)?,
                preview_digest: row.get(15)?,
                metadata: serde_json::from_str(&row.get::<_, String>(16)?).unwrap_or(json!({})),
                created_at: row.get(17)?,
                updated_at: row.get(18)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow!("visual not found: {id}"))
}

fn list_visuals(conn: &Connection, query: &VisualQuery) -> Result<Vec<VisualRecord>> {
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0).max(0);
    if let Some(status) = query.status.as_deref() {
        if !matches!(status, "draft" | "live" | "saved" | "failed" | "archived") {
            bail!("invalid visual status filter");
        }
    }
    let mut sql = String::from(
        "SELECT id, current_revision, title, template_id, status, renderer_kind, bindings_json,
                session_id, message_id, run_id, trace_id, parent_visual_id, source_agent_id,
                source_model, content_digest, preview_digest, metadata_json, created_at, updated_at
         FROM visuals WHERE 1 = 1",
    );
    let mut binds: Vec<String> = Vec::new();
    if let Some(status) = &query.status {
        sql.push_str(" AND status = ?");
        binds.push(status.clone());
    } else {
        sql.push_str(" AND status != 'archived'");
    }
    if let Some(session_id) = &query.session_id {
        sql.push_str(" AND session_id = ?");
        binds.push(session_id.clone());
    }
    if let Some(template_id) = &query.template_id {
        sql.push_str(" AND template_id = ?");
        binds.push(template_id.clone());
    }
    if let Some(search) = &query.search {
        sql.push_str(" AND (title LIKE ? OR template_id LIKE ? OR id LIKE ?)");
        let needle = format!("%{search}%");
        binds.push(needle.clone());
        binds.push(needle.clone());
        binds.push(needle);
    }
    sql.push_str(" ORDER BY updated_at DESC, id ASC LIMIT ? OFFSET ?");
    binds.push(limit.to_string());
    binds.push(offset.to_string());

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(binds.iter()), |row| {
        Ok(VisualRecord {
            schema_version: VISUAL_SCHEMA_VERSION.to_string(),
            id: row.get(0)?,
            current_revision: row.get(1)?,
            title: row.get(2)?,
            template_id: row.get(3)?,
            status: VisualStatus::parse(&row.get::<_, String>(4)?),
            renderer_kind: RendererKind::parse(&row.get::<_, String>(5)?),
            bindings: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or(json!({})),
            session_id: row.get(7)?,
            message_id: row.get(8)?,
            run_id: row.get(9)?,
            trace_id: row.get(10)?,
            parent_visual_id: row.get(11)?,
            source_agent_id: row.get(12)?,
            source_model: row.get(13)?,
            content_digest: row.get(14)?,
            preview_digest: row.get(15)?,
            metadata: serde_json::from_str(&row.get::<_, String>(16)?).unwrap_or(json!({})),
            created_at: row.get(17)?,
            updated_at: row.get(18)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn list_revisions(conn: &Connection, visual_id: &str) -> Result<Vec<VisualRevision>> {
    let mut stmt = conn.prepare(
        "SELECT visual_id, revision, template_id, renderer_kind, content_digest, bindings_digest,
                bindings_json, preview_digest, author_agent_id, parent_revision, created_at
         FROM visual_revisions
         WHERE visual_id = ?1
         ORDER BY revision DESC",
    )?;
    let rows = stmt.query_map(params![visual_id], |row| {
        Ok(VisualRevision {
            visual_id: row.get(0)?,
            revision: row.get(1)?,
            template_id: row.get(2)?,
            renderer_kind: RendererKind::parse(&row.get::<_, String>(3)?),
            content_digest: row.get(4)?,
            bindings_digest: row.get(5)?,
            bindings: row
                .get::<_, Option<String>>(6)?
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            preview_digest: row.get(7)?,
            author_agent_id: row.get(8)?,
            parent_revision: row.get(9)?,
            created_at: row.get(10)?,
        })
    })?;
    let mut revisions = Vec::new();
    for row in rows {
        revisions.push(row?);
    }
    Ok(revisions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use tempfile::tempdir;

    fn non_mermaid_template(registry: &VisualRegistry) -> Option<String> {
        registry.list_templates(None).ok().and_then(|templates| {
            templates
                .into_iter()
                .find(|template| template.id != mermaid::TEMPLATE_ID)
                .map(|template| template.id)
        })
    }

    #[tokio::test]
    async fn visual_create_rolls_back_when_journal_append_fails() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let db = storage.database().clone();
        let registry = VisualRegistry::new(
            db.clone(),
            EventJournal::new(db.clone()),
            ContentStore::new(storage.content_root()),
        );
        let Some(template_id) = non_mermaid_template(&registry) else {
            return;
        };
        db.with_conn(|conn| {
            conn.execute_batch(
                "CREATE TRIGGER reject_visual_events
                 BEFORE INSERT ON events
                 WHEN NEW.kind = 'visual.created'
                 BEGIN SELECT RAISE(ABORT, 'forced journal failure'); END;",
            )?;
            Ok(())
        })
        .unwrap();

        let result = registry
            .create(VisualCreateRequest {
                template_id: template_id.clone(),
                title: Some("Must roll back".into()),
                bindings: Some(json!({})),
                id: Some("vis_atomic_failure".into()),
                status: None,
                renderer_kind: None,
                session_id: Some("sess_atomic_failure".into()),
                message_id: None,
                run_id: None,
                trace_id: None,
                parent_visual_id: None,
                source_agent_id: None,
                source_model: None,
                content: None,
                metadata: None,
            })
            .await;
        assert!(result.is_err());
        db.with_conn(|conn| {
            let visuals: i64 = conn.query_row(
                "SELECT COUNT(*) FROM visuals WHERE id = 'vis_atomic_failure'",
                [],
                |row| row.get(0),
            )?;
            let sessions: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE id = 'sess_atomic_failure'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(visuals, 0);
            assert_eq!(sessions, 0);
            Ok(())
        })
        .unwrap();
    }

    #[tokio::test]
    async fn create_update_save_fork_and_show() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let registry = VisualRegistry::new(
            storage.database().clone(),
            journal,
            ContentStore::new(storage.content_root()),
        );
        // Skip if templates are not present in this checkout layout.
        let Some(template_id) = non_mermaid_template(&registry) else {
            return;
        };
        let (created, _) = registry
            .create(VisualCreateRequest {
                template_id: template_id.clone(),
                title: Some("Reward chart".into()),
                bindings: Some(json!({"steps":[1,2,3]})),
                id: None,
                status: Some(VisualStatus::Live),
                renderer_kind: None,
                session_id: Some("sess_visual".into()),
                message_id: None,
                run_id: None,
                trace_id: None,
                parent_visual_id: None,
                source_agent_id: Some("codex".into()),
                source_model: Some("laguna-xs-2.1".into()),
                content: None,
                metadata: None,
            })
            .await
            .unwrap();
        assert_eq!(created.current_revision, 1);
        let (saved, _) = registry.save(created.id.clone(), None).await.unwrap();
        assert_eq!(saved.status, VisualStatus::Saved);
        assert!(saved.content_digest.is_some());
        assert!(saved.current_revision >= 2);
        let (forked, _) = registry
            .fork(created.id.clone(), Some("Fork".into()), None)
            .await
            .unwrap();
        assert_ne!(forked.id, created.id);
        let (shown, event) = registry.show(created.id.clone(), None).await.unwrap();
        assert_eq!(shown.id, created.id);
        assert_eq!(event["kind"], "visual.show");
        let listed = registry.list(VisualQuery::default()).await.unwrap();
        assert!(listed.iter().any(|visual| visual.id == created.id));

        // Filtering must happen in SQLite before pagination: the newer noise row
        // must not consume the only requested result.
        registry
            .create(VisualCreateRequest {
                template_id,
                title: Some("Unrelated newest visual".into()),
                bindings: Some(json!({})),
                id: None,
                status: Some(VisualStatus::Draft),
                renderer_kind: None,
                session_id: None,
                message_id: None,
                run_id: None,
                trace_id: None,
                parent_visual_id: None,
                source_agent_id: None,
                source_model: None,
                content: None,
                metadata: None,
            })
            .await
            .unwrap();
        let filtered = registry
            .list(VisualQuery {
                search: Some("Reward chart".into()),
                limit: Some(1),
                ..VisualQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, created.id);
    }

    #[tokio::test]
    async fn mermaid_create_requires_content_and_renders_svg() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let registry = VisualRegistry::new(
            storage.database().clone(),
            EventJournal::new(storage.database().clone()),
            ContentStore::new(storage.content_root()),
        );
        if registry.get_template(mermaid::TEMPLATE_ID).is_err() {
            return;
        }
        let missing = registry
            .create(VisualCreateRequest {
                template_id: mermaid::TEMPLATE_ID.into(),
                title: Some("Policy pin".into()),
                bindings: Some(json!({})),
                id: Some("vis_mermaid_missing".into()),
                status: None,
                renderer_kind: None,
                session_id: None,
                message_id: None,
                run_id: None,
                trace_id: None,
                parent_visual_id: None,
                source_agent_id: None,
                source_model: None,
                content: None,
                metadata: None,
            })
            .await;
        assert!(
            missing.is_err(),
            "mermaid create must fail closed without content"
        );

        let (created, _) = registry
            .create(VisualCreateRequest {
                template_id: mermaid::TEMPLATE_ID.into(),
                title: Some("Policy pin".into()),
                bindings: Some(json!({})),
                id: Some("vis_mermaid_ok".into()),
                status: Some(VisualStatus::Live),
                renderer_kind: None,
                session_id: Some("sess_mermaid".into()),
                message_id: None,
                run_id: None,
                trace_id: None,
                parent_visual_id: None,
                source_agent_id: Some("mcp".into()),
                source_model: None,
                content: Some("flowchart LR\nAgent --> MCP --> Registry".into()),
                metadata: Some(json!({"presentation": "pane"})),
            })
            .await
            .unwrap();
        assert_eq!(created.renderer_kind, RendererKind::Mermaid);
        assert!(created.content_digest.is_some());
        assert!(created.preview_digest.is_some());
        assert_eq!(created.metadata["renderStatus"], "ready");
        assert_eq!(created.metadata["diagramKind"], "flowchart");
        let asset = registry
            .mermaid_rendition(created.id.clone(), Some("svg".into()), None, None)
            .await
            .unwrap();
        let svg = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(asset.base64)
                .unwrap(),
        )
        .unwrap();
        assert!(svg.contains("<svg"));
        let source = registry.mermaid_source(created.id.clone()).await.unwrap();
        assert_eq!(source.media_type, mermaid::MEDIA_TYPE_SOURCE);
        let (updated, _) = registry
            .update(
                created.id.clone(),
                VisualUpdateRequest {
                    title: None,
                    bindings: None,
                    status: None,
                    renderer_kind: None,
                    message_id: None,
                    run_id: None,
                    trace_id: None,
                    content: Some(
                        "sequenceDiagram\nAgent->>MCP: policy_ref\nMCP->>Container: POST /rollouts"
                            .into(),
                    ),
                    metadata: None,
                    bump_revision: Some(true),
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.current_revision, 2);
        assert_eq!(updated.metadata["diagramKind"], "sequence");
        assert_ne!(updated.content_digest, created.content_digest);
    }

    #[tokio::test]
    async fn systems_scenes_create_render_and_persist_posters() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let registry = VisualRegistry::new(
            storage.database().clone(),
            EventJournal::new(storage.database().clone()),
            ContentStore::new(storage.content_root()),
        );
        let static_source = r#"{"version":1,"theme":"technical-dark","canvas":{"width":500,"height":240},"nodes":[{"id":"a","x":20,"y":80,"width":120,"height":50,"label":"Source"},{"id":"b","x":350,"y":80,"width":120,"height":50,"label":"Target"}],"edges":[{"from":"a","to":"b","label":"evidence"}]}"#;
        let (static_visual, _) = registry
            .create(VisualCreateRequest {
                template_id: systems::STATIC_TEMPLATE_ID.into(),
                title: Some("Map".into()),
                bindings: Some(json!({})),
                id: Some("vis_systems_static".into()),
                status: None,
                renderer_kind: None,
                session_id: None,
                message_id: None,
                run_id: None,
                trace_id: None,
                parent_visual_id: None,
                source_agent_id: None,
                source_model: None,
                content: Some(static_source.into()),
                metadata: None,
            })
            .await
            .unwrap();
        assert_eq!(static_visual.renderer_kind, RendererKind::Systems);
        assert_eq!(static_visual.metadata["renderStatus"], "ready");
        assert!(static_visual.preview_digest.is_some());
        let poster = registry
            .visual_rendition(static_visual.id, Some("svg".into()), None, None)
            .await
            .unwrap();
        assert_eq!(poster.theme.as_deref(), Some("dark"));
        let svg = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(poster.base64)
                .unwrap(),
        )
        .unwrap();
        assert!(svg.contains("Source") && svg.contains("evidence"));

        let dynamic_source = r#"{"version":1,"canvas":{"width":500,"height":240},"nodes":[{"id":"a","x":20,"y":80,"width":120,"height":50,"label":"Source"},{"id":"b","x":350,"y":80,"width":120,"height":50,"label":"Target","visible":false}],"durationMs":3000,"posterTimeMs":1500,"reducedMotion":"poster","beats":[{"id":"start","atMs":0,"caption":"Start"},{"id":"arrive","atMs":1000,"caption":"Arrive"}],"timeline":[{"atMs":1000,"durationMs":500,"easing":"ease-out","target":"b","changes":{"visible":true}}]}"#;
        let (dynamic, _) = registry
            .create(VisualCreateRequest {
                template_id: systems::DYNAMIC_TEMPLATE_ID.into(),
                title: Some("Explainer".into()),
                bindings: Some(json!({})),
                id: Some("vis_systems_dynamic".into()),
                status: None,
                renderer_kind: None,
                session_id: None,
                message_id: None,
                run_id: None,
                trace_id: None,
                parent_visual_id: None,
                source_agent_id: None,
                source_model: None,
                content: Some(dynamic_source.into()),
                metadata: None,
            })
            .await
            .unwrap();
        assert_eq!(dynamic.renderer_kind, RendererKind::SystemsDynamic);
        assert_eq!(dynamic.metadata["durationMs"], 3000);
        assert_eq!(dynamic.metadata["beatCount"], 2);
        let source = registry.visual_source(dynamic.id).await.unwrap();
        assert_eq!(source.media_type, systems::MEDIA_TYPE_SOURCE);
    }
}
