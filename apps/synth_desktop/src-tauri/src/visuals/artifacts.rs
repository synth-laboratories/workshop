use super::{
    VisualAnnotation, VisualAnnotationCreate, VisualRegistry, VisualSeal, VisualSealBundle,
    VisualUpload,
};
use crate::storage::{EventAppend, EventSource};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

const BUNDLE_SCHEMA: &str = "synth.artifact-bundle.v1";
const COMPILER_NAME: &str = "workshop";
const WORKSHOP_UPLOAD_SCHEMA: &str = "synth.workshop-artifact-upload.v1";
const FROZEN_RUNTIME: &str = r#"(()=>{const d=JSON.parse(document.getElementById('synth-artifact-data').textContent);const root=document.getElementById('app');root.innerHTML='';const h=document.createElement('header');const k=document.createElement('p');k.className='kicker';k.textContent=d.template_id;const t=document.createElement('h1');t.textContent=d.title;h.append(k,t);const s=document.createElement('section');s.className='visual';const pre=document.createElement('pre');pre.textContent=JSON.stringify(d.bindings,null,2);s.append(pre);root.append(h,s);})();"#;

impl VisualRegistry {
    pub async fn annotations(&self, visual_id: String) -> Result<Vec<VisualAnnotation>> {
        let db = self.db.clone();
        db.run(move |conn| {
            let mut statement = conn.prepare(
                "SELECT id, visual_id, visual_revision, source_digest, selector_json, kind,
                        body, metadata_json, author_id, supersedes_id, tombstoned,
                        created_at, updated_at
                 FROM visual_annotations WHERE visual_id = ?1
                 ORDER BY created_at ASC, id ASC",
            )?;
            let rows = statement.query_map([visual_id], annotation_from_row)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Into::into)
        })
        .await
    }

    pub async fn create_annotation(
        &self,
        visual_id: String,
        request: VisualAnnotationCreate,
    ) -> Result<(VisualAnnotation, Value)> {
        validate_annotation_request(&request)?;
        let revisions = self.revisions(visual_id.clone()).await?;
        let target = revisions
            .iter()
            .find(|row| row.revision == request.visual_revision)
            .ok_or_else(|| anyhow!("visual annotation target revision does not exist"))?;
        if let Some(expected) = request.source_digest.as_deref() {
            if target.content_digest.as_deref() != Some(expected)
                && target.bindings_digest.as_deref() != Some(expected)
            {
                bail!("visual annotation source digest does not match target revision");
            }
        }
        let now = Utc::now().to_rfc3339();
        let annotation = VisualAnnotation {
            id: format!("ann_{}", Uuid::new_v4().simple()),
            visual_id: visual_id.clone(),
            visual_revision: request.visual_revision,
            source_digest: request.source_digest,
            selector: request.selector,
            kind: request.kind,
            body: request.body,
            metadata: request.metadata.unwrap_or_else(|| json!({})),
            author_id: request.author_id.unwrap_or_else(|| "user".into()),
            supersedes_id: request.supersedes_id,
            tombstoned: false,
            created_at: now.clone(),
            updated_at: now,
        };
        let db = self.db.clone();
        let stored = annotation.clone();
        let (stored, event) = db.run_transaction(move |conn| {
            if let Some(parent) = stored.supersedes_id.as_deref() {
                let changed = conn.execute(
                    "UPDATE visual_annotations SET tombstoned = 1, updated_at = ?1
                     WHERE id = ?2 AND visual_id = ?3 AND tombstoned = 0",
                    params![stored.updated_at, parent, stored.visual_id],
                )?;
                if changed != 1 { bail!("annotation supersession target is missing or stale"); }
            }
            conn.execute(
                "INSERT INTO visual_annotations(
                    id, visual_id, visual_revision, source_digest, selector_json, kind,
                    body, metadata_json, author_id, supersedes_id, tombstoned, created_at, updated_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0,?11,?12)",
                params![
                    stored.id, stored.visual_id, stored.visual_revision, stored.source_digest,
                    serde_json::to_string(&stored.selector)?, stored.kind, stored.body,
                    serde_json::to_string(&stored.metadata)?, stored.author_id,
                    stored.supersedes_id, stored.created_at, stored.updated_at,
                ],
            )?;
            let event = crate::storage::append_event(conn, EventAppend {
                event_id: None,
                session_id: None,
                run_id: None,
                source: EventSource::Visual,
                kind: "visual.annotation.created".into(),
                payload: json!({
                    "visualId": stored.visual_id,
                    "visualRevision": stored.visual_revision,
                    "annotationId": stored.id,
                    "kind": stored.kind,
                }),
                remote_sequence: None,
                command_id: None,
                created_at: None,
            })?;
            Ok((stored, serde_json::to_value(event)?))
        }).await?;
        Ok((stored, event))
    }

    pub async fn overlay_digest(&self, visual_id: String, revision: i64) -> Result<String> {
        let annotations = self.annotations(visual_id).await?;
        let active = annotations
            .into_iter()
            .filter(|row| !row.tombstoned && row.visual_revision <= revision)
            .collect::<Vec<_>>();
        Ok(hex_sha256(&canonical_json(&serde_json::to_value(active)?)?))
    }

    pub async fn seal(&self, visual_id: String, revision: i64) -> Result<(VisualSeal, Value)> {
        let visual = self.get(visual_id.clone()).await?;
        let gate = visual
            .metadata
            .get("qualityGate")
            .filter(|gate| gate.get("ready").and_then(Value::as_bool) == Some(true))
            .filter(|gate| gate.get("revision").and_then(Value::as_i64) == Some(revision))
            .ok_or_else(|| anyhow!("visual revision has not passed the E1 quality gate"))?;
        let _ = gate;
        let source = self
            .revisions(visual_id.clone())
            .await?
            .into_iter()
            .find(|row| row.revision == revision)
            .ok_or_else(|| anyhow!("visual seal target revision does not exist"))?;
        let bindings = source
            .bindings
            .clone()
            .unwrap_or_else(|| visual.bindings.clone());
        let frozen_bindings = freeze_bindings(bindings)?;
        let annotations = self
            .annotations(visual_id.clone())
            .await?
            .into_iter()
            .filter(|row| !row.tombstoned && row.visual_revision <= revision)
            .collect::<Vec<_>>();
        let overlay_value = serde_json::to_value(&annotations)?;
        let overlay_digest = hex_sha256(&canonical_json(&overlay_value)?);
        let bindings_digest = source
            .bindings_digest
            .clone()
            .unwrap_or_else(|| hex_sha256(&canonical_json(&frozen_bindings).unwrap_or_default()));
        let artifact_id = format!("visual:{}", visual.id);
        let builder_run_id = format!("seal:{}:{}", visual.id, revision);
        let source_identity = json!({
            "visual_id": visual.id,
            "revision": revision,
            "content_digest": source.content_digest,
            "bindings_digest": bindings_digest,
            "overlay_digest": overlay_digest,
            "source_run_id": visual.run_id,
            "builder_run_id": builder_run_id,
        });
        let limitations = declared_limitations(&frozen_bindings);
        let data = json!({
            "schema_version": BUNDLE_SCHEMA,
            "artifact_id": artifact_id,
            "source": source_identity,
            "template_id": source.template_id,
            "renderer_kind": source.renderer_kind.as_str(),
            "title": visual.title,
            "bindings": frozen_bindings,
            "evidence": evidence_refs(&visual),
            "overlays": overlay_value,
            "claims": [],
            "limitations": limitations,
        });
        scan_forbidden(&data, "$")?;
        let data_bytes = canonical_json(&data)?;
        let runtime_digest = hex_sha256(FROZEN_RUNTIME.as_bytes());
        let index_html = build_index_html(&data, &runtime_digest)?;
        refuse_network_html(&index_html)?;
        let index_bytes = index_html.as_bytes();
        let data_digest = hex_sha256(&data_bytes);
        let index_digest = hex_sha256(index_bytes);
        let receipt = json!({
            "schema_version": BUNDLE_SCHEMA,
            "artifact_id": data["artifact_id"],
            "source": data["source"],
            "compiler": {
                "name": COMPILER_NAME,
                "version": env!("CARGO_PKG_VERSION"),
                "runtime_digest": runtime_digest,
            },
            "members": [
                {"logical_path":"data.json","digest_sha256":data_digest,"size_bytes":data_bytes.len(),"media_type":"application/vnd.synth.artifact-bundle-data+json"},
                {"logical_path":"index.html","digest_sha256":index_digest,"size_bytes":index_bytes.len(),"media_type":"text/html; charset=utf-8"}
            ]
        });
        let receipt_bytes = canonical_json(&receipt)?;
        let receipt_digest = hex_sha256(&receipt_bytes);
        let stored_index = self.content.put_bytes("artifact_bundles", index_bytes)?;
        let stored_data = self.content.put_bytes("artifact_bundles", &data_bytes)?;
        let stored_receipt = self.content.put_bytes("artifact_bundles", &receipt_bytes)?;
        if stored_index != index_digest
            || stored_data != data_digest
            || stored_receipt != receipt_digest
        {
            bail!("local artifact CAS digest verification failed");
        }
        let now = Utc::now().to_rfc3339();
        let seal = VisualSeal {
            receipt_digest: receipt_digest.clone(),
            visual_id: visual_id.clone(),
            visual_revision: revision,
            artifact_id: data["artifact_id"].as_str().unwrap_or_default().to_string(),
            schema_version: BUNDLE_SCHEMA.into(),
            compiler_name: COMPILER_NAME.into(),
            compiler_version: env!("CARGO_PKG_VERSION").into(),
            runtime_digest: receipt["compiler"]["runtime_digest"]
                .as_str()
                .unwrap_or_default()
                .into(),
            index_digest,
            data_digest,
            receipt_size_bytes: receipt_bytes.len() as i64,
            total_size_bytes: (index_bytes.len() + data_bytes.len() + receipt_bytes.len()) as i64,
            created_at: now,
        };
        let db = self.db.clone();
        let stored = seal.clone();
        let (stored, event) = db.run_transaction(move |conn| {
            conn.execute(
                "INSERT OR IGNORE INTO visual_seals(
                    receipt_digest,visual_id,visual_revision,artifact_id,schema_version,
                    compiler_name,compiler_version,runtime_digest,index_digest,data_digest,
                    receipt_size_bytes,total_size_bytes,created_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![stored.receipt_digest, stored.visual_id, stored.visual_revision,
                    stored.artifact_id, stored.schema_version, stored.compiler_name,
                    stored.compiler_version, stored.runtime_digest, stored.index_digest,
                    stored.data_digest, stored.receipt_size_bytes, stored.total_size_bytes,
                    stored.created_at],
            )?;
            let event = crate::storage::append_event(conn, EventAppend {
                event_id: None, session_id: None, run_id: None, source: EventSource::Visual,
                kind: "visual.sealed".into(),
                payload: json!({"visualId":stored.visual_id,"revision":stored.visual_revision,"receiptDigest":stored.receipt_digest}),
                remote_sequence: None, command_id: None, created_at: None,
            })?;
            Ok((stored, serde_json::to_value(event)?))
        }).await?;
        Ok((stored, event))
    }

    pub async fn list_seals(&self, visual_id: Option<String>) -> Result<Vec<VisualSeal>> {
        let db = self.db.clone();
        db.run(move |conn| {
            let sql = if visual_id.is_some() {
                "SELECT receipt_digest,visual_id,visual_revision,artifact_id,schema_version,compiler_name,compiler_version,runtime_digest,index_digest,data_digest,receipt_size_bytes,total_size_bytes,created_at FROM visual_seals WHERE visual_id = ?1 ORDER BY visual_revision DESC, created_at DESC"
            } else {
                "SELECT receipt_digest,visual_id,visual_revision,artifact_id,schema_version,compiler_name,compiler_version,runtime_digest,index_digest,data_digest,receipt_size_bytes,total_size_bytes,created_at FROM visual_seals ORDER BY created_at DESC"
            };
            let mut statement = conn.prepare(sql)?;
            let rows = if let Some(id) = visual_id {
                statement.query_map([id], seal_from_row)?.collect::<std::result::Result<Vec<_>, _>>()?
            } else {
                statement.query_map([], seal_from_row)?.collect::<std::result::Result<Vec<_>, _>>()?
            };
            Ok(rows)
        }).await
    }

    pub async fn get_seal(&self, receipt_digest: String) -> Result<VisualSealBundle> {
        let db = self.db.clone();
        let lookup = receipt_digest.clone();
        let seal = db.run(move |conn| {
            conn.query_row(
                "SELECT receipt_digest,visual_id,visual_revision,artifact_id,schema_version,compiler_name,compiler_version,runtime_digest,index_digest,data_digest,receipt_size_bytes,total_size_bytes,created_at FROM visual_seals WHERE receipt_digest = ?1",
                [lookup], seal_from_row,
            ).optional()?.ok_or_else(|| anyhow!("visual seal does not exist"))
        }).await?;
        let index_html = String::from_utf8(
            self.content
                .get_bytes("artifact_bundles", &seal.index_digest)?,
        )
        .context("sealed index.html must be UTF-8")?;
        let data: Value = serde_json::from_slice(
            &self
                .content
                .get_bytes("artifact_bundles", &seal.data_digest)?,
        )?;
        let receipt: Value = serde_json::from_slice(
            &self
                .content
                .get_bytes("artifact_bundles", &seal.receipt_digest)?,
        )?;
        Ok(VisualSealBundle {
            seal,
            index_html,
            data,
            receipt,
        })
    }

    pub async fn open_shared_url(
        &self,
        committed_url: String,
        backend_url: String,
        api_key: String,
    ) -> Result<VisualSealBundle> {
        let backend = reqwest::Url::parse(backend_url.trim_end_matches('/'))
            .context("configured Synth backend URL is invalid")?;
        let shared =
            reqwest::Url::parse(committed_url.trim()).context("private artifact URL is invalid")?;
        if shared.scheme() != backend.scheme()
            || shared.host_str() != backend.host_str()
            || shared.port_or_known_default() != backend.port_or_known_default()
            || shared.query().is_some()
            || shared.fragment().is_some()
        {
            bail!("private artifact URL must use the configured Synth backend origin");
        }
        let backend_path = backend.path().trim_end_matches('/');
        let route_prefix = format!("{backend_path}/artifacts/v1/publications/");
        if !shared.path().starts_with(&route_prefix)
            || !shared.path().ends_with("/assets/index.html")
        {
            bail!("private artifact URL is not an immutable Artifact publication URL");
        }
        if api_key.trim().is_empty() {
            bail!("opening a private shared visual requires a signed-in Synth account");
        }
        let asset_root = committed_url
            .trim()
            .strip_suffix("index.html")
            .ok_or_else(|| anyhow!("private artifact URL must end in index.html"))?;
        let client = reqwest::Client::new();
        let mut fetched = BTreeMap::new();
        for logical_path in ["receipt.json", "data.json", "index.html"] {
            let response = client
                .get(format!("{asset_root}{logical_path}"))
                .bearer_auth(&api_key)
                .send()
                .await
                .with_context(|| format!("fetch private {logical_path}"))?;
            if !response.status().is_success() {
                bail!(
                    "fetch private {logical_path} failed ({})",
                    response.status()
                );
            }
            let bytes = response.bytes().await?;
            if bytes.len() > 64 * 1024 * 1024 {
                bail!("private {logical_path} exceeds the 64 MiB viewer limit");
            }
            fetched.insert(logical_path, bytes.to_vec());
        }
        validate_hosted_bundle(
            fetched.remove("index.html").unwrap_or_default(),
            fetched.remove("data.json").unwrap_or_default(),
            fetched.remove("receipt.json").unwrap_or_default(),
        )
    }

    pub async fn upload_status(&self, receipt_digest: String) -> Result<Option<VisualUpload>> {
        let db = self.db.clone();
        db.run(move |conn| {
            conn.query_row(
                "SELECT receipt_digest,collection_id,publication_id,publication_revision,state,committed_url,error,updated_at
                 FROM visual_uploads WHERE receipt_digest = ?1",
                [receipt_digest],
                upload_from_row,
            )
            .optional()
            .map_err(Into::into)
        })
        .await
    }

    pub async fn share_seal(
        &self,
        receipt_digest: String,
        backend_url: String,
        api_key: String,
    ) -> Result<(VisualUpload, Value)> {
        let bundle = self.get_seal(receipt_digest.clone()).await?;
        if let Some(existing) = self.upload_status(receipt_digest.clone()).await? {
            if existing.state == "committed" {
                return Ok((existing, json!({"kind":"visual.upload.idempotent"})));
            }
        }
        let members = BTreeMap::from([
            (
                "data.json",
                (
                    self.content
                        .get_bytes("artifact_bundles", &bundle.seal.data_digest)?,
                    bundle.seal.data_digest.clone(),
                    "application/vnd.synth.artifact-bundle-data+json",
                ),
            ),
            (
                "index.html",
                (
                    self.content
                        .get_bytes("artifact_bundles", &bundle.seal.index_digest)?,
                    bundle.seal.index_digest.clone(),
                    "text/html; charset=utf-8",
                ),
            ),
            (
                "receipt.json",
                (
                    self.content
                        .get_bytes("artifact_bundles", &bundle.seal.receipt_digest)?,
                    bundle.seal.receipt_digest.clone(),
                    "application/vnd.synth.artifact-bundle-receipt+json",
                ),
            ),
        ]);
        for (path, (bytes, digest, _)) in &members {
            if hex_sha256(bytes) != *digest {
                bail!("local {path} no longer matches its sealed digest");
            }
        }
        self.write_upload(VisualUpload {
            receipt_digest: receipt_digest.clone(),
            collection_id: None,
            publication_id: None,
            publication_revision: None,
            state: "prepared".into(),
            committed_url: None,
            error: None,
            updated_at: Utc::now().to_rfc3339(),
        })
        .await?;
        let result = self
            .perform_upload(
                &bundle.seal,
                &members,
                backend_url.trim_end_matches('/'),
                &api_key,
            )
            .await;
        let upload = match result {
            Ok(upload) => upload,
            Err(error) => {
                self.write_upload(VisualUpload {
                    receipt_digest: receipt_digest.clone(),
                    collection_id: None,
                    publication_id: None,
                    publication_revision: None,
                    state: "failed".into(),
                    committed_url: None,
                    error: Some(error.to_string()),
                    updated_at: Utc::now().to_rfc3339(),
                })
                .await?;
                return Err(error);
            }
        };
        let db = self.db.clone();
        let stored = upload.clone();
        let (stored, event) = db
            .run_transaction(move |conn| {
                upsert_upload(conn, &stored)?;
                let event = crate::storage::append_event(
                    conn,
                    EventAppend {
                        event_id: None,
                        session_id: None,
                        run_id: None,
                        source: EventSource::Visual,
                        kind: "visual.upload.committed".into(),
                        payload: json!({
                            "receiptDigest": stored.receipt_digest,
                            "publicationId": stored.publication_id,
                            "committedUrl": stored.committed_url,
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

    async fn perform_upload(
        &self,
        seal: &VisualSeal,
        members: &BTreeMap<&str, (Vec<u8>, String, &str)>,
        backend_url: &str,
        api_key: &str,
    ) -> Result<VisualUpload> {
        if api_key.trim().is_empty() {
            bail!("Share requires a signed-in Synth account");
        }
        let client = crate::http::http_client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(90))
            .build()?;
        let declarations = members
            .iter()
            .map(|(path, (bytes, digest, media_type))| {
                json!({
                    "logical_path": path,
                    "digest_sha256": digest,
                    "size_bytes": bytes.len(),
                    "media_type": media_type,
                })
            })
            .collect::<Vec<_>>();
        let prepare_url = format!("{backend_url}/artifacts/v1/workshop/prepare");
        let response = client
            .post(&prepare_url)
            .bearer_auth(api_key)
            .json(&json!({
                "schema_version": WORKSHOP_UPLOAD_SCHEMA,
                "visual_id": seal.visual_id,
                "visual_revision": seal.visual_revision,
                "receipt_digest": seal.receipt_digest,
                "bundle_schema_version": seal.schema_version,
                "objects": declarations,
            }))
            .send()
            .await
            .context("prepare private visual upload")?;
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            bail!(
                "prepare private visual upload failed ({status}): {}",
                String::from_utf8_lossy(&body)
            );
        }
        let prepared: WorkshopPrepareResponse = serde_json::from_slice(&body)?;
        self.write_upload(VisualUpload {
            receipt_digest: seal.receipt_digest.clone(),
            collection_id: Some(prepared.collection_id.clone()),
            publication_id: Some(prepared.publication_id.clone()),
            publication_revision: Some(prepared.revision),
            state: "uploading".into(),
            committed_url: None,
            error: None,
            updated_at: Utc::now().to_rfc3339(),
        })
        .await?;
        for target in &prepared.upload_targets {
            let (bytes, digest, _) = members
                .get(target.logical_path.as_str())
                .ok_or_else(|| anyhow!("backend requested an undeclared bundle member"))?;
            if target.digest_sha256 != *digest {
                bail!(
                    "backend upload target digest differs for {}",
                    target.logical_path
                );
            }
            let mut request = client.put(&target.upload_url).body(bytes.clone());
            for (name, value) in &target.required_headers {
                request = request.header(name, value);
            }
            let response = request
                .send()
                .await
                .context("upload sealed bundle member")?;
            if !response.status().is_success() {
                bail!(
                    "upload failed for {} ({})",
                    target.logical_path,
                    response.status()
                );
            }
        }
        self.write_upload(VisualUpload {
            receipt_digest: seal.receipt_digest.clone(),
            collection_id: Some(prepared.collection_id.clone()),
            publication_id: Some(prepared.publication_id.clone()),
            publication_revision: Some(prepared.revision),
            state: "finalizing".into(),
            committed_url: None,
            error: None,
            updated_at: Utc::now().to_rfc3339(),
        })
        .await?;
        let finalize_url = format!(
            "{backend_url}/artifacts/v1/workshop/{}/finalize",
            prepared.publication_id
        );
        let response = client
            .post(finalize_url)
            .bearer_auth(api_key)
            .json(&json!({
                "schema_version": WORKSHOP_UPLOAD_SCHEMA,
                "visual_id": seal.visual_id,
                "receipt_digest": seal.receipt_digest,
            }))
            .send()
            .await
            .context("finalize private visual upload")?;
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            bail!(
                "finalize private visual upload failed ({status}): {}",
                String::from_utf8_lossy(&body)
            );
        }
        let committed: WorkshopCommittedResponse = serde_json::from_slice(&body)?;
        if committed.status != "committed"
            || committed.publication_id != prepared.publication_id
            || committed.collection_id != prepared.collection_id
            || committed.revision != prepared.revision
            || committed.manifest_digest != prepared.manifest_digest
        {
            bail!("committed upload identity differs from its preparation");
        }
        let committed_url = if committed.committed_url.starts_with('/') {
            format!("{backend_url}{}", committed.committed_url)
        } else {
            committed.committed_url
        };
        Ok(VisualUpload {
            receipt_digest: seal.receipt_digest.clone(),
            collection_id: Some(committed.collection_id),
            publication_id: Some(committed.publication_id),
            publication_revision: Some(committed.revision),
            state: "committed".into(),
            committed_url: Some(committed_url),
            error: None,
            updated_at: Utc::now().to_rfc3339(),
        })
    }

    async fn write_upload(&self, upload: VisualUpload) -> Result<()> {
        let db = self.db.clone();
        db.run(move |conn| upsert_upload(conn, &upload)).await
    }
}

#[derive(Debug, serde::Deserialize)]
struct WorkshopUploadTarget {
    logical_path: String,
    digest_sha256: String,
    upload_url: String,
    required_headers: BTreeMap<String, String>,
}

#[derive(Debug, serde::Deserialize)]
struct WorkshopPrepareResponse {
    publication_id: String,
    collection_id: String,
    revision: i64,
    manifest_digest: String,
    upload_targets: Vec<WorkshopUploadTarget>,
}

#[derive(Debug, serde::Deserialize)]
struct WorkshopCommittedResponse {
    publication_id: String,
    collection_id: String,
    revision: i64,
    manifest_digest: String,
    status: String,
    committed_url: String,
}

fn annotation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VisualAnnotation> {
    let selector: String = row.get(4)?;
    let metadata: String = row.get(7)?;
    Ok(VisualAnnotation {
        id: row.get(0)?,
        visual_id: row.get(1)?,
        visual_revision: row.get(2)?,
        source_digest: row.get(3)?,
        selector: serde_json::from_str(&selector).unwrap_or(Value::Null),
        kind: row.get(5)?,
        body: row.get(6)?,
        metadata: serde_json::from_str(&metadata).unwrap_or_else(|_| json!({})),
        author_id: row.get(8)?,
        supersedes_id: row.get(9)?,
        tombstoned: row.get::<_, i64>(10)? != 0,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn seal_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VisualSeal> {
    Ok(VisualSeal {
        receipt_digest: row.get(0)?,
        visual_id: row.get(1)?,
        visual_revision: row.get(2)?,
        artifact_id: row.get(3)?,
        schema_version: row.get(4)?,
        compiler_name: row.get(5)?,
        compiler_version: row.get(6)?,
        runtime_digest: row.get(7)?,
        index_digest: row.get(8)?,
        data_digest: row.get(9)?,
        receipt_size_bytes: row.get(10)?,
        total_size_bytes: row.get(11)?,
        created_at: row.get(12)?,
    })
}

fn upload_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VisualUpload> {
    Ok(VisualUpload {
        receipt_digest: row.get(0)?,
        collection_id: row.get(1)?,
        publication_id: row.get(2)?,
        publication_revision: row.get(3)?,
        state: row.get(4)?,
        committed_url: row.get(5)?,
        error: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn upsert_upload(conn: &rusqlite::Connection, upload: &VisualUpload) -> Result<()> {
    conn.execute(
        "INSERT INTO visual_uploads(
            receipt_digest,collection_id,publication_id,publication_revision,
            prepare_expires_at,completed_members_json,state,committed_url,error,updated_at
         ) VALUES (?1,?2,?3,?4,NULL,'[]',?5,?6,?7,?8)
         ON CONFLICT(receipt_digest) DO UPDATE SET
            collection_id=excluded.collection_id,
            publication_id=excluded.publication_id,
            publication_revision=excluded.publication_revision,
            state=excluded.state,
            committed_url=excluded.committed_url,
            error=excluded.error,
            updated_at=excluded.updated_at",
        params![
            upload.receipt_digest,
            upload.collection_id,
            upload.publication_id,
            upload.publication_revision,
            upload.state,
            upload.committed_url,
            upload.error,
            upload.updated_at,
        ],
    )?;
    Ok(())
}

fn validate_annotation_request(request: &VisualAnnotationCreate) -> Result<()> {
    if !matches!(
        request.kind.as_str(),
        "note" | "bug" | "highlight" | "reward" | "acceptance"
    ) {
        bail!("unsupported visual annotation kind");
    }
    let selector_type = request
        .selector
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("visual annotation selector requires type"))?;
    if !matches!(
        selector_type,
        "frame" | "span" | "candidate" | "trial" | "chart_mark"
    ) {
        bail!("unsupported visual annotation selector type");
    }
    if request.body.as_deref().map(str::len).unwrap_or(0) > 4096 {
        bail!("visual annotation body exceeds 4096 bytes");
    }
    Ok(())
}

fn freeze_bindings(mut value: Value) -> Result<Value> {
    fn walk(value: &mut Value) -> Result<()> {
        match value {
            Value::Object(object) => {
                if object.get("kind").and_then(Value::as_str) == Some("live_sse") {
                    let snapshot = object
                        .remove("snapshot")
                        .ok_or_else(|| anyhow!("live SSE binding has no frozen snapshot"))?;
                    object.insert("kind".into(), Value::String("inline".into()));
                    object.insert("data".into(), snapshot);
                    object.remove("source");
                    object.remove("poll_url");
                    object.remove("pollUrl");
                }
                for child in object.values_mut() {
                    walk(child)?;
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    walk(&mut value)?;
    Ok(value)
}

fn evidence_refs(visual: &super::VisualRecord) -> Vec<Value> {
    let mut values = Vec::new();
    if let Some(trace) = visual.trace_id.as_ref() {
        values.push(json!({"kind":"trace_v5","id":trace}));
    }
    if let Some(run) = visual.run_id.as_ref() {
        values.push(json!({"kind":"run","id":run}));
    }
    values
}

fn declared_limitations(value: &Value) -> Vec<String> {
    fn walk(value: &Value, path: &str, out: &mut Vec<String>) {
        match value {
            Value::Null => out.push(format!("{path} is missing")),
            Value::Object(object) => {
                for (key, child) in object {
                    walk(child, &format!("{path}.{key}"), out);
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    walk(child, &format!("{path}[{index}]"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, "bindings", &mut out);
    out
}

fn scan_forbidden(value: &Value, path: &str) -> Result<()> {
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
                    "environment",
                    "hidden_reasoning",
                    "password",
                    "process_env",
                    "provider_url",
                    "refresh_token",
                    "secret",
                    "storage_uri",
                    "object_key",
                ]
                .iter()
                .any(|needle| normalized.contains(needle))
                {
                    bail!("seal policy forbids {path}.{key}");
                }
                scan_forbidden(child, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                scan_forbidden(child, &format!("{path}[{index}]"))?;
            }
        }
        Value::String(text)
            if text.contains("s3://")
                || text.contains("gs://")
                || text.contains("AWS_ACCESS_KEY_ID")
                || text.contains("AWS_SECRET_ACCESS_KEY") =>
        {
            bail!("seal policy forbids storage or credential locator at {path}")
        }
        _ => {}
    }
    Ok(())
}

fn build_index_html(data: &Value, runtime_digest: &str) -> Result<String> {
    let inline = serde_json::to_string(data)?.replace("</script", "<\\/script");
    Ok(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; font-src 'none'; connect-src 'none'; frame-src 'none';"><title>Sealed Workshop visual</title><style>html{{font:14px system-ui;color:#20232a;background:#f7f7f5}}body{{margin:0}}#app{{max-width:1100px;margin:auto;padding:32px}}.kicker{{color:#6b7280}}h1{{font-size:30px}}.visual{{background:white;border:1px solid #ddd;border-radius:14px;padding:20px}}pre{{white-space:pre-wrap;overflow-wrap:anywhere}}</style></head><body><main id="app"></main><script id="synth-artifact-data" type="application/json">{inline}</script><script data-runtime-digest="{runtime_digest}">{FROZEN_RUNTIME}</script></body></html>"#
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
            bail!("sealed index.html contains forbidden network capability: {token}");
        }
    }
    Ok(())
}

fn validate_hosted_bundle(
    index_bytes: Vec<u8>,
    data_bytes: Vec<u8>,
    receipt_bytes: Vec<u8>,
) -> Result<VisualSealBundle> {
    let data: Value = serde_json::from_slice(&data_bytes).context("decode hosted data.json")?;
    let receipt: Value =
        serde_json::from_slice(&receipt_bytes).context("decode hosted receipt.json")?;
    if canonical_json(&data)? != data_bytes || canonical_json(&receipt)? != receipt_bytes {
        bail!("hosted ArtifactBundle JSON is not canonical");
    }
    if data.get("schema_version").and_then(Value::as_str) != Some(BUNDLE_SCHEMA)
        || receipt.get("schema_version").and_then(Value::as_str) != Some(BUNDLE_SCHEMA)
    {
        bail!("hosted bundle is not ArtifactBundle v1");
    }
    if data.get("artifact_id") != receipt.get("artifact_id")
        || data.get("source") != receipt.get("source")
    {
        bail!("hosted data and receipt identities differ");
    }
    let members = receipt
        .get("members")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("hosted receipt has no members"))?;
    if members.len() != 2 {
        bail!("hosted receipt must declare exactly index.html and data.json");
    }
    let declaration = |path: &str| -> Result<&Value> {
        members
            .iter()
            .find(|member| member.get("logical_path").and_then(Value::as_str) == Some(path))
            .ok_or_else(|| anyhow!("hosted receipt does not declare {path}"))
    };
    let index_declaration = declaration("index.html")?;
    let data_declaration = declaration("data.json")?;
    let index_digest = hex_sha256(&index_bytes);
    let data_digest = hex_sha256(&data_bytes);
    for (path, declaration, bytes, digest) in [
        ("index.html", index_declaration, &index_bytes, &index_digest),
        ("data.json", data_declaration, &data_bytes, &data_digest),
    ] {
        if declaration.get("digest_sha256").and_then(Value::as_str) != Some(digest)
            || declaration.get("size_bytes").and_then(Value::as_u64) != Some(bytes.len() as u64)
        {
            bail!("hosted {path} does not match its receipt");
        }
    }
    let index_html = String::from_utf8(index_bytes).context("hosted index.html must be UTF-8")?;
    refuse_network_html(&index_html)?;
    let receipt_digest = hex_sha256(&receipt_bytes);
    let source = data
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("hosted data has no source identity"))?;
    let compiler = receipt
        .get("compiler")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("hosted receipt has no compiler identity"))?;
    let required_string = |object: &Map<String, Value>, key: &str| -> Result<String> {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("hosted bundle identity is missing {key}"))
    };
    let seal = VisualSeal {
        receipt_digest,
        visual_id: required_string(source, "visual_id")?,
        visual_revision: source
            .get("revision")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("hosted bundle identity is missing revision"))?,
        artifact_id: data
            .get("artifact_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("hosted bundle identity is missing artifact_id"))?
            .to_owned(),
        schema_version: BUNDLE_SCHEMA.into(),
        compiler_name: required_string(compiler, "name")?,
        compiler_version: required_string(compiler, "version")?,
        runtime_digest: required_string(compiler, "runtime_digest")?,
        index_digest,
        data_digest,
        receipt_size_bytes: receipt_bytes.len() as i64,
        total_size_bytes: (index_html.len() + data_bytes.len() + receipt_bytes.len()) as i64,
        created_at: Utc::now().to_rfc3339(),
    };
    Ok(VisualSealBundle {
        seal,
        index_html,
        data,
        receipt,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{ContentStore, EventJournal, Storage};
    use crate::visuals::{VisualCreateRequest, VisualStatus, VisualUpdateRequest};
    use tempfile::tempdir;

    #[test]
    fn live_binding_requires_snapshot_and_removes_stream_urls() {
        let frozen = freeze_bindings(json!({"slots":[{"kind":"live_sse","source":"http://127.0.0.1/events","snapshot":{"reward":null}}]})).unwrap();
        assert_eq!(frozen["slots"][0]["kind"], "inline");
        assert!(frozen["slots"][0].get("source").is_none());
        assert!(frozen["slots"][0]["data"]["reward"].is_null());
        assert!(freeze_bindings(json!({"kind":"live_sse","source":"x"})).is_err());
    }

    #[test]
    fn canonical_json_is_stable_and_keeps_null() {
        let a = canonical_json(&json!({"z":null,"a":1})).unwrap();
        let b = canonical_json(&json!({"a":1,"z":null})).unwrap();
        assert_eq!(a, b);
        assert_eq!(String::from_utf8(a).unwrap(), "{\"a\":1,\"z\":null}\n");
    }

    #[test]
    fn redaction_and_network_policy_fail_closed() {
        assert!(scan_forbidden(&json!({"api_key":"nope"}), "$").is_err());
        assert!(refuse_network_html("<script>fetch('/x')</script>").is_err());
    }

    #[tokio::test]
    async fn seal_round_trip_is_immutable_offline_and_annotation_aware() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let registry = VisualRegistry::new(
            storage.database().clone(),
            EventJournal::new(storage.database().clone()),
            ContentStore::new(storage.content_root()),
        );
        let Some(template) = registry
            .list_templates(None)
            .unwrap()
            .into_iter()
            .find(|row| !row.id.starts_with("diagram."))
        else {
            return;
        };
        let (created, _) = registry
            .create(VisualCreateRequest {
                template_id: template.id,
                title: Some("Sealed reward".into()),
                bindings: Some(json!({
                    "payload": {
                        "kind": "live_sse",
                        "source": "http://127.0.0.1/private-stream",
                        "snapshot": {"reward": null, "count": 2}
                    }
                })),
                id: Some("vis_seal_round_trip".into()),
                status: Some(VisualStatus::Saved),
                renderer_kind: None,
                session_id: None,
                message_id: None,
                run_id: None,
                trace_id: None,
                parent_visual_id: None,
                source_agent_id: Some("test".into()),
                source_model: None,
                content: None,
                metadata: Some(json!({"qualityGate":{"ready":true,"revision":1}})),
            })
            .await
            .unwrap();
        let (annotation, _) = registry
            .create_annotation(
                created.id.clone(),
                VisualAnnotationCreate {
                    visual_revision: 1,
                    source_digest: None,
                    selector: json!({"type":"chart_mark","markId":"reward"}),
                    kind: "note".into(),
                    body: Some("Missing is intentional".into()),
                    metadata: None,
                    author_id: Some("user".into()),
                    supersedes_id: None,
                },
            )
            .await
            .unwrap();
        let (sealed, _) = registry.seal(created.id.clone(), 1).await.unwrap();
        let (sealed_retry, _) = registry.seal(created.id.clone(), 1).await.unwrap();
        assert_eq!(sealed.receipt_digest, sealed_retry.receipt_digest);
        let bundle = registry
            .get_seal(sealed.receipt_digest.clone())
            .await
            .unwrap();
        assert_eq!(bundle.data["bindings"]["payload"]["kind"], "inline");
        assert!(bundle.data["bindings"]["payload"]["data"]["reward"].is_null());
        assert_eq!(bundle.data["overlays"][0]["id"], annotation.id);
        assert!(!bundle.index_html.contains("private-stream"));
        assert!(!bundle.index_html.contains("EventSource"));
        let hosted = validate_hosted_bundle(
            bundle.index_html.as_bytes().to_vec(),
            canonical_json(&bundle.data).unwrap(),
            canonical_json(&bundle.receipt).unwrap(),
        )
        .unwrap();
        assert_eq!(hosted.seal.receipt_digest, sealed.receipt_digest);
        let mut tampered_data = canonical_json(&bundle.data).unwrap();
        tampered_data.push(b' ');
        assert!(validate_hosted_bundle(
            bundle.index_html.as_bytes().to_vec(),
            tampered_data,
            canonical_json(&bundle.receipt).unwrap(),
        )
        .is_err());

        let (_updated, _) = registry
            .update(
                created.id.clone(),
                VisualUpdateRequest {
                    title: None,
                    bindings: Some(json!({"reward": 99})),
                    status: None,
                    renderer_kind: None,
                    message_id: None,
                    run_id: None,
                    trace_id: None,
                    content: None,
                    metadata: None,
                    bump_revision: Some(true),
                },
            )
            .await
            .unwrap();
        let reopened = registry.get_seal(sealed.receipt_digest).await.unwrap();
        assert!(reopened.data["bindings"]["payload"]["data"]["reward"].is_null());
        assert!(registry.seal(created.id, 2).await.is_err());
    }
}
