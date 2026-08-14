use super::models::{
    ReportComment, ReportCommentCreate, ReportSeal, ReportSealBundle, ReportUpload,
    REPORT_BUNDLE_SCHEMA,
};
use super::registry::ReportRegistry;
use crate::storage::{EventAppend, EventSource};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

const WORKSHOP_REPORT_UPLOAD_SCHEMA: &str = "synth.workshop-report-upload.v1";
const REPORT_DATA_MEDIA: &str = "application/vnd.synth.report-bundle-data+json";
const REPORT_RECEIPT_MEDIA: &str = "application/vnd.synth.report-bundle-receipt+json";
const REPORT_INDEX_MEDIA: &str = "text/html; charset=utf-8";

impl ReportRegistry {
    pub async fn upload_status(&self, receipt_digest: String) -> Result<Option<ReportUpload>> {
        let db = self.db.clone();
        db.run(move |conn| {
            conn.query_row(
                "SELECT receipt_digest,collection_id,publication_id,publication_revision,state,committed_url,error,updated_at
                 FROM report_uploads WHERE receipt_digest = ?1",
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
    ) -> Result<(ReportUpload, Value)> {
        let bundle = self.get_seal(receipt_digest.clone()).await?;
        if let Some(existing) = self.upload_status(receipt_digest.clone()).await? {
            if existing.state == "committed" {
                return Ok((existing, json!({"kind":"report.upload.idempotent"})));
            }
        }
        let members = BTreeMap::from([
            (
                "data.json",
                (
                    self.content
                        .get_bytes("report_bundles", &bundle.seal.data_digest)?,
                    bundle.seal.data_digest.clone(),
                    REPORT_DATA_MEDIA,
                ),
            ),
            (
                "index.html",
                (
                    self.content
                        .get_bytes("report_bundles", &bundle.seal.index_digest)?,
                    bundle.seal.index_digest.clone(),
                    REPORT_INDEX_MEDIA,
                ),
            ),
            (
                "receipt.json",
                (
                    self.content
                        .get_bytes("report_bundles", &bundle.seal.receipt_digest)?,
                    bundle.seal.receipt_digest.clone(),
                    REPORT_RECEIPT_MEDIA,
                ),
            ),
        ]);
        for (path, (bytes, digest, _)) in &members {
            if hex_sha256(bytes) != *digest {
                bail!("local {path} no longer matches its sealed digest");
            }
        }
        self.write_upload(ReportUpload {
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
                self.write_upload(ReportUpload {
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
                        source: EventSource::Report,
                        kind: "report.upload.committed".into(),
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

    pub async fn open_shared_url(
        &self,
        committed_url: String,
        backend_url: String,
        api_key: String,
    ) -> Result<ReportSealBundle> {
        let backend = reqwest::Url::parse(backend_url.trim_end_matches('/'))
            .context("configured Synth backend URL is invalid")?;
        let shared =
            reqwest::Url::parse(committed_url.trim()).context("private Report URL is invalid")?;
        if shared.scheme() != backend.scheme()
            || shared.host_str() != backend.host_str()
            || shared.port_or_known_default() != backend.port_or_known_default()
            || shared.query().is_some()
            || shared.fragment().is_some()
        {
            bail!("private Report URL must use the configured Synth backend origin");
        }
        let backend_path = backend.path().trim_end_matches('/');
        let route_prefix = format!("{backend_path}/reports/v1/publications/");
        if !shared.path().starts_with(&route_prefix)
            || shared.path().trim_end_matches('/').ends_with("index.html")
        {
            bail!("private Report URL must be a Report publication URL, not a direct asset");
        }
        if api_key.trim().is_empty() {
            bail!("opening a private shared Report requires a signed-in Synth account");
        }
        let client = crate::http::http_client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(90))
            .build()?;
        let response = client
            .get(committed_url.trim())
            .bearer_auth(&api_key)
            .send()
            .await
            .context("fetch private Report publication")?;
        if !response.status().is_success() {
            bail!(
                "fetch private Report publication failed ({})",
                response.status()
            );
        }
        let publication: Value = response.json().await.context("decode Report publication")?;
        let asset_root = publication
            .get("asset_root")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Report publication is missing asset_root"))?
            .trim_end_matches('/');
        let asset_root = if asset_root.starts_with('/') {
            format!(
                "{backend_url}{asset_root}",
                backend_url = backend.as_str().trim_end_matches('/')
            )
        } else {
            asset_root.to_string()
        };
        let mut fetched = BTreeMap::new();
        for logical_path in ["receipt.json", "data.json", "index.html"] {
            let response = client
                .get(format!("{asset_root}/{logical_path}"))
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
        validate_hosted_report_bundle(
            fetched.remove("index.html").unwrap_or_default(),
            fetched.remove("data.json").unwrap_or_default(),
            fetched.remove("receipt.json").unwrap_or_default(),
        )
    }

    pub async fn list_comments(
        &self,
        report_id: String,
        revision: Option<i64>,
    ) -> Result<Vec<ReportComment>> {
        let db = self.db.clone();
        db.run(move |conn| {
            let sql = if revision.is_some() {
                "SELECT comment_id, report_id, report_revision, receipt_digest, publication_id, anchor, body, author_id, created_at FROM report_review_comments WHERE report_id = ?1 AND report_revision = ?2 ORDER BY created_at ASC"
            } else {
                "SELECT comment_id, report_id, report_revision, receipt_digest, publication_id, anchor, body, author_id, created_at FROM report_review_comments WHERE report_id = ?1 ORDER BY created_at ASC"
            };
            let mut statement = conn.prepare(sql)?;
            let rows = if let Some(revision) = revision {
                statement
                    .query_map(params![report_id, revision], comment_from_row)?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            } else {
                statement
                    .query_map(params![report_id], comment_from_row)?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            Ok(rows)
        })
        .await
    }

    pub async fn create_comment(
        &self,
        report_id: String,
        revision: i64,
        request: ReportCommentCreate,
    ) -> Result<ReportComment> {
        let body = request.body.trim().to_string();
        if body.is_empty() {
            bail!("review comment body is required");
        }
        if body.len() > 4096 {
            bail!("review comment body exceeds 4096 bytes");
        }
        let comment = ReportComment {
            comment_id: format!("cmt_{}", Uuid::new_v4().simple()),
            report_id,
            report_revision: revision,
            receipt_digest: request.receipt_digest,
            publication_id: request.publication_id,
            anchor: request.anchor.filter(|value| !value.trim().is_empty()),
            body,
            author_id: request.author_id.unwrap_or_else(|| "user".into()),
            created_at: Utc::now().to_rfc3339(),
        };
        let db = self.db.clone();
        let stored = comment.clone();
        db.run(move |conn| {
            conn.execute(
                "INSERT INTO report_review_comments(
                    comment_id, report_id, report_revision, receipt_digest, publication_id,
                    anchor, body, author_id, created_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    stored.comment_id,
                    stored.report_id,
                    stored.report_revision,
                    stored.receipt_digest,
                    stored.publication_id,
                    stored.anchor,
                    stored.body,
                    stored.author_id,
                    stored.created_at,
                ],
            )?;
            Ok(stored)
        })
        .await
    }

    async fn perform_upload(
        &self,
        seal: &ReportSeal,
        members: &BTreeMap<&str, (Vec<u8>, String, &str)>,
        backend_url: &str,
        api_key: &str,
    ) -> Result<ReportUpload> {
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
        let prepare_url = format!("{backend_url}/artifacts/v1/workshop/reports/prepare");
        let response = client
            .post(&prepare_url)
            .bearer_auth(api_key)
            .json(&json!({
                "schema_version": WORKSHOP_REPORT_UPLOAD_SCHEMA,
                "report_id": seal.report_id,
                "report_revision": seal.report_revision,
                "receipt_digest": seal.receipt_digest,
                "bundle_schema_version": seal.schema_version,
                "objects": declarations,
            }))
            .send()
            .await
            .context("prepare private Report upload")?;
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            bail!(
                "prepare private Report upload failed ({status}): {}",
                String::from_utf8_lossy(&body)
            );
        }
        let prepared: WorkshopPrepareResponse = serde_json::from_slice(&body)?;
        self.write_upload(ReportUpload {
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
                .context("upload sealed Report bundle member")?;
            if !response.status().is_success() {
                bail!(
                    "upload failed for {} ({})",
                    target.logical_path,
                    response.status()
                );
            }
        }
        self.write_upload(ReportUpload {
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
            "{backend_url}/artifacts/v1/workshop/reports/{}/finalize",
            prepared.publication_id
        );
        let response = client
            .post(finalize_url)
            .bearer_auth(api_key)
            .json(&json!({
                "schema_version": WORKSHOP_REPORT_UPLOAD_SCHEMA,
                "report_id": seal.report_id,
                "receipt_digest": seal.receipt_digest,
            }))
            .send()
            .await
            .context("finalize private Report upload")?;
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            bail!(
                "finalize private Report upload failed ({status}): {}",
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
        if committed.committed_url.ends_with("index.html") {
            bail!("private Report URL must not be a direct index.html asset");
        }
        let committed_url = if committed.committed_url.starts_with('/') {
            format!("{backend_url}{}", committed.committed_url)
        } else {
            committed.committed_url
        };
        Ok(ReportUpload {
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

    async fn write_upload(&self, upload: ReportUpload) -> Result<()> {
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

fn validate_hosted_report_bundle(
    index_html: Vec<u8>,
    data_bytes: Vec<u8>,
    receipt_bytes: Vec<u8>,
) -> Result<ReportSealBundle> {
    let index_html =
        String::from_utf8(index_html).context("hosted report index.html must be UTF-8")?;
    let data: Value = serde_json::from_slice(&data_bytes)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;
    if data.get("schema_version").and_then(Value::as_str) != Some(REPORT_BUNDLE_SCHEMA)
        && data.get("schemaVersion").and_then(Value::as_str) != Some(REPORT_BUNDLE_SCHEMA)
    {
        bail!("hosted Report data is not synth.report-bundle.v1");
    }
    let receipt_digest = hex_sha256(&receipt_bytes);
    let data_digest = hex_sha256(&data_bytes);
    let index_digest = hex_sha256(index_html.as_bytes());
    let report_id = receipt
        .get("report_id")
        .or_else(|| data.pointer("/revision/report_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let report_revision = receipt
        .get("revision")
        .or_else(|| data.pointer("/revision/revision"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Ok(ReportSealBundle {
        seal: ReportSeal {
            receipt_digest,
            report_id,
            report_revision,
            schema_version: REPORT_BUNDLE_SCHEMA.into(),
            compiler_name: receipt
                .pointer("/compiler/name")
                .and_then(Value::as_str)
                .unwrap_or("workshop")
                .into(),
            compiler_version: receipt
                .pointer("/compiler/version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            runtime_digest: receipt
                .pointer("/compiler/runtime_digest")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            index_digest,
            data_digest,
            receipt_size_bytes: receipt_bytes.len() as i64,
            total_size_bytes: (index_html.len() + data_bytes.len() + receipt_bytes.len()) as i64,
            created_at: Utc::now().to_rfc3339(),
        },
        index_html,
        data,
        receipt,
    })
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn upload_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReportUpload> {
    Ok(ReportUpload {
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

fn comment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReportComment> {
    Ok(ReportComment {
        comment_id: row.get(0)?,
        report_id: row.get(1)?,
        report_revision: row.get(2)?,
        receipt_digest: row.get(3)?,
        publication_id: row.get(4)?,
        anchor: row.get(5)?,
        body: row.get(6)?,
        author_id: row.get(7)?,
        created_at: row.get(8)?,
    })
}

fn upsert_upload(conn: &rusqlite::Connection, upload: &ReportUpload) -> Result<()> {
    conn.execute(
        "INSERT INTO report_uploads(
            receipt_digest,collection_id,publication_id,publication_revision,
            state,committed_url,error,updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{json_response, serve_connections, LoopbackBody};
    use crate::reports::{ReportCommentCreate, ReportCreateRequest, ReportRegistry};
    use crate::storage::{ContentStore, EventJournal, Storage};
    use crate::visuals::VisualRegistry;
    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::body::Incoming;
    use hyper::{Method, Request, Response, StatusCode};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::convert::Infallible;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    const SLOT_KEY: &str = "slot-key";
    const PUBLICATION_ID: &str = "22222222-2222-2222-2222-222222222222";
    const COLLECTION_ID: &str = "11111111-1111-1111-1111-111111111111";
    const MANIFEST_DIGEST: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct SlotState {
        fail_puts: bool,
        files: BTreeMap<String, Vec<u8>>,
        report_id: Option<String>,
        report_revision: Option<i64>,
        committed: bool,
    }

    fn registry(dir: &std::path::Path) -> ReportRegistry {
        let storage = Storage::open(dir).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let content = ContentStore::new(storage.content_root());
        let visuals =
            VisualRegistry::new(storage.database().clone(), journal.clone(), content.clone());
        ReportRegistry::new(storage.database().clone(), journal, content, visuals)
    }

    async fn seed_sealed(reports: &ReportRegistry, id: &str) -> ReportSeal {
        let (created, _) = reports
            .create(ReportCreateRequest {
                title: Some("Local slot hosted note".into()),
                summary: Some("Private Report URL via local slot.".into()),
                authors: Some(vec!["researcher".into()]),
                project_ref: None,
                id: Some(id.into()),
                created_by: Some("user".into()),
                blocks: None,
            })
            .await
            .unwrap();
        reports.seal(created.id, 1).await.unwrap().0
    }

    fn bytes_response(
        status: StatusCode,
        body: Vec<u8>,
        content_type: &str,
    ) -> Response<LoopbackBody> {
        Response::builder()
            .status(status)
            .header("content-type", content_type)
            .body(
                Full::new(Bytes::from(body))
                    .map_err(|never| match never {})
                    .boxed(),
            )
            .expect("static loopback body")
    }

    fn authorized(request: &Request<Incoming>) -> bool {
        request
            .headers()
            .get(hyper::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            == Some(&format!("Bearer {SLOT_KEY}"))
    }

    async fn handle_slot(
        request: Request<Incoming>,
        origin: String,
        state: Arc<Mutex<SlotState>>,
    ) -> Response<LoopbackBody> {
        let method = request.method().clone();
        let path = request.uri().path().to_string();
        if method != Method::PUT && !authorized(&request) {
            return json_response(
                StatusCode::UNAUTHORIZED,
                json!({"error":"signed-in local slot required"}),
            );
        }
        if method == Method::POST && path == "/artifacts/v1/workshop/reports/prepare" {
            let collected = http_body_util::Limited::new(request.into_body(), 1024 * 1024)
                .collect()
                .await
                .expect("prepare body");
            let body: Value = serde_json::from_slice(&collected.to_bytes()).unwrap();
            let objects = body["objects"].as_array().cloned().unwrap_or_default();
            let report_id = body["report_id"].as_str().unwrap_or_default().to_string();
            let report_revision = body["report_revision"].as_i64().unwrap_or(1);
            {
                let mut slot = state.lock().unwrap();
                slot.report_id = Some(report_id);
                slot.report_revision = Some(report_revision);
            }
            let upload_targets = objects
                .iter()
                .map(|object| {
                    let logical_path = object["logical_path"].as_str().unwrap_or_default();
                    json!({
                        "logical_path": logical_path,
                        "digest_sha256": object["digest_sha256"],
                        "upload_url": format!("{origin}/upload/{logical_path}"),
                        "required_headers": {
                            "Content-Type": object["media_type"],
                        }
                    })
                })
                .collect::<Vec<_>>();
            return json_response(
                StatusCode::OK,
                json!({
                    "publication_id": PUBLICATION_ID,
                    "collection_id": COLLECTION_ID,
                    "revision": report_revision,
                    "manifest_digest": MANIFEST_DIGEST,
                    "upload_targets": upload_targets,
                }),
            );
        }
        if method == Method::PUT && path.starts_with("/upload/") {
            let logical_path = path.trim_start_matches("/upload/").to_string();
            if state.lock().unwrap().fail_puts {
                return json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({"error":"put failed"}),
                );
            }
            let collected = http_body_util::Limited::new(request.into_body(), 8 * 1024 * 1024)
                .collect()
                .await
                .expect("put body");
            state
                .lock()
                .unwrap()
                .files
                .insert(logical_path, collected.to_bytes().to_vec());
            return bytes_response(StatusCode::OK, Vec::new(), "application/octet-stream");
        }
        if method == Method::POST
            && path.starts_with("/artifacts/v1/workshop/reports/")
            && path.ends_with("/finalize")
        {
            let mut slot = state.lock().unwrap();
            if slot.files.len() != 3 {
                return json_response(
                    StatusCode::CONFLICT,
                    json!({"error":"partial upload creates no Report URL"}),
                );
            }
            slot.committed = true;
            return json_response(
                StatusCode::OK,
                json!({
                    "publication_id": PUBLICATION_ID,
                    "collection_id": COLLECTION_ID,
                    "revision": slot.report_revision.unwrap_or(1),
                    "manifest_digest": MANIFEST_DIGEST,
                    "status": "committed",
                    "committed_url": format!("/reports/v1/publications/{PUBLICATION_ID}"),
                }),
            );
        }
        if method == Method::GET && path == format!("/reports/v1/publications/{PUBLICATION_ID}") {
            let slot = state.lock().unwrap();
            if !slot.committed {
                return json_response(StatusCode::NOT_FOUND, json!({"error":"not committed"}));
            }
            return json_response(
                StatusCode::OK,
                json!({
                    "report_id": slot.report_id.clone().unwrap_or_default(),
                    "report_revision": slot.report_revision.unwrap_or(1),
                    "asset_root": format!("/artifacts/v1/publications/{PUBLICATION_ID}/assets"),
                }),
            );
        }
        if method == Method::GET
            && path.starts_with(&format!(
                "/artifacts/v1/publications/{PUBLICATION_ID}/assets/"
            ))
        {
            let logical_path = path.rsplit('/').next().unwrap_or_default().to_string();
            let slot = state.lock().unwrap();
            let Some(bytes) = slot.files.get(&logical_path).cloned() else {
                return json_response(StatusCode::NOT_FOUND, json!({"error":"missing asset"}));
            };
            let media = match logical_path.as_str() {
                "index.html" => REPORT_INDEX_MEDIA,
                "receipt.json" => REPORT_RECEIPT_MEDIA,
                _ => REPORT_DATA_MEDIA,
            };
            return bytes_response(StatusCode::OK, bytes, media);
        }
        json_response(
            StatusCode::NOT_FOUND,
            json!({"error":"unknown local-slot route"}),
        )
    }

    async fn spawn_slot(
        fail_puts: bool,
    ) -> (String, tokio::task::JoinHandle<()>, Arc<Mutex<SlotState>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let origin = format!("http://{addr}");
        let state = Arc::new(Mutex::new(SlotState {
            fail_puts,
            files: BTreeMap::new(),
            report_id: None,
            report_revision: None,
            committed: false,
        }));
        let served = state.clone();
        let served_origin = origin.clone();
        let task = tokio::spawn(async move {
            let _ = serve_connections(listener, move |request| {
                let state = served.clone();
                let origin = served_origin.clone();
                async move { Ok::<_, Infallible>(handle_slot(request, origin, state).await) }
            })
            .await;
        });
        (origin, task, state)
    }

    #[tokio::test]
    async fn private_share_against_a_local_slot_creates_a_report_url_not_index_html() {
        let dir = tempdir().unwrap();
        let reports = registry(dir.path());
        let seal = seed_sealed(&reports, "rep_local_slot").await;
        let (origin, task, _) = spawn_slot(false).await;
        let (upload, _) = reports
            .share_seal(seal.receipt_digest.clone(), origin.clone(), SLOT_KEY.into())
            .await
            .unwrap();
        assert_eq!(upload.state, "committed");
        let url = upload.committed_url.clone().expect("committed Report URL");
        assert!(
            url.ends_with(&format!("/reports/v1/publications/{PUBLICATION_ID}")),
            "unexpected Report URL {url}"
        );
        assert!(!url.ends_with("index.html"));
        let opened = reports
            .open_shared_url(url, origin, SLOT_KEY.into())
            .await
            .unwrap();
        assert_eq!(opened.seal.receipt_digest, seal.receipt_digest);
        assert_eq!(opened.seal.report_id, "rep_local_slot");
        task.abort();
    }

    #[tokio::test]
    async fn failed_local_slot_put_leaves_no_report_url() {
        let dir = tempdir().unwrap();
        let reports = registry(dir.path());
        let seal = seed_sealed(&reports, "rep_local_slot_fail").await;
        let (origin, task, _) = spawn_slot(true).await;
        let error = reports
            .share_seal(seal.receipt_digest.clone(), origin, SLOT_KEY.into())
            .await
            .expect_err("partial upload must fail closed");
        assert!(
            error.to_string().contains("upload failed"),
            "unexpected error: {error}"
        );
        let status = reports
            .upload_status(seal.receipt_digest)
            .await
            .unwrap()
            .expect("failed upload row");
        assert_eq!(status.state, "failed");
        assert!(status.committed_url.is_none());
        task.abort();
    }

    #[tokio::test]
    async fn open_shared_refuses_a_direct_index_html_asset() {
        let dir = tempdir().unwrap();
        let reports = registry(dir.path());
        let error = reports
            .open_shared_url(
                "http://127.0.0.1:41109/reports/v1/publications/22222222-2222-2222-2222-222222222222/index.html"
                    .into(),
                "http://127.0.0.1:41109".into(),
                SLOT_KEY.into(),
            )
            .await
            .expect_err("index.html is not a Report URL");
        assert!(
            error.to_string().contains("not a direct asset"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn review_comments_overlay_a_seal_without_changing_its_digest() {
        let dir = tempdir().unwrap();
        let reports = registry(dir.path());
        let seal = seed_sealed(&reports, "rep_local_slot_comments").await;
        reports
            .create_comment(
                seal.report_id.clone(),
                seal.report_revision,
                ReportCommentCreate {
                    body: "Private overlay only.".into(),
                    anchor: None,
                    author_id: Some("user".into()),
                    receipt_digest: Some(seal.receipt_digest.clone()),
                    publication_id: None,
                },
            )
            .await
            .unwrap();
        let reopened = reports.get_seal(seal.receipt_digest.clone()).await.unwrap();
        assert_eq!(reopened.seal.receipt_digest, seal.receipt_digest);
        assert_eq!(
            reports
                .list_comments(seal.report_id, Some(seal.report_revision))
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
