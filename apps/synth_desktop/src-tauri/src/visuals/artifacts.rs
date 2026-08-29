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

    /// Derive every Trace V5 projection this revision's bindings name, before
    /// the bindings are frozen.
    ///
    /// One async seam instead of an async recursion: the freeze walk stays a
    /// pure function over documents, and the one call that has to touch the
    /// database and possibly the trace CLI happens once per distinct
    /// (archive, projection) pair rather than once per binding.
    ///
    /// The resolver is `crate::data::DataStore::resolve_trace_projection` —
    /// the same one `registry.rs` uses to read a `trace_v5` chart input, so a
    /// seal and a render agree about what that binding means by construction.
    /// A failure is the seal's failure: the alternative is a bundle that
    /// carries a CAS pointer the reader cannot follow, which is the defect
    /// this closes.
    async fn resolve_trace_projections(
        &self,
        bindings: &Value,
        visual_id: &str,
        revision: i64,
    ) -> Result<BTreeMap<TraceRequest, ResolvedTraceEvidence>> {
        let requests = trace_binding_requests(bindings);
        if requests.is_empty() {
            return Ok(BTreeMap::new());
        }
        let data = crate::data::DataStore::new(self.db.clone(), self.content.clone());
        let mut resolved = BTreeMap::new();
        for request in requests {
            let projection = data
                .resolve_trace_projection(request.0.clone(), request.1.clone())
                .await
                .with_context(|| {
                    format!(
                        "sealing visual {visual_id} revision {revision}: binding names Trace V5 \
                         archive {} but this host holds no trusted, self-contained bundle it can \
                         derive the {} projection from. Import the sealed Trace V5 bundle before \
                         sealing.",
                        request.0, request.1,
                    )
                })?;
            resolved.insert(
                request,
                ResolvedTraceEvidence {
                    payload: projection.payload,
                    trace_digest: projection.trace_digest,
                    projection_schema: projection.projection_schema,
                    payload_digest: projection.payload_digest,
                },
            );
        }
        Ok(resolved)
    }

    pub async fn seal(&self, visual_id: String, revision: i64) -> Result<(VisualSeal, Value)> {
        let visual = self.get(visual_id.clone()).await?;
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
        let authoring_gate_ready = visual
            .metadata
            .get("qualityGate")
            .filter(|gate| gate.get("ready").and_then(Value::as_bool) == Some(true))
            .filter(|gate| gate.get("revision").and_then(Value::as_i64) == Some(revision))
            .is_some();
        let optimizer_evidence_gate_ready = self
            .terminal_primary_optimizer_evidence_ready(&visual, &bindings)
            .await?;
        if !optimizer_evidence_gate_ready && !authoring_gate_ready {
            bail!("visual revision has not passed the E1 quality gate");
        }
        let traces = self
            .resolve_trace_projections(&bindings, &visual.id, revision)
            .await?;
        let (frozen_bindings, live_views) = freeze_bindings(
            bindings,
            &SealEvidence {
                visual_id: &visual.id,
                revision,
                content: &self.content,
                traces,
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
        let folded_live = !live_views.is_empty();
        let mut views = live_views;
        views.extend(locate_sealed_projections(&data["bindings"]));
        if !views.is_empty() {
            let sealed_template_id = data["template_id"].clone();
            let mut produced_by = json!({
                "compiler": COMPILER_NAME,
                "compiler_version": env!("CARGO_PKG_VERSION"),
                "template_id": sealed_template_id,
            });
            // Named only when a fold actually ran. A trace inspector's views
            // are a projection the trace tooling derived from a sealed
            // archive; a receipt claiming the live-eval fold produced them
            // would send a verifier to code that never saw the bytes.
            if folded_live {
                produced_by["fold"] = json!(LIVE_FOLD_ID);
            }
            let projection = json!({
                "schema_version": VISUAL_PROJECTION_SCHEMA,
                "produced_by": produced_by,
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

    /// Product-owned optimizer visuals are projections of an already admitted
    /// run, not author-created artwork. Their release gate is the run's durable
    /// terminal evidence, so asking an operator to manufacture two E1 authoring
    /// reviews adds no evidence. Secondary/workbench visuals and every ordinary
    /// visual continue to use the E1 gate above.
    async fn terminal_primary_optimizer_evidence_ready(
        &self,
        visual: &super::VisualRecord,
        bindings: &Value,
    ) -> Result<bool> {
        let run_ids = super::declared_optimizer_run_ids(bindings);
        let [run_id] = run_ids.as_slice() else {
            return Ok(false);
        };
        let service = self.optimizer_runs.get().ok_or_else(|| {
            anyhow!(
                "optimizer visual cannot be sealed because its run evidence service is unavailable"
            )
        })?;
        let run = service.get(run_id.clone()).await?;
        let view = serde_json::to_value(service.run_view_v2(run_id.clone()).await?)?;
        if !optimizer_view_is_primary(&visual.id, run_id, &view)? {
            return Ok(false);
        }
        require_primary_optimizer_seal_evidence(&visual.id, run_id, &run.summary, &view)?;
        Ok(true)
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

fn require_primary_optimizer_seal_evidence(
    visual_id: &str,
    run_id: &str,
    run_summary: &Value,
    run_view: &Value,
) -> Result<()> {
    if !optimizer_view_is_primary(visual_id, run_id, run_view)? {
        bail!("visual revision has not passed the E1 quality gate");
    }
    let header = &run_view["header"];
    if header.get("lifecycle").and_then(Value::as_str) != Some("terminal")
        || header.get("terminal").is_none_or(Value::is_null)
    {
        bail!("optimizer visual can be sealed only after its run finishes");
    }
    let projected_complete = header
        .pointer("/evidence/completeness")
        .and_then(Value::as_str)
        == Some("complete");
    let terminal_complete = header
        .pointer("/terminal/evidence/completeness")
        .and_then(Value::as_str)
        == Some("complete");
    if !projected_complete || !terminal_complete {
        let completeness = header
            .pointer("/terminal/evidence/completeness")
            .and_then(Value::as_str)
            .or_else(|| {
                header
                    .pointer("/evidence/completeness")
                    .and_then(Value::as_str)
            })
            .unwrap_or("missing");
        bail!(
            "optimizer visual cannot be sealed because run evidence is {completeness}, not complete"
        );
    }
    if optimizer_runtime_evidence_rejected(run_summary) {
        bail!("optimizer visual cannot be sealed because runtime evidence was rejected");
    }
    Ok(())
}

fn optimizer_view_is_primary(visual_id: &str, run_id: &str, run_view: &Value) -> Result<bool> {
    let header = run_view.get("header").ok_or_else(|| {
        anyhow!("optimizer visual cannot be sealed because its run view has no header")
    })?;
    if header.get("runId").and_then(Value::as_str) != Some(run_id) {
        bail!("optimizer visual cannot be sealed because its run identity changed");
    }
    Ok(header
        .get("visualRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|reference| {
            reference.get("kind").and_then(Value::as_str) == Some("visual")
                && reference.get("id").and_then(Value::as_str) == Some(visual_id)
                && reference.get("role").and_then(Value::as_str) == Some("primary")
        }))
}

fn optimizer_runtime_evidence_rejected(summary: &Value) -> bool {
    let authoritative = summary
        .pointer("/progress/authoritative/evidence/completeness")
        .and_then(Value::as_str);
    if matches!(authoritative, Some("rejected" | "unusable")) {
        return true;
    }
    summary
        .get("records")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|record| {
            if record
                .get("evidenceState")
                .or_else(|| record.get("evidence_state"))
                .and_then(Value::as_str)
                == Some("rejected")
            {
                return true;
            }
            let detail = record
                .get("error")
                .and_then(Value::as_str)
                .or_else(|| {
                    record
                        .pointer("/evidenceOutcome/detail")
                        .and_then(Value::as_str)
                })
                .or_else(|| {
                    record
                        .pointer("/evidenceOutcome/reason")
                        .and_then(Value::as_str)
                })
                .unwrap_or_default()
                .to_ascii_lowercase();
            [
                "digest mismatch",
                "integrity validation",
                "evidence rejected",
                "unusable evidence",
            ]
            .iter()
            .any(|marker| detail.contains(marker))
        })
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

/// Where a seal looks for the evidence behind a `live_sse` or `trace_v5`
/// binding.
///
/// Held together rather than passed as four arguments because the ladder in
/// [`resolve_live_evidence`] consults all of it, in order, for every live
/// binding a visual declares.
///
/// `traces` is resolved before the walk rather than during it: reading a Trace
/// V5 projection is `async` (it may shell out to the trace CLI against a
/// trusted archive) and the walk is not. Pre-resolving keeps one async seam at
/// the top of `seal` instead of an async recursion through every binding, and
/// it makes the freeze itself a pure function of documents a test can supply.
struct SealEvidence<'a> {
    visual_id: &'a str,
    revision: i64,
    content: &'a crate::storage::ContentStore,
    traces: BTreeMap<TraceRequest, ResolvedTraceEvidence>,
}

/// The trace projection one `trace_v5` descriptor names: the sealed archive
/// digest it points at, and the consumer projection it wants derived from it.
type TraceRequest = (String, String);

/// A Trace V5 projection document the seal froze into a binding, and the
/// provenance a verifier reads to know which archive it came from.
struct ResolvedTraceEvidence {
    /// The projection payload, verbatim. It is self-describing — it carries
    /// its own `schema_version` — which is what lets
    /// [`locate_sealed_projections`] name it as a view without a second
    /// registry of where projections live.
    payload: Value,
    /// The `sha256:`-qualified digest of the sealed archive it was derived
    /// from, as the resolver normalised it.
    trace_digest: String,
    /// The format the archive's own manifest declared.
    projection_schema: String,
    /// The digest of the projection payload, as the trace tooling computed it.
    /// A verifier re-deriving the projection compares this, not the bindings.
    payload_digest: String,
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

/// The archive digest and projection kind one `trace_v5` descriptor names.
///
/// `projection` is the key a chart panel writes; `schema` is the key the trace
/// pane stamps when it creates the inspector visual. Both name the same thing —
/// which consumer projection to derive — so both are read here rather than one
/// being privileged and the other silently defaulted. The strip mirrors
/// `data.rs::projection_consumer_kind`, which does the same in the other
/// direction for the cache key.
fn trace_binding_request(object: &Map<String, Value>) -> Option<TraceRequest> {
    let source = object
        .get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?;
    let kind = object
        .get("projection")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            object
                .get("schema")
                .and_then(Value::as_str)
                .and_then(|schema| schema.strip_prefix("synth.trace-projection."))
                .and_then(|rest| rest.strip_suffix(".v1"))
                .map(str::to_string)
        })
        .unwrap_or_else(|| super::registry::CHART_DEFAULT_PROJECTION.to_string());
    Some((source.to_string(), kind))
}

/// Every distinct Trace V5 projection a bindings tree asks for.
///
/// Two descriptors naming the same archive and the same projection resolve
/// once; two naming different projections of one archive resolve twice, which
/// is what the key being a pair buys.
fn trace_binding_requests(bindings: &Value) -> Vec<TraceRequest> {
    fn walk(value: &Value, out: &mut Vec<TraceRequest>) {
        match value {
            Value::Object(object) => {
                if object.get("kind").and_then(Value::as_str) == Some("trace_v5") {
                    if let Some(request) = trace_binding_request(object) {
                        if !out.contains(&request) {
                            out.push(request);
                        }
                    }
                }
                for child in object.values() {
                    walk(child, out);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(bindings, &mut out);
    out
}

/// Freeze one `trace_v5` binding into the projection it names.
///
/// The defect this closes is the live-eval one in the other major visual
/// class. A `trace.rollout_inspector.v1` seal carried `{kind: "trace_v5",
/// source: <digest>}` verbatim: a pointer into a content-addressed store the
/// reader does not have. The bundle was reproducible only on the machine that
/// wrote it, and the frozen viewer — having nothing to render — printed the
/// bindings into a `<pre>`.
///
/// There is deliberately no caller-supplied rung here, unlike the live ladder.
/// `visuals_ipc` already refuses an MCP caller's projection bytes outright and
/// re-resolves from the local inventory; a seal that accepted them would
/// reopen that door in the one place whose whole product is a receipt. The
/// single rung is the trusted local Trace V5 inventory, which requires nothing
/// of any caller — the property the live ladder was rebuilt around.
fn resolve_trace_evidence<'a>(
    object: &Map<String, Value>,
    evidence: &'a SealEvidence<'_>,
) -> Result<&'a ResolvedTraceEvidence> {
    let input = super::descriptor_input_name(&Value::Object(object.clone()))
        .unwrap_or_else(|_| "projection".to_string());
    let Some(request) = trace_binding_request(object) else {
        bail!(
            "trace input \"{input}\" is bound as trace_v5 but names no `source`, so the seal has \
             no sealed archive to derive its projection from. Bind the Trace V5 digest with \
             visual_bind_data_source before sealing."
        );
    };
    evidence.traces.get(&request).ok_or_else(|| {
        anyhow!(
            "trace input \"{input}\" names Trace V5 archive {} but the seal found no replayable \
             evidence for it: this host holds no trusted, self-contained bundle for that digest, \
             so the {} projection cannot be derived. Import the sealed Trace V5 bundle on this \
             machine before sealing visual {} revision {}.",
            request.0,
            request.1,
            evidence.visual_id,
            evidence.revision,
        )
    })
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
                if object.get("kind").and_then(Value::as_str) == Some("trace_v5") {
                    froze_evidence = true;
                    let projection_kind = trace_binding_request(object)
                        .map(|request| request.1)
                        .unwrap_or_default();
                    let resolved = resolve_trace_evidence(object, evidence)?;
                    // A projection that does not say what it is would be
                    // frozen, sealed, and then silently unrenderable: the
                    // viewer and `locate_sealed_projections` both key on the
                    // document's own `schema_version`, and a document without
                    // one falls through to the `<pre>` this change exists to
                    // remove. Refusing here is the difference between a seal
                    // that carries evidence and one that promises it.
                    let declared = resolved
                        .payload
                        .get("schema_version")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !declared.starts_with("synth.trace-projection.") {
                        bail!(
                            "Trace V5 archive {} resolved a {} projection whose document declares \
                             schema_version {:?}; a sealed projection must declare its own \
                             synth.trace-projection.* schema or no reader can render it.",
                            resolved.trace_digest,
                            resolved.projection_schema,
                            declared,
                        );
                    }
                    object.insert("kind".into(), Value::String("inline".into()));
                    object.insert("data".into(), resolved.payload.clone());
                    // The archive digest moves into `evidence` rather than
                    // staying in `source`: a frozen binding whose `source`
                    // still named a CAS entry is exactly the pointer this
                    // change removes, and a reader must not be able to mistake
                    // one for a thing they can fetch.
                    object.remove("source");
                    object.remove("projection");
                    object.insert(
                        "evidence".into(),
                        json!({
                            "origin": "trace_inventory",
                            "trace_digest": resolved.trace_digest,
                            "projection_kind": projection_kind,
                            "projection_schema": resolved.projection_schema,
                            "payload_digest": resolved.payload_digest,
                        }),
                    );
                    // No view is pushed. The projection now *is* the binding's
                    // document, so `locate_sealed_projections` names it by
                    // pointer — one mechanism, and a bundle that does not carry
                    // a megabyte-scale projection twice.
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
            object
                .entry("schemaVersion")
                .or_insert_with(|| json!(super::VISUAL_BINDINGS_SCHEMA_VERSION));
        }
        return Ok((super::canonicalize_bindings(&value)?.value, views));
    }
    Ok((value, views))
}

/// Whether this descriptor is one the seal froze producer evidence into.
///
/// One predicate, because three passes over the frozen bindings — the
/// limitations report, the projection locator and the redaction scan — all have
/// to tell the host's own binding *metadata* from the producer bytes underneath
/// it, and three spellings of that test would drift into three different
/// answers about the same document.
///
/// A descriptor snapshot supplied inline by a caller is deliberately *not*
/// this: it writes no `evidence` block (so that seals of that shape keep their
/// digests), and it is author-supplied rather than host-observed, so it stays
/// under the stricter reading everywhere.
fn frozen_evidence_descriptor(object: &Map<String, Value>) -> bool {
    object.get("kind").and_then(Value::as_str) == Some("inline")
        && object.get("evidence").is_some_and(Value::is_object)
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
///
/// A frozen evidence document is a candidate, never a haystack. A `trace_v5`
/// binding's frozen `data` *is* the projection, so it is checked; a live
/// stream's frozen `data` is a hundred thousand producer envelopes, and one of
/// them may legitimately quote a `synth.trace-projection.*` document — an
/// optimizer event carrying a proposer's trace does exactly that. Searching
/// inside producer bytes would name that envelope as the seal's own view and
/// point the rollout inspector at it.
fn locate_sealed_projections(bindings: &Value) -> Vec<Value> {
    fn escape(key: &str) -> String {
        key.replace('~', "~0").replace('/', "~1")
    }
    fn projection_schema(value: &Value) -> Option<&str> {
        value
            .get("schema_version")
            .and_then(Value::as_str)
            .filter(|schema| schema.starts_with("synth.trace-projection."))
    }
    fn walk(value: &Value, pointer: &str, out: &mut Vec<Value>) {
        match value {
            Value::Object(object) => {
                if let Some(schema) = projection_schema(value) {
                    out.push(json!({
                        "schema_version": schema,
                        "ref": format!("/bindings{pointer}"),
                    }));
                    return;
                }
                let frozen = frozen_evidence_descriptor(object);
                for (key, child) in object {
                    if frozen && key.as_str() == "data" {
                        if let Some(schema) = projection_schema(child) {
                            out.push(json!({
                                "schema_version": schema,
                                "ref": format!("/bindings{pointer}/data"),
                            }));
                        }
                        continue;
                    }
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
                let frozen_evidence = frozen_evidence_descriptor(object);
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

/// Key fragments that name a secret wherever they appear.
///
/// A value filed under one of these is a credential by the writer's own
/// account, whether the writer was the host or a producer. These stay in force
/// inside frozen evidence.
const SECRET_KEY_FRAGMENTS: [&str; 8] = [
    "api_key",
    "access_token",
    "authorization",
    "credential",
    "password",
    "process_env",
    "refresh_token",
    "secret",
];

/// Key fragments that name the *host's own configuration*.
///
/// This is the list the policy was written for: a sealed bundle must not carry
/// the machine's provider endpoints, bucket names or environment. In producer
/// evidence the same words are data — a Craftax rollout's `environment_ref`
/// names a gym, not this laptop's env; an eval's `chain_of_thought` is the
/// thing the run was recording — so they are enforced on binding metadata and
/// not on the bytes the seal exists to preserve.
const HOST_CONFIG_KEY_FRAGMENTS: [&str; 6] = [
    "chain_of_thought",
    "environment",
    "hidden_reasoning",
    "object_key",
    "provider_url",
    "storage_uri",
];

/// A credential *by shape*, wherever it appears.
///
/// Key names do not survive the trip into producer evidence, but the bytes of a
/// real key do: they have recognisable, high-entropy forms. This is what keeps
/// "producer evidence is data" from meaning "producer evidence is unscanned" —
/// a run that leaked an access key into a log still refuses to seal.
///
/// Every pattern requires the marker to start a word and to be followed by an
/// opaque run of the length that key format actually has. Prose that merely
/// mentions a scheme does not trip it, and neither does a base64 payload that
/// happens to contain the four letters `AKIA` somewhere in the middle — a
/// false refusal on a megabyte of honest evidence is the same defect as the
/// one this scope change is fixing.
fn credential_literal(text: &str) -> Option<&'static str> {
    fn is_token_byte(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
    }
    fn token_run(text: &str, from: usize) -> usize {
        text[from..]
            .bytes()
            .take_while(|&b| is_token_byte(b))
            .count()
    }
    fn starts_a_word(text: &str, at: usize) -> bool {
        at == 0 || !is_token_byte(text.as_bytes()[at - 1])
    }
    // Names of the variables themselves, which is how a dumped environment
    // reaches a payload.
    for name in ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"] {
        if text.contains(name) {
            return Some(name);
        }
    }
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY-----") {
        return Some("private key block");
    }
    // (marker, the opaque run that must follow it, label). AWS ids are exactly
    // twenty characters, so their run is pinned rather than bounded below; the
    // longer markers are distinctive enough on their own.
    for (marker, run, label) in [
        ("AKIA", 16..=16, "AWS access key id"),
        ("ASIA", 16..=16, "AWS session key id"),
        ("sk-ant-", 24..=usize::MAX, "Anthropic API key"),
        ("sk-proj-", 24..=usize::MAX, "OpenAI project key"),
        ("ghp_", 20..=usize::MAX, "GitHub token"),
        ("gho_", 20..=usize::MAX, "GitHub token"),
        ("ghu_", 20..=usize::MAX, "GitHub token"),
        ("ghs_", 20..=usize::MAX, "GitHub token"),
        ("github_pat_", 20..=usize::MAX, "GitHub token"),
        ("xoxb-", 20..=usize::MAX, "Slack token"),
        ("xoxp-", 20..=usize::MAX, "Slack token"),
        ("xapp-", 20..=usize::MAX, "Slack token"),
        ("AIza", 30..=usize::MAX, "Google API key"),
    ] {
        let mut cursor = 0;
        while let Some(offset) = text[cursor..].find(marker) {
            let at = cursor + offset;
            let start = at + marker.len();
            if starts_a_word(text, at) && run.contains(&token_run(text, start)) {
                return Some(label);
            }
            cursor = start;
        }
    }
    // `Bearer` followed by an opaque run is a live token, not a description of
    // one; `Bearer <token>` and `Bearer …` are not. The length guard is here
    // because this is the one pattern that has to fold case, and folding every
    // envelope body in a hundred-thousand-envelope seal to find a header that
    // cannot fit is the kind of cost that turns a scan into a timeout.
    if text.len() < "bearer ".len() + 24 {
        return None;
    }
    let lowered = text.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(offset) = lowered[cursor..].find("bearer ") {
        let start = cursor + offset + "bearer ".len();
        if token_run(text, start) >= 24 {
            return Some("bearer token");
        }
        cursor = start;
    }
    None
}

/// Refuse to seal a document that carries the host's configuration or anyone's
/// credential.
///
/// Two scopes, because the document has two kinds of content in it. Binding
/// *metadata* is written by this host and gets the full policy. Frozen producer
/// evidence — the envelopes a run emitted, the projection a sealed trace
/// declares — is the payload the seal exists to preserve verbatim, and is
/// rendered as text rather than executed, so it is scanned for credentials by
/// name and by shape but not for words that only mean something about a host.
///
/// Before commit b7926f5c this distinction did not have to exist: sealing a
/// live visual always failed, so no producer evidence ever reached the scan.
/// Once evidence did reach it, a Craftax payload with an `environment_ref` key
/// refused an honest run, which is a fail-closed policy failing the wrong way.
fn scan_forbidden(value: &Value, path: &str) -> Result<()> {
    scan_document(value, path, false)
}

fn scan_document(value: &Value, path: &str, evidence: bool) -> Result<()> {
    match value {
        Value::Object(object) => {
            let frozen = !evidence && frozen_evidence_descriptor(object);
            for (key, child) in object {
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                if SECRET_KEY_FRAGMENTS
                    .iter()
                    .any(|needle| normalized.contains(needle))
                {
                    bail!("seal policy forbids {path}.{key}");
                }
                if !evidence
                    && HOST_CONFIG_KEY_FRAGMENTS
                        .iter()
                        .any(|needle| normalized.contains(needle))
                {
                    bail!("seal policy forbids host configuration at {path}.{key}");
                }
                let child_is_evidence = evidence || (frozen && key.as_str() == "data");
                scan_document(child, &format!("{path}.{key}"), child_is_evidence)?;
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                scan_document(child, &format!("{path}[{index}]"), evidence)?;
            }
        }
        Value::String(text) => {
            if let Some(label) = credential_literal(text) {
                bail!("seal policy forbids a {label} at {path}");
            }
            // A storage locator is the host naming a bucket it can reach. A
            // producer quoting `s3://dataset/...` in a message is describing
            // where its data came from, which is evidence.
            if !evidence && (text.contains("s3://") || text.contains("gs://")) {
                bail!("seal policy forbids storage or credential locator at {path}");
            }
        }
        _ => {}
    }
    Ok(())
}

/// The opening tag of the sealed page's data island, shared by the writer and
/// the reader so the two cannot drift into disagreeing about where it starts.
const DATA_ISLAND_OPEN: &str = r#"<script id="synth-artifact-data" type="application/json">"#;
const SCRIPT_CLOSE: &str = "</script>";
/// What actually ends a script element for the HTML tokenizer: the `>` may be
/// any of `>`, `/`, or whitespace, so the prefix is the honest boundary.
const SCRIPT_CLOSE_PREFIX: &str = "</script";

fn build_index_html(data: &Value, runtime_digest: &str) -> Result<String> {
    // Every `<`, not just `</script`. In JSON a `<` can only occur inside a
    // string and `<` parses back to exactly the same string, so this is
    // free; what it buys is that the island cannot open a tag, close its own
    // element, or start a comment. The previous escape was case-sensitive
    // while HTML's end-tag matching is not, so a producer message containing
    // `</SCRIPT>` closed the island and injected markup into the page — a
    // theoretical hole while nothing producer-written reached the island, and
    // a reachable one now that frozen evidence does.
    let inline = serde_json::to_string(data)?.replace('<', "\\u003c");
    let css = INSPECTOR_CSS.replace("</style", "<\\/style");
    Ok(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; font-src 'none'; connect-src 'none'; frame-src 'none';"><title>Sealed Workshop visual</title><style>html{{font:14px system-ui;color:#20232a;background:#f7f7f5}}body{{margin:0}}#app{{max-width:1100px;margin:auto;padding:32px}}.kicker{{color:#6b7280}}h1{{font-size:30px}}.visual{{background:white;border:1px solid #ddd;border-radius:14px;padding:20px}}pre{{white-space:pre-wrap;overflow-wrap:anywhere}}{css}</style></head><body><main id="app"></main>{DATA_ISLAND_OPEN}{inline}{SCRIPT_CLOSE}<script data-runtime-digest="{runtime_digest}">{FROZEN_RUNTIME}</script></body></html>"#
    ))
}

/// Refuse a sealed page that could make a network request.
///
/// The page has two parts and only one of them runs. The `application/json`
/// island is parsed as data and rendered through `textContent` and an escaping
/// serialiser; the rest — the runtime script, the stylesheet, the markup — is
/// what a browser executes and fetches from. Scanning the island for `fetch(`
/// was scanning the evidence, and it refused a seal because a model had written
/// `fetch(` in a message.
///
/// So the token scan runs over the executable remainder, and the island is held
/// to a stronger rule instead: it must be unable to leave itself. It cannot
/// close its own element or open an HTML comment, which is the whole of what
/// makes it data. The page's Content-Security-Policy (`default-src 'none';
/// connect-src 'none'`) is the enforcement underneath both.
fn refuse_network_html(html: &str) -> Result<()> {
    let (executable, island) = split_data_island(html)?;
    if let Some(island) = island {
        // The island runs to the *first* end tag a browser would honour, so
        // this parse is the whole guarantee: if anything inside the data could
        // close the element early, what remains is a truncated document and
        // does not parse. A token list cannot make that promise; JSON can.
        serde_json::from_str::<Value>(island).map_err(|error| {
            anyhow!("sealed index.html data island is not a single JSON document: {error}")
        })?;
        // `<!--` followed by `<script` puts the tokenizer in the double-escaped
        // state, where the writer's own end tag no longer ends the element.
        // ASCII case-insensitive, because HTML tag matching is.
        let lowered = island.to_ascii_lowercase();
        if lowered.contains("<!--") || lowered.contains("<script") {
            bail!("sealed index.html data island can change the parser's state");
        }
    }
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
        if executable.contains(token) {
            bail!("sealed index.html contains forbidden network capability: {token}");
        }
    }
    Ok(())
}

/// Split a sealed page into what a browser executes and the data it carries.
///
/// A page with no island — the frozen runtime on its own, in the ratchet test —
/// is all executable, which is the conservative reading. An island that never
/// closes is refused rather than guessed at.
///
/// The island ends at the first `</script`, case-insensitively, because that is
/// where a browser ends it. Cutting anywhere later would scan as data bytes a
/// browser would already be treating as markup.
fn split_data_island(html: &str) -> Result<(String, Option<&str>)> {
    let Some(open) = html.find(DATA_ISLAND_OPEN) else {
        return Ok((html.to_string(), None));
    };
    let body = open + DATA_ISLAND_OPEN.len();
    let Some(offset) = html[body..].to_ascii_lowercase().find(SCRIPT_CLOSE_PREFIX) else {
        bail!("sealed index.html data island is never closed");
    };
    let end = body + offset;
    Ok((
        format!("{}{}", &html[..open], &html[end..]),
        Some(&html[body..end]),
    ))
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

