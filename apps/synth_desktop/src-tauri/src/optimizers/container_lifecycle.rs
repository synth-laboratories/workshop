//! Start a catalogued container, wait until it is healthy, register it.
//!
//! Registration is the durable handle. This module only brings a process up
//! (or attaches an already-running URL) so agents do not scan ports or execute
//! out of a cookbook pin.

use super::{
    container_catalog::{self, ContainerSource},
    workspace_recipe::{self, ContainerSpec, PolicyLocality},
};
use crate::storage::Database;
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::json;
use std::{
    collections::HashMap,
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
    pub source_id: String,
    pub manifest_hash: String,
    pub spec_id: String,
    pub locality: PolicyLocality,
}

pub async fn ensure(
    db: &Arc<Database>,
    source_id: &str,
    spec_id: &str,
) -> Result<EnsuredContainer> {
    let (source, spec) = container_catalog::resolve(db, source_id, spec_id).await?;
    ensure_spec(db, &source, &spec).await
}

/// Compatibility path for recipe declarations which only name a `spec_id`.
/// Ambiguous matches are refused; the caller must first select a source via
/// `container_discover` and use the returned explicit identity.
pub async fn ensure_unique(db: &Arc<Database>, spec_id: &str) -> Result<EnsuredContainer> {
    let (source, spec) = container_catalog::resolve_unique(db, spec_id).await?;
    ensure_spec(db, &source, &spec).await
}

async fn ensure_spec(
    db: &Arc<Database>,
    source: &ContainerSource,
    spec: &ContainerSpec,
) -> Result<EnsuredContainer> {
    let base_url = if let Some(url) = spec.url.as_deref() {
        let base = url.trim_end_matches('/').to_string();
        // A declaration is both a launch recipe and a durable attachment
        // contract. Probe its declared health URL before spawning so a second
        // ensure can register an already-running compatible service without
        // racing it for the port.
        if !spec.command.is_empty() && !is_healthy(&base, &spec.health).await {
            start_command(source, spec)?;
        }
        base
    } else {
        bail!(
            "container `{}` must declare url so Workshop can probe /health without scanning ports",
            spec.id
        );
    };
    wait_healthy(&base_url, &spec.health).await?;
    let container_id = upsert_ready(db, source, spec, &base_url).await?;
    Ok(EnsuredContainer {
        container_id,
        base_url,
        source_id: source.id.clone(),
        manifest_hash: source.manifest_hash.clone(),
        spec_id: spec.id.clone(),
        locality: spec.locality,
    })
}

fn start_command(source: &ContainerSource, spec: &ContainerSpec) -> Result<()> {
    let cwd = if spec.cwd.exists() {
        spec.cwd.clone()
    } else {
        workspace_recipe::resolve_source_path(&source.root, ".")?
    };
    if !cwd.starts_with(
        source
            .root
            .canonicalize()
            .unwrap_or_else(|_| source.root.to_path_buf()),
    ) {
        bail!(
            "container `{}` cwd {} is not inside source `{}`",
            spec.id,
            cwd.display(),
            source.id,
        );
    }
    let program = spec
        .command
        .first()
        .ok_or_else(|| anyhow!("container `{}` command is empty", spec.id))?;
    let mut command = Command::new(program);
    command
        .args(&spec.command[1..])
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command
        .spawn()
        .with_context(|| format!("start container `{}` command {:?}", spec.id, spec.command))?;
    children()
        .lock()
        .expect("container process table")
        .insert(format!("{}:{}", source.id, spec.id), child);
    Ok(())
}

fn health_url(base_url: &str, health_path: &str) -> String {
    format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        if health_path.starts_with('/') {
            health_path.to_string()
        } else {
            format!("/{health_path}")
        }
    )
}

async fn is_healthy(base_url: &str, health_path: &str) -> bool {
    let Ok(client) = crate::http::http_client_builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return false;
    };
    matches!(
        client.get(health_url(base_url, health_path)).send().await,
        Ok(response) if response.status().is_success()
    )
}

async fn wait_healthy(base_url: &str, health_path: &str) -> Result<()> {
    let url = health_url(base_url, health_path);
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
    source: &ContainerSource,
    spec: &ContainerSpec,
    base_url: &str,
) -> Result<String> {
    let spec_id = spec.id.clone();
    let family = spec.family.clone();
    let contract = spec.contract.clone();
    let locality = spec.locality.as_str().to_string();
    let base_url = base_url.to_string();
    let source_id = source.id.clone();
    let source_path = source.root.to_string_lossy().to_string();
    let manifest_hash = source.manifest_hash.clone();
    let git_revision = source.git_revision.clone();
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
                "sourceId": source_id,
                "sourcePath": source_path,
                "manifestHash": manifest_hash,
                "gitRevision": git_revision,
                "specId": spec_id,
                "contract": contract,
                "locality": locality,
                "source": "container_catalog",
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
