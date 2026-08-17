//! Bundled VictoriaLogs sidecar supervisor.
//!
//! The index is disposable, and this file is written as if that is true: a
//! missing binary, a refused port, a crash loop, or a wiped data directory all
//! resolve to diagnostics `degraded` and nothing else. Workshop stays healthy,
//! the journal stays authoritative, and queries fall back to SQLite.
//!
//! Startup is lazy on purpose — [`super::service::DiagnosticsService::start`]
//! calls in after the main window is interactive, so a slow first launch of the
//! index cannot sit in front of the user's first paint.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

pub const DESCRIPTOR_SCHEMA: &str = "synth.diagnostics-descriptor.v1";
pub const BINARY_ENV: &str = "SYNTH_VICTORIALOGS_BIN";
pub const DISABLE_ENV: &str = "SYNTH_DIAGNOSTICS_SIDECAR";

/// Relative location of the bundled executable inside the packaged app.
pub const BUNDLED_RELATIVE_PATH: &str = "services/victoria-logs/victoria-logs";

pub const READY_WAIT: Duration = Duration::from_secs(20);
pub const READY_POLL: Duration = Duration::from_millis(200);
pub const STOP_GRACE: Duration = Duration::from_millis(750);

/// Restart backoff, in order. After the last one the supervisor stays degraded
/// until something asks it to start again: an index that cannot hold a process
/// is not worth an endless respawn loop behind a healthy app.
pub const RESTART_BACKOFF: &[Duration] = &[
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
];

/// Defaults: seven days or the disk quota, whichever comes first.
pub const DEFAULT_RETENTION_DAYS: u32 = 7;
pub const DEFAULT_QUOTA_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "reason")]
pub enum SidecarState {
    Stopped,
    Starting,
    Ready,
    /// Indexing is unavailable; queries answer from the journal.
    Degraded(String),
}

impl SidecarState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Degraded(_) => "degraded",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Degraded(reason) => Some(reason),
            _ => None,
        }
    }
}

/// Safe local connection data, modeled on the existing IPC descriptors. It
/// carries no token, because VictoriaLogs is reachable only from loopback and
/// only by this process.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarDescriptor {
    pub schema: String,
    pub instance: Option<String>,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub data_dir: String,
    pub retention_days: u32,
    pub quota_bytes: u64,
    pub updated_at: String,
}

#[derive(Clone, Debug)]
pub struct SidecarConfig {
    pub root: PathBuf,
    pub retention_days: u32,
    pub quota_bytes: u64,
    /// Deterministic listen port for development and failure-injection tests.
    /// Production leaves this unset and asks the OS for an ephemeral port.
    #[doc(hidden)]
    pub listen_port: Option<u16>,
}

impl SidecarConfig {
    pub fn for_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            retention_days: DEFAULT_RETENTION_DAYS,
            quota_bytes: DEFAULT_QUOTA_BYTES,
            listen_port: None,
        }
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join("victorialogs-data")
    }

    pub fn descriptor_path(&self) -> PathBuf {
        self.root.join("descriptor.json")
    }

    pub fn log_path(&self) -> PathBuf {
        self.root.join("victoria-logs.log")
    }
}

#[derive(Default)]
struct Inner {
    child: Option<Child>,
    state: Option<SidecarState>,
    url: Option<String>,
    attempt: usize,
}

pub struct VictoriaLogsSidecar {
    config: SidecarConfig,
    inner: Mutex<Inner>,
}

impl VictoriaLogsSidecar {
    pub fn new(config: SidecarConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            inner: Mutex::new(Inner::default()),
        })
    }

    pub fn config(&self) -> &SidecarConfig {
        &self.config
    }

    pub async fn state(&self) -> SidecarState {
        self.inner
            .lock()
            .await
            .state
            .clone()
            .unwrap_or(SidecarState::Stopped)
    }

    pub async fn url(&self) -> Option<String> {
        self.inner.lock().await.url.clone()
    }

    /// Start the sidecar, or record precisely why it is degraded.
    ///
    /// Never returns an error: a diagnostics index that fails to start must be
    /// a status, not a failure the caller has to handle.
    pub async fn start(self: &Arc<Self>) -> SidecarState {
        if std::env::var(DISABLE_ENV).as_deref() == Ok("0") {
            return self
                .set_state(
                    SidecarState::Degraded("disabled_by_environment".into()),
                    None,
                )
                .await;
        }
        {
            let inner = self.inner.lock().await;
            if matches!(inner.state, Some(SidecarState::Ready)) && inner.child.is_some() {
                return SidecarState::Ready;
            }
        }
        let Some(binary) = locate_binary() else {
            return self
                .set_state(SidecarState::Degraded("binary_missing".into()), None)
                .await;
        };
        if let Err(error) = std::fs::create_dir_all(self.config.data_dir()) {
            return self
                .set_state(
                    SidecarState::Degraded(format!("data_dir_unavailable: {error}")),
                    None,
                )
                .await;
        }
        self.set_state(SidecarState::Starting, None).await;
        self.reap_stale_process().await;

        let port = match self.config.listen_port {
            Some(port) => port,
            None => match reserve_port().await {
                Ok(port) => port,
                Err(error) => {
                    return self
                        .set_state(SidecarState::Degraded(format!("no_port: {error}")), None)
                        .await
                }
            },
        };
        let url = format!("http://127.0.0.1:{port}");
        let child = match self.spawn_process(&binary, port).await {
            Ok(child) => child,
            Err(error) => {
                return self
                    .set_state(
                        SidecarState::Degraded(format!("spawn_failed: {error}")),
                        None,
                    )
                    .await
            }
        };
        let pid = child.id();
        {
            let mut inner = self.inner.lock().await;
            inner.child = Some(child);
            inner.url = Some(url.clone());
        }

        let client = match super::victorialogs::VictoriaLogsClient::new(&url) {
            Ok(client) => client,
            Err(error) => {
                return self
                    .set_state(
                        SidecarState::Degraded(format!("client_failed: {error}")),
                        None,
                    )
                    .await
            }
        };
        let deadline = std::time::Instant::now() + READY_WAIT;
        while std::time::Instant::now() < deadline {
            if client.healthy().await {
                self.inner.lock().await.attempt = 0;
                return self.set_state(SidecarState::Ready, pid).await;
            }
            if self.exited().await {
                self.terminate().await;
                return self
                    .set_state(
                        SidecarState::Degraded("process_exited_before_ready".into()),
                        None,
                    )
                    .await;
            }
            tokio::time::sleep(READY_POLL).await;
        }
        self.terminate().await;
        self.set_state(SidecarState::Degraded("readiness_timeout".into()), None)
            .await
    }

    /// Restart with bounded backoff after an unexpected exit.
    pub async fn restart_with_backoff(self: &Arc<Self>) -> SidecarState {
        let attempt = {
            let mut inner = self.inner.lock().await;
            inner.attempt += 1;
            inner.attempt
        };
        let Some(delay) = RESTART_BACKOFF.get(attempt - 1) else {
            return self
                .set_state(
                    SidecarState::Degraded("restart_budget_exhausted".into()),
                    None,
                )
                .await;
        };
        tokio::time::sleep(*delay).await;
        self.start().await
    }

    /// Has the process exited without us asking it to?
    pub async fn exited(&self) -> bool {
        let mut inner = self.inner.lock().await;
        let Some(child) = inner.child.as_mut() else {
            return false;
        };
        matches!(child.try_wait(), Ok(Some(_)))
    }

    pub async fn stop(&self) -> Result<()> {
        self.terminate().await;
        let mut inner = self.inner.lock().await;
        inner.state = Some(SidecarState::Stopped);
        inner.url = None;
        drop(inner);
        self.write_descriptor(&SidecarState::Stopped, None);
        Ok(())
    }

    /// Delete the index and its cursor. Authoritative journal rows, traces, and
    /// run evidence are untouched: the next indexer pass rebuilds from zero.
    pub async fn clear_index(&self) -> Result<()> {
        self.terminate().await;
        let data_dir = self.config.data_dir();
        if data_dir.exists() {
            std::fs::remove_dir_all(&data_dir)
                .with_context(|| format!("remove diagnostic index at {}", data_dir.display()))?;
        }
        let cursor = super::indexer::cursor_path(&self.config.root);
        if cursor.exists() {
            let _ = std::fs::remove_file(cursor);
        }
        let mut inner = self.inner.lock().await;
        inner.state = Some(SidecarState::Stopped);
        inner.url = None;
        inner.attempt = 0;
        Ok(())
    }

    /// Bytes currently used by the index (for the quota indicator).
    pub fn index_size_bytes(&self) -> u64 {
        directory_size(&self.config.data_dir())
    }

    async fn spawn_process(&self, binary: &Path, port: u16) -> Result<Child> {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.config.log_path())
            .context("open VictoriaLogs log")?;
        let mut command = Command::new(binary);
        isolate_process_group(&mut command);
        command
            .arg(format!(
                "-storageDataPath={}",
                self.config.data_dir().display()
            ))
            .arg(format!("-httpListenAddr=127.0.0.1:{port}"))
            .arg(format!("-retentionPeriod={}d", self.config.retention_days))
            .arg(format!(
                "-retention.maxDiskSpaceUsageBytes={}",
                self.config.quota_bytes
            ))
            .arg("-loggerLevel=WARN")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        command.spawn().context("spawn bundled VictoriaLogs")
    }

    /// A previous Workshop process may have died without draining. Kill only a
    /// process that is demonstrably the VictoriaLogs we started for *this*
    /// instance's data directory — never a pid we merely found recorded.
    async fn reap_stale_process(&self) {
        let Some(descriptor) = self.read_descriptor() else {
            return;
        };
        let Some(pid) = descriptor.pid else { return };
        if descriptor.data_dir != self.config.data_dir().display().to_string() {
            return;
        }
        if !process_is_our_sidecar(pid, &self.config.data_dir()) {
            return;
        }
        terminate_process_group(pid).await;
    }

    async fn terminate(&self) {
        let child = self.inner.lock().await.child.take();
        let Some(mut child) = child else { return };
        if let Some(pid) = child.id() {
            terminate_process_group(pid).await;
        }
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill().await;
        }
        let _ = child.wait().await;
    }

    async fn set_state(&self, state: SidecarState, pid: Option<u32>) -> SidecarState {
        let changed = {
            let mut inner = self.inner.lock().await;
            let changed = inner.state.as_ref() != Some(&state);
            inner.state = Some(state.clone());
            if !matches!(state, SidecarState::Ready | SidecarState::Starting) {
                inner.url = None;
            }
            changed
        };
        // The index loop retries a degraded sidecar forever. Rewriting the same
        // descriptor every retry would be churn nobody reads.
        if changed {
            self.write_descriptor(&state, pid);
        }
        state
    }

    pub fn read_descriptor(&self) -> Option<SidecarDescriptor> {
        let raw = std::fs::read_to_string(self.config.descriptor_path()).ok()?;
        serde_json::from_str(&raw).ok()
    }

    fn write_descriptor(&self, state: &SidecarState, pid: Option<u32>) {
        let descriptor = SidecarDescriptor {
            schema: DESCRIPTOR_SCHEMA.into(),
            instance: crate::instance::name(),
            state: state.label().into(),
            reason: state.reason().map(str::to_owned),
            url: futures_url(self),
            pid,
            data_dir: self.config.data_dir().display().to_string(),
            retention_days: self.config.retention_days,
            quota_bytes: self.config.quota_bytes,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        let path = self.config.descriptor_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(body) = serde_json::to_vec_pretty(&json!(descriptor)) else {
            return;
        };
        if std::fs::write(&path, body).is_ok() {
            set_private_file(&path);
        }
    }
}

/// `url()` is async; the descriptor writer is not. Read the cached value
/// without awaiting by going through the try-lock.
fn futures_url(sidecar: &VictoriaLogsSidecar) -> Option<String> {
    sidecar
        .inner
        .try_lock()
        .ok()
        .and_then(|inner| inner.url.clone())
}

impl crate::services::ManagedService for VictoriaLogsSidecar {
    fn name(&self) -> &'static str {
        "diagnostics-index"
    }

    fn stop(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move { VictoriaLogsSidecar::stop(self).await })
    }
}

/// Find the bundled executable.
///
/// Order: explicit override, then the packaged `Contents/Resources` layout,
/// then the development checkout. A missing binary is not an error here — the
/// caller turns `None` into `degraded`.
pub fn locate_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(BINARY_ENV) {
        let path = PathBuf::from(path);
        return is_executable(&path).then_some(path);
    }
    for root in resource_roots() {
        let candidate = root.join(BUNDLED_RELATIVE_PATH);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn resource_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(dir) = executable.parent() {
            roots.push(dir.to_owned());
            roots.push(dir.join("Resources"));
            roots.push(dir.join("resources"));
            if let Some(parent) = dir.parent() {
                roots.push(parent.join("Resources"));
                roots.push(parent.join("resources"));
            }
        }
    }
    // Development checkout: src-tauri -> synth_desktop -> apps -> workshop.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(workshop) = manifest
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
    {
        roots.push(workshop.to_owned());
    }
    roots
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Ask the OS for a free loopback port, then release it for the child.
///
/// A short race window exists between release and bind; a failed bind surfaces
/// as `degraded` and the next start attempt draws a new port.
async fn reserve_port() -> Result<u16> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .context("reserve a loopback port for the diagnostics index")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Only a process whose command line names *our* data directory may be killed.
#[cfg(unix)]
pub fn process_is_our_sidecar(pid: u32, data_dir: &Path) -> bool {
    let Ok(output) = std::process::Command::new("/bin/ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .output()
    else {
        return false;
    };
    let command = String::from_utf8_lossy(&output.stdout);
    command_owns_data_dir(&command, data_dir)
}

#[cfg(not(unix))]
pub fn process_is_our_sidecar(_pid: u32, _data_dir: &Path) -> bool {
    false
}

pub(crate) fn command_owns_data_dir(command: &str, data_dir: &Path) -> bool {
    let data_dir = data_dir.display().to_string();
    command.contains("victoria-logs") && command.contains(&data_dir)
}

#[cfg(unix)]
async fn terminate_process_group(pid: u32) {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        eprintln!("refusing to terminate diagnostics sidecar with invalid pid {pid}");
        return;
    };
    if pid <= 1 {
        eprintln!("refusing to terminate diagnostics sidecar with unsafe pid {pid}");
        return;
    }
    // Sidecars are spawned with process_group(0), so pid must equal pgid and
    // must not be the host process group. Never signal a raw pid that failed
    // that check: `kill(host, SIGTERM)` is how a cleanup path takes down
    // ChatGPT, Terminal, and the Workshop test runner.
    let isolated_group = unsafe {
        let pgid = libc::getpgid(pid);
        (pgid > 1 && pgid == pid && pgid != libc::getpgrp()).then_some(pgid)
    };
    let Some(pgid) = isolated_group else {
        eprintln!("refusing to terminate diagnostics sidecar with unsafe or unowned pid {pid}");
        return;
    };
    unsafe {
        libc::kill(-pgid, libc::SIGTERM);
    }
    tokio::time::sleep(STOP_GRACE).await;
    unsafe {
        if libc::kill(-pgid, 0) == 0 {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
async fn terminate_process_group(_pid: u32) {}

#[cfg(unix)]
fn isolate_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
}

#[cfg(not(unix))]
fn isolate_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn set_private_file(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) {}

pub fn directory_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => directory_size(&entry.path()),
            Ok(_) => entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn a_missing_binary_is_degraded_and_never_an_error() {
        let dir = tempdir().unwrap();
        std::env::set_var(BINARY_ENV, dir.path().join("does-not-exist"));
        let sidecar = VictoriaLogsSidecar::new(SidecarConfig::for_root(dir.path()));
        let state = sidecar.start().await;
        std::env::remove_var(BINARY_ENV);
        assert_eq!(state, SidecarState::Degraded("binary_missing".into()));
        assert_eq!(sidecar.url().await, None);
    }

    #[tokio::test]
    async fn degraded_state_is_written_to_the_descriptor() {
        let dir = tempdir().unwrap();
        std::env::set_var(BINARY_ENV, dir.path().join("absent"));
        let sidecar = VictoriaLogsSidecar::new(SidecarConfig::for_root(dir.path()));
        sidecar.start().await;
        std::env::remove_var(BINARY_ENV);
        let descriptor = sidecar.read_descriptor().expect("descriptor");
        assert_eq!(descriptor.schema, DESCRIPTOR_SCHEMA);
        assert_eq!(descriptor.state, "degraded");
        assert_eq!(descriptor.reason.as_deref(), Some("binary_missing"));
        assert_eq!(descriptor.retention_days, DEFAULT_RETENTION_DAYS);
        assert!(descriptor.url.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn the_descriptor_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let config = SidecarConfig::for_root(dir.path());
        let sidecar = VictoriaLogsSidecar::new(config.clone());
        sidecar.write_descriptor(&SidecarState::Stopped, None);
        let mode = std::fs::metadata(config.descriptor_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sidecar_cleanup_refuses_host_and_unowned_process_groups() {
        terminate_process_group(0).await;
        terminate_process_group(1).await;
        terminate_process_group(u32::MAX).await;
        terminate_process_group(std::process::id()).await;

        let mut child = Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        terminate_process_group(child.id().unwrap()).await;
        assert!(
            child.try_wait().unwrap().is_none(),
            "an unowned ChatGPT/Terminal/dev process must survive sidecar cleanup"
        );
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }

    #[test]
    fn only_our_own_index_process_may_be_reaped() {
        let ours = Path::new("/Users/x/Library/Synth Desktop/diagnostics/victorialogs-data");
        assert!(command_owns_data_dir(
            "/App.app/Contents/Resources/services/victoria-logs/victoria-logs -storageDataPath=/Users/x/Library/Synth Desktop/diagnostics/victorialogs-data -httpListenAddr=127.0.0.1:1",
            ours
        ));
        // Another instance's index.
        assert!(!command_owns_data_dir(
            "victoria-logs -storageDataPath=/Users/x/other-instance/diagnostics/victorialogs-data",
            ours
        ));
        // Something else entirely that happens to mention the path.
        assert!(!command_owns_data_dir(
            "grep -r victorialogs-data /Users/x/Library/Synth Desktop/diagnostics/victorialogs-data",
            ours
        ));
    }

    #[tokio::test]
    async fn two_instances_never_share_an_index_directory() {
        let dir = tempdir().unwrap();
        let first = SidecarConfig::for_root(dir.path().join("instance-a/diagnostics"));
        let second = SidecarConfig::for_root(dir.path().join("instance-b/diagnostics"));
        assert_ne!(first.data_dir(), second.data_dir());
        assert_ne!(first.descriptor_path(), second.descriptor_path());
    }

    #[tokio::test]
    async fn clearing_the_index_removes_data_and_cursor_only() {
        let dir = tempdir().unwrap();
        let config = SidecarConfig::for_root(dir.path());
        std::fs::create_dir_all(config.data_dir()).unwrap();
        std::fs::write(config.data_dir().join("part.bin"), b"index").unwrap();
        std::fs::write(super::super::indexer::cursor_path(dir.path()), b"{}").unwrap();
        let keep = dir.path().join("bundles");
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::write(keep.join("bundle.json"), b"{}").unwrap();

        let sidecar = VictoriaLogsSidecar::new(config.clone());
        sidecar.clear_index().await.unwrap();

        assert!(!config.data_dir().exists());
        assert!(!super::super::indexer::cursor_path(dir.path()).exists());
        assert!(keep.join("bundle.json").exists());
    }

    #[test]
    fn restart_backoff_is_bounded() {
        assert!(RESTART_BACKOFF.len() <= 8);
        assert!(RESTART_BACKOFF.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[tokio::test]
    async fn reserved_ports_are_loopback_only() {
        let port = reserve_port().await.unwrap();
        assert!(port > 0);
    }

    #[test]
    fn index_size_reports_zero_for_a_missing_directory() {
        assert_eq!(
            directory_size(Path::new("/nonexistent-diagnostics-path")),
            0
        );
    }
}
