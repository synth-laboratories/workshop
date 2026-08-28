use super::{
    VisualAnnotation, VisualAnnotationCreate, VisualRegistry, VisualSeal, VisualSealBundle,
    VisualUpload,
};
use crate::http::http_client;
use crate::storage::{EventAppend, EventSource};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

const BUNDLE_SCHEMA: &str = "synth.artifact-bundle.v1";
/// Envelope for the projections a seal carries. A reader that does not know
/// this string renders the fallback rather than guessing at the views.
const VISUAL_PROJECTION_SCHEMA: &str = "synth.visual-projection.v1";
/// The code that produced a live-eval view, named rather than assumed. Part IV:
/// a seal records which fold and which projection schema made it, because the
/// fold may not exist on the machine that opens the seal.
const LIVE_FOLD_ID: &str = "stream_fold::project_live_eval";
const COMPILER_NAME: &str = "workshop";
const WORKSHOP_UPLOAD_SCHEMA: &str = "synth.workshop-artifact-upload.v1";
const FROZEN_RUNTIME: &str = concat!(
    include_str!("../reports/rollout_inspector.js"),
    "\n",
    include_str!("frozen_runtime.js")
);
const INSPECTOR_CSS: &str = include_str!("../reports/rollout_inspector.css");

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
        let (frozen_bindings, live_views) = freeze_bindings(
            bindings,
            &SealEvidence {
                visual_id: &visual.id,
                revision,
                content: &self.content,
            },
        )?;
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
        let template_source = embedded_template_source(&source.template_id)?;
        let artifact_id = format!("visual:{}", visual.id);
        let builder_run_id = format!("seal:{}:{}", visual.id, revision);
        let mut source_identity = json!({
            "visual_id": visual.id,
            "revision": revision,
            "content_digest": source.content_digest,
            "bindings_digest": bindings_digest,
            "overlay_digest": overlay_digest,
            "source_run_id": visual.run_id,
            "builder_run_id": builder_run_id,
        });
        // The identity document is the receipt, and the receipt is what a
        // verifier reads. A seal over instance-local code has to say which code
        // in the place identity is claimed, not only carry it in the payload.
        if let Some(embedded) = &template_source {
            source_identity["template_source_digest"] = embedded["digest_sha256"].clone();
        }
        let limitations = declared_limitations(&frozen_bindings);
        let mut data = json!({
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
        // Absent, not null, for a bundled template: a key that is always
        // present would change `data_digest` for every seal this build has ever
        // written, and a bundled family is already pinned by `compiler_version`.
        // Only the tier that is *not* pinned by anything gains a field.
        if let Some(embedded) = template_source {
            data["template_source"] = embedded;
        }
        // Item 3: the seal carries the projection, not a promise that whoever
        // opens it will still have the fold that produced one. A viewer that
        // re-derives a view from raw bindings is a second implementation of a
        // projection — and one that stops existing the moment the plugin or
        // the user template that owned it is uninstalled. Absent, not empty,
        // when there is nothing to name: an always-present key would
        // re-digest every seal this build has ever written.
        let mut views = live_views;
        views.extend(locate_sealed_projections(&data["bindings"]));
        if !views.is_empty() {
            let sealed_template_id = data["template_id"].clone();
            let projection = json!({
                "schema_version": VISUAL_PROJECTION_SCHEMA,
                "produced_by": {
                    "compiler": COMPILER_NAME,
                    "compiler_version": env!("CARGO_PKG_VERSION"),
                    "fold": LIVE_FOLD_ID,
                    "template_id": sealed_template_id,
                },
                "views": views,
            });
            data["projection"] = projection;
        }
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
        let client = http_client();
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

/// The `shell.tsx` of a `source_kind: "user"` template, embedded whole.
///
/// **Why the source and not a CAS reference.** A seal exists so an artifact can
/// be re-derived somewhere else; a visual sealed against a user template
/// references code that exists on exactly one machine, and `receipt_digest`,
/// `runtime_digest`, `index_digest` and `data_digest` pin none of it. Putting
/// the shell in the CAS and referencing its digest would not fix that, because
/// the CAS is instance-local and `share_seal` uploads exactly three members —
/// `data.json`, `index.html`, `receipt.json`. A CAS-addressed shell would travel
/// as a digest pointing at a blob only the authoring machine holds: the same
/// failure, now with a hash in front of it. This is the plugin-uninstall
/// convergence in part IV — *store the output, not a reference to something that
/// may not exist later.*
///
/// A fourth bundle member was the other option and is worse for two reasons.
/// `validate_hosted_bundle` refuses a receipt that does not declare exactly two
/// members, and the backend prepare/finalize protocol is written against that
/// member list, so a third file is a wire change on both sides. And `index.html`
/// already embeds `FROZEN_RUNTIME` by `include_str!` rather than referencing it,
/// for exactly this reason — the runtime that renders the bundle travels inside
/// the bundle. A user template's shell is the same class of thing: code the
/// artifact cannot be read without.
///
/// The cost is size, and it is bounded: the pane caps sourced TSX at 256 KiB and
/// `templates.rs` caps any file in this tier at 1.5 MB, against a 64 MiB viewer
/// limit that inline evidence already spends more of.
///
/// Managed `renderer.html` packages have the same defect and are deliberately
/// *not* handled here. Their text would land inside the sealed page, where
/// `refuse_network_html` scans for `@import`, `src="http` and friends — legal in
/// a CSP-sandboxed iframe, forbidden in the sealed inspector page. Embedding one
/// would turn a currently-sealable managed visual into a failed seal. That
/// conflict wants a decision about what a managed seal means, not a drive-by.
///
/// A template id the registry cannot resolve embeds nothing, because from here a
/// deleted user template and a bundled family absent from an unstaged checkout
/// are the same `Err`. Closing that would mean recording the tier on the
/// revision at create time.
fn embedded_template_source(template_id: &str) -> Result<Option<Value>> {
    let Ok(meta) = super::templates::resolve_template(template_id) else {
        return Ok(None);
    };
    if meta.source_kind.as_deref() != Some(super::user_templates::USER_SOURCE_KIND) {
        return Ok(None);
    }
    let text = super::user_templates::shell_source(template_id).with_context(|| {
        format!("sealing against user visual template {template_id} requires its shell.tsx")
    })?;
    // No `path` field: it names the author's home directory and says nothing a
    // reader elsewhere can use. `logical_path` is the name inside the template
    // package, which is what a re-derivation needs.
    Ok(Some(json!({
        "template_id": meta.id,
        "source_kind": meta.source_kind,
        "version": meta.version,
        "logical_path": "shell.tsx",
        "digest_sha256": hex_sha256(text.as_bytes()),
        "size_bytes": text.len(),
        "text": text,
    })))
}

/// Where a seal looks for the evidence behind a `live_sse` binding.
///
/// Held together rather than passed as four arguments because the ladder in
/// [`resolve_live_evidence`] consults all of it, in order, for every live
/// binding a visual declares.
struct SealEvidence<'a> {
    visual_id: &'a str,
    revision: i64,
    content: &'a crate::storage::ContentStore,
}

/// Replayable evidence for one live binding, and where the seal found it.
struct ResolvedEvidence {
    /// The evidence bodies. Empty for the opaque descriptor snapshot below,
    /// which is not an envelope log and cannot be projected.
    envelopes: Vec<Value>,
    /// The verbatim value to freeze into the binding's `data`.
    data: Value,
    /// `descriptor` (an inline `snapshot`), `spool` (a CAS digest named on the
    /// binding) or `host_observation` (what Desktop polled). Recorded on the
    /// binding so a verifier reads how the evidence was obtained rather than
    /// inferring it.
    origin: &'static str,
    spool_digest: Option<String>,
    truncated: bool,
}

/// The identity a declared live stream is recorded and resolved under.
///
/// The same rule `stream_receipt::declared_streams` applies — declared
/// `source`, falling back to the poll URL — so the evidence the host recorded
/// while polling and the evidence the seal asks for are the same key by
/// construction, not by two functions agreeing.
fn binding_stream_id(object: &Map<String, Value>) -> Option<String> {
    for key in ["source", "poll_url", "pollUrl"] {
        if let Some(value) = object
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

/// Find replayable evidence for one `live_sse` binding, or say what is missing.
///
/// The ladder is ordered so that the rung requiring nothing of a caller is the
/// one that normally answers. A required key nothing produces is how this path
/// came to be dead code; a host observation nobody has to remember to attach
/// cannot fail the same way.
fn resolve_live_evidence(
    object: &mut Map<String, Value>,
    evidence: &SealEvidence<'_>,
) -> Result<ResolvedEvidence> {
    let input = super::descriptor_input_name(&Value::Object(object.clone()))
        .unwrap_or_else(|_| super::LIVE_EVAL_INPUT.to_string());
    let stream_id = binding_stream_id(object);

    // 1. An inline snapshot on the descriptor. Kept because a caller that
    //    genuinely holds the evidence should not be refused, and because a
    //    snapshot may be any shape a template renders — it is frozen verbatim
    //    and, not being an envelope log, yields no projection.
    if let Some(snapshot) = object.remove("snapshot") {
        let envelopes = snapshot_envelopes(&snapshot);
        return Ok(ResolvedEvidence {
            data: snapshot,
            envelopes,
            origin: "descriptor",
            spool_digest: None,
            truncated: false,
        });
    }

    // 2. A CAS spool named on the descriptor. `storage/live_spool.rs` persists
    //    raw envelopes for exactly this after-the-fact replay, and a digest
    //    survives the engine, the process and the machine.
    let declared_digest = ["spool_digest", "spoolDigest"]
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::to_string);
    if let Some(digest) = declared_digest {
        let spool = crate::storage::load_live_spool(evidence.content, &digest)
            .with_context(|| format!("sealing live input \"{input}\" from spool {digest}"))?;
        return Ok(ResolvedEvidence {
            data: json!({ "events": spool.envelopes.clone() }),
            envelopes: spool.envelopes,
            origin: "spool",
            spool_digest: Some(spool.digest),
            truncated: false,
        });
    }

    // 3. What this host actually polled. Nothing had to be attached for this
    //    to be here, which is the point.
    if let Some(stream_id) = stream_id.as_deref() {
        if let Some(observed) = super::live_eval::observed_stream_evidence(
            evidence.visual_id,
            evidence.revision,
            stream_id,
        ) {
            let spool = crate::storage::persist_live_envelopes(
                evidence.content,
                Some(stream_id),
                None,
                observed.events.clone(),
            )
            .with_context(|| format!("spooling observed evidence for live input \"{input}\""))?;
            return Ok(ResolvedEvidence {
                data: json!({ "events": spool.envelopes.clone() }),
                envelopes: spool.envelopes,
                origin: "host_observation",
                spool_digest: Some(spool.digest),
                truncated: observed.truncated,
            });
        }
    }

    // Naming the stream and the three ways to supply it, because "live SSE
    // binding has no frozen snapshot" named a key no caller could write and
    // sent every reader looking for a producer that did not exist.
    bail!(
        "live input \"{input}\" declares stream {} but the seal found no replayable evidence for it: \
         this host recorded no envelopes for visual {} revision {}, the binding names no \
         spool_digest, and it carries no inline snapshot. Open the visual in Desktop so the \
         declared stream is polled, or bind a {} digest before sealing.",
        stream_id.as_deref().unwrap_or("<none declared>"),
        evidence.visual_id,
        evidence.revision,
        crate::storage::LIVE_SPOOL_SCHEMA,
    )
}

/// The envelope log inside a descriptor snapshot, if it is one.
///
/// A snapshot may be any shape a template renders. Only the two shapes that
/// *are* an ordered envelope log are projected; anything else is frozen
/// verbatim and carries no projection, which is honest rather than a guess.
fn snapshot_envelopes(snapshot: &Value) -> Vec<Value> {
    if let Some(rows) = snapshot.as_array() {
        return rows.clone();
    }
    snapshot
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Freeze a visual's bindings into evidence, and collect the projections a
/// sealed viewer renders.
///
/// Returns the frozen bindings and one view per live stream. The views are the
/// point: a sealed bundle that stores raw bindings and a runtime that re-folds
/// them stores a *promise* that the fold will still exist and still agree.
/// Storing the fold's output instead is what makes a seal survive the plugin,
/// the template and the build that produced it.
fn freeze_bindings(mut value: Value, evidence: &SealEvidence<'_>) -> Result<(Value, Vec<Value>)> {
    fn walk(value: &mut Value, evidence: &SealEvidence<'_>, views: &mut Vec<Value>) -> Result<()> {
        match value {
            Value::Object(object) => {
                let mut froze_evidence = false;
                if object.get("kind").and_then(Value::as_str) == Some("live_sse") {
                    froze_evidence = true;
                    let stream_id = binding_stream_id(object);
                    let input = super::descriptor_input_name(&Value::Object(object.clone())).ok();
                    let resolved = resolve_live_evidence(object, evidence)?;
                    object.insert("kind".into(), Value::String("inline".into()));
                    object.insert("data".into(), resolved.data);
                    object.remove("source");
                    object.remove("poll_url");
                    object.remove("pollUrl");
                    object.remove("spool_digest");
                    object.remove("spoolDigest");
                    // Absent, not null, for the descriptor snapshot path: a
                    // key that is always present would re-digest every seal
                    // that already worked, and a verbatim snapshot has no
                    // provenance to report beyond having been supplied.
                    if resolved.origin != "descriptor" {
                        let mut provenance = Map::new();
                        provenance.insert("origin".into(), json!(resolved.origin));
                        // The stream's *digest*, never its URL. A sealed
                        // bundle that names a loopback engine points at a
                        // machine the reader does not have and leaks the
                        // topology of one they do; the digest still tells a
                        // verifier holding the bindings that this evidence
                        // came from that stream and not another.
                        provenance.insert(
                            "stream_digest".into(),
                            stream_id
                                .as_deref()
                                .map(|id| json!(hex_sha256(id.as_bytes())))
                                .unwrap_or(Value::Null),
                        );
                        provenance.insert("envelope_count".into(), json!(resolved.envelopes.len()));
                        provenance.insert("truncated".into(), json!(resolved.truncated));
                        if let Some(digest) = &resolved.spool_digest {
                            provenance.insert("spool_digest".into(), json!(digest));
                            provenance.insert(
                                "spool_schema".into(),
                                json!(crate::storage::LIVE_SPOOL_SCHEMA),
                            );
                        }
                        object.insert("evidence".into(), Value::Object(provenance));
                    }
                    if !resolved.envelopes.is_empty() {
                        let mut view = Map::new();
                        if let Some(input) = input {
                            view.insert("input".into(), json!(input));
                        }
                        let projection = super::live_eval::seal_projection(&resolved.envelopes)?;
                        view.insert(
                            "schema_version".into(),
                            projection
                                .get("schema_version")
                                .cloned()
                                .unwrap_or(Value::Null),
                        );
                        view.insert("data".into(), projection);
                        views.push(Value::Object(view));
                    }
                }
                // Frozen evidence is producer data, not a binding tree. An
                // envelope whose payload happens to describe a `live_sse`
                // binding — an eval streaming a visual's own configuration —
                // would otherwise be "frozen" a second time and fail the seal.
                for (key, child) in object.iter_mut() {
                    if froze_evidence && key.as_str() == "data" {
                        continue;
                    }
                    walk(child, evidence, views)?;
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, evidence, views)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut views = Vec::new();
    walk(&mut value, evidence, &mut views)?;
    if value.get("inputs").is_some() || value.get("slots").is_some() {
        if let Some(object) = value.as_object_mut() {
            object.entry("schemaVersion").or_insert_with(|| {
                json!(super::VISUAL_BINDINGS_SCHEMA_VERSION)
            });
        }
        return Ok((super::canonicalize_bindings(&value)?.value, views));
    }
    Ok((value, views))
}

/// Projection documents a template's own resolver already placed in the
/// bindings, named by JSON Pointer rather than copied.
///
/// The trace inspector's projection is computed upstream and rides inside the
/// bindings today. The runtime used to *find* it by scanning every binding for
/// a known `schema_version` — a locator in the viewer, which is the thing item
/// 3 removes. Naming its location at seal time moves that knowledge into the
/// sealed document, where it is pinned, and costs a pointer rather than a
/// second copy of a projection that can run to megabytes.
fn locate_sealed_projections(bindings: &Value) -> Vec<Value> {
    fn escape(key: &str) -> String {
        key.replace('~', "~0").replace('/', "~1")
    }
    fn walk(value: &Value, pointer: &str, out: &mut Vec<Value>) {
        match value {
            Value::Object(object) => {
                if let Some(schema) = object.get("schema_version").and_then(Value::as_str) {
                    if schema.starts_with("synth.trace-projection.") {
                        out.push(json!({
                            "schema_version": schema,
                            "ref": format!("/bindings{pointer}"),
                        }));
                        return;
                    }
                }
                for (key, child) in object {
                    walk(child, &format!("{pointer}/{}", escape(key)), out);
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    walk(child, &format!("{pointer}/{index}"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(bindings, "", &mut out);
    out
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
                // A limitation is a binding the seal could not fill. Nulls
                // inside frozen live evidence are the *producer's* nulls —
                // an unset reward on envelope 4,912 — and walking a hundred
                // thousand envelopes to report each one would bury the
                // handful of limitations that mean something. Only the
                // provenance-carrying descriptors this seal writes are
                // skipped, so no binding shape that existed before is.
                let frozen_evidence = object.contains_key("evidence")
                    && object.get("kind").and_then(Value::as_str) == Some("inline");
                for (key, child) in object {
                    if frozen_evidence && key.as_str() == "data" {
                        continue;
                    }
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
    let css = INSPECTOR_CSS.replace("</style", "<\\/style");
    Ok(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; font-src 'none'; connect-src 'none'; frame-src 'none';"><title>Sealed Workshop visual</title><style>html{{font:14px system-ui;color:#20232a;background:#f7f7f5}}body{{margin:0}}#app{{max-width:1100px;margin:auto;padding:32px}}.kicker{{color:#6b7280}}h1{{font-size:30px}}.visual{{background:white;border:1px solid #ddd;border-radius:14px;padding:20px}}pre{{white-space:pre-wrap;overflow-wrap:anywhere}}{css}</style></head><body><main id="app"></main><script id="synth-artifact-data" type="application/json">{inline}</script><script data-runtime-digest="{runtime_digest}">{FROZEN_RUNTIME}</script></body></html>"#
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

    /// A `ContentStore` and a visual id nothing else in the process shares.
    fn evidence_fixture(name: &str) -> (tempfile::TempDir, ContentStore, String) {
        let dir = tempdir().unwrap();
        let store = ContentStore::new(dir.path());
        let visual_id = format!("vis_{name}");
        super::super::live_eval::forget_live_evidence(&visual_id);
        (dir, store, visual_id)
    }

    /// A stream whose last envelope is the verifier that carries the reward.
    fn envelopes(stream: &str, count: i64) -> Vec<Value> {
        (1..=count)
            .map(|sequence| {
                let last = sequence == count;
                let kind = if last { "verifier" } else { "observation" };
                let payload = if last {
                    json!({"reward.txt": 1.0, "usage": {"prompt_tokens": 12, "completion_tokens": 4}})
                } else {
                    json!({"text": "inspect"})
                };
                json!({
                    "kind": kind,
                    "stream_id": stream,
                    "event_id": format!("e{sequence}"),
                    "sequence": sequence,
                    "payload": payload,
                })
            })
            .collect()
    }

    /// A snapshot a caller genuinely holds is still frozen verbatim, and still
    /// gains nothing: this is the one live shape that ever sealed, and its
    /// bytes must not move.
    #[test]
    fn a_descriptor_snapshot_freezes_verbatim_and_gains_no_field() {
        let (_dir, content, visual_id) = evidence_fixture("descriptor_snapshot");
        let (frozen, views) = freeze_bindings(
            json!({"slots":[{"input":"stream","kind":"live_sse","source":"http://127.0.0.1/events","snapshot":{"reward":null}}]}),
            &SealEvidence { visual_id: &visual_id, revision: 1, content: &content },
        )
        .unwrap();
        assert_eq!(frozen["inputs"][0]["kind"], "inline");
        assert!(frozen.get("slots").is_none());
        assert!(frozen["inputs"][0].get("source").is_none());
        assert!(frozen["inputs"][0]["data"]["reward"].is_null());
        // No provenance block and no view: an opaque snapshot is not an
        // envelope log, and an always-present key would re-digest every seal
        // of this shape that already exists.
        assert!(frozen["inputs"][0].get("evidence").is_none());
        assert!(views.is_empty());
    }

    /// The defect: sealing a live-eval visual required a `snapshot` key that
    /// no production code and no MCP schema could write, so Harbor, Craftax
    /// and every eval stream failed to seal outright. The host now keeps the
    /// evidence it polled, and the seal reads it — nothing has to be attached,
    /// so nothing can be forgotten.
    #[test]
    fn a_live_binding_seals_from_host_observed_evidence() {
        let (_dir, content, visual_id) = evidence_fixture("host_observed");
        let source = "http://127.0.0.1:8098/declared/stream_r1";
        super::super::live_eval::record_live_evidence(&visual_id, 3, source, &envelopes("s1", 4));
        // A replayed page must not double-count: the fold decides, not the
        // caller.
        super::super::live_eval::record_live_evidence(&visual_id, 3, source, &envelopes("s1", 4));

        let (frozen, views) = freeze_bindings(
            json!({"inputs":[{"input":"stream","kind":"live_sse","schema":"synth.trace-stream-event.v1","source":source,"poll_url":"http://127.0.0.1:8098/declared/stream_r1"}]}),
            &SealEvidence { visual_id: &visual_id, revision: 3, content: &content },
        )
        .unwrap();

        let slot = &frozen["inputs"][0];
        assert_eq!(slot["kind"], "inline");
        assert!(slot.get("source").is_none());
        assert!(slot.get("poll_url").is_none());
        assert_eq!(slot["data"]["events"].as_array().map(Vec::len), Some(4));
        assert_eq!(slot["evidence"]["origin"], "host_observation");
        // The stream is named by digest: a sealed bundle never carries the
        // loopback URL it was polled from.
        assert_eq!(
            slot["evidence"]["stream_digest"],
            hex_sha256(source.as_bytes())
        );
        assert!(!serde_json::to_string(&frozen).unwrap().contains(source));
        assert_eq!(slot["evidence"]["envelope_count"], 4);
        assert_eq!(slot["evidence"]["truncated"], false);
        assert_eq!(
            slot["evidence"]["spool_schema"],
            crate::storage::LIVE_SPOOL_SCHEMA
        );
        // The evidence is in the CAS, not only in this process: the seal is
        // replayable after the engine, the process and the machine are gone.
        let digest = slot["evidence"]["spool_digest"].as_str().unwrap();
        let spool = crate::storage::load_live_spool(&content, digest).unwrap();
        assert_eq!(spool.envelopes.len(), 4);

        assert_eq!(views.len(), 1);
        assert_eq!(views[0]["input"], "stream");
        assert_eq!(
            views[0]["schema_version"],
            super::super::live_eval::LIVE_EVAL_PROJECTION_SCHEMA
        );
        let projection = &views[0]["data"];
        assert_eq!(projection["event_count"], 4);
        assert_eq!(projection["reward"], 1.0);
        assert_eq!(projection["has_reward_txt"], true);
        assert_eq!(projection["usage"]["prompt_tokens"], 12.0);
        // The envelope bodies are frozen once, in the binding. A sealed bundle
        // that carried every envelope twice would be twice the upload for a
        // projection whose derived values are all that the viewer renders.
        assert!(projection.get("events").is_none());
    }

    /// The durable rung: a digest already in the CAS. `live_spool.rs` persists
    /// raw envelopes for exactly this after-the-fact replay.
    #[test]
    fn a_live_binding_seals_from_a_declared_spool_digest() {
        let (_dir, content, visual_id) = evidence_fixture("spool_digest");
        let spool =
            crate::storage::persist_live_envelopes(&content, Some("s1"), None, envelopes("s1", 3))
                .unwrap();
        let (frozen, views) = freeze_bindings(
            json!({"inputs":[{"input":"stream","kind":"live_sse","source":"http://127.0.0.1/declared/s1","spool_digest":spool.digest.clone()}]}),
            &SealEvidence { visual_id: &visual_id, revision: 1, content: &content },
        )
        .unwrap();
        let slot = &frozen["inputs"][0];
        assert_eq!(slot["kind"], "inline");
        assert!(slot.get("spool_digest").is_none());
        assert_eq!(slot["evidence"]["origin"], "spool");
        assert_eq!(slot["evidence"]["spool_digest"], spool.digest);
        assert_eq!(slot["data"]["events"].as_array().map(Vec::len), Some(3));
        assert_eq!(views.len(), 1);
        assert_eq!(views[0]["data"]["event_count"], 3);
    }

    /// A seal either contains replayable evidence or refuses with a reason
    /// naming what is missing and how to get it. The message it replaced —
    /// "live SSE binding has no frozen snapshot" — named a key no caller could
    /// write and pointed at no producer.
    #[test]
    fn a_live_binding_without_evidence_names_the_stream_and_the_remedy() {
        let (_dir, content, visual_id) = evidence_fixture("no_evidence");
        let error = freeze_bindings(
            json!({"inputs":[{"input":"stream","kind":"live_sse","source":"http://127.0.0.1/declared/s9"}]}),
            &SealEvidence { visual_id: &visual_id, revision: 2, content: &content },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("http://127.0.0.1/declared/s9"), "{error}");
        assert!(error.contains(&visual_id), "{error}");
        assert!(error.contains("revision 2"), "{error}");
        assert!(error.contains("spool_digest"), "{error}");
        assert!(error.contains(crate::storage::LIVE_SPOOL_SCHEMA), "{error}");
        assert!(error.contains("Open the visual"), "{error}");
    }

    /// Evidence recorded against another revision does not answer for this
    /// one, and a stream nobody polled is not silently borrowed from a stream
    /// somebody did.
    #[test]
    fn observed_evidence_is_keyed_to_the_revision_and_the_stream() {
        let (_dir, content, visual_id) = evidence_fixture("revision_scope");
        super::super::live_eval::record_live_evidence(
            &visual_id,
            1,
            "http://127.0.0.1/declared/a",
            &envelopes("s1", 2),
        );
        let bindings = json!({"inputs":[{"input":"stream","kind":"live_sse","source":"http://127.0.0.1/declared/a"}]});
        assert!(freeze_bindings(
            bindings.clone(),
            &SealEvidence {
                visual_id: &visual_id,
                revision: 2,
                content: &content
            }
        )
        .is_err());
        let other = json!({"inputs":[{"input":"stream","kind":"live_sse","source":"http://127.0.0.1/declared/b"}]});
        assert!(freeze_bindings(
            other,
            &SealEvidence {
                visual_id: &visual_id,
                revision: 1,
                content: &content
            }
        )
        .is_err());
        assert!(freeze_bindings(
            bindings,
            &SealEvidence {
                visual_id: &visual_id,
                revision: 1,
                content: &content
            }
        )
        .is_ok());
    }

    /// A projection the template's resolver already placed in the bindings is
    /// named by pointer, not copied: the viewer stops scanning for it, and the
    /// bundle does not grow a second copy of a projection that can run to
    /// megabytes.
    #[test]
    fn a_resident_projection_is_named_by_pointer_not_copied() {
        let views = locate_sealed_projections(&json!({
            "inputs": [{
                "input": "trace",
                "kind": "inline",
                "data": {"schema_version": "synth.trace-projection.rollout-inspector.v1", "visual": {}}
            }]
        }));
        assert_eq!(views.len(), 1);
        assert_eq!(
            views[0]["schema_version"],
            "synth.trace-projection.rollout-inspector.v1"
        );
        assert_eq!(views[0]["ref"], "/bindings/inputs/0/data");
        assert!(views[0].get("data").is_none());
        // Nothing to find is not a projection.
        assert!(locate_sealed_projections(
            &json!({"inputs":[{"input":"a","kind":"inline","data":{"count":1}}]})
        )
        .is_empty());
    }

    /// Item 3's ratchet. The viewer renders sealed views and locates nothing:
    /// if `extractProjection` ever comes back into the frozen runtime, the
    /// projection has a second implementation again.
    #[test]
    fn the_frozen_runtime_renders_sealed_views_and_locates_nothing() {
        let runtime = include_str!("frozen_runtime.js");
        assert!(runtime.contains("data.projection"));
        assert!(
            !runtime.contains("extractProjection"),
            "the sealed viewer must not locate a projection by scanning bindings"
        );
        assert!(
            runtime.contains("views"),
            "the sealed viewer renders the views the seal named"
        );
        // And the whole runtime is still offline-only.
        refuse_network_html(&format!("<script>{FROZEN_RUNTIME}</script>")).unwrap();
    }

    /// Frozen live evidence is not a list of unfilled bindings. Walking every
    /// producer null in a hundred thousand envelopes would bury the handful of
    /// limitations that mean something.
    #[test]
    fn limitations_do_not_walk_frozen_live_evidence() {
        let frozen = json!({
            "inputs": [
                {"input": "stream", "kind": "inline", "evidence": {"origin": "host_observation"},
                 "data": {"events": [{"payload": {"reward": null}}]}},
                {"input": "spec", "kind": "inline", "data": {"reward": null}}
            ]
        });
        let limitations = declared_limitations(&frozen);
        assert_eq!(
            limitations,
            vec!["bindings.inputs[1].data.reward is missing"]
        );
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

    /// A seal over a user template must carry the code, not a pointer to it.
    ///
    /// Deleting the template after sealing is the whole test: the sealed bundle
    /// still contains the shell it was drawn against, so the artifact can be
    /// re-derived on a machine that never had the template — which is the
    /// premise the visuals system rests on and the one a one-machine reference
    /// would have quietly broken.
    #[tokio::test]
    async fn a_seal_over_a_user_template_embeds_its_shell_source() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-seal-user-template");
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let registry = VisualRegistry::new(
            storage.database().clone(),
            EventJournal::new(storage.database().clone()),
            ContentStore::new(storage.content_root()),
        );
        let shell = "export default function Shell() { return null; }\n";
        let root = crate::instance::state_root()
            .join("visuals")
            .join("templates")
            .join("user.sealed.v1");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("template.json"),
            r#"{"schemaVersion":"synth.visual-template.v1","id":"user.sealed.v1","version":"1.2.3"}"#,
        )
        .unwrap();
        std::fs::write(root.join("shell.tsx"), shell).unwrap();

        let (created, _) = registry
            .create(VisualCreateRequest {
                template_id: "user.sealed.v1".into(),
                title: Some("Sealed user template".into()),
                bindings: Some(json!({"payload": {"count": 2}})),
                id: Some("vis_seal_user_template".into()),
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
        let (sealed, _) = registry.seal(created.id.clone(), 1).await.unwrap();

        // The template is gone; the seal is not.
        std::fs::remove_dir_all(&root).unwrap();
        let bundle = registry
            .get_seal(sealed.receipt_digest.clone())
            .await
            .unwrap();
        let embedded = &bundle.data["template_source"];
        assert_eq!(embedded["template_id"], "user.sealed.v1");
        assert_eq!(embedded["source_kind"], "user");
        assert_eq!(embedded["version"], "1.2.3");
        assert_eq!(embedded["logical_path"], "shell.tsx");
        assert_eq!(embedded["text"], shell);
        assert_eq!(embedded["digest_sha256"], hex_sha256(shell.as_bytes()));
        // The receipt is the identity document, so the pin has to be visible
        // there and not only in the payload it happens to travel with.
        assert_eq!(
            bundle.receipt["source"]["template_source_digest"],
            embedded["digest_sha256"]
        );
        // And the shell reaches an offline reader, not just the database.
        assert!(bundle.index_html.contains("export default function Shell"));
        // Round-tripping the hosted form still validates: embedding rides
        // inside data.json rather than adding a fourth bundle member, which
        // `validate_hosted_bundle` would have refused.
        validate_hosted_bundle(
            bundle.index_html.as_bytes().to_vec(),
            canonical_json(&bundle.data).unwrap(),
            canonical_json(&bundle.receipt).unwrap(),
        )
        .unwrap();
    }

    /// A bundled family is pinned by `compiler_version`, so its seal keeps the
    /// exact bytes it had before this field existed. An always-present key
    /// would have re-digested every seal in the product for nothing.
    #[tokio::test]
    async fn a_bundled_template_seal_gains_no_field() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-seal-bundled");
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
            .find(|row| {
                !crate::visuals::requires_canonical_source(&row.id) && row.source_kind.is_none()
            })
        else {
            return;
        };
        let (created, _) = registry
            .create(VisualCreateRequest {
                template_id: template.id,
                title: Some("Bundled".into()),
                bindings: Some(json!({"payload": {"count": 1}})),
                id: Some("vis_seal_bundled".into()),
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
        let (sealed, _) = registry.seal(created.id.clone(), 1).await.unwrap();
        let bundle = registry.get_seal(sealed.receipt_digest).await.unwrap();
        assert!(bundle.data.get("template_source").is_none());
        assert!(bundle.receipt["source"]
            .get("template_source_digest")
            .is_none());
        // And gains no `projection` either. An artifact with no live stream
        // and no resident projection has nothing new to say, so its
        // `data_digest` is the bytes it had before item 3 existed.
        assert!(bundle.data.get("projection").is_none());
    }

    /// End to end: a visual bound to a live eval stream seals, and the sealed
    /// bundle opens offline with a projection in it. This is the path that was
    /// dead — `freeze_bindings` demanded a `snapshot` key no producer wrote and
    /// the MCP bind schema could not express, so Harbor, Craftax and every eval
    /// stream failed here with a message about a key that did not exist.
    #[tokio::test]
    async fn a_live_eval_visual_seals_from_what_the_host_polled() {
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
            .find(|row| !crate::visuals::requires_canonical_source(&row.id))
        else {
            return;
        };
        let visual_id = "vis_seal_live_eval";
        let source = "http://127.0.0.1:8098/declared/stream_live_eval";
        super::super::live_eval::forget_live_evidence(visual_id);
        let (created, _) = registry
            .create(VisualCreateRequest {
                template_id: template.id,
                title: Some("Live eval".into()),
                bindings: Some(json!({
                    "schemaVersion": "synth.visual-bindings.v1",
                    "inputs": [{
                        "input": "stream",
                        "kind": "live_sse",
                        "schema": "synth.trace-stream-event.v1",
                        "source": source,
                        "poll_url": source
                    }]
                })),
                id: Some(visual_id.into()),
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

        // Before the host has seen anything, the refusal says what is missing
        // rather than naming a key nobody can write.
        let refusal = registry
            .seal(created.id.clone(), 1)
            .await
            .unwrap_err()
            .to_string();
        assert!(refusal.contains("no replayable evidence"), "{refusal}");

        // The poll seam records what it delivered; nothing had to be attached.
        super::super::live_eval::record_live_evidence(
            visual_id,
            1,
            source,
            &[
                json!({"kind":"stream.subscribed","stream_id":"s1","sequence":0}),
                json!({"kind":"observation","stream_id":"s1","sequence":1,"payload":{"text":"inspect"}}),
                json!({"kind":"frame","stream_id":"s1","sequence":2,"payload":{"format":"png","url":"/rollouts/r1/frames/2.png"}}),
                json!({"kind":"verifier","stream_id":"s1","sequence":3,"payload":{"reward.txt":1.0}}),
            ],
        );

        let (sealed, _) = registry.seal(created.id.clone(), 1).await.unwrap();
        let bundle = registry
            .get_seal(sealed.receipt_digest.clone())
            .await
            .unwrap();

        let slot = &bundle.data["bindings"]["inputs"][0];
        assert_eq!(slot["kind"], "inline");
        assert!(slot.get("source").is_none());
        // Control envelopes are not evidence; the other three are.
        assert_eq!(slot["data"]["events"].as_array().map(Vec::len), Some(3));
        assert_eq!(slot["evidence"]["origin"], "host_observation");

        let view = &bundle.data["projection"]["views"][0];
        assert_eq!(
            view["schema_version"],
            super::super::live_eval::LIVE_EVAL_PROJECTION_SCHEMA
        );
        assert_eq!(view["data"]["reward"], 1.0);
        assert_eq!(view["data"]["has_live_frames"], true);
        assert_eq!(view["data"]["event_count"], 3);
        assert_eq!(
            bundle.data["projection"]["produced_by"]["fold"],
            LIVE_FOLD_ID
        );

        // The bundle opens offline, carries no stream URL, and validates as a
        // hosted artifact — the whole point of sealing one.
        assert!(!bundle.index_html.contains(source));
        assert!(!bundle.index_html.contains("EventSource"));
        validate_hosted_bundle(
            bundle.index_html.as_bytes().to_vec(),
            canonical_json(&bundle.data).unwrap(),
            canonical_json(&bundle.receipt).unwrap(),
        )
        .unwrap();

        // Sealing twice is the same artifact: the evidence prefix is the
        // process's, but the seal over it is deterministic.
        let (again, _) = registry.seal(created.id, 1).await.unwrap();
        assert_eq!(again.receipt_digest, sealed.receipt_digest);
        super::super::live_eval::forget_live_evidence(visual_id);
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
            .find(|row| !crate::visuals::requires_canonical_source(&row.id))
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
        // Bindings are sealed in the canonical envelope, and the live stream
        // is frozen to inline evidence so the bundle opens offline.
        let sealed_slot = bundle.data["bindings"]
            .get("inputs")
            .and_then(|value| value.get(0))
            .unwrap();
        assert_eq!(sealed_slot["input"], "payload");
        assert_eq!(sealed_slot["kind"], "inline");
        assert!(sealed_slot["data"]["reward"].is_null());
        // An opaque descriptor snapshot is not an envelope log, so this seal
        // names no view and keeps the exact bytes it had before item 3.
        assert!(sealed_slot.get("evidence").is_none());
        assert!(bundle.data.get("projection").is_none());
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
        assert!(reopened.data["bindings"]
            .get("inputs")
            .and_then(|value| value.get(0))
            .unwrap()["data"]["reward"]
            .is_null());
        assert!(registry.seal(created.id, 2).await.is_err());
    }
}
