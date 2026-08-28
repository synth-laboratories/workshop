use super::chart_data;
use super::charts;
use super::mermaid::{self, Theme};
use super::models::{
    binding_descriptors, canonicalize_bindings, descriptor_input_name, BindingsForm, RendererKind,
    VisualCreateRequest, VisualQuery, VisualRecord, VisualRevision, VisualStatus,
    VisualUpdateRequest, VISUAL_SCHEMA_VERSION,
};
use super::renditions::{self, VisualAsset, VisualRendition};
use super::sourced;
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
    /// Attached once by the composition root, like diagnostics. Charts can bind
    /// an optimizer run's typed result; the optimizer service owns that read and
    /// the registry must not grow a second way to compute it.
    pub(super) optimizer_runs: Arc<std::sync::OnceLock<crate::optimizers::OptimizerService>>,
}

impl VisualRegistry {
    pub fn new(db: Arc<Database>, journal: EventJournal, content: ContentStore) -> Self {
        Self {
            db,
            journal,
            content,
            diagnostics: Arc::new(std::sync::OnceLock::new()),
            optimizer_runs: Arc::new(std::sync::OnceLock::new()),
        }
    }

    pub(crate) fn content(&self) -> &ContentStore {
        &self.content
    }

    /// Wire the optimizer service in after both exist. Idempotent.
    ///
    /// The two hold each other — the optimizer service owns a `VisualRegistry`
    /// clone — so this is a composition-root wiring for a pair that lives as
    /// long as the process, not a general-purpose dependency.
    pub fn attach_optimizer_runs(&self, service: crate::optimizers::OptimizerService) {
        let _ = self.optimizer_runs.set(service);
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
        crate::platform::logging::report(
            "visuals",
            "eprintln",
            format!(
                "synth-desktop: upgraded {} visual bindings for {visual_id} rev {revision} \
             (template {template_id}, slots {upgraded_slots:?}); writers must send {}",
                form.as_str(),
                super::models::VISUAL_BINDINGS_SCHEMA_VERSION
            ),
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
        input.details.insert("inputs".into(), json!(upgraded_slots));
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
        validate_optimizer_run_bindings(&bindings)?;
        let is_mermaid = mermaid::is_mermaid_template(&template.id);
        let systems_kind = systems::template_kind(&template.id);
        let is_chart = charts::is_chart_template(&template.id);
        let is_sourced = sourced::is_sourced_template(&template.id);
        let is_managed_html =
            template.source_kind.as_deref() == Some("managed") && template.renderer_path.is_some();
        // Imported HTML is immutable package source. Accepting caller content
        // here would make a reviewed import indistinguishable from arbitrary
        // HTML authored at create time.
        if is_managed_html
            && request
                .content
                .as_deref()
                .is_some_and(|content| !content.trim().is_empty())
        {
            bail!(
                "{} is a managed HTML template; create it without content",
                template.id
            );
        }
        let managed_html_content = if is_managed_html {
            let path = template
                .renderer_path
                .as_deref()
                .expect("managed HTML renderer path");
            Some(
                std::fs::read_to_string(path)
                    .with_context(|| format!("read managed renderer {path}"))?,
            )
        } else {
            None
        };
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
        if is_chart {
            let source = request
                .content
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("{} requires content", template.id))?;
            charts::validate_source(source)?;
        }
        if is_sourced {
            let source = request
                .content
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("{} requires content", template.id))?;
            if source.len() > sourced::MAX_SOURCE_BYTES {
                bail!(
                    "{} exceeds {} bytes",
                    template.id,
                    sourced::MAX_SOURCE_BYTES
                );
            }
        }
        let status = request.status.unwrap_or(VisualStatus::Draft);
        let renderer_kind = if is_mermaid {
            RendererKind::Mermaid
        } else if systems_kind == Some(SystemsKind::Static) {
            RendererKind::Systems
        } else if systems_kind == Some(SystemsKind::Dynamic) {
            RendererKind::SystemsDynamic
        } else if is_chart {
            RendererKind::Chart
        } else if is_sourced {
            RendererKind::Tsx
        } else if is_managed_html {
            RendererKind::Html
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
        if is_chart {
            if let Some(object) = metadata.as_object_mut() {
                object
                    .entry("presentation")
                    .or_insert_with(|| json!("pane"));
                object.insert("mediaType".into(), json!(charts::MEDIA_TYPE_SOURCE));
                object.insert("renderStatus".into(), json!("queued"));
                object.insert("rendererVersion".into(), json!(charts::RENDERER_VERSION));
                object.insert("specSchema".into(), json!(charts::SCHEMA_VERSION));
            }
        }
        if is_sourced {
            if let Some(object) = metadata.as_object_mut() {
                object
                    .entry("presentation")
                    .or_insert_with(|| json!("pane"));
                object.insert("mediaType".into(), json!(sourced::MEDIA_TYPE_SOURCE));
                object.insert("visualKind".into(), json!(sourced::KIND));
                object.insert("protocolId".into(), json!(sourced::PROTOCOL_ID));
            }
        }
        if is_managed_html {
            if let Some(object) = metadata.as_object_mut() {
                object
                    .entry("presentation")
                    .or_insert_with(|| json!("pane"));
                object.insert("mediaType".into(), json!("text/html"));
                object.insert("managedTemplate".into(), json!(true));
            }
        }
        let canonical_content = managed_html_content.as_ref().or(request.content.as_ref());
        let content_digest = if let Some(content) = canonical_content {
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
        if charts::is_chart_template(&record.template_id) {
            let rendered = self.render_chart(&record.id).await?;
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
        let existing = self.get(id.clone()).await?;
        let content_changed = request.content.is_some();
        let bindings_changed = request.bindings.is_some();
        if let Some(source) = request.content.as_deref() {
            if mermaid::is_mermaid_template(&existing.template_id) {
                mermaid::validate_source(source)?;
            }
            if let Some(kind) = systems::template_kind(&existing.template_id) {
                systems::validate_source(source, kind)?;
            }
            if charts::is_chart_template(&existing.template_id) {
                charts::validate_source(source)?;
            }
            if sourced::is_sourced_template(&existing.template_id) {
                let trimmed = source.trim();
                if trimmed.is_empty() {
                    bail!("{} requires content", existing.template_id);
                }
                if trimmed.len() > sourced::MAX_SOURCE_BYTES {
                    bail!(
                        "{} exceeds {} bytes",
                        existing.template_id,
                        sourced::MAX_SOURCE_BYTES
                    );
                }
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
            if mermaid::is_mermaid_template(&existing.template_id) {
                refuse_mermaid_stream_slot(bindings)?;
            }
            if systems::template_kind(&existing.template_id).is_some() {
                refuse_mermaid_stream_slot(bindings)?;
            }
        }
        let effective_bindings = request.bindings.as_ref().unwrap_or(&existing.bindings);
        validate_optimizer_run_bindings(effective_bindings)?;
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
        if (content_changed || bindings_changed) && charts::is_chart_template(&updated.template_id)
        {
            let rendered = self.render_chart(&updated.id).await?;
            return Ok((rendered, serde_json::to_value(event)?));
        }
        Ok((updated, serde_json::to_value(event)?))
    }

    pub async fn save(&self, id: String, tsx: Option<String>) -> Result<(VisualRecord, Value)> {
        let current = self.get(id.clone()).await?;
        let body = if sourced::is_sourced_template(&current.template_id) {
            tsx.filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("{} requires content", current.template_id))?
        } else {
            tsx.unwrap_or_else(|| default_tsx_stub(&current))
        };
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
        let owner = session_id
            .clone()
            .or_else(|| record.session_id.clone())
            .filter(|value| !value.trim().is_empty());
        if let Some(session) = owner.clone() {
            let visual_id = record.id.clone();
            self.db
                .clone()
                .run_transaction(move |conn| persist_selected_visual(conn, &session, &visual_id))
                .await?;
        }
        let event = self
            .journal
            .append(EventAppend {
                event_id: None,
                session_id: owner.clone().or_else(|| record.session_id.clone()),
                run_id: record.run_id.clone(),
                source: EventSource::Visual,
                kind: "visual.show".into(),
                payload: json!({
                    "visualId": record.id,
                    "revision": record.current_revision,
                    "title": record.title,
                    "templateId": record.template_id,
                    "bindings": record.bindings,
                    "metadata": record.metadata,
                    "status": record.status.as_str(),
                    "runId": record.run_id,
                    "traceId": record.trace_id,
                    "messageId": record.message_id,
                    // Who *owns* this visual, which is not who opened it. The
                    // registry is instance-global: without this, a chat that
                    // displayed another chat's visual could not be told apart
                    // from the chat that authored it.
                    "ownerSessionId": record.session_id,
                    "openVisualId": record.id,
                }),
                remote_sequence: None,
                command_id: None,
                created_at: None,
            })
            .await?;
        Ok((
            self.get(record.id.clone()).await.unwrap_or(record),
            serde_json::to_value(event)?,
        ))
    }

    pub async fn selected_for_session(&self, session_id: String) -> Result<Option<String>> {
        let db = self.db.clone();
        db.run(move |conn| load_selected_visual(conn, &session_id))
            .await
    }

    pub fn list_templates(&self, genre: Option<&str>) -> Result<Vec<TemplateMeta>> {
        super::templates::list_templates(genre)
    }

    pub fn get_template(&self, template_id: &str) -> Result<TemplateMeta> {
        resolve_template(template_id)
    }

    /// **Ungated seam. Always refuses.**
    ///
    /// This is the route `visual_import_template` reaches over the agent HTTP
    /// seam, and until now it wrote a `renderer.html` package into the instance
    /// state root with no confirmation of any kind — an agent could leave
    /// renderer code that runs at every launch, unprompted. That is the same act
    /// `visual_save_template` performs, so gating only the new writers would
    /// have gated nothing.
    ///
    /// It keeps its signature so the IPC dispatcher still compiles while the
    /// route is moved to [`Self::import_template_approved`]; it cannot keep its
    /// behaviour, because this signature has nowhere to put a session id and no
    /// way to await a person. Refusing is the only honest thing a synchronous
    /// entry point can do about a decision that requires one.
    pub fn import_template(&self, source_path: &str) -> Result<TemplateMeta> {
        Err(crate::session::template_persist::unapproved(
            "import",
            source_path,
        ))
    }

    /// Import one reviewed, networkless `renderer.html` package, once a person
    /// has allowed it.
    ///
    /// The manifest is read before the card so the card can name the id and the
    /// bytes, and `import_managed_template` still performs every structural and
    /// networkless check afterwards — this reads two files to describe the
    /// decision, it does not decide anything.
    pub async fn import_template_approved<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        session_id: Option<&str>,
        source_path: &str,
    ) -> Result<TemplateMeta> {
        let request = managed_import_request(source_path)?;
        let consent =
            crate::session::template_persist::authorize(app, session_id, &request).await?;
        // Re-describe the package after the decision. An approval names a
        // digest, and a card can be open for a long time; if the bytes moved
        // while it was, the grant does not cover what is now on disk. The
        // remaining window — between this read and the import's own — is the one
        // that cannot be closed without reimplementing the import, and it is
        // microseconds against a human's seconds.
        let confirmed = managed_import_request(source_path)?;
        consent.bind(&confirmed)?;
        super::templates::import_managed_template(source_path)
    }

    /// TSX source of a user-authored template's `shell.tsx`, for the pane to
    /// compile through `compileSourcedModule`. Refuses every other tier: a
    /// bundled family resolves its shell through Vite's static graph, and a
    /// `managed` package is `renderer.html` behind an iframe CSP.
    pub fn template_shell_source(&self, template_id: &str) -> Result<String> {
        super::user_templates::shell_source(template_id)
    }

    /// **Ungated seam. Always refuses.** See [`Self::import_template`].
    ///
    /// The pane's own Save button reaches this through a synchronous Tauri
    /// command, which can neither name a session nor wait for a card without
    /// blocking the thread the card would have to be drawn on. So this refuses
    /// and [`Self::save_template_approved`] is the route; making the command
    /// async and giving it a session id is the change that reconnects it.
    pub fn save_template(
        &self,
        template_id: &str,
        _manifest_json: &str,
        _source: &str,
    ) -> Result<TemplateMeta> {
        Err(crate::session::template_persist::unapproved(
            "save",
            template_id,
        ))
    }

    /// Promote authored TSX into a durable, reusable template under the
    /// instance state root, once a person has allowed it.
    ///
    /// The registry is a pass-through for everything except the gate: what a
    /// user template *is* belongs to `templates.rs`, and writing one is
    /// `user_templates.rs` asking `templates.rs` to confirm the bytes it just
    /// wrote. A copy of the manifest rules on this side is the drift the plan
    /// exists to remove.
    pub async fn save_template_approved<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        session_id: Option<&str>,
        template_id: &str,
        manifest_json: &str,
        source: &str,
    ) -> Result<TemplateMeta> {
        let prepared = super::user_templates::prepare_save(template_id, manifest_json, source)?;
        let consent =
            crate::session::template_persist::authorize(app, session_id, prepared.request())
                .await?;
        super::user_templates::commit(prepared, &consent)
    }

    /// **Ungated seam. Always refuses.** See [`Self::save_template`].
    pub fn create_template(
        &self,
        template_id: &str,
        _from_template_id: &str,
        _title: Option<&str>,
    ) -> Result<TemplateMeta> {
        Err(crate::session::template_persist::unapproved(
            "fork",
            template_id,
        ))
    }

    /// Scaffold a new user template by forking an existing one under a new id,
    /// once a person has allowed it.
    pub async fn create_template_approved<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        session_id: Option<&str>,
        template_id: &str,
        from_template_id: &str,
        title: Option<&str>,
    ) -> Result<TemplateMeta> {
        let prepared = super::user_templates::prepare_fork(template_id, from_template_id, title)?;
        let consent =
            crate::session::template_persist::authorize(app, session_id, prepared.request())
                .await?;
        super::user_templates::commit(prepared, &consent)
    }

    /// Structural verdict on one user template directory. The import allowlist
    /// is not checked here; `sourcedValidate.ts` owns it and the report says so.
    pub fn validate_template(
        &self,
        template_id: &str,
    ) -> super::user_templates::UserTemplateValidation {
        super::user_templates::validate(template_id)
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
        } else if charts::is_chart_template(&visual.template_id) {
            charts::MEDIA_TYPE_SOURCE
        } else if sourced::is_sourced_template(&visual.template_id) {
            sourced::MEDIA_TYPE_SOURCE
        } else if visual.renderer_kind == RendererKind::Html {
            "text/html"
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
        let is_chart = charts::is_chart_template(&visual.template_id);
        if !mermaid::is_mermaid_template(&visual.template_id) && systems_kind.is_none() && !is_chart
        {
            bail!("visual {} has no SVG rendition renderer", visual.id);
        }
        let format = if format.as_deref() == Some("png") {
            "svg".to_string()
        } else {
            format.unwrap_or_else(|| "svg".into())
        };
        let theme = match theme {
            Some(value) => value,
            None if is_chart => spec_theme(
                &visual
                    .content_digest
                    .as_deref()
                    .and_then(|digest| self.content.get_bytes("blobs", digest).ok())
                    .unwrap_or_default(),
            ),
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
        let renderer_version = if is_chart {
            charts::RENDERER_VERSION
        } else if systems_kind.is_some() {
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
        } else if charts::is_chart_template(&visual.template_id) {
            self.render_chart(id).await
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

    /// Resolve the evidence a chart's `from` blocks name.
    ///
    /// One document per input: a chart is a still image of one thing per input,
    /// and two bindings on one input leave no way to say which one the picture
    /// came from. Kinds that only exist while something is running — a live
    /// stream — are refused here rather than sampled arbitrarily.
    async fn chart_documents(
        &self,
        visual: &VisualRecord,
        wanted: &std::collections::BTreeMap<String, Option<String>>,
    ) -> Result<(std::collections::BTreeMap<String, Value>, Value)> {
        let slots = binding_descriptors(&visual.bindings).unwrap_or_default();
        let mut documents = std::collections::BTreeMap::new();
        let mut provenance = serde_json::Map::new();
        for (slot, projection) in wanted {
            let matching: Vec<&Value> = slots
                .iter()
                .filter(|descriptor| {
                    descriptor_input_name(descriptor).ok().as_deref() == Some(slot.as_str())
                })
                .collect();
            let descriptor = match matching.len() {
                0 => bail!(
                    "panel reads input {slot}, which has no binding; bind it with visual_bind_data_source"
                ),
                1 => matching[0],
                count => bail!(
                    "input {slot} has {count} bindings; a chart reads one document per input"
                ),
            };
            let kind = descriptor
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let source = descriptor.get("source").and_then(Value::as_str);
            let mut receipt = json!({ "kind": kind });
            let document = match kind {
                "inline" => {
                    let document = descriptor
                        .get("data")
                        .cloned()
                        .ok_or_else(|| anyhow!("inline input {slot} carries no data"))?;
                    receipt["digest"] = json!(digest_json(&document));
                    document
                }
                "fixture" => {
                    let path = source
                        .ok_or_else(|| anyhow!("fixture input {slot} needs a source path"))?;
                    let (document, digest) = read_visual_fixture(path)?;
                    receipt["source"] = json!(path);
                    // A fixture is a file on disk, so its path is not identity.
                    // The digest is what makes a render re-derivable.
                    receipt["digest"] = json!(digest);
                    document
                }
                "local_cas" => {
                    let digest = source
                        .ok_or_else(|| anyhow!("local_cas input {slot} needs a blob digest"))?;
                    let bytes = self.content.get_bytes("blobs", digest)?;
                    receipt["digest"] = json!(digest);
                    serde_json::from_slice(&bytes)
                        .with_context(|| format!("blob {digest} is not JSON"))?
                }
                "trace_v5" => {
                    let trace = source
                        .ok_or_else(|| anyhow!("trace_v5 input {slot} needs a trace digest"))?;
                    let kind = projection
                        .clone()
                        .unwrap_or_else(|| CHART_DEFAULT_PROJECTION.to_string());
                    let resolved = crate::data::DataStore::new(
                        self.db.clone(),
                        self.content.clone(),
                    )
                    .resolve_trace_projection(trace.to_string(), kind.clone())
                    .await?;
                    receipt["source"] = json!(trace);
                    receipt["projection"] = json!(kind);
                    receipt["digest"] = json!(resolved.payload_digest);
                    receipt["schema"] = json!(resolved.projection_schema);
                    resolved.payload
                }
                "query_snapshot" => {
                    let snapshot_id = source
                        .ok_or_else(|| anyhow!("query_snapshot input {slot} needs a snapshot id"))?;
                    let snapshot = crate::data::DataStore::new(
                        self.db.clone(),
                        self.content.clone(),
                    )
                    .query_snapshot(snapshot_id.to_string())
                    .await?;
                    receipt["source"] = json!(snapshot_id);
                    receipt["digest"] = json!(snapshot.result_digest);
                    serde_json::to_value(snapshot)?
                }
                "optimizer_run" => {
                    let run_id = source
                        .ok_or_else(|| anyhow!("optimizer_run input {slot} needs a run id"))?;
                    let service = self.optimizer_runs.get().ok_or_else(|| {
                        anyhow!("this runtime has no optimizer service attached, so input {slot} cannot be read")
                    })?;
                    // The typed result points at the per-trial ledger
                    // (`evidenceRefs.records`) rather than carrying it, and a
                    // chart of ten trials needs the rows, not their count. The
                    // bound document is both: the record, whose summary holds
                    // the ledger, and the typed result beside it.
                    let record = service.get(run_id.to_string()).await?;
                    let result = service.get_result(run_id.to_string()).await?;
                    let document = json!({
                        "schemaVersion": OPTIMIZER_RUN_DOCUMENT_SCHEMA,
                        "run": serde_json::to_value(&record)?,
                        "result": result,
                    });
                    // A run that has not sealed is still readable, but the
                    // reading is a snapshot: record the cursor it was taken at
                    // and the digest of what was taken, so a chart drawn twice
                    // from a moving run can be told apart afterwards.
                    merge_receipt(
                        &mut receipt,
                        optimizer_run_receipt(run_id, &document, document.pointer("/result")),
                    );
                    document
                }
                other => bail!(
                    "input {slot} is bound as {other}, which a chart cannot read; supported kinds are inline, fixture, local_cas, trace_v5, query_snapshot, optimizer_run"
                ),
            };
            provenance.insert(slot.clone(), receipt);
            documents.insert(slot.clone(), document);
        }
        Ok((documents, Value::Object(provenance)))
    }

    pub async fn render_chart(&self, id: &str) -> Result<VisualRecord> {
        let visual = self.get(id.to_string()).await?;
        if !charts::is_chart_template(&visual.template_id) {
            bail!("visual {id} is not a chart");
        }
        let digest = visual
            .content_digest
            .clone()
            .ok_or_else(|| anyhow!("{} requires content", visual.template_id))?;
        let source = String::from_utf8(self.content.get_bytes("blobs", &digest)?)
            .context("chart spec must be UTF-8")?;
        let spec = charts::parse_and_validate(&source)?;
        let wanted = chart_data::required_slots(&spec);
        // Evidence that cannot be read is a state of the visual, not a bad
        // request: it is committed as a render failure with the reason, so the
        // author sees it on the pane and through the tool result.
        let (spec, provenance) = if wanted.is_empty() {
            (spec, Value::Null)
        } else {
            match self.chart_documents(&visual, &wanted).await {
                Ok((documents, provenance)) => match chart_data::resolve(&spec, &documents)
                    .and_then(|resolved| {
                        charts::validate_spec(&resolved)?;
                        Ok(resolved)
                    }) {
                    Ok(resolved) => (resolved, provenance),
                    Err(error) => return self.commit_chart_failure(visual, error).await,
                },
                Err(error) => return self.commit_chart_failure(visual, error).await,
            }
        };
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
                    "visualKind": "chart",
                    "panelCount": spec.panels.len(),
                }),
                remote_sequence: None,
                command_id: None,
                created_at: None,
            })
            .await;
        let theme = spec.theme.clone();
        let findings = charts::authoring_findings_for(&spec);
        let spec_for_render = spec.clone();
        let rendered = tokio::task::spawn_blocking(move || charts::render_spec(&spec_for_render))
            .await
            .context("join chart render")?;
        match rendered {
            Ok(chart) => {
                self.commit_chart_success(visual, theme, chart, provenance, findings)
                    .await
            }
            Err(error) => self.commit_chart_failure(visual, error).await,
        }
    }

    async fn commit_chart_success(
        &self,
        mut visual: VisualRecord,
        theme: String,
        chart: charts::RenderedChart,
        provenance: Value,
        findings: Vec<String>,
    ) -> Result<VisualRecord> {
        validate_svg_bytes(chart.svg.as_bytes())?;
        let preview_digest = self.content.put_bytes("previews", chart.svg.as_bytes())?;
        visual.preview_digest = Some(preview_digest.clone());
        visual.updated_at = Utc::now().to_rfc3339();
        if let Some(object) = visual.metadata.as_object_mut() {
            object.insert("mediaType".into(), json!(charts::MEDIA_TYPE_SOURCE));
            object.insert("visualKind".into(), json!("chart"));
            object.insert("rendererVersion".into(), json!(charts::RENDERER_VERSION));
            object.insert("specSchema".into(), json!(charts::SCHEMA_VERSION));
            object.insert("renderStatus".into(), json!("ready"));
            object.insert("renderWidthPx".into(), json!(chart.width));
            object.insert("renderHeightPx".into(), json!(chart.height));
            // Findings and provenance are recorded against the render that
            // produced them, so the authoring context reads what was drawn
            // rather than re-deriving it from a spec whose evidence may since
            // have moved.
            object.insert("authoringFindings".into(), json!(findings));
            object.insert("dataProvenance".into(), provenance);
            object.remove("renderError");
        }
        let stored = visual.clone();
        let preview = preview_digest.clone();
        let rendered = chart.clone();
        self.db
            .clone()
            .run_transaction(move |conn| {
                persist_visual(conn, &stored)?;
                for size_class in ["pane", "thumbnail"] {
                    renditions::insert_chart_svg_rendition(
                        conn,
                        &stored.id,
                        stored.current_revision,
                        &preview,
                        &rendered,
                        &theme,
                        size_class,
                    )?;
                }
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
                            "visualKind": "chart",
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

    async fn commit_chart_failure(
        &self,
        mut visual: VisualRecord,
        error: anyhow::Error,
    ) -> Result<VisualRecord> {
        let message = error.to_string();
        visual.updated_at = Utc::now().to_rfc3339();
        if let Some(object) = visual.metadata.as_object_mut() {
            object.insert("visualKind".into(), json!("chart"));
            object.insert("rendererVersion".into(), json!(charts::RENDERER_VERSION));
            object.insert("renderStatus".into(), json!("failed"));
            object.insert("renderError".into(), json!(message));
            object.insert("authoringFindings".into(), json!([]));
        }
        let stored = visual.clone();
        let fail_message = message.clone();
        self.db
            .clone()
            .run_transaction(move |conn| {
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
                            "visualKind": "chart",
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
            json!({ "visual_kind": "chart" }),
        );
        self.get(visual.id).await
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

/// Describe a `renderer.html` package well enough for a person to decide.
///
/// Deliberately not a validator. `import_managed_template` owns every rule
/// about what a managed package may be — absolute real directory, two regular
/// non-symlink files, the size cap, the manifest schema, the networkless scan —
/// and runs all of them after the approval. This reads the two facts a card
/// cannot be written without: which id the package claims, and how many bytes of
/// renderer code it is. Anything it cannot read is an error here rather than a
/// card, because a package that will be refused must not cost a click.
fn managed_import_request(
    source_path: &str,
) -> Result<crate::session::template_persist::PersistRequest> {
    use crate::session::template_persist::{PersistDisposition, PersistRequest};
    let source = std::path::Path::new(source_path);
    if !source.is_absolute() {
        bail!("source_path must be an absolute directory");
    }
    let manifest = std::fs::read_to_string(source.join("template.json"))
        .map_err(|_| anyhow!("managed template requires template.json and renderer.html"))?;
    let manifest: Value = serde_json::from_str(&manifest)
        .with_context(|| format!("managed template manifest is not JSON: {source_path}"))?;
    let template_id = manifest
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("managed template manifest declares no id"))?;
    let renderer = std::fs::read(source.join("renderer.html"))
        .map_err(|_| anyhow!("managed template requires template.json and renderer.html"))?;
    let destination = super::templates::user_templates_root().join(template_id);
    // "Something already indexes under this id" is the fact the card needs, and
    // the registry is the only thing that can answer it. A directory that exists
    // but does not index (a scaffold, a refused shape) is not an overwrite of
    // anything a person has seen.
    let overwrites = resolve_template(template_id).is_ok();
    Ok(PersistRequest::new(
        template_id,
        "managed",
        "import",
        &renderer,
        PersistDisposition {
            overwrites,
            forked_from: None,
        },
        &destination,
    ))
}

/// One visual may declare at most one optimizer authority. The generic
/// `VisualRecord.run_id` belongs to the separate `runs` domain, so optimizer
/// identity remains in this typed binding and is never copied into that FK.
fn validate_optimizer_run_bindings(bindings: &Value) -> Result<()> {
    let declared = super::models::declared_optimizer_run_ids(bindings)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if declared.len() > 1 {
        bail!(
            "visual declares conflicting optimizer_run bindings: {}",
            declared.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(())
}

fn refuse_mermaid_stream_slot(bindings: &Value) -> Result<()> {
    let Ok(slots) = binding_descriptors(bindings) else {
        return Ok(());
    };
    for slot in slots {
        if descriptor_input_name(&slot).ok().as_deref() == Some("stream") {
            bail!("diagram.mermaid.v1 must not bind input stream");
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

/// The consumer projection a `trace_v5` binding means when it names none.
///
/// Visible to the seal as well as the renderer: `artifacts.rs` has to derive
/// the same projection a render would, and a second literal would let a seal
/// and a chart quietly disagree about which document a binding points at.
pub(super) const CHART_DEFAULT_PROJECTION: &str = "rollout-inspector";

/// What a chart sees when it binds an optimizer run: the record — whose
/// `summary.records` is the per-trial ledger — beside the typed result.
const OPTIMIZER_RUN_DOCUMENT_SCHEMA: &str = "synth.visual.optimizer-run-document.v1";

/// Fixtures live under the repository's `visuals/` root and nowhere else. The
/// path is joined and then checked against that root, so `..` cannot walk out
/// of it and an absolute path cannot ignore it.
fn read_visual_fixture(relative: &str) -> Result<(Value, String)> {
    let root = super::templates::visuals_root();
    let candidate = root.join(relative);
    let canonical_root = std::fs::canonicalize(&root)
        .with_context(|| format!("visuals root {} is not readable", root.display()))?;
    let canonical = std::fs::canonicalize(&candidate)
        .with_context(|| format!("fixture {relative} not found under {}", root.display()))?;
    if !canonical.starts_with(&canonical_root) {
        bail!("fixture {relative} resolves outside the visuals root");
    }
    let bytes = std::fs::read(&canonical)
        .with_context(|| format!("read fixture {}", canonical.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = format!("sha256:{:x}", hasher.finalize());
    let document = serde_json::from_slice(&bytes)
        .with_context(|| format!("fixture {relative} is not JSON"))?;
    Ok((document, digest))
}

/// What a chart records about the optimizer run it read.
///
/// A sealed run is a fact. An unsealed one is a reading taken at a moment, so
/// it carries the cursor it was taken at and a digest of exactly what was
/// taken — two charts drawn from one moving run can then be told apart instead
/// of silently disagreeing.
fn optimizer_run_receipt(run_id: &str, document: &Value, result: Option<&Value>) -> Value {
    let result = result.unwrap_or(document);
    let sealed = result
        .get("terminalManifest")
        .is_some_and(|value| !value.is_null());
    let mut receipt = json!({
        "source": run_id,
        "sealed": sealed,
        "cursor": result.get("finalCursor").cloned().unwrap_or(Value::Null),
        "digest": digest_json(document),
    });
    if !sealed {
        receipt["snapshotOfLiveRun"] = json!(true);
    }
    receipt
}

fn merge_receipt(receipt: &mut Value, extra: Value) {
    let (Some(target), Some(source)) = (receipt.as_object_mut(), extra.as_object()) else {
        return;
    };
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }
}

/// A chart declares its own theme; the rendition key follows the spec rather
/// than the caller so a pane and a capture never disagree about which one ran.
fn spec_theme(bytes: &[u8]) -> String {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| {
            value
                .get("theme")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|theme| matches!(theme.as_str(), "light" | "dark"))
        .unwrap_or_else(|| "light".into())
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

fn persist_selected_visual(conn: &Connection, session_id: &str, visual_id: &str) -> Result<()> {
    ensure_session(conn, session_id)?;
    let now = Utc::now().to_rfc3339();
    let metadata_json: String = conn
        .query_row(
            "SELECT metadata_json FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or_else(|| "{}".into());
    let mut metadata: Value = serde_json::from_str(&metadata_json).unwrap_or_else(|_| json!({}));
    if let Some(object) = metadata.as_object_mut() {
        object.insert("openVisualId".into(), json!(visual_id));
    } else {
        metadata = json!({ "openVisualId": visual_id });
    }
    conn.execute(
        "UPDATE sessions SET metadata_json = ?2, updated_at = ?3 WHERE id = ?1",
        params![session_id, metadata.to_string(), now],
    )?;
    if let Ok(mut visual) = load_visual(conn, visual_id) {
        let mut visual_metadata = visual.metadata.clone();
        if let Some(object) = visual_metadata.as_object_mut() {
            object.insert("selectedForSession".into(), json!(session_id));
            object.insert("openVisualId".into(), json!(visual_id));
        } else {
            visual_metadata = json!({
                "selectedForSession": session_id,
                "openVisualId": visual_id,
            });
        }
        visual.metadata = visual_metadata;
        visual.updated_at = now;
        persist_visual(conn, &visual)?;
    }
    Ok(())
}

fn load_selected_visual(conn: &Connection, session_id: &str) -> Result<Option<String>> {
    let metadata_json: Option<String> = conn
        .query_row(
            "SELECT metadata_json FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(metadata_json) = metadata_json {
        let metadata: Value = serde_json::from_str(&metadata_json).unwrap_or_else(|_| json!({}));
        if let Some(visual_id) = metadata
            .get("openVisualId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            if load_visual(conn, visual_id).is_ok() {
                return Ok(Some(visual_id.to_string()));
            }
        }
    }
    let listed = list_visuals(
        conn,
        &VisualQuery {
            session_id: Some(session_id.to_string()),
            ..VisualQuery::default()
        },
    )?;
    Ok(listed
        .into_iter()
        .find(|visual| visual.template_id != "synth.subagents.v1")
        .map(|visual| visual.id))
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

