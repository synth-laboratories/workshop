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

#[derive(Clone, Debug)]
pub struct StoppedContainer {
    pub container_id: String,
    pub spec_id: String,
    pub pid: u32,
}

pub fn declared_spec_id(db: &Arc<Database>, container_id: &str) -> Result<String> {
    let id = container_id.to_string();
    db.with_conn(|conn| {
        let metadata: String = conn.query_row(
            "SELECT metadata_json FROM containers WHERE id=?1",
            [&id],
            |row| row.get(0),
        )?;
        let metadata: serde_json::Value = serde_json::from_str(&metadata)?;
        metadata
            .get("workspaceSpecId")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                anyhow!(
                    "launch_declaration_missing: container `{id}` has no workspace spec identity"
                )
            })
    })
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
    let (base_url, process) = if let Some(url) = spec.url.as_deref() {
        let base = url.trim_end_matches('/').to_string();
        let process = if spec.command.is_empty() {
            None
        } else if healthy_now(&base, &spec.health, spec).await? {
            None
        } else {
            Some(start_command(workspace, spec)?)
        };
        (base, process)
    } else {
        bail!(
            "container `{}` must declare url so Workshop can probe /health without scanning ports",
            spec.id
        );
    };
    wait_healthy(&base_url, &spec.health, spec).await?;
    let container_id = upsert_ready(db, spec, &base_url, process).await?;
    Ok(EnsuredContainer {
        container_id,
        base_url,
        spec_id: spec.id.clone(),
        locality: spec.locality,
    })
}

/// Re-run the exact declared launch command even when the endpoint is healthy.
///
/// This is the intentionally permissive recovery path. The workload's launch
/// declaration decides how replacement happens (for example, a container CLI
/// may remove and recreate its named target). Workshop does not discover a PID
/// from the port and does not pretend that endpoint identity grants ownership.
pub async fn replace_declared(
    db: &Arc<Database>,
    workspace: &Path,
    spec_id: &str,
) -> Result<EnsuredContainer> {
    let spec = workspace_recipe::find_container_spec(workspace, spec_id)?;
    let base_url = spec
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .ok_or_else(|| anyhow!("container `{}` must declare url", spec.id))?;
    let process = start_command(workspace, &spec)?;
    wait_healthy(&base_url, &spec.health, &spec).await?;
    let container_id = upsert_ready(db, &spec, &base_url, Some(process)).await?;
    Ok(EnsuredContainer {
        container_id,
        base_url,
        spec_id: spec.id,
        locality: spec.locality,
    })
}

fn start_command(workspace: &Path, spec: &ContainerSpec) -> Result<(u32, String)> {
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
    // Provider credentials never enter the launched process. The declaration
    // names which Workshop proxy routes may be minted later for an approved
    // run; the proxy remains the sole holder of provider secret material.
    for provider in &spec.credential_providers {
        if provider != "openrouter" {
            bail!(
                "container `{}` requests unsupported credential provider `{provider}`",
                spec.id
            );
        }
    }
    command
        .args(&spec.command[1..])
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command
        .spawn()
        .with_context(|| format!("start container `{}` command {:?}", spec.id, spec.command))?;
    let pid = child.id();
    let start = crate::instance::process_start_identity(pid)
        .ok_or_else(|| anyhow!("container `{}` process identity is unavailable", spec.id))?;
    children()
        .lock()
        .expect("container process table")
        .insert(spec.id.clone(), child);
    Ok((pid, start))
}

pub async fn stop(db: &Arc<Database>, container_id: &str) -> Result<StoppedContainer> {
    let id = container_id.to_string();
    let (spec_id, pid, expected_start): (String, u32, String) = db.with_conn(|conn| {
        let metadata: String = conn.query_row(
            "SELECT metadata_json FROM containers WHERE id=?1",
            [&id],
            |row| row.get(0),
        )?;
        let metadata: serde_json::Value = serde_json::from_str(&metadata)?;
        let spec_id = metadata
            .get("workspaceSpecId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("container `{id}` is not a Workshop-owned process"))?
            .to_string();
        let pid = metadata
            .get("supervisedPid")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| anyhow!("container `{id}` has no supervised process receipt"))?;
        let start = metadata
            .get("processStartIdentity")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("container `{id}` has no process start identity"))?
            .to_string();
        Ok((spec_id, pid, start))
    })?;

    let actual_start = crate::instance::process_start_identity(pid);
    if actual_start.as_deref() != Some(expected_start.as_str()) {
        bail!("container `{container_id}` process identity is stale; refusing to signal PID {pid}");
    }
    if let Some(mut child) = children()
        .lock()
        .expect("container process table")
        .remove(&spec_id)
    {
        child.kill().context("stop supervised container")?;
        let _ = child.wait();
    } else {
        let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        if result != 0 {
            return Err(std::io::Error::last_os_error()).context("stop restored container process");
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && crate::instance::process_start_identity(pid).is_some() {
            std::thread::sleep(Duration::from_millis(50));
        }
        if crate::instance::process_start_identity(pid).as_deref() == Some(expected_start.as_str())
        {
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
    let stopped_id = id.clone();
    db.clone()
        .run_transaction(move |conn| {
            conn.execute(
                "UPDATE containers SET status='stopped',health_json='{\"ok\":false}',updated_at=?2 WHERE id=?1",
                params![stopped_id, chrono::Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
        .await?;
    Ok(StoppedContainer {
        container_id: id,
        spec_id,
        pid,
    })
}

async fn wait_healthy(base_url: &str, health_path: &str, spec: &ContainerSpec) -> Result<()> {
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
    let timeout = Duration::from_secs(spec.launch.readiness_timeout_seconds);
    let deadline = Instant::now() + timeout;
    let mut last = String::from("not probed");
    while Instant::now() < deadline {
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                let payload = response
                    .json::<serde_json::Value>()
                    .await
                    .context("health_contract_invalid: response is not JSON")?;
                validate_health_identity(spec, &payload)?;
                return Ok(());
            }
            Ok(response) => last = format!("HTTP {}", response.status()),
            Err(error) => last = error.to_string(),
        }
        tokio::time::sleep(HEALTH_POLL).await;
    }
    bail!("readiness_timeout: container at {base_url} never became healthy on {url}: {last}")
}

async fn healthy_now(base_url: &str, health_path: &str, spec: &ContainerSpec) -> Result<bool> {
    let path = if health_path.starts_with('/') {
        health_path.to_string()
    } else {
        format!("/{health_path}")
    };
    let Ok(client) = crate::http::http_client_builder()
        .timeout(Duration::from_millis(500))
        .build()
    else {
        return Ok(false);
    };
    let response = match client
        .get(format!("{}{path}", base_url.trim_end_matches('/')))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response,
        _ => return Ok(false),
    };
    let payload = response
        .json::<serde_json::Value>()
        .await
        .context("health_contract_invalid: response is not JSON")?;
    validate_health_identity(spec, &payload)?;
    Ok(true)
}

fn validate_health_identity(spec: &ContainerSpec, payload: &serde_json::Value) -> Result<()> {
    let launch = &spec.launch;
    let status = payload.get("status").and_then(serde_json::Value::as_str);
    anyhow::ensure!(
        status == Some("ok"),
        "health_contract_invalid: expected status=ok, got {status:?}"
    );
    let target = payload.get("target").and_then(serde_json::Value::as_str);
    anyhow::ensure!(
        target == Some(launch.health_target.as_str()),
        "container_identity_mismatch: expected health target {}, got {target:?}",
        launch.health_target
    );
    Ok(())
}

async fn upsert_ready(
    db: &Arc<Database>,
    spec: &ContainerSpec,
    base_url: &str,
    process: Option<(u32, String)>,
) -> Result<String> {
    let spec_id = spec.id.clone();
    let family = spec.family.clone();
    let contract = spec.contract.clone();
    let locality = spec.locality.as_str().to_string();
    let source_path = spec.cwd.display().to_string();
    let policy_source_path = spec.policy_source.clone();
    let source_revision = spec.source_revision.clone();
    let manifest_digest = spec.manifest_digest.clone();
    let base_url = base_url.to_string();
    let (supervised_pid, process_start_identity) = process
        .map(|(pid, start)| (Some(pid), Some(start)))
        .unwrap_or((None, None));
    db.clone()
        .run_transaction(move |conn| {
            let existing: Option<(String, String)> = conn
                .query_row(
                    "SELECT id,metadata_json FROM containers WHERE base_url = ?1 LIMIT 1",
                    params![&base_url],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let prior_metadata = existing
                .as_ref()
                .and_then(|(_, raw)| serde_json::from_str::<serde_json::Value>(raw).ok());
            let id = existing
                .map(|(id, _)| id)
                .unwrap_or_else(|| format!("ctr_{}", Uuid::new_v4().simple()));
            let retained_pid = supervised_pid.or_else(|| {
                prior_metadata
                    .as_ref()
                    .and_then(|value| value.get("supervisedPid"))
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
            });
            let retained_start = process_start_identity.or_else(|| {
                prior_metadata
                    .as_ref()
                    .and_then(|value| value.get("processStartIdentity"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            });
            let now = chrono::Utc::now().to_rfc3339();
            let metadata = serde_json::to_string(&json!({
                "workspaceSpecId": spec_id,
                "contract": contract,
                "locality": locality,
                "source": "workspace",
                "sourcePath": source_path,
                "policySourcePath": policy_source_path,
                "gitRevision": source_revision,
                "manifestHash": manifest_digest,
                "capabilities": {
                    "protocol": contract,
                    "revision": source_revision,
                },
                "supervisedPid": retained_pid,
                "processStartIdentity": retained_start,
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
