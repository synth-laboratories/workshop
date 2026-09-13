//! Workshop "Prepare Contribution": selected files → manifest/provenance/rights/
//! visibility preview → private draft upload → explicit submit → QA status.
//!
//! Uses the canonical backend intake (`/api/v1/index/contributions`), never a
//! parallel path. The API key stays in Rust; the renderer holds no secrets.
//! Nothing here approves or publishes: public release is only a *requested*
//! audience that the owner must confirm and an independent reviewer must accept.

pub mod client;
pub mod export;

use anyhow::{anyhow, Context, Result};
use client::IndexClient;
use export::{collect_selected_files, sha256_hex, ExportError, IndexSelectedFile, SelectedAsset};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, time::Duration};

const HTTP_TIMEOUT: Duration = Duration::from_secs(60);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);
const PENDING_CONTRIBUTION: &str = "pending_contribution";
const PENDING_REVISION: &str = "pending_revision";

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexReference {
    pub contribution_id: String,
    pub revision_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexExportMetadata {
    pub title: String,
    pub abstract_text: String,
    pub kind: String,
    pub research_areas: Vec<String>,
    pub workflow_stages: Vec<String>,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    pub reusable: String,
    pub how_to_use: String,
    pub observed: String,
    pub limitations: String,
    pub resources: String,
    pub claim_kind: String,
    pub claim_statement: String,
    pub claim_scope: String,
    pub claim_assessment: String,
    pub missing_evidence_reason: Option<String>,
    pub reproduction_level: String,
    pub reproduction_instructions: String,
    pub expected_outputs: String,
    pub restrictions: String,
    pub license: String,
    pub rights_attested: bool,
    pub sensitive_data: String,
    pub requested_audience: String,
    #[serde(default)]
    pub confirm_public_release: bool,
    pub contributor_principal_id: String,
    pub contributor_roles: Vec<String>,
    #[serde(default)]
    pub upstream: Vec<IndexReference>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexExportRequest {
    /// Client-generated stable id; retries with the same id reuse the draft and publication.
    pub export_id: String,
    pub root: String,
    pub files: Vec<IndexSelectedFile>,
    pub metadata: IndexExportMetadata,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexExportPreview {
    pub assets: Vec<SelectedAsset>,
    pub total_bytes: u64,
    pub requested_audience: String,
    pub package: Value,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexExportOutcome {
    pub reference: IndexReference,
    pub publication_id: String,
    pub uploaded_paths: Vec<String>,
    pub finalized: Value,
    /// Always "finalized": submission for review is a separate explicit command.
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexSubmitRequest {
    pub reference: IndexReference,
    pub export_id: String,
}

fn invalid(message: impl Into<String>) -> ExportError {
    ExportError::Invalid(message.into())
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.starts_with(|c: char| c.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
}

fn export_key(export_id: &str) -> Result<String, ExportError> {
    if !is_identifier(export_id) || export_id.len() > 110 {
        return Err(invalid("export id must be a short identifier"));
    }
    Ok(format!("workshop-{export_id}"))
}

/// Stable UUID-shaped publication id per export, so uncertain retries resume.
pub fn publication_id_for(export_id: &str) -> String {
    use sha2::Digest;
    let digest =
        sha2::Sha256::digest(format!("synth.index.workshop.publication:{export_id}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

fn by_role<'a>(
    assets: &'a [SelectedAsset],
    role: &'a str,
) -> impl Iterator<Item = &'a SelectedAsset> + 'a {
    assets.iter().filter(move |asset| asset.role == role)
}

/// Canonical `synth.contribution.v1` package for exactly the selected assets.
pub fn build_package(
    contribution_id: &str,
    revision_id: &str,
    meta: &IndexExportMetadata,
    assets: &[SelectedAsset],
) -> Result<Value, ExportError> {
    if !meta.rights_attested {
        return Err(invalid(
            "attest that you have the rights to share every selected file",
        ));
    }
    match meta.requested_audience.as_str() {
        "private" | "org" => {}
        "public" if meta.confirm_public_release => {}
        "public" => return Err(invalid("confirm the requested public release explicitly")),
        other => return Err(invalid(format!("unknown audience {other:?}"))),
    }
    if meta.title.trim().is_empty() || meta.title.chars().count() > 200 {
        return Err(invalid("title must be 1–200 characters"));
    }
    if !is_identifier(&meta.contributor_principal_id) {
        return Err(invalid("contributor id must be your Synth user id"));
    }
    let evidence_kind = if meta.claim_kind == "empirical" {
        "comparison"
    } else {
        "experiment"
    };
    let evidence: Vec<Value> = by_role(assets, "evidence")
        .map(|asset| {
            json!({
                "evidence_id": format!("ev_{}", asset.asset_id).chars().take(128).collect::<String>(),
                "kind": evidence_kind,
                "asset_id": asset.asset_id,
                "locator": asset.logical_path,
            })
        })
        .collect();
    let evidence_ids: Vec<Value> = evidence
        .iter()
        .map(|item| item["evidence_id"].clone())
        .collect();
    let mut claim = json!({
        "claim_id": "claim_1",
        "kind": meta.claim_kind,
        "statement": meta.claim_statement,
        "scope": meta.claim_scope,
    });
    if evidence_ids.is_empty() {
        let reason = meta
            .missing_evidence_reason
            .as_deref()
            .filter(|reason| !reason.trim().is_empty())
            .ok_or_else(|| invalid("without evidence files, explain why the claim is untested"))?;
        claim["author_assessment"] = json!("not_tested");
        claim["missing_evidence_reason"] = json!(reason);
    } else {
        claim["author_assessment"] = json!(meta.claim_assessment);
        let side = if meta.claim_assessment == "disproven" {
            "contradicting_evidence_ids"
        } else {
            "supporting_evidence_ids"
        };
        claim[side] = json!(evidence_ids);
    }
    let entrypoint = by_role(assets, "reproduce")
        .next()
        .map(|asset| asset.asset_id.clone());
    let environment: Vec<String> = by_role(assets, "environment")
        .map(|asset| asset.asset_id.clone())
        .collect();
    if matches!(meta.reproduction_level.as_str(), "runnable" | "reproduced")
        && (entrypoint.is_none() || environment.is_empty())
    {
        return Err(invalid(
            "runnable reproduction needs a 'reproduce' and an 'environment' file",
        ));
    }
    Ok(json!({
        "schema_version": "synth.contribution.v1",
        "contribution_id": contribution_id,
        "revision_id": revision_id,
        "kind": meta.kind,
        "title": meta.title.trim(),
        "abstract": meta.abstract_text,
        "research_areas": meta.research_areas,
        "workflow_stages": meta.workflow_stages,
        "tag_ids": meta.tag_ids,
        "card": {
            "reusable": meta.reusable,
            "how_to_use": meta.how_to_use,
            "observed": meta.observed,
            "limitations": meta.limitations,
            "resources": meta.resources,
        },
        "assets": assets.iter().map(|asset| json!({
            "asset_id": asset.asset_id,
            "role": asset.role,
            "object": {
                "logical_path": asset.logical_path,
                "digest_sha256": asset.digest_sha256,
                "size_bytes": asset.size_bytes,
                "media_type": asset.media_type,
            },
            "license": meta.license,
        })).collect::<Vec<_>>(),
        "claims": [claim],
        "evidence": evidence,
        "provenance": {
            "origin": "user",
            "contributors": [{
                "principal_id": meta.contributor_principal_id,
                "roles": meta.contributor_roles,
            }],
            "upstream": meta.upstream.iter().map(|item| json!({
                "contribution_id": item.contribution_id,
                "revision_id": item.revision_id,
            })).collect::<Vec<_>>(),
            "tools": ["synth-workshop"],
        },
        "reproduction": {
            "level": meta.reproduction_level,
            "instructions": meta.reproduction_instructions,
            "expected_outputs": meta.expected_outputs,
            "restrictions": meta.restrictions,
            "entrypoint_asset_id": entrypoint,
            "environment_asset_ids": environment,
        },
        "requested_audience": meta.requested_audience,
        "rights_attested": meta.rights_attested,
        "sensitive_data": meta.sensitive_data,
    }))
}

fn warnings(meta: &IndexExportMetadata, assets: &[SelectedAsset]) -> Vec<String> {
    let mut notes = Vec::new();
    if meta.requested_audience == "public" {
        notes.push(
            "Public release is a request: an independent reviewer must approve it first.".into(),
        );
    }
    if !assets.iter().any(|asset| asset.role == "report") {
        notes
            .push("No report file selected; research reports and replications require one.".into());
    }
    if meta.sensitive_data != "none_declared" {
        notes.push(
            "Sensitive data is declared or unknown; reviewers may narrow the audience.".into(),
        );
    }
    notes
}

/// Local-only preview: reads the selected files, never contacts the network.
pub fn preview(request: &IndexExportRequest) -> Result<IndexExportPreview> {
    export_key(&request.export_id)?;
    let assets = collect_selected_files(Path::new(&request.root), &request.files)?;
    let package = build_package(
        PENDING_CONTRIBUTION,
        PENDING_REVISION,
        &request.metadata,
        &assets,
    )?;
    Ok(IndexExportPreview {
        total_bytes: assets.iter().map(|asset| asset.size_bytes).sum(),
        requested_audience: request.metadata.requested_audience.clone(),
        warnings: warnings(&request.metadata, &assets),
        package,
        assets,
    })
}

fn client_from_config() -> Result<IndexClient> {
    let backend = crate::synth_config::resolve().context("resolve Synth backend")?;
    let api_key = backend
        .api_key
        .ok_or_else(|| anyhow!("Sign in to Synth before preparing a Contribution"))?;
    Ok(IndexClient::connect(
        &backend.backend_url,
        api_key,
        HTTP_TIMEOUT,
    )?)
}

fn string_at<'a>(value: &'a Value, pointer: &str) -> Result<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Index response is missing {pointer}"))
}

/// Create (or resume) the private draft, upload the selected bytes, finalize.
pub async fn upload(request: IndexExportRequest) -> Result<IndexExportOutcome> {
    let key = export_key(&request.export_id)?;
    let assets = tokio::task::spawn_blocking({
        let root = request.root.clone();
        let files = request.files.clone();
        move || collect_selected_files(Path::new(&root), &files)
    })
    .await
    .context("collect selected files")??;
    let client = client_from_config()?;
    let draft = client.create_draft(&key).await?;
    let contribution_id = string_at(&draft, "/reference/contribution_id")?.to_owned();
    let revision_id = string_at(&draft, "/reference/revision_id")?.to_owned();
    let package = build_package(&contribution_id, &revision_id, &request.metadata, &assets)?;
    let publication_id = publication_id_for(&request.export_id);
    let prepared = client
        .prepare_upload(&contribution_id, &revision_id, &publication_id, &package)
        .await?;
    if string_at(&prepared, "/transfer/publication_id")? != publication_id {
        return Err(anyhow!("prepared publication does not match this export"));
    }
    let descriptor = string_at(&prepared, "/descriptor_json")?
        .as_bytes()
        .to_vec();
    let mut bodies: BTreeMap<String, Vec<u8>> = assets
        .iter()
        .map(|asset| (asset.logical_path.clone(), asset.bytes.clone()))
        .collect();
    bodies.insert("contribution.json".into(), descriptor);
    let mut targets = Vec::new();
    for target in prepared
        .pointer("/transfer/upload_targets")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Index response is missing upload targets"))?
    {
        let path = string_at(target, "/logical_path")?;
        let body = bodies
            .get(path)
            .ok_or_else(|| anyhow!("upload target {path:?} was not selected"))?;
        if sha256_hex(body) != string_at(target, "/digest_sha256")? {
            return Err(anyhow!(
                "upload target {path:?} digest differs from the selected bytes"
            ));
        }
        let headers: BTreeMap<String, String> =
            serde_json::from_value(target.get("required_headers").cloned().unwrap_or(json!({})))?;
        targets.push((
            string_at(target, "/upload_url")?.to_owned(),
            headers,
            body.clone(),
        ));
    }
    client::transfer(&targets, TRANSFER_TIMEOUT).await?;
    let finalized = client
        .finalize(&contribution_id, &revision_id, &publication_id)
        .await?;
    if string_at(&finalized, "/status")? != "committed" {
        return Err(anyhow!("finalization did not commit the publication"));
    }
    Ok(IndexExportOutcome {
        reference: IndexReference {
            contribution_id,
            revision_id,
        },
        publication_id,
        uploaded_paths: assets
            .iter()
            .map(|asset| asset.logical_path.clone())
            .collect(),
        finalized,
        status: "finalized".into(),
    })
}

/// Explicit owner action: seal the finalized revision for independent review.
pub async fn submit(request: IndexSubmitRequest) -> Result<Value> {
    export_key(&request.export_id)?;
    let client = client_from_config()?;
    Ok(client
        .submit(
            &request.reference.contribution_id,
            &request.reference.revision_id,
            &publication_id_for(&request.export_id),
        )
        .await?)
}

/// Current revision status and reviewer assessments under live authorization.
pub async fn status(reference: IndexReference) -> Result<Value> {
    let client = client_from_config()?;
    Ok(client
        .revision(&reference.contribution_id, &reference.revision_id)
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> IndexExportMetadata {
        IndexExportMetadata {
            title: "Verifier ablations".into(),
            abstract_text: "What we tried.".into(),
            kind: "research_report".into(),
            research_areas: vec!["verifier".into()],
            workflow_stages: vec!["evaluation".into()],
            tag_ids: vec![],
            reusable: "r".into(),
            how_to_use: "h".into(),
            observed: "o".into(),
            limitations: "l".into(),
            resources: "s".into(),
            claim_kind: "diagnostic".into(),
            claim_statement: "c".into(),
            claim_scope: "two tasks".into(),
            claim_assessment: "supported".into(),
            missing_evidence_reason: None,
            reproduction_level: "inspectable".into(),
            reproduction_instructions: "read".into(),
            expected_outputs: "none".into(),
            restrictions: "none".into(),
            license: "CC-BY-4.0".into(),
            rights_attested: true,
            sensitive_data: "none_declared".into(),
            requested_audience: "private".into(),
            confirm_public_release: false,
            contributor_principal_id: "00000000-0000-4000-8000-000000000002".into(),
            contributor_roles: vec!["research".into()],
            upstream: vec![],
        }
    }

    fn asset(role: &str, path: &str) -> SelectedAsset {
        SelectedAsset {
            asset_id: path.replace('/', "_"),
            role: role.into(),
            logical_path: path.into(),
            media_type: "text/markdown".into(),
            size_bytes: 1,
            digest_sha256: "a".repeat(64),
            bytes: vec![b'x'],
        }
    }

    #[test]
    fn package_declares_only_selected_assets_with_evidence_links() {
        let assets = [asset("report", "report.md"), asset("evidence", "runs.json")];
        let package = build_package("ctr_a", "rev_a", &meta(), &assets).unwrap();
        assert_eq!(package["assets"].as_array().unwrap().len(), 2);
        assert_eq!(
            package["claims"][0]["supporting_evidence_ids"][0],
            "ev_runs.json"
        );
        assert_eq!(package["requested_audience"], "private");
        assert_eq!(package["provenance"]["origin"], "user");
    }

    #[test]
    fn rights_public_release_and_missing_evidence_are_explicit() {
        let assets = [asset("report", "report.md")];
        let mut unattested = meta();
        unattested.rights_attested = false;
        assert!(build_package("c", "r", &unattested, &assets).is_err());
        let mut public = meta();
        public.requested_audience = "public".into();
        public.missing_evidence_reason = Some("qualitative".into());
        assert!(build_package("c", "r", &public, &assets).is_err());
        public.confirm_public_release = true;
        assert!(build_package("c", "r", &public, &assets).is_ok());
        let untested = build_package("c", "r", &meta(), &assets);
        assert!(untested.is_err(), "no evidence requires an explanation");
    }

    #[test]
    fn publication_id_is_stable_uuid_v4_shape() {
        let first = publication_id_for("export_a");
        assert_eq!(first, publication_id_for("export_a"));
        assert_ne!(first, publication_id_for("export_b"));
        let parsed = uuid::Uuid::parse_str(&first).unwrap();
        assert_eq!(parsed.get_version_num(), 4);
    }
}
