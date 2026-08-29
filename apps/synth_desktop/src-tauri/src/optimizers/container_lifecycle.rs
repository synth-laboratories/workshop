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

fn declaration_manifest_hash(spec: &ContainerSpec) -> Result<String> {
    let bytes = std::fs::read(&spec.origin.manifest_path).with_context(|| {
        format!(
            "read container declaration {}",
            spec.origin.manifest_path.display()
        )
    })?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

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
    session_id: &str,
    container_id: &str,
) -> Result<workspace_recipe::ContainerSpec> {
    let (spec_id, metadata) = declared_record(db, container_id)?;
    let search_roots = workspace_recipe::session_search_roots(db, session_id)?;
    let stored = workspace_recipe::origin_from_metadata(&metadata, &spec_id);
    workspace_recipe::resolve_container_spec(&search_roots, &spec_id, stored.as_ref())
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
    // Stamp the launch with the revision this declaration named, so the
    // running process can answer what it loaded. Freshness cannot be read off
    // the declaration alone: the v9 harness source moved while the launch
    // declaration stayed byte-identical, so its digest went on matching and
    // nothing could say the managed container had not been replaced. A
    // container left over from an earlier launch echoes that earlier
    // revision, and admission refuses it.
    if let Some(revision) = spec
        .source_revision
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        command.env(crate::limits::CONTAINER_SOURCE_REVISION_ENV, revision);
    }
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
                match validate_declared_runtime_identity(&client, base_url, spec).await {
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
    Ok(validate_declared_runtime_identity(&client, base_url, spec)
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
) -> Result<()> {
    let expected_image = spec.environment.get("SYNTH_CONTAINER_IMAGE_DIGEST");
    let expected_producer = spec
        .environment
        .get("SYNTH_CONTAINER_PRODUCER_SOURCE_REVISION");
    if expected_image.is_none() && expected_producer.is_none() {
        return Ok(());
    }
    let info_url = format!("{}/info", base_url.trim_end_matches('/'));
    let response = client
        .get(&info_url)
        .send()
        .await
        .with_context(|| format!("container_identity_pending: query {info_url}"))?;
    anyhow::ensure!(
        response.status().is_success(),
        "container_identity_pending: {info_url} returned HTTP {}",
        response.status()
    );
    let info = response
        .json::<Value>()
        .await
        .context("container_identity_pending: /info response is not JSON")?;
    if let Some(expected) = expected_image {
        let actual = info.get("imageDigest").and_then(Value::as_str);
        anyhow::ensure!(
            actual == Some(expected.as_str()),
            "container_identity_pending: expected imageDigest {expected}, got {actual:?}"
        );
    }
    if let Some(expected) = expected_producer {
        let actual = info.get("producerSourceRevision").and_then(Value::as_str);
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
    let info = crate::http::http_client_builder()
        .timeout(Duration::from_secs(2))
        .build()?
        .get(format!("{}/info", base_url.trim_end_matches('/')))
        .send()
        .await
        .context("container_identity_pending: query /info before registration")?
        .error_for_status()
        .context("container_identity_pending: /info was not successful")?
        .json::<Value>()
        .await
        .context("container_identity_pending: /info response is not JSON")?;
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
    let manifest_digest = Some(declaration_manifest_hash(spec)?);
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
                "info": info,
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
    let manifest_digest = Some(declaration_manifest_hash(spec)?);
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

