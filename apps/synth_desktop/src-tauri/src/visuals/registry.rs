use super::models::{
    validate_bindings, RendererKind, VisualCreateRequest, VisualQuery, VisualRecord,
    VisualRevision, VisualStatus, VisualUpdateRequest, VISUAL_SCHEMA_VERSION,
};
use super::templates::{resolve_template, TemplateMeta};
use crate::storage::{ContentStore, Database, EventAppend, EventJournal, EventSource};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct VisualRegistry {
    db: Arc<Database>,
    journal: EventJournal,
    content: ContentStore,
}

impl VisualRegistry {
    pub fn new(db: Arc<Database>, journal: EventJournal, content: ContentStore) -> Self {
        Self {
            db,
            journal,
            content,
        }
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
        let bindings = request.bindings.unwrap_or_else(|| json!({}));
        validate_bindings(&bindings)?;
        let status = request.status.unwrap_or(VisualStatus::Draft);
        let renderer_kind = request.renderer_kind.unwrap_or(RendererKind::Template);
        let metadata = request.metadata.unwrap_or_else(|| json!({}));
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
        Ok((record, serde_json::to_value(event)?))
    }

    pub async fn update(
        &self,
        id: String,
        request: VisualUpdateRequest,
    ) -> Result<(VisualRecord, Value)> {
        validate_visual_id(&id)?;
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
                }
                let mut new_bindings = None;
                if let Some(bindings) = request.bindings {
                    validate_bindings(&bindings)?;
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

fn digest_json(value: &Value) -> String {
    let encoded = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(encoded);
    format!("{:x}", hasher.finalize())
}

fn default_tsx_stub(visual: &VisualRecord) -> String {
    format!(
        r#"/** Auto-saved Synth visual instance.
 * visualId: {id}
 * templateId: {template}
 * title: {title}
 */
import Shell from "../templates/{template}/shell";

export const visualId = {id_json};
export const templateId = {template_json};
export const title = {title_json};
export const bindings = {bindings} as const;

export default function VisualInstance(props: Record<string, unknown>) {{
  return <Shell title={{title}} bindings={{bindings}} {{...props}} />;
}}
"#,
        id = visual.id,
        template = visual.template_id,
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
            json!({"kind":"local","model":"laguna-xs-2.1","adapter":null}).to_string(),
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
        let Ok(templates) = registry.list_templates(None) else {
            return;
        };
        let Some(template) = templates.first() else {
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
                template_id: template.id.clone(),
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
        let Ok(templates) = registry.list_templates(None) else {
            return;
        };
        if templates.is_empty() {
            return;
        }
        let template_id = templates[0].id.clone();
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
}
