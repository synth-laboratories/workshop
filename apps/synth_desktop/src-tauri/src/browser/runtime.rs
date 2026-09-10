use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const LOCAL_ORIGINS: [&str; 2] = ["http://localhost", "http://127.0.0.1"];

/// Where the managed-browser runtime that will actually be used came from.
/// Readiness and process launch resolve this once, through [`resolve_runtime`],
/// so Settings can never report a different runtime than the one that starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSource {
    /// `SYNTH_BROWSER_RUNTIME_ROOT`: an explicit operator override.
    Override,
    /// The assembled runtime installed inside the application bundle.
    Bundled,
    /// `apps/synth_desktop/browser/runtime` in a developer checkout.
    Repository,
    /// No assembled runtime: a developer `PATH` interpreter and the checkout's
    /// own `node_modules`. Never an acceptable state for a packaged build.
    Path,
}

impl RuntimeSource {
    fn as_str(self) -> &'static str {
        match self {
            RuntimeSource::Override => "override",
            RuntimeSource::Bundled => "bundled",
            RuntimeSource::Repository => "repository",
            RuntimeSource::Path => "path",
        }
    }
}

/// The interpreter chosen for the backend, independent of the package root:
/// `SYNTH_BROWSER_NODE` stays honoured even when a bundled runtime is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeSource {
    Bundled,
    Override,
    Path,
}

impl NodeSource {
    fn as_str(self) -> &'static str {
        match self {
            NodeSource::Bundled => "bundled",
            NodeSource::Override => "override",
            NodeSource::Path => "path",
        }
    }
}

/// The candidate locations a resolution is computed from. Kept separate from
/// process state so the selection rules are unit-testable without a bundle.
#[derive(Clone, Debug, Default)]
pub struct RuntimeInputs {
    pub runtime_root_override: Option<PathBuf>,
    pub node_override: Option<PathBuf>,
    pub bundled_root: Option<PathBuf>,
    pub repository_root: Option<PathBuf>,
}

/// One resolved runtime. `problem` is `Some` when the selected runtime cannot
/// be used; resolution still reports which runtime was selected so readiness
/// names the real artifacts rather than falling back and hiding the fault.
#[derive(Clone, Debug)]
pub struct RuntimeResolution {
    pub source: RuntimeSource,
    pub root: Option<PathBuf>,
    pub node: PathBuf,
    pub node_source: NodeSource,
    pub playwright_package: Option<PathBuf>,
    pub browsers_path: Option<PathBuf>,
    pub problem: Option<String>,
}

impl RuntimeResolution {
    pub fn usable(&self) -> bool {
        self.problem.is_none()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPolicy {
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRuntimeStatus {
    pub phase: String,
    pub detail: String,
    pub backend_present: bool,
    pub node_present: bool,
    pub playwright_present: bool,
    pub chromium_present: bool,
    pub node_version: Option<String>,
    pub backend_path: String,
    pub profile_root: String,
    pub allowed_origins: Vec<String>,
    pub default_local_origins: Vec<String>,
    /// Which runtime readiness actually measured, and which the next managed
    /// session will start. `path` means no assembled runtime was found.
    pub runtime_source: String,
    pub runtime_root: Option<String>,
    pub node_source: String,
    pub node_path: String,
    pub playwright_version: Option<String>,
    pub chromium_path: Option<String>,
}

pub fn policy_path() -> PathBuf {
    crate::storage::app_data_root().join("browser/policy.json")
}

pub fn profile_root() -> PathBuf {
    crate::storage::app_data_root().join("browser-profiles")
}

/// `Contents/Resources/browser` for a packaged build, if this process is one.
fn bundled_browser_resources() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let candidate = exe.parent()?.join("../Resources/browser");
    candidate.is_dir().then_some(candidate)
}

fn repository_browser_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../browser")
}

pub fn backend_script_path() -> PathBuf {
    if let Some(configured) = env::var_os("SYNTH_BROWSER_BACKEND_SCRIPT") {
        return PathBuf::from(configured);
    }
    if let Some(resources) = bundled_browser_resources() {
        let bundled = resources.join("playwright_backend.mjs");
        if bundled.is_file() {
            return bundled;
        }
    }
    repository_browser_dir().join("playwright_backend.mjs")
}

/// Candidate runtime locations for this process.
pub fn runtime_inputs() -> RuntimeInputs {
    RuntimeInputs {
        runtime_root_override: env::var_os("SYNTH_BROWSER_RUNTIME_ROOT").map(PathBuf::from),
        node_override: env::var_os("SYNTH_BROWSER_NODE").map(PathBuf::from),
        bundled_root: bundled_browser_resources().map(|resources| resources.join("runtime")),
        repository_root: Some(repository_browser_dir().join("runtime")),
    }
}

fn runtime_node_binary(root: &Path) -> PathBuf {
    root.join("node/bin/node")
}

fn playwright_package_dir(root: &Path) -> PathBuf {
    root.join("node_modules/playwright")
}

fn browsers_dir(root: &Path) -> PathBuf {
    root.join("browsers")
}

fn chromium_install_present(browsers: &Path) -> bool {
    let Ok(entries) = fs::read_dir(browsers) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with("chromium-")
    })
}

/// Inspect one assembled runtime root and describe the first blocking problem.
/// Every message names the artifact and the path an operator has to repair.
fn assembled_runtime_problem(source: RuntimeSource, root: &Path) -> Option<String> {
    let label = match source {
        RuntimeSource::Override => "SYNTH_BROWSER_RUNTIME_ROOT runtime",
        RuntimeSource::Bundled => "packaged browser runtime",
        RuntimeSource::Repository => "checkout browser runtime",
        RuntimeSource::Path => "browser runtime",
    };
    if !root.is_dir() {
        return Some(format!("{label} is missing at {}", root.display()));
    }
    let node = runtime_node_binary(root);
    if !node.is_file() {
        return Some(format!(
            "{label} has no pinned Node executable at {}",
            node.display()
        ));
    }
    let package = playwright_package_dir(root).join("package.json");
    if !package.is_file() {
        return Some(format!(
            "{label} has no Playwright package at {}",
            package.display()
        ));
    }
    let browsers = browsers_dir(root);
    if !chromium_install_present(&browsers) {
        return Some(format!(
            "{label} has no installed Chromium under {}",
            browsers.display()
        ));
    }
    None
}

/// Select the single runtime that both readiness and launch will use.
///
/// An assembled runtime always wins over `PATH`: a packaged build must never
/// silently borrow a developer interpreter, and an incomplete assembled
/// runtime is reported as a fault rather than hidden behind a fallback.
pub fn resolve_runtime(inputs: &RuntimeInputs) -> RuntimeResolution {
    let selected = inputs
        .runtime_root_override
        .clone()
        .map(|root| (RuntimeSource::Override, root))
        .or_else(|| {
            inputs
                .bundled_root
                .clone()
                .map(|root| (RuntimeSource::Bundled, root))
        })
        .or_else(|| {
            inputs
                .repository_root
                .clone()
                .filter(|root| root.is_dir())
                .map(|root| (RuntimeSource::Repository, root))
        });

    let Some((source, root)) = selected else {
        let (node, node_source) = match inputs.node_override.clone() {
            Some(node) => (node, NodeSource::Override),
            None => (PathBuf::from("node"), NodeSource::Path),
        };
        return RuntimeResolution {
            source: RuntimeSource::Path,
            root: None,
            node,
            node_source,
            playwright_package: None,
            browsers_path: None,
            problem: None,
        };
    };

    let problem = assembled_runtime_problem(source, &root);
    // An explicit interpreter override stays in force for the assembled
    // runtime; operators use it to test an interpreter against pinned
    // packages without reassembling.
    let (node, node_source) = match inputs.node_override.clone() {
        Some(node) => (node, NodeSource::Override),
        None => (runtime_node_binary(&root), NodeSource::Bundled),
    };
    RuntimeResolution {
        source,
        root: Some(root.clone()),
        node,
        node_source,
        playwright_package: Some(playwright_package_dir(&root)),
        browsers_path: Some(browsers_dir(&root)),
        problem,
    }
}

/// The resolution for this process.
pub fn current_runtime() -> RuntimeResolution {
    resolve_runtime(&runtime_inputs())
}

/// The readiness probe is blocking and the backend launch is asynchronous;
/// both configure their environment from one resolution through this trait so
/// the two paths cannot drift apart.
pub trait RuntimeCommand {
    fn set_runtime_env(&mut self, key: &str, value: &Path);
    fn clear_runtime_env(&mut self, key: &str);
}

impl RuntimeCommand for Command {
    fn set_runtime_env(&mut self, key: &str, value: &Path) {
        self.env(key, value);
    }
    fn clear_runtime_env(&mut self, key: &str) {
        self.env_remove(key);
    }
}

impl RuntimeCommand for tokio::process::Command {
    fn set_runtime_env(&mut self, key: &str, value: &Path) {
        self.env(key, value);
    }
    fn clear_runtime_env(&mut self, key: &str) {
        self.env_remove(key);
    }
}

/// Apply a resolution to a command that runs the managed-browser backend or
/// its readiness probe. The runtime's own browser cache always wins over an
/// ambient `PLAYWRIGHT_BROWSERS_PATH`, so readiness cannot measure one
/// Chromium while a session launches another.
pub fn apply_runtime_env<C: RuntimeCommand + ?Sized>(
    command: &mut C,
    resolution: &RuntimeResolution,
) {
    if let Some(root) = resolution.root.as_ref() {
        command.set_runtime_env("SYNTH_BROWSER_RUNTIME_ROOT", root);
    } else {
        command.clear_runtime_env("SYNTH_BROWSER_RUNTIME_ROOT");
    }
    if let Some(browsers) = resolution.browsers_path.as_ref() {
        command.set_runtime_env("PLAYWRIGHT_BROWSERS_PATH", browsers);
    }
}

pub fn load_policy() -> Result<BrowserPolicy> {
    let path = policy_path();
    let body = match fs::read_to_string(&path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BrowserPolicy::default())
        }
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    serde_json::from_str(&body).with_context(|| format!("parse {}", path.display()))
}

fn save_policy(policy: &BrowserPolicy) -> Result<()> {
    let path = policy_path();
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("browser policy has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    fs::write(&path, serde_json::to_vec_pretty(policy)?)
        .with_context(|| format!("write {}", path.display()))?;
    set_private_file(&path)?;
    Ok(())
}

fn normalize_origin(value: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(value.trim()).context("origin must be an absolute URL")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(anyhow!("only HTTP(S) origins can be approved"));
    }
    if parsed.username() != ""
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(anyhow!(
            "enter an origin only, for example https://example.com"
        ));
    }
    Ok(parsed.origin().ascii_serialization())
}

pub fn allow_origin(value: &str) -> Result<BrowserPolicy> {
    let origin = normalize_origin(value)?;
    let mut policy = load_policy()?;
    if !LOCAL_ORIGINS.contains(&origin.as_str()) && !policy.allowed_origins.contains(&origin) {
        policy.allowed_origins.push(origin);
        policy.allowed_origins.sort();
    }
    save_policy(&policy)?;
    Ok(policy)
}

pub fn revoke_origin(value: &str) -> Result<BrowserPolicy> {
    let origin = normalize_origin(value)?;
    let mut policy = load_policy()?;
    policy
        .allowed_origins
        .retain(|candidate| candidate != &origin);
    save_policy(&policy)?;
    Ok(policy)
}

/// The readiness probe shipped beside the backend. Both files resolve
/// Playwright the same way, so Settings measures the package a session will
/// actually load rather than a separately written approximation.
pub fn readiness_probe_path() -> PathBuf {
    if let Some(configured) = env::var_os("SYNTH_BROWSER_READINESS_PROBE") {
        return PathBuf::from(configured);
    }
    let beside_backend = backend_script_path()
        .parent()
        .map(|dir| dir.join("readiness_probe.mjs"));
    match beside_backend {
        Some(candidate) if candidate.is_file() => candidate,
        _ => repository_browser_dir().join("readiness_probe.mjs"),
    }
}

#[derive(Default)]
struct ProbeReport {
    playwright: bool,
    chromium: bool,
    version: Option<String>,
    chromium_path: Option<String>,
    error: Option<String>,
}

fn probe_playwright(resolution: &RuntimeResolution) -> ProbeReport {
    let probe = readiness_probe_path();
    if !probe.is_file() {
        return ProbeReport {
            error: Some(format!(
                "the readiness probe is missing from this build at {}",
                probe.display()
            )),
            ..ProbeReport::default()
        };
    }
    let mut command = Command::new(&resolution.node);
    command.arg(&probe);
    apply_runtime_env(&mut command, resolution);
    let Ok(output) = command.output() else {
        return ProbeReport {
            error: Some("the pinned Node interpreter could not be started".to_owned()),
            ..ProbeReport::default()
        };
    };
    let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
    let Some(parsed) = parsed else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return ProbeReport {
            error: Some(
                stderr
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("the readiness probe produced no report")
                    .trim()
                    .to_owned(),
            ),
            ..ProbeReport::default()
        };
    };
    let text = |key: &str| {
        parsed
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    ProbeReport {
        playwright: parsed
            .get("playwright")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        chromium: parsed
            .get("chromium")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        version: text("version"),
        chromium_path: text("chromiumPath"),
        error: text("error"),
    }
}

pub fn runtime_status() -> BrowserRuntimeStatus {
    let backend = backend_script_path();
    let backend_present = backend.is_file();
    let resolution = current_runtime();

    let node_probe = Command::new(&resolution.node).arg("--version").output();
    let node_present = node_probe
        .as_ref()
        .is_ok_and(|output| output.status.success());
    let node_version = node_probe.ok().and_then(|output| {
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    });

    let probe = (backend_present && node_present && resolution.usable())
        .then(|| probe_playwright(&resolution))
        .unwrap_or_default();

    let ready = backend_present
        && node_present
        && resolution.usable()
        && probe.playwright
        && probe.chromium;
    let detail = if ready {
        format!(
            "Playwright {} and its pinned Chromium are ready from the {} runtime.",
            probe.version.as_deref().unwrap_or("runtime"),
            resolution.source.as_str()
        )
    } else if !backend_present {
        "The Workshop Browser backend resource is missing from this build.".to_owned()
    } else if let Some(problem) = resolution.problem.as_deref() {
        problem.to_owned()
    } else if !node_present {
        format!(
            "The pinned Node interpreter at {} could not be started.",
            resolution.node.display()
        )
    } else if !probe.playwright {
        match probe.error.as_deref() {
            Some(error) => format!("The managed browser backend cannot load Playwright: {error}"),
            None => "The Playwright package is unavailable to the managed browser backend."
                .to_owned(),
        }
    } else if let Some(path) = probe.chromium_path.as_deref() {
        format!("Playwright is installed, but its pinned Chromium is missing at {path}.")
    } else {
        "Playwright is installed, but its pinned Chromium executable is missing.".to_owned()
    };

    let policy = load_policy().unwrap_or_default();
    BrowserRuntimeStatus {
        phase: if ready { "ready" } else { "not_ready" }.to_owned(),
        detail,
        backend_present,
        node_present,
        playwright_present: probe.playwright,
        chromium_present: probe.chromium,
        node_version,
        backend_path: backend.display().to_string(),
        profile_root: profile_root().display().to_string(),
        allowed_origins: policy.allowed_origins,
        default_local_origins: LOCAL_ORIGINS
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        runtime_source: resolution.source.as_str().to_owned(),
        runtime_root: resolution
            .root
            .as_ref()
            .map(|root| root.display().to_string()),
        node_source: resolution.node_source.as_str().to_owned(),
        node_path: resolution.node.display().to_string(),
        playwright_version: probe.version,
        chromium_path: probe.chromium_path,
    }
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<()> {
    Ok(())
}

