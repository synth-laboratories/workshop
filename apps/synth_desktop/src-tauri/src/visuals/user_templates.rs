//! Native authoring boundary for instance-local TSX templates.
use super::{templates, TemplateMeta};
use crate::{contract::specta::OpaqueJson, error::AppError, session::template_persist};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::fs;

pub fn shell_source(id: &str) -> Result<String> {
    let path = templates::user_template_path(id)?;
    let meta =
        templates::instance_template(&path)?.context("user template is incomplete or missing")?;
    if meta.source_kind.as_deref() != Some("user") {
        bail!("only user TSX templates expose shell source");
    }
    let bytes = fs::read(path.join("shell.tsx"))?;
    if bytes.len() > 256 * 1024 {
        bail!("user template exceeds 256 KiB renderer limit");
    }
    String::from_utf8(bytes).context("user template source must be UTF-8")
}

async fn save(
    app: &tauri::AppHandle,
    session: &str,
    id: &str,
    manifest: &str,
    source: &str,
) -> Result<TemplateMeta> {
    let prepared = templates::prepare_user_save(id, manifest, source)?;
    let consent = template_persist::authorize(app, Some(session), &prepared.request()?).await?;
    prepared.persist(consent)
}

#[tauri::command]
#[specta::specta]
pub fn visuals_template_shell_source(template_id: String) -> Result<String, AppError> {
    shell_source(&template_id).map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn visuals_template_save(
    app: tauri::AppHandle,
    session_id: String,
    template_id: String,
    manifest: String,
    source: String,
) -> Result<TemplateMeta, AppError> {
    save(&app, &session_id, &template_id, &manifest, &source)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn visuals_template_create(
    app: tauri::AppHandle,
    session_id: String,
    template_id: String,
    from_template_id: String,
    title: Option<String>,
) -> Result<TemplateMeta, AppError> {
    let result: Result<TemplateMeta> = async {
        let destination = templates::user_template_path(&template_id)?;
        if fs::symlink_metadata(&destination).is_ok() {
            bail!("fork requires an unused template id; use save to update an existing template");
        }
        if template_id == from_template_id {
            bail!("fork requires a new template id");
        }
        let origin = templates::resolve_template(&from_template_id)?;
        let path = origin
            .path
            .as_deref()
            .context("origin template has no source directory")?;
        let source = fs::read_to_string(
            origin
                .shell_path
                .as_deref()
                .context("only TSX templates can be forked")?,
        )?;
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(std::path::Path::new(path).join("template.json"))?)?;
        manifest["id"] = json!(template_id);
        manifest["rendererKind"] = json!("template");
        manifest["forkedFrom"] = json!({"templateId": from_template_id, "version": origin.version});
        if let Some(title) = title {
            manifest["title"] = json!(title);
        }
        save(
            &app,
            &session_id,
            &template_id,
            &serde_json::to_string_pretty(&manifest)?,
            &source,
        )
        .await
    }
    .await;
    result.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn visuals_template_validate(template_id: String) -> Result<OpaqueJson, AppError> {
    let result = templates::user_template_path(&template_id)
        .and_then(|path| {
            templates::instance_template(&path)?
                .context("template requires template.json and shell.tsx")
        })
        .and_then(|meta| {
            if meta.source_kind.as_deref() != Some("user") {
                bail!("not a user TSX template");
            }
            Ok(meta)
        });
    let (ok, source_kind, findings) = match result {
        Ok(meta) => (true, meta.source_kind, vec![]),
        Err(error) => (
            false,
            None,
            vec![json!({"code":"user_template_unavailable", "message":error.to_string()})],
        ),
    };
    Ok(OpaqueJson(
        json!({"schemaVersion":"synth.user-template-validation.v1", "id":template_id,
        "path":templates::user_template_path(&template_id).ok().map(|path| path.display().to_string()),
        "ok":ok, "sourceKind":source_kind, "findings":findings,
        "sourceScan":"Import allowlist and forbidden-token validation run in the visual pane; structural validation is not code approval."}),
    ))
}
