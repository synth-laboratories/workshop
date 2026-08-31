//! Start a workspace-declared container, wait until it is healthy, register it.
//!
//! Registration is the durable handle. This module only brings a process up
//! (or attaches an already-running URL) so agents do not scan ports or execute
//! out of a cookbook pin.

use super::workspace_recipe::{self, ContainerSpec, PolicyLocality};
use crate::storage::Database;
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use uuid::Uuid;

const HEALTH_POLL: Duration = Duration::from_millis(200);

fn children() -> &'static Mutex<HashMap<u32, Child>> {
    static CHILDREN: OnceLock<Mutex<HashMap<u32, Child>>> = OnceLock::new();
    CHILDREN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A launch remains armed until readiness has been established against the
/// declaration that started it. Dropping an armed launch (including when an
/// async ensure future is cancelled) terminates the whole launch process
/// group, so a slow builder cannot replace an already-validated endpoint
/// later.
struct LaunchedCommand {
    pid: u32,
    start_identity: String,
    shutdown_grace: Duration,
    armed: bool,
}

impl LaunchedCommand {
    fn receipt(&self) -> (u32, String) {
        (self.pid, self.start_identity.clone())
    }

    fn commit(mut self) -> (u32, String) {
        self.armed = false;
        (self.pid, self.start_identity.clone())
    }
}

impl Drop for LaunchedCommand {
    fn drop(&mut self) {
        if self.armed {
            terminate_process_group(self.pid, self.shutdown_grace);
        }
    }
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

/// Canonical identity of every field that can change what a validated
/// declaration launches or how Workshop decides that launch is ready.
///
/// The repository dirty digest covers the declaration's source inputs, not
/// necessarily the manifest itself. Binding approval to this projection keeps
/// a click from authorizing a command, endpoint, environment, or health target
/// that appeared while the approval card was open.
pub(crate) fn approval_declaration_digest(spec: &ContainerSpec) -> Result<String> {
    let projection = json!({
        "id": spec.id,
        "command": spec.command,
        "cwd": spec.cwd.display().to_string(),
        "url": spec.url,
        "health": spec.health,
        "contract": spec.contract,
        "locality": spec.locality.as_str(),
        "family": spec.family,
        "credentialProviders": spec.credential_providers,
        "environment": spec.environment,
        "policySource": spec.policy_source,
        "sourceRevision": spec.source_revision,
        "manifestDigest": spec.manifest_digest,
        "origin": spec.origin.to_json(),
        "launch": {
            "workingDirectory": spec.launch.working_directory.display().to_string(),
            "command": spec.launch.command,
            "readinessTimeoutSeconds": spec.launch.readiness_timeout_seconds,
            "shutdownGraceSeconds": spec.launch.shutdown_grace_seconds,
            "expectedPort": spec.launch.expected_port,
            "imageRef": spec.launch.image_ref,
            "healthTarget": spec.launch.health_target,
            "declaredEnvironment": spec.launch.declared_environment,
            "environment": spec.launch.environment,
            "trackedRevision": spec.launch.tracked_revision,
            "dirtyDigest": spec.launch.dirty_digest,
            "include": spec.launch.include,
        },
    });
    let encoded = serde_json::to_vec(&projection).context("encode launch approval identity")?;
    let mut hasher = Sha256::new();
    hasher.update(encoded);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub(crate) fn require_approved_declaration(
    validated: &ContainerSpec,
    current: &ContainerSpec,
    approved_digest: &str,
) -> Result<()> {
    let current_digest = approval_declaration_digest(current)?;
    if current.origin != validated.origin
        || current.id != validated.id
        || current_digest != approved_digest
    {
        bail!(
            "launch_declaration_changed: the declaration changed after approval; request replacement again"
        );
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct ContainerReplacementOutcome {
    pub approval_id: String,
    pub declaration: ContainerSpec,
    pub stopped: Option<StoppedContainer>,
    pub ensured: EnsuredContainer,
}

/// The approval-gated destructive continuation. It owns the authorization
/// result and consumes it before touching reconciliation, process state, or
/// the registry. A rejection therefore cannot reach mutation, while a granted
/// approval cannot be replayed even by accidentally calling the continuation
/// twice in one request handler.
pub(crate) struct ContainerReplacementContinuation {
    authorization: Option<Result<String>>,
    declaration_digest: String,
}

impl ContainerReplacementContinuation {
    pub(crate) fn new(authorization: Result<String>, declaration_digest: String) -> Self {
        Self {
            authorization: Some(authorization),
            declaration_digest,
        }
    }

    pub(crate) async fn consume(
        &mut self,
        db: &Arc<Database>,
        session_id: &str,
        container_id: &str,
        validated: &ContainerSpec,
    ) -> Result<ContainerReplacementOutcome> {
        let authorization = self.authorization.take().ok_or_else(|| {
            anyhow!("container_lifecycle_approval_consumed: approval already consumed")
        })?;
        let approval_id = authorization?;
        let declaration = resolve_declared_spec(db, session_id, container_id)?;
        require_approved_declaration(validated, &declaration, &self.declaration_digest)?;
        let stopped = stop(db, container_id).await.ok();
        let ensured = replace_declared(db, &declaration).await?;
        Ok(ContainerReplacementOutcome {
            approval_id,
            declaration,
            stopped,
            ensured,
        })
    }
}

pub fn declared_spec_id(db: &Arc<Database>, container_id: &str) -> Result<String> {
    Ok(declared_record(db, container_id)?.0)
}

pub fn declared_record(
    db: &Arc<Database>,
    container_id: &str,
) -> Result<(String, serde_json::Value)> {
    let id = container_id.to_string();
    db.with_conn(|conn| {
        let metadata: String = conn.query_row(
            "SELECT metadata_json FROM containers WHERE id=?1",
            [&id],
            |row| row.get(0),
        )?;
        let metadata: serde_json::Value = serde_json::from_str(&metadata)?;
        let spec_id = metadata
            .get("workspaceSpecId")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                anyhow!(
                    "launch_declaration_missing: container `{id}` has no workspace spec identity"
                )
            })?;
        Ok((spec_id, metadata))
    })
}

pub fn resolve_declared_spec(
    db: &Arc<Database>,
    _session_id: &str,
    container_id: &str,
) -> Result<workspace_recipe::ContainerSpec> {
    let (spec_id, metadata) = declared_record(db, container_id)?;
    let stored = workspace_recipe::origin_from_metadata(&metadata, &spec_id).ok_or_else(|| {
        anyhow!(
            "launch_declaration_missing: container `{container_id}` has no persisted declaration origin"
        )
    })?;
    workspace_recipe::load_container_specs_from_manifest(&stored.manifest_path)?
        .into_iter()
        .find(|candidate| candidate.id == spec_id)
        .ok_or_else(|| {
            anyhow!(
                "container spec `{spec_id}` is not declared in persisted manifest {}",
                stored.manifest_path.display()
            )
        })
}

pub fn resolve_spec_for_session(
    db: &Arc<Database>,
    session_id: &str,
    spec_id: &str,
) -> Result<workspace_recipe::ContainerSpec> {
    let search_roots = workspace_recipe::session_search_roots(db, session_id)?;
    workspace_recipe::resolve_container_spec(&search_roots, spec_id, None)
}

/// Re-read and validate the declaring manifest, then merge provenance into the
/// registry without requiring a healthy runtime.
pub fn reconcile_declaration(
    db: &Arc<Database>,
    session_id: &str,
    container_id: &str,
) -> Result<workspace_recipe::ContainerSpec> {
    match resolve_declared_spec(db, session_id, container_id) {
        Ok(spec) => {
            merge_declaration_metadata(db, container_id, &spec)?;
            Ok(spec)
        }
        Err(error) => {
            let _ = stamp_invalid_declaration(db, container_id, &error);
            Err(error)
        }
    }
}

pub async fn ensure(
    db: &Arc<Database>,
    workspace: &Path,
    spec_id: &str,
) -> Result<EnsuredContainer> {
    let spec = workspace_recipe::find_container_spec(workspace, spec_id)?;
    ensure_spec(db, &spec).await
}

pub async fn ensure_from_session(
    db: &Arc<Database>,
    session_id: &str,
    spec_id: &str,
) -> Result<EnsuredContainer> {
    let spec = resolve_spec_for_session(db, session_id, spec_id)?;
    ensure_spec(db, &spec).await
}

pub async fn ensure_spec(db: &Arc<Database>, spec: &ContainerSpec) -> Result<EnsuredContainer> {
    let (base_url, launch) = if let Some(url) = spec.url.as_deref() {
        let base = url.trim_end_matches('/').to_string();
        let launch = if spec.command.is_empty() {
            None
        } else if healthy_now(&base, &spec.health, spec).await? {
            None
        } else {
            Some(start_command(spec)?)
        };
        (base, launch)
    } else {
        bail!(
            "container `{}` must declare url so Workshop can probe /health without scanning ports",
            spec.id
        );
    };
    wait_healthy(&base_url, &spec.health, spec).await?;
    let process = launch.as_ref().map(LaunchedCommand::receipt);
    let container_id = upsert_ready(db, spec, &base_url, process).await?;
    if let Some(launch) = launch {
        launch.commit();
    }
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
    spec: &ContainerSpec,
) -> Result<EnsuredContainer> {
    let base_url = spec
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .ok_or_else(|| anyhow!("container `{}` must declare url", spec.id))?;
    let launch = start_command(spec)?;
    wait_healthy(&base_url, &spec.health, spec).await?;
    let process = launch.receipt();
    let container_id = upsert_ready(db, spec, &base_url, Some(process)).await?;
    launch.commit();
    Ok(EnsuredContainer {
        container_id,
        base_url,
        spec_id: spec.id.clone(),
        locality: spec.locality,
    })
}

fn start_command(spec: &ContainerSpec) -> Result<LaunchedCommand> {
    let source_root = spec
        .origin
        .source_root
        .canonicalize()
        .unwrap_or_else(|_| spec.origin.source_root.clone());
    let cwd = if spec.cwd.exists() {
        spec.cwd.clone()
    } else {
        workspace_recipe::resolve_repository_path(&spec.origin, ".")
            .map(|path| path.absolute_path)
            .map_err(workspace_recipe::LaunchDeclarationError::into_anyhow)?
    };
    if !cwd.starts_with(&source_root) {
        bail!(
            "container `{}` cwd {} is not inside the declaring repository {}",
            spec.id,
            cwd.display(),
            source_root.display()
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
        .process_group(0)
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
        .insert(pid, child);
    Ok(LaunchedCommand {
        pid,
        start_identity: start,
        shutdown_grace: Duration::from_secs(spec.launch.shutdown_grace_seconds),
        armed: true,
    })
}

fn terminate_process_group(pid: u32, shutdown_grace: Duration) {
    let Some(mut child) = children()
        .lock()
        .expect("container process table")
        .remove(&pid)
    else {
        return;
    };
    // Every declared launch receives its own process group. Signalling the
    // group is essential for shell launchers: killing only the shell leaves a
    // docker build or delayed replacement child free to mutate the endpoint.
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGTERM);
    }
    let deadline = Instant::now() + shutdown_grace;
    let mut leader_reaped = false;
    loop {
        if !leader_reaped {
            leader_reaped = matches!(child.try_wait(), Ok(Some(_)));
        }
        let group_exists = unsafe { libc::kill(-(pid as libc::pid_t), 0) == 0 }
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        if !group_exists {
            if !leader_reaped {
                let _ = child.wait();
            }
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
    }
    if !leader_reaped {
        let _ = child.wait();
    }
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
    if children()
        .lock()
        .expect("container process table")
        .contains_key(&pid)
    {
        terminate_process_group(pid, Duration::from_secs(5));
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
                match validate_declared_runtime_identity(&client, base_url, spec, &payload).await {
                    Ok(()) => return Ok(()),
                    Err(error) => last = error.to_string(),
                }
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
    Ok(validate_declared_runtime_identity(&client, base_url, spec, &payload)
        .await
        .is_ok())
}

/// Bind readiness to the immutable runtime pins carried by the launch
/// declaration. A matching health target alone is not enough during
/// replacement: an old process at the same URL can recover while a slow
/// launcher is still building its successor.
async fn validate_declared_runtime_identity(
    client: &reqwest::Client,
    base_url: &str,
    spec: &ContainerSpec,
    health: &Value,
) -> Result<()> {
    let expected_image = spec.environment.get("SYNTH_CONTAINER_IMAGE_DIGEST");
    let expected_producer = spec
        .environment
        .get("SYNTH_CONTAINER_PRODUCER_SOURCE_REVISION");
    if expected_image.is_none() && expected_producer.is_none() {
        return Ok(());
    }
    let mut evidence = vec![health.clone()];
    for path in ["/info"] {
        let url = format!("{}{path}", base_url.trim_end_matches('/'));
        let response = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("container_identity_pending: query {url}"))?;
        if response.status().is_success() {
            evidence.push(
                response
                    .json::<Value>()
                    .await
                    .with_context(|| format!("container_identity_pending: {path} response is not JSON"))?,
            );
        }
    }
    anyhow::ensure!(
        !evidence.is_empty(),
        "container_identity_pending: neither /info nor /health returned identity evidence"
    );
    let identity_field = |camel: &str, snake: &str| {
        evidence.iter().find_map(|payload| {
            payload
                .get(camel)
                .and_then(Value::as_str)
                .or_else(|| {
                    payload
                        .get("runtime_identity")
                        .and_then(|identity| identity.get(snake))
                        .and_then(Value::as_str)
                })
        })
    };
    if let Some(expected) = expected_image {
        let actual = identity_field("imageDigest", "image_digest");
        anyhow::ensure!(
            actual == Some(expected.as_str()),
            "container_identity_pending: expected imageDigest {expected}, got {actual:?}"
        );
    }
    if let Some(expected) = expected_producer {
        let actual = identity_field("producerSourceRevision", "producer_source_revision");
        anyhow::ensure!(
            actual == Some(expected.as_str()),
            "container_identity_pending: expected producerSourceRevision {expected}, got {actual:?}"
        );
    }
    Ok(())
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
    let source_path = spec.origin.source_root.display().to_string();
    let origin = spec.origin.to_json();
    let launch_declaration = json!({
        "valid": true,
        "command": spec.command,
        "workingDirectory": spec.cwd.display().to_string(),
        "sourceRoot": spec.origin.source_root.display().to_string(),
        "manifestPath": spec.origin.manifest_path.display().to_string(),
        "sourceRevision": spec.origin.source_revision,
        "sourceDigest": spec.origin.source_digest,
    });
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
                "declarationOrigin": origin,
                "launchDeclaration": launch_declaration,
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

fn merge_declaration_metadata(
    db: &Arc<Database>,
    container_id: &str,
    spec: &ContainerSpec,
) -> Result<()> {
    let id = container_id.to_string();
    let origin = spec.origin.to_json();
    let source_path = spec.origin.source_root.display().to_string();
    let launch_declaration = json!({
        "valid": true,
        "command": spec.command,
        "workingDirectory": spec.cwd.display().to_string(),
        "sourceRoot": spec.origin.source_root.display().to_string(),
        "manifestPath": spec.origin.manifest_path.display().to_string(),
        "sourceRevision": spec.origin.source_revision,
        "sourceDigest": spec.origin.source_digest,
    });
    let spec_id = spec.id.clone();
    let source_revision = spec.source_revision.clone();
    let manifest_digest = spec.manifest_digest.clone();
    let policy_source_path = spec.policy_source.clone();
    db.with_conn(move |conn| {
        let raw: String = conn.query_row(
            "SELECT metadata_json FROM containers WHERE id=?1",
            [&id],
            |row| row.get(0),
        )?;
        let mut metadata: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
        apply_declaration_to_metadata(
            &mut metadata,
            &spec_id,
            &source_path,
            origin,
            launch_declaration,
            source_revision,
            manifest_digest,
            policy_source_path,
        )?;
        conn.execute(
            "UPDATE containers SET metadata_json=?2, updated_at=?3 WHERE id=?1",
            params![
                id,
                serde_json::to_string(&metadata)?,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    })
}

fn apply_declaration_to_metadata(
    metadata: &mut Value,
    spec_id: &str,
    source_path: &str,
    origin: Value,
    launch_declaration: Value,
    source_revision: Option<String>,
    manifest_digest: Option<String>,
    policy_source_path: Option<String>,
) -> Result<()> {
    let object = metadata
        .as_object_mut()
        .ok_or_else(|| anyhow!("container metadata is not an object"))?;
    object.insert("workspaceSpecId".into(), json!(spec_id));
    object.insert("sourcePath".into(), json!(source_path));
    object.insert("declarationOrigin".into(), origin);
    object.insert("launchDeclaration".into(), launch_declaration);
    object.insert("gitRevision".into(), json!(source_revision));
    object.insert("manifestHash".into(), json!(manifest_digest));
    object.insert("policySourcePath".into(), json!(policy_source_path));
    Ok(())
}

fn stamp_invalid_declaration(
    db: &Arc<Database>,
    container_id: &str,
    error: &anyhow::Error,
) -> Result<()> {
    let failure = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<crate::error::StructuredFailure>())
        .cloned();
    let Some(failure) = failure else {
        return Ok(());
    };
    let id = container_id.to_string();
    db.with_conn(move |conn| {
        let raw: String = conn.query_row(
            "SELECT metadata_json FROM containers WHERE id=?1",
            [&id],
            |row| row.get(0),
        )?;
        let mut metadata: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
        apply_invalid_launch_to_metadata(&mut metadata, &failure)?;
        conn.execute(
            "UPDATE containers SET metadata_json=?2, updated_at=?3 WHERE id=?1",
            params![
                id,
                serde_json::to_string(&metadata)?,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    })
}

pub(crate) fn apply_invalid_launch_to_metadata(
    metadata: &mut Value,
    failure: &crate::error::StructuredFailure,
) -> Result<()> {
    let object = metadata
        .as_object_mut()
        .ok_or_else(|| anyhow!("container metadata is not an object"))?;
    object.insert(
        "launchDeclaration".into(),
        json!({
            "valid": false,
            "error": failure.to_json(),
        }),
    );
    Ok(())
}

pub fn stamp_metadata_freshness(
    metadata: &mut serde_json::Map<String, serde_json::Value>,
    live: bool,
    observed_at: &str,
) {
    let revision = metadata
        .get("gitRevision")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let previous = metadata
        .get("hydratedAt")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let freshness = |kind: &str, at: &str, reason: Option<&str>| {
        let mut value = json!({ "kind": kind, "observedAt": at });
        if let Some(revision) = &revision {
            value["sourceRevision"] = json!(revision);
        }
        if let Some(reason) = reason {
            value["reason"] = json!(reason);
        }
        value
    };
    if live {
        metadata.insert(
            "runtimeFreshness".into(),
            freshness("live", observed_at, None),
        );
        metadata.insert(
            "taskCatalogFreshness".into(),
            if metadata.contains_key("taskCatalog") {
                freshness("live", observed_at, None)
            } else {
                freshness("unavailable", observed_at, Some("not_reported"))
            },
        );
        metadata.insert(
            "interfaceFreshness".into(),
            if metadata
                .get("info")
                .and_then(|value| value.get("capabilities"))
                .is_some()
            {
                freshness("live", observed_at, None)
            } else {
                freshness("unavailable", observed_at, Some("not_reported"))
            },
        );
        metadata.insert(
            "policyFreshness".into(),
            if metadata.contains_key("policyState") {
                freshness("live", observed_at, None)
            } else {
                freshness("unavailable", observed_at, Some("not_reported"))
            },
        );
        return;
    }
    let cached_at = previous.as_deref().unwrap_or(observed_at);
    metadata.insert(
        "runtimeFreshness".into(),
        freshness("unavailable", observed_at, Some("unhealthy")),
    );
    metadata.insert(
        "taskCatalogFreshness".into(),
        if metadata.contains_key("taskCatalog") {
            freshness("cached", cached_at, None)
        } else {
            freshness("unavailable", observed_at, Some("runtime_offline"))
        },
    );
    metadata.insert(
        "interfaceFreshness".into(),
        if metadata
            .get("info")
            .and_then(|value| value.get("capabilities"))
            .is_some()
        {
            freshness("cached", cached_at, None)
        } else {
            freshness("unavailable", observed_at, Some("runtime_offline"))
        },
    );
    metadata.insert(
        "policyFreshness".into(),
        freshness("unavailable", observed_at, Some("runtime_offline")),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        fs,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };
    use tempfile::tempdir;

    fn write_manifest(root: &Path, command: &str, port: u16, target: &str) {
        fs::write(
            root.join("workshop.containers.toml"),
            format!(
                r#"
[[container]]
id = "fixture-container"
url = "http://127.0.0.1:{port}"
locality = "container"
[container.launch]
schema_version = "synth.container-launch.v1"
working_directory = "."
command = ["./{command}", "validated", "launch-marker"]
readiness_timeout_seconds = 2
shutdown_grace_seconds = 1
expected_port = {port}
image_ref = "fixture-image"
health_target = "{target}"
[container.launch.source]
revision_policy = "exact-or-dirty-digest"
tracked_revision = "fixture-revision"
include = ["launch-a.sh", "launch-b.sh"]
"#
            ),
        )
        .unwrap();
    }

    fn write_launcher(path: &Path) {
        fs::write(
            path,
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$2\"\nexec sleep 30\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn write_delayed_launcher(path: &Path, delay_seconds: u64, linger: bool) {
        fs::write(
            path,
            format!(
                "#!/bin/sh\nsleep {delay_seconds}\nprintf '%s\\n' \"$1\" > \"$2\"\n{}\n",
                if linger { "exec sleep 30" } else { "" }
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn write_detached_delayed_launcher(path: &Path) {
        fs::write(
            path,
            "#!/bin/sh\n(trap '' TERM; sleep 2; printf '%s\\n' \"$1\" > \"$2\") &\nexit 0\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn register_declared(db: &Arc<Database>, root: &Path, spec: &ContainerSpec, status: &str) {
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO sessions(id,title,target_json,status,metadata_json,created_at,updated_at) VALUES('session','Session','{}','ready','{}','now','now')",
                [],
            )?;
            conn.execute(
                "INSERT INTO conversation_workspace_scopes(session_id,workspace,created_at,updated_at) VALUES('session',?1,'now','now')",
                [root.display().to_string()],
            )?;
            conn.execute(
                "INSERT INTO containers(id,name,location,status,base_url,health_json,metadata_json,created_at,updated_at) VALUES('ctr_fixture','fixture','local',?1,?2,'{\"ok\":false}',?3,'now','now')",
                params![status, spec.url, json!({
                    "workspaceSpecId": spec.id,
                    "declarationOrigin": spec.origin.to_json(),
                }).to_string()],
            )?;
            Ok(())
        })
        .unwrap();
    }

    fn serve_health(
        listener: TcpListener,
        target: String,
        live: Arc<AtomicBool>,
    ) -> thread::JoinHandle<()> {
        listener.set_nonblocking(true).unwrap();
        thread::spawn(move || {
            while live.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut request = [0_u8; 1024];
                        let _ = stream.read(&mut request);
                        let body = format!(r#"{{"status":"ok","target":"{target}"}}"#);
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(), body
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        })
    }

    fn serve_pinned_health(
        listener: TcpListener,
        target: String,
        replacement_marker: std::path::PathBuf,
        live: Arc<AtomicBool>,
    ) -> thread::JoinHandle<()> {
        listener.set_nonblocking(true).unwrap();
        let first_health = Arc::new(AtomicBool::new(true));
        thread::spawn(move || {
            while live.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut request = [0_u8; 1024];
                        let count = stream.read(&mut request).unwrap_or(0);
                        let request = String::from_utf8_lossy(&request[..count]);
                        let is_info = request.starts_with("GET /info ");
                        if !is_info && first_health.swap(false, Ordering::AcqRel) {
                            // Force healthy_now's single 500ms request to miss,
                            // while leaving the old endpoint available to the
                            // readiness loop immediately afterward.
                            thread::sleep(Duration::from_millis(650));
                        }
                        let replaced = replacement_marker.exists();
                        let body = if is_info {
                            json!({
                                "imageDigest": if replaced { "sha256:new" } else { "sha256:old" },
                                "producerSourceRevision": if replaced { "producer@new" } else { "producer@old" },
                            })
                            .to_string()
                        } else {
                            json!({"status": "ok", "target": target}).to_string()
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(), body
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        })
    }

    fn probe_health(port: u16) -> bool {
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
            return false;
        };
        if stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .is_err()
        {
            return false;
        }
        let mut response = String::new();
        stream.read_to_string(&mut response).is_ok() && response.contains("200 OK")
    }

    #[test]
    fn unhealthy_runtime_labels_cached_catalog_and_keeps_policy_unavailable() {
        let mut metadata = serde_json::Map::from_iter([
            (
                "taskCatalog".into(),
                json!({"tasks": [{"id": "one"}, {"id": "two"}]}),
            ),
            (
                "info".into(),
                json!({"capabilities": {"rollouts.prepare": true}}),
            ),
            ("hydratedAt".into(), json!("2026-08-26T00:00:00Z")),
        ]);
        stamp_metadata_freshness(&mut metadata, false, "2026-08-27T12:00:00Z");
        assert_eq!(metadata["runtimeFreshness"]["kind"], "unavailable");
        assert_eq!(metadata["taskCatalogFreshness"]["kind"], "cached");
        assert_eq!(
            metadata["taskCatalogFreshness"]["observedAt"],
            "2026-08-26T00:00:00Z"
        );
        assert_eq!(metadata["interfaceFreshness"]["kind"], "cached");
        assert_eq!(metadata["policyFreshness"]["kind"], "unavailable");
        assert_eq!(metadata["policyFreshness"]["reason"], "runtime_offline");
    }

    #[test]
    fn invalid_launch_is_stamped_without_requiring_health() {
        let mut metadata = json!({"workspaceSpecId": "nanohorizon-craftax"});
        let failure = crate::error::StructuredFailure::new(
            "launch_source_path_not_found",
            "Couldn't find `scripts/up_craftax_container.sh` in the declaring repository.",
            "Resolve the include against the declaring repository.",
        )
        .with_details(json!({
            "declared_path": "scripts/up_craftax_container.sh",
            "resolved_path": "/chat/scripts/up_craftax_container.sh"
        }));
        apply_invalid_launch_to_metadata(&mut metadata, &failure).unwrap();
        assert_eq!(metadata["launchDeclaration"]["valid"], false);
        assert_eq!(
            metadata["launchDeclaration"]["error"]["code"],
            "launch_source_path_not_found"
        );
        assert_eq!(
            metadata["launchDeclaration"]["error"]["declared_path"],
            "scripts/up_craftax_container.sh"
        );
    }

    #[test]
    fn reconciliation_refuses_a_manifest_changed_after_approval() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("source");
        fs::create_dir_all(&root).unwrap();
        write_launcher(&root.join("launch-a.sh"));
        write_launcher(&root.join("launch-b.sh"));
        write_manifest(&root, "launch-a.sh", 31001, "fixture-target");
        let initial = workspace_recipe::find_container_spec(&root, "fixture-container").unwrap();
        let approved_digest = approval_declaration_digest(&initial).unwrap();
        let db = Arc::new(Database::open(dir.path().join("state.sqlite3")).unwrap());
        register_declared(&db, &root, &initial, "unhealthy");

        let validated = reconcile_declaration(&db, "session", "ctr_fixture").unwrap();
        assert_eq!(
            approval_declaration_digest(&validated).unwrap(),
            approved_digest
        );
        write_manifest(&root, "launch-b.sh", 31001, "fixture-target");
        let current = reconcile_declaration(&db, "session", "ctr_fixture").unwrap();
        let error = require_approved_declaration(&validated, &current, &approved_digest)
            .unwrap_err()
            .to_string();
        assert!(error.contains("launch_declaration_changed"), "{error}");
        assert_ne!(
            approval_declaration_digest(&current).unwrap(),
            approved_digest,
            "the complete validated declaration, not only its optional source digest, is approval-bound"
        );
    }

    #[tokio::test]
    async fn rejected_continuation_changes_no_registry_process_or_launch_state() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("source");
        fs::create_dir_all(&root).unwrap();
        write_launcher(&root.join("launch-a.sh"));
        write_launcher(&root.join("launch-b.sh"));
        write_manifest(&root, "launch-a.sh", 31002, "fixture-target");
        let spec = workspace_recipe::find_container_spec(&root, "fixture-container").unwrap();
        let digest = approval_declaration_digest(&spec).unwrap();
        let db = Arc::new(Database::open(dir.path().join("state.sqlite3")).unwrap());
        register_declared(&db, &root, &spec, "unhealthy");
        let before: (String, String) = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT status,metadata_json FROM containers WHERE id='ctr_fixture'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?)
            })
            .unwrap();
        let mut unrelated = Command::new("sleep").arg("30").spawn().unwrap();
        let unrelated_pid = unrelated.id();
        let unrelated_start = crate::instance::process_start_identity(unrelated_pid).unwrap();
        let mut continuation =
            ContainerReplacementContinuation::new(Err(anyhow!("approval rejected")), digest);

        let error = continuation
            .consume(&db, "session", "ctr_fixture", &spec)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("approval rejected"), "{error}");
        let after: (String, String) = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT status,metadata_json FROM containers WHERE id='ctr_fixture'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(
            after, before,
            "reject must not reconcile or mutate the registry"
        );
        assert!(
            !root.join("launch-marker").exists(),
            "reject must not launch"
        );
        assert_eq!(
            crate::instance::process_start_identity(unrelated_pid).as_deref(),
            Some(unrelated_start.as_str()),
            "reject must not change process state"
        );
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().unwrap();
        let _ = unrelated.wait();
    }

    #[tokio::test]
    async fn declared_replacement_never_kills_the_unrelated_listener_on_its_expected_port() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("source");
        fs::create_dir_all(&root).unwrap();
        write_launcher(&root.join("launch-a.sh"));
        write_launcher(&root.join("launch-b.sh"));
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let live = Arc::new(AtomicBool::new(true));
        let server = serve_health(listener, "fixture-target".into(), live.clone());
        write_manifest(&root, "launch-a.sh", port, "fixture-target");
        let spec = workspace_recipe::find_container_spec(&root, "fixture-container").unwrap();
        let digest = approval_declaration_digest(&spec).unwrap();
        let db = Arc::new(Database::open(dir.path().join("state.sqlite3")).unwrap());
        register_declared(&db, &root, &spec, "unhealthy");

        let mut continuation =
            ContainerReplacementContinuation::new(Ok("approval-once".into()), digest);
        let outcome = continuation
            .consume(&db, "session", "ctr_fixture", &spec)
            .await
            .unwrap();
        let marker = root.join("launch-marker");
        let deadline = Instant::now() + Duration::from_secs(1);
        while !marker.exists() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(fs::read_to_string(&marker).unwrap(), "validated\n");
        assert!(
            probe_health(port),
            "the pre-existing listener must remain alive"
        );

        let replay = continuation
            .consume(&db, "session", "ctr_fixture", &spec)
            .await
            .unwrap_err()
            .to_string();
        assert!(replay.contains("approval already consumed"), "{replay}");
        assert_eq!(
            fs::read_to_string(&marker).unwrap(),
            "validated\n",
            "approve once must invoke the declaration exactly once"
        );

        stop(&db, &outcome.ensured.container_id).await.unwrap();
        assert!(
            probe_health(port),
            "stopping the supervised launcher must not signal a process selected by port"
        );
        live.store(false, Ordering::Release);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn slow_replacement_is_not_masked_by_the_old_healthy_endpoint() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("source");
        fs::create_dir_all(&root).unwrap();
        write_delayed_launcher(&root.join("launch-a.sh"), 1, true);
        write_launcher(&root.join("launch-b.sh"));
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let marker = root.join("launch-marker");
        let live = Arc::new(AtomicBool::new(true));
        let server = serve_pinned_health(
            listener,
            "fixture-target".into(),
            marker.clone(),
            live.clone(),
        );
        write_manifest(&root, "launch-a.sh", port, "fixture-target");
        let mut spec = workspace_recipe::find_container_spec(&root, "fixture-container").unwrap();
        spec.environment
            .insert("SYNTH_CONTAINER_IMAGE_DIGEST".into(), "sha256:new".into());
        spec.environment.insert(
            "SYNTH_CONTAINER_PRODUCER_SOURCE_REVISION".into(),
            "producer@new".into(),
        );
        let db = Arc::new(Database::open(dir.path().join("state.sqlite3")).unwrap());

        let ensured = ensure_spec(&db, &spec).await.unwrap();
        assert!(
            marker.exists(),
            "old health must not let ensure return before the declared replacement is live"
        );

        stop(&db, &ensured.container_id).await.unwrap();
        live.store(false, Ordering::Release);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn readiness_timeout_terminates_the_launcher_process_group() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("source");
        fs::create_dir_all(&root).unwrap();
        write_delayed_launcher(&root.join("launch-a.sh"), 2, false);
        write_launcher(&root.join("launch-b.sh"));
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        write_manifest(&root, "launch-a.sh", port, "fixture-target");
        let mut spec = workspace_recipe::find_container_spec(&root, "fixture-container").unwrap();
        spec.launch.readiness_timeout_seconds = 1;
        spec.launch.shutdown_grace_seconds = 1;
        let db = Arc::new(Database::open(dir.path().join("state.sqlite3")).unwrap());

        let error = ensure_spec(&db, &spec).await.unwrap_err().to_string();
        assert!(error.contains("readiness_timeout"), "{error}");
        tokio::time::sleep(Duration::from_millis(1_250)).await;
        assert!(
            !root.join("launch-marker").exists(),
            "a timed-out launcher must not remain able to mutate the runtime"
        );
    }

    #[tokio::test]
    async fn cancelled_readiness_wait_terminates_the_launcher_process_group() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("source");
        fs::create_dir_all(&root).unwrap();
        write_detached_delayed_launcher(&root.join("launch-a.sh"));
        write_launcher(&root.join("launch-b.sh"));
        write_manifest(&root, "launch-a.sh", 31999, "fixture-target");
        let spec = workspace_recipe::find_container_spec(&root, "fixture-container").unwrap();

        let cancelled = tokio::time::timeout(Duration::from_millis(150), async {
            let launch = start_command(&spec)?;
            wait_healthy("http://127.0.0.1:31999", "/health", &spec).await?;
            launch.commit();
            Ok::<_, anyhow::Error>(())
        })
        .await;
        assert!(
            cancelled.is_err(),
            "the readiness future should be cancelled"
        );
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        assert!(
            !root.join("launch-marker").exists(),
            "cancelling ensure must reap the launcher and its delayed children"
        );
    }
}
