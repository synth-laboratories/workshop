//! Start a workspace-declared container, wait until it is healthy, register it.
//!
//! Registration is the durable handle. This module only brings a process up
//! (or attaches an already-running URL) so agents do not scan ports or execute
//! out of a cookbook pin.

use super::workspace_recipe::{self, ContainerSpec, PolicyLocality};
use crate::storage::Database;
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::json;
use std::{
    collections::HashMap,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use uuid::Uuid;

const HEALTH_WAIT: Duration = Duration::from_secs(60);
const HEALTH_POLL: Duration = Duration::from_millis(200);

fn children() -> &'static Mutex<HashMap<String, Child>> {
    static CHILDREN: OnceLock<Mutex<HashMap<String, Child>>> = OnceLock::new();
    CHILDREN.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone, Debug)]
pub struct EnsuredContainer {
    pub container_id: String,
    pub base_url: String,
    pub spec_id: String,
    pub locality: PolicyLocality,
}

pub async fn ensure(
    db: &Arc<Database>,
    workspace: &Path,
    spec_id: &str,
) -> Result<EnsuredContainer> {
    let spec = workspace_recipe::find_container_spec(workspace, spec_id)?;
    ensure_spec(db, workspace, &spec).await
}

pub async fn ensure_spec(
    db: &Arc<Database>,
    workspace: &Path,
    spec: &ContainerSpec,
) -> Result<EnsuredContainer> {
    let base_url = if let Some(url) = spec.url.as_deref() {
        let base = url.trim_end_matches('/').to_string();
        if !spec.command.is_empty() {
            start_command(workspace, spec)?;
        }
        base
    } else {
        bail!(
            "container `{}` must declare url so Workshop can probe /health without scanning ports",
            spec.id
        );
    };
    wait_healthy(&base_url, &spec.health).await?;
    let container_id = upsert_ready(db, spec, &base_url).await?;
    Ok(EnsuredContainer {
        container_id,
        base_url,
        spec_id: spec.id.clone(),
        locality: spec.locality,
    })
}

fn start_command(workspace: &Path, spec: &ContainerSpec) -> Result<()> {
    let cwd = if spec.cwd.exists() {
        spec.cwd.clone()
    } else {
        workspace_recipe::resolve_workspace_path(workspace, ".")?
    };
    if !cwd.starts_with(
        workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.to_path_buf()),
    ) {
        bail!(
            "container `{}` cwd {} is not inside the workspace",
            spec.id,
            cwd.display()
        );
    }
    let program = spec
        .command
        .first()
        .ok_or_else(|| anyhow!("container `{}` command is empty", spec.id))?;
    let mut command = Command::new(program);
    command.envs(&spec.environment);
    for provider in &spec.credential_providers {
        match provider.as_str() {
            "openrouter" => {
                let value = crate::synth_config::openrouter_api_key()?
                    .ok_or_else(|| anyhow!("container `{}` requires an OpenRouter credential", spec.id))?;
                command.env("OPENROUTER_API_KEY", value);
            }
            other => bail!("container `{}` requests unsupported credential provider `{other}`", spec.id),
        }
    }
    command
        .args(&spec.command[1..])
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command.spawn().with_context(|| {
        format!(
            "start container `{}` command {:?}",
            spec.id, spec.command
        )
    })?;
    children()
        .lock()
        .expect("container process table")
        .insert(spec.id.clone(), child);
    Ok(())
}

async fn wait_healthy(base_url: &str, health_path: &str) -> Result<()> {
    let url = format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        if health_path.starts_with('/') {
            health_path.to_string()
        } else {
            format!("/{health_path}")
        }
    );
    let client = crate::http::http_client_builder()
        .timeout(Duration::from_secs(2))
        .build()?;
    let deadline = Instant::now() + HEALTH_WAIT;
    let mut last = String::from("not probed");
    while Instant::now() < deadline {
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => last = format!("HTTP {}", response.status()),
            Err(error) => last = error.to_string(),
        }
        tokio::time::sleep(HEALTH_POLL).await;
    }
    bail!("container at {base_url} never became healthy on {url}: {last}")
}

async fn upsert_ready(
    db: &Arc<Database>,
    spec: &ContainerSpec,
    base_url: &str,
) -> Result<String> {
    let spec_id = spec.id.clone();
    let family = spec.family.clone();
    let contract = spec.contract.clone();
    let locality = spec.locality.as_str().to_string();
    let base_url = base_url.to_string();
    db.clone()
        .run_transaction(move |conn| {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM containers WHERE base_url = ?1 LIMIT 1",
                    params![&base_url],
                    |row| row.get(0),
                )
                .optional()?;
            let id = existing.unwrap_or_else(|| format!("ctr_{}", Uuid::new_v4().simple()));
            let now = chrono::Utc::now().to_rfc3339();
            let metadata = serde_json::to_string(&json!({
                "workspaceSpecId": spec_id,
                "contract": contract,
                "locality": locality,
                "source": "workspace",
            }))?;
            conn.execute(
                "INSERT INTO containers(id,name,location,status,base_url,task_family,health_json,metadata_json,created_at,updated_at)
                 VALUES(?1,?2,'local','ready',?3,?4,'{\"ok\":true}',?5,?6,?6)
                 ON CONFLICT(id) DO UPDATE SET
                    name=excluded.name,
                    status=excluded.status,
                    base_url=excluded.base_url,
                    task_family=excluded.task_family,
                    health_json=excluded.health_json,
                    metadata_json=excluded.metadata_json,
                    updated_at=excluded.updated_at",
                params![
                    id,
                    spec_id,
                    base_url,
                    family,
                    metadata,
                    now
                ],
            )?;
            Ok(id)
        })
        .await
}
