//! Which Workshop this process is, where its data lives, and the proof that it
//! is the only process holding that data root.
//!
//! Identity has one naming source. The bundle descriptor
//! (`Contents/Resources/instance.json`, written at build time) is authoritative
//! because it survives every launch path — Finder, `open -b`, a LaunchServices
//! relaunch — none of which reliably carry the launcher's environment.
//! Environment variables may select non-identity runtime paths for an
//! undescriptored development binary, but they never name an instance or
//! override descriptor identity. A bundle whose identifier marks it
//! as a named development instance (`.dev.`) but that has neither source
//! refuses to start: the one thing it must never do is open the canonical
//! profile under a window titled with another instance's name.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::{
    env,
    fmt::{self, Display},
    fs::{self, File, OpenOptions},
    io::{self, Read as _, Seek as _, SeekFrom, Write as _},
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

#[path = "instance_paths.rs"]
pub mod paths;

#[allow(unused_imports)]
pub use paths::{
    bundle_root, InstanceDescriptor, DATA_ROOT_ENV, DESCRIPTOR_RELATIVE_PATH,
    DESCRIPTOR_SCHEMA_VERSION,
};

pub const INSTANCE_ENV: &str = "SYNTH_DESKTOP_INSTANCE";
pub const MANIFEST_ENV: &str = "SYNTH_DESKTOP_INSTANCE_MANIFEST";
pub const APP_NAME_ENV: &str = "SYNTH_DESKTOP_APP_NAME";
pub const BUNDLE_ID_ENV: &str = "SYNTH_DESKTOP_BUNDLE_ID";

/// A bundle identifier carrying this is a named development instance and must
/// never open the canonical profile.
pub const DEV_BUNDLE_MARKER: &str = ".dev.";
pub const LOCK_FILE_NAME: &str = "instance.lock";
pub const LOCK_SCHEMA_VERSION: &str = "synth.desktop.instance-lock.v1";
/// How many quarantined stale lock records to keep per data root. They are
/// evidence for a crash investigation, not an archive.
const STALE_LOCKS_KEPT: usize = 5;

/// `sysexits` `EX_TEMPFAIL`: the condition is another live process, try later.
pub const EXIT_INSTANCE_LOCKED: i32 = 75;
/// `sysexits` `EX_CONFIG`: the bundle, env, or descriptor disagree.
pub const EXIT_IDENTITY_REFUSED: i32 = 78;

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstanceDiagnostics {
    pub mode: String,
    pub name: Option<String>,
    pub display_name: String,
    pub app_version: String,
    pub source_revision: String,
    pub build_revision: String,
    pub build_timestamp: String,
    pub executable_digest: Option<String>,
    pub process_id: u32,
    pub executable: String,
    pub data_root: String,
    pub vite_url: Option<String>,
    pub manifest: Option<String>,
}

/// Identity of *this run of the backend*, not of the installation.
///
/// A durable row that says a turn is `running` proves only what was true when
/// it was written. Stamping the owner with this value is what lets a later boot
/// tell "a live worker in this process owns that turn" apart from "a previous
/// process died holding it". A new value every start is the point: the previous
/// owner can never accidentally match.
pub fn boot_epoch() -> &'static str {
    static BOOT_EPOCH: OnceLock<String> = OnceLock::new();
    BOOT_EPOCH.get_or_init(|| format!("inst_{}", uuid::Uuid::new_v4().simple()))
}

// ---------------------------------------------------------------------------
// Identity resolution
// ---------------------------------------------------------------------------

/// What the launcher's environment said, before reconciliation with the
/// descriptor. Each field is independently optional: a partial environment is
/// checked on the fields it has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvIdentity {
    pub instance: Option<String>,
    pub data_root: Option<PathBuf>,
    pub bundle_id: Option<String>,
}

impl EnvIdentity {
    pub fn from_process_env() -> Self {
        Self {
            instance: None,
            data_root: env::var_os(DATA_ROOT_ENV)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            bundle_id: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.instance.is_none() && self.data_root.is_none() && self.bundle_id.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySource {
    /// Bundle descriptor only (a LaunchServices launch).
    Descriptor,
    /// Legacy serialized value; new resolution never emits it.
    DescriptorAndEnv,
    /// An undescriptored development process with an environment-selected data root.
    Env,
    /// Nothing named an instance: the installed app.
    Canonical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub source: IdentitySource,
    /// Validated instance name; `None` for the canonical profile.
    pub instance: Option<String>,
    /// `None` means the canonical roots.
    pub data_root: Option<PathBuf>,
    pub bundle_id: Option<String>,
    pub descriptor: Option<InstanceDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityRefusal {
    /// Descriptor and environment name different instances.
    Mismatch {
        field: &'static str,
        descriptor: String,
        env: String,
    },
    /// The descriptor inside this bundle was generated for another bundle.
    ForeignDescriptor {
        descriptor_bundle_id: String,
        plist_bundle_id: String,
    },
    /// `SYNTH_DESKTOP_BUNDLE_ID` is not the bundle this executable runs from.
    EnvBundleMismatch { env: String, plist: String },
    /// A `.dev.` bundle with nothing to say which instance it is.
    DevBundleWithoutIdentity { bundle_id: String },
    /// A descriptor exists and cannot be used.
    UnusableDescriptor { detail: String },
}

impl IdentityRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Mismatch { .. } => "identity_mismatch",
            Self::ForeignDescriptor { .. } => "descriptor_foreign",
            Self::EnvBundleMismatch { .. } => "bundle_id_mismatch",
            Self::DevBundleWithoutIdentity { .. } => "dev_bundle_without_identity",
            Self::UnusableDescriptor { .. } => "descriptor_unusable",
        }
    }
}

impl Display for IdentityRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch {
                field,
                descriptor,
                env,
            } => write!(
                f,
                "the bundle descriptor and the environment disagree on {field}: \
                 descriptor says {descriptor:?}, environment says {env:?}"
            ),
            Self::ForeignDescriptor {
                descriptor_bundle_id,
                plist_bundle_id,
            } => write!(
                f,
                "the descriptor inside this bundle was generated for {descriptor_bundle_id:?}, \
                 but this bundle's CFBundleIdentifier is {plist_bundle_id:?}"
            ),
            Self::EnvBundleMismatch { env, plist } => write!(
                f,
                "{BUNDLE_ID_ENV} is {env:?}, but this bundle's CFBundleIdentifier is {plist:?}"
            ),
            Self::DevBundleWithoutIdentity { bundle_id } => write!(
                f,
                "{bundle_id} is a named development instance but was launched with neither a \
                 bundle descriptor nor {DATA_ROOT_ENV}; refusing to open the canonical profile. \
                 Launch it through scripts/desktop-instance.sh, or rebuild it with cua-build \
                 so the bundle carries its descriptor."
            ),
            Self::UnusableDescriptor { detail } => {
                write!(f, "the bundle descriptor cannot be used: {detail}")
            }
        }
    }
}

impl std::error::Error for IdentityRefusal {}

/// Facts read once from the bundle this process runs from.
#[derive(Clone, Debug, Default)]
pub struct BundleFacts {
    #[allow(dead_code)]
    pub root: Option<PathBuf>,
    pub plist_bundle_id: Option<String>,
    pub descriptor: Option<Result<InstanceDescriptor, String>>,
}

impl BundleFacts {
    fn read(root: Option<PathBuf>) -> Self {
        let Some(root) = root else {
            return Self::default();
        };
        let descriptor = match paths::read_descriptor(&paths::descriptor_path(&root)) {
            Ok(None) => None,
            Ok(Some(descriptor)) => Some(Ok(descriptor)),
            Err(error) => Some(Err(error)),
        };
        Self {
            plist_bundle_id: info_plist_bundle_identifier(&root),
            root: Some(root),
            descriptor,
        }
    }
}

fn running_bundle_facts() -> &'static BundleFacts {
    static FACTS: OnceLock<BundleFacts> = OnceLock::new();
    FACTS.get_or_init(|| BundleFacts::read(paths::running_bundle_root()))
}

/// `CFBundleIdentifier` from a bundle's `Info.plist`.
///
/// Tauri writes the XML form, which a line scan reads without a plist parser;
/// a binary plist (anything `plutil -convert binary1` touched) falls back to
/// `plutil`, the same derivation the Computer Use helper uses.
pub fn info_plist_bundle_identifier(bundle: &Path) -> Option<String> {
    let plist = bundle.join("Contents/Info.plist");
    let raw = fs::read(&plist).ok()?;
    if raw.starts_with(b"bplist") {
        return Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
            .arg(&plist)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .filter(|value| !value.is_empty());
    }
    xml_plist_string(&String::from_utf8_lossy(&raw), "CFBundleIdentifier")
}

/// The `<string>` following `<key>{key}</key>` in an XML plist.
pub fn xml_plist_string(xml: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let after_key = &xml[xml.find(&marker)? + marker.len()..];
    let start = after_key.find("<string>")? + "<string>".len();
    let end = after_key[start..].find("</string>")? + start;
    let value = after_key[start..end].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// The one resolution rule, pure so the tests can drive every branch.
pub fn resolve_identity(
    descriptor: Option<Result<InstanceDescriptor, String>>,
    env: &EnvIdentity,
    plist_bundle_id: Option<&str>,
) -> Result<Identity, IdentityRefusal> {
    let descriptor = match descriptor {
        Some(Err(detail)) => return Err(IdentityRefusal::UnusableDescriptor { detail }),
        Some(Ok(descriptor)) => Some(descriptor),
        None => None,
    };
    if let (Some(descriptor), Some(plist)) = (&descriptor, plist_bundle_id) {
        if descriptor.bundle_id != plist {
            return Err(IdentityRefusal::ForeignDescriptor {
                descriptor_bundle_id: descriptor.bundle_id.clone(),
                plist_bundle_id: plist.to_owned(),
            });
        }
    }
    let is_dev_bundle = plist_bundle_id.is_some_and(|id| id.contains(DEV_BUNDLE_MARKER));

    match descriptor {
        Some(descriptor) => {
            if !validate_name(&descriptor.instance_id) {
                return Err(IdentityRefusal::UnusableDescriptor {
                    detail: format!(
                        "instance_id {:?} is not a valid instance name",
                        descriptor.instance_id
                    ),
                });
            }
            Ok(Identity {
                source: IdentitySource::Descriptor,
                instance: Some(descriptor.instance_id.clone()),
                data_root: Some(descriptor.data_root.clone()),
                bundle_id: Some(descriptor.bundle_id.clone()),
                descriptor: Some(descriptor),
            })
        }
        None => {
            if is_dev_bundle && env.data_root.is_none() {
                return Err(IdentityRefusal::DevBundleWithoutIdentity {
                    bundle_id: plist_bundle_id.unwrap_or_default().to_owned(),
                });
            }
            Ok(Identity {
                source: if env.is_empty() {
                    IdentitySource::Canonical
                } else {
                    IdentitySource::Env
                },
                instance: None,
                data_root: env.data_root.clone(),
                bundle_id: plist_bundle_id.map(str::to_owned),
                descriptor: None,
            })
        }
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.components().eq(right.components())
}

/// This process's identity, resolved from the running bundle and the current
/// environment. The bundle is read once; the environment is read per call so
/// nothing here caches a value a test or a launcher changed under it.
pub fn identity() -> Result<Identity, IdentityRefusal> {
    let facts = running_bundle_facts();
    resolve_identity(
        facts.descriptor.clone(),
        &EnvIdentity::from_process_env(),
        facts.plist_bundle_id.as_deref(),
    )
}

/// Descriptor-owned instance id for runtime correlation. Process environment
/// is deliberately not consulted for naming.
pub fn instance_id() -> String {
    identity()
        .ok()
        .and_then(|identity| identity.instance)
        .unwrap_or_else(|| "canonical".into())
}

/// Boot-time assertion. Prints one structured line and, on refusal, shows a
/// dialog so a double-clicked bundle explains itself instead of silently
/// opening the wrong profile. The caller exits; this never returns a default.
pub fn assert_boot_identity() -> Result<Identity, IdentityRefusal> {
    match identity() {
        Ok(identity) => {
            eprintln!(
                "synth-desktop: instance identity source={:?} instance={} data_root={} bundle_id={}",
                identity.source,
                identity.instance.as_deref().unwrap_or("canonical"),
                identity
                    .data_root
                    .as_deref()
                    .unwrap_or(&paths::canonical_data_root())
                    .display(),
                identity.bundle_id.as_deref().unwrap_or("-"),
            );
            Ok(identity)
        }
        Err(refusal) => {
            eprintln!(
                "synth-desktop: identity_refused code={} {refusal}",
                refusal.code()
            );
            show_refusal_dialog("Synth Workshop cannot start", &refusal.to_string());
            Err(refusal)
        }
    }
}

/// Best-effort native alert before any Tauri window exists. Detached: the
/// refusing process exits immediately and the alert outlives it, so a launcher
/// polling the exit code is never blocked on a dismiss.
fn show_refusal_dialog(title: &str, message: &str) {
    if env::var_os("SYNTH_DESKTOP_REFUSAL_DIALOG").is_some_and(|value| value == "0") {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display alert {} message {} as critical",
            applescript_string(title),
            applescript_string(message)
        );
        let _ = Command::new("/usr/bin/osascript")
            .args(["-e", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (title, message);
    }
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn name() -> Option<String> {
    identity().ok().and_then(|identity| identity.instance)
}

pub fn validate_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn instance_data_root() -> Option<PathBuf> {
    match identity() {
        Ok(identity) => identity.data_root,
        // A refused identity never reaches here in the app (boot exits), but
        // library callers still get the launcher's answer, never a guess.
        Err(_) => env::var_os(DATA_ROOT_ENV).map(PathBuf::from),
    }
}

/// One process-wide lock for tests that repoint this process's instance root
/// or other environment this module reads.
///
/// `cargo test` runs test functions on threads that share one `std::env`, so a
/// helper that sets `SYNTH_DESKTOP_DATA_ROOT` for "its" test is really setting
/// it for whatever else is running at that instant. Several modules had their
/// own helper and at most a module-local lock, which is no lock at all against
/// the others: a test could read another test's instance root mid-assertion.
/// Every such helper takes this one.
#[cfg(test)]
pub(crate) fn environment_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A poisoned lock means an earlier test panicked while holding it. The
    // environment is still ours to take; cascading the panic just hides the
    // original failure behind a dozen unrelated ones.
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Product-owned durable data. Named instances always resolve to their private
/// root; the unset path preserves the canonical installed-app location.
pub fn data_root() -> PathBuf {
    instance_data_root().unwrap_or_else(paths::canonical_data_root)
}

/// User configuration, secrets, Codex homes, and default workspaces. For an
/// isolated instance this intentionally collapses into its private data root.
pub fn state_root() -> PathBuf {
    instance_data_root()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".synth-desktop"))
}

pub fn display_name() -> String {
    name()
        .map(|value| format!("Synth Desktop · {value}"))
        .unwrap_or_else(|| "Synth Desktop".into())
}

pub fn bundle_id() -> Option<String> {
    identity().ok().and_then(|identity| identity.bundle_id)
}

/// Mutable manifest beside the instance root recorded by the descriptor.
pub fn manifest_path() -> Option<PathBuf> {
    let descriptor = identity().ok()?.descriptor?;
    let candidate = descriptor.instance_root?.join("instance.json");
    candidate.is_file().then_some(candidate)
}

pub fn diagnostics() -> InstanceDiagnostics {
    let instance_name = name();
    // Integration tests include this module directly, outside the package
    // target that receives build.rs values. Keep those builds diagnostic-only
    // instead of making compile-time metadata a hard requirement.
    let build_revision = option_env!("SYNTH_BUILD_REVISION").unwrap_or("unknown");
    let build_timestamp = option_env!("SYNTH_BUILD_TIMESTAMP").unwrap_or("unknown");
    InstanceDiagnostics {
        mode: if instance_name.is_some() {
            "development"
        } else {
            "canonical"
        }
        .into(),
        name: instance_name,
        display_name: display_name(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        source_revision: env::var("SYNTH_DESKTOP_SOURCE_REVISION")
            .unwrap_or_else(|_| build_revision.into()),
        build_revision: build_revision.into(),
        build_timestamp: build_timestamp.into(),
        executable_digest: executable_digest(),
        process_id: std::process::id(),
        executable: env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "unknown".into()),
        data_root: data_root().display().to_string(),
        vite_url: env::var("SYNTH_DESKTOP_VITE_URL").ok(),
        manifest: manifest_path().map(|path| path.display().to_string()),
    }
}

fn executable_digest() -> Option<String> {
    static CURRENT_EXECUTABLE_DIGEST: OnceLock<Option<String>> = OnceLock::new();
    let current = CURRENT_EXECUTABLE_DIGEST
        .get_or_init(current_executable_digest)
        .clone();
    preferred_executable_digest(current, manifest_executable_digest())
}

fn preferred_executable_digest(
    current: Option<String>,
    manifest: Option<String>,
) -> Option<String> {
    current.or(manifest)
}

fn manifest_executable_digest() -> Option<String> {
    let path = manifest_path()?;
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let candidate = manifest
        .get("executableDigest")
        .or_else(|| manifest.pointer("/provenance/executableDigest"))
        .or_else(|| manifest.pointer("/runtime/executableDigest"))
        .and_then(serde_json::Value::as_str)?;
    valid_sha256_digest(candidate).then(|| candidate.to_ascii_lowercase())
}

fn valid_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn current_executable_digest() -> Option<String> {
    let executable = fs::File::open(env::current_exe().ok()?).ok()?;
    sha256_digest(executable).ok()
}

fn sha256_digest(mut reader: impl io::Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

/// Best-effort runtime receipt for exact CUA/process targeting. The launcher
/// owns the manifest contract; the app only updates its `runtime` member.
pub fn mark_manifest_running() {
    let Some(path) = manifest_path() else {
        return;
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let diagnostics = diagnostics();
    let process = ProcessIdentity::current();
    manifest["runtime"] = serde_json::json!({
        "status": "running",
        "pid": diagnostics.process_id,
        "processStartIdentity": process.start,
        "bootEpoch": boot_epoch(),
        "executable": diagnostics.executable,
        "executableDigest": diagnostics.executable_digest,
        "sourceRevision": diagnostics.source_revision,
        "buildRevision": diagnostics.build_revision,
        "buildTimestamp": diagnostics.build_timestamp,
    });
    if let Ok(body) = serde_json::to_vec_pretty(&manifest) {
        let temporary = path.with_extension("json.running");
        if fs::write(&temporary, body).is_ok() {
            let _ = fs::rename(temporary, path);
        }
    }
}

// ---------------------------------------------------------------------------
// Process identity
// ---------------------------------------------------------------------------

/// A pid plus what makes it *this* process and not a later reuse of the
/// number. `start` is the kernel's start time as `ps` prints it (macOS/BSD) or
/// field 22 of `/proc/<pid>/stat` (Linux): the same derivation the optimizer
/// sidecar heartbeat uses, so the two sides can compare.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start: String,
    pub exe: String,
}

impl ProcessIdentity {
    pub fn current() -> Self {
        let pid = std::process::id();
        Self {
            pid,
            start: process_start_identity(pid).unwrap_or_default(),
            exe: env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        }
    }

    /// Whether `pid` is still the process this identity described: alive, and
    /// started when we recorded it started.
    pub fn is_still_running(&self) -> bool {
        pid_exists(self.pid)
            && match process_start_identity(self.pid) {
                Some(start) => start == self.start,
                // A process we can see but cannot describe is not proof of
                // reuse; treat it as the same process rather than steal from it.
                None => true,
            }
    }
}

/// Whether a process with this pid exists. `EPERM` means it exists and is not
/// ours to signal, which for liveness is still "exists".
pub fn pid_exists(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Start-time identity of a pid, or `None` when the process is gone.
#[cfg(target_os = "linux")]
pub fn process_start_identity(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name is parenthesised and may contain spaces; fields start
    // after the closing parenthesis. Field 22 (1-based) is starttime.
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19).map(str::to_owned)
}

#[cfg(not(target_os = "linux"))]
pub fn process_start_identity(pid: u32) -> Option<String> {
    let output = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let start = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!start.is_empty()).then_some(start)
}

// ---------------------------------------------------------------------------
// Instance lock
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstanceLockRecord {
    pub schema_version: String,
    pub pid: u32,
    pub process_start_identity: String,
    pub boot_epoch: String,
    pub bundle_id: Option<String>,
    pub exe: String,
    pub acquired_at: String,
}

impl InstanceLockRecord {
    pub fn current() -> Self {
        let process = ProcessIdentity::current();
        Self {
            schema_version: LOCK_SCHEMA_VERSION.into(),
            pid: process.pid,
            process_start_identity: process.start,
            boot_epoch: boot_epoch().to_owned(),
            bundle_id: bundle_id(),
            exe: process.exe,
            acquired_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn process(&self) -> ProcessIdentity {
        ProcessIdentity {
            pid: self.pid,
            start: self.process_start_identity.clone(),
            exe: self.exe.clone(),
        }
    }
}

/// Held for the life of the process. Dropping it releases the `flock`; the
/// app parks it in a static so that happens only at exit.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
    #[allow(dead_code)]
    pub path: PathBuf,
    #[allow(dead_code)]
    pub record: InstanceLockRecord,
}

#[derive(Debug)]
pub enum LockError {
    /// Another live process holds the data root.
    Held {
        path: PathBuf,
        holder: Option<InstanceLockRecord>,
    },
    Io(io::Error),
}

impl Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Held {
                path,
                holder: Some(holder),
            } => write!(
                f,
                "instance_locked pid={} epoch={} path={}",
                holder.pid,
                holder.boot_epoch,
                path.display()
            ),
            Self::Held { path, holder: None } => write!(
                f,
                "instance_locked pid=unknown epoch=unknown path={}",
                path.display()
            ),
            Self::Io(error) => write!(f, "instance lock: {error}"),
        }
    }
}

impl std::error::Error for LockError {}

impl From<io::Error> for LockError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn lock_path(data_root: &Path) -> PathBuf {
    data_root.join(LOCK_FILE_NAME)
}

pub fn stale_lock_path(lock: &Path, pid: u32) -> PathBuf {
    lock.with_file_name(format!("{LOCK_FILE_NAME}.stale-{pid}"))
}

/// Take the instance lock for this process's data root.
pub fn acquire_instance_lock() -> Result<InstanceLock, LockError> {
    acquire_instance_lock_at(&lock_path(&data_root()), InstanceLockRecord::current())
}

/// `flock(LOCK_EX | LOCK_NB)` on `path`, recording who holds it.
///
/// Contention against a holder whose pid is alive with the recorded start
/// identity is a real second process: refuse. Contention against a dead or
/// reused pid is an inherited descriptor (a child outlived its parent with the
/// fd open); the file is quarantined to `instance.lock.stale-<pid>` and a fresh
/// inode is locked, which the inherited descriptor cannot contend for. A record
/// found unlocked from another pid is the crash case: quarantined the same way
/// before it is overwritten, so the evidence survives.
pub fn acquire_instance_lock_at(
    path: &Path,
    record: InstanceLockRecord,
) -> Result<InstanceLock, LockError> {
    use fs2::FileExt;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    for _attempt in 0..2 {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock_exclusive() {
            Ok(()) => {
                if let Some(previous) = read_lock_record(&mut file) {
                    if previous.pid != record.pid {
                        quarantine_record(path, &previous);
                    }
                }
                file.set_len(0)?;
                file.seek(SeekFrom::Start(0))?;
                file.write_all(&serde_json::to_vec_pretty(&record).map_err(io::Error::other)?)?;
                file.sync_all()?;
                return Ok(InstanceLock {
                    _file: file,
                    path: path.to_path_buf(),
                    record,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let holder = read_lock_record(&mut file);
                drop(file);
                match holder {
                    Some(holder) if holder.process().is_still_running() => {
                        return Err(LockError::Held {
                            path: path.to_path_buf(),
                            holder: Some(holder),
                        });
                    }
                    Some(holder) => {
                        fs::rename(path, stale_lock_path(path, holder.pid))?;
                        prune_stale_locks(path);
                        continue;
                    }
                    None => {
                        return Err(LockError::Held {
                            path: path.to_path_buf(),
                            holder: None,
                        });
                    }
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(LockError::Held {
        path: path.to_path_buf(),
        holder: read_lock_record_at(path),
    })
}

fn read_lock_record(file: &mut File) -> Option<InstanceLockRecord> {
    let mut raw = String::new();
    file.seek(SeekFrom::Start(0)).ok()?;
    file.read_to_string(&mut raw).ok()?;
    serde_json::from_str(&raw).ok()
}

fn read_lock_record_at(path: &Path) -> Option<InstanceLockRecord> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn quarantine_record(path: &Path, previous: &InstanceLockRecord) {
    if let Ok(body) = serde_json::to_vec_pretty(previous) {
        let _ = fs::write(stale_lock_path(path, previous.pid), body);
    }
    prune_stale_locks(path);
}

fn prune_stale_locks(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    let prefix = format!("{LOCK_FILE_NAME}.stale-");
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    let mut stale: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(&prefix))
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    stale.sort_by(|left, right| right.0.cmp(&left.0));
    for (_, old) in stale.into_iter().skip(STALE_LOCKS_KEPT) {
        let _ = fs::remove_file(old);
    }
}

/// Park the lock for the life of the process.
pub fn hold_instance_lock(lock: InstanceLock) {
    static HELD: OnceLock<InstanceLock> = OnceLock::new();
    let _ = HELD.set(lock);
}

/// Boot identity, then the data-root lock. Called from `run()` before
/// `tauri::Builder` so a second process never reaches SQLite.
///
/// On refusal this prints one structured line and exits: it does not return.
pub fn install_boot_identity_and_lock() {
    if assert_boot_identity().is_err() {
        std::process::exit(EXIT_IDENTITY_REFUSED);
    }
    match acquire_instance_lock() {
        Ok(lock) => hold_instance_lock(lock),
        Err(error) => {
            eprintln!("synth-desktop: {error}");
            let identifier = bundle_id().unwrap_or_else(|| "com.synth.desktop".into());
            let _ = focus_existing_instance(&identifier);
            std::process::exit(EXIT_INSTANCE_LOCKED);
        }
    }
}

/// Ask the process that already holds this identity to show its window.
///
/// Speaks the `tauri-plugin-single-instance` socket protocol: the socket is
/// keyed by the Tauri identifier (the bundle id), and the listener's callback
/// in `lib.rs` focuses the main window. Best effort — the socket may be gone.
pub fn focus_existing_instance(identifier: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let socket = format!("/tmp/{}_si.sock", identifier.replace(['.', '-'], "_"));
        let mut stream = UnixStream::connect(socket)?;
        let cwd = env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let args = env::args().collect::<Vec<_>>().join("\0");
        stream.write_all(cwd.as_bytes())?;
        stream.write_all(b"\0\0")?;
        stream.write_all(args.as_bytes())?;
        stream.flush()
    }
    #[cfg(not(unix))]
    {
        let _ = identifier;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(instance: &str, data_root: &str, bundle_id: &str) -> InstanceDescriptor {
        InstanceDescriptor {
            schema_version: DESCRIPTOR_SCHEMA_VERSION.into(),
            instance_id: instance.into(),
            instance_root: Some(PathBuf::from(format!("/tmp/instances/{instance}"))),
            config_path: None,
            data_root: PathBuf::from(data_root),
            bundle_id: bundle_id.into(),
            release_line: Some("v07".into()),
            source_revision: Some("abc".into()),
            generated_at: None,
        }
    }

    fn env(instance: Option<&str>, data_root: Option<&str>, bundle_id: Option<&str>) -> EnvIdentity {
        EnvIdentity {
            instance: instance.map(str::to_owned),
            data_root: data_root.map(PathBuf::from),
            bundle_id: bundle_id.map(str::to_owned),
        }
    }

    const DEV_BUNDLE: &str = "com.synth.desktop.v07.dev.alpha";

    #[test]
    fn running_executable_digest_wins_over_mutable_manifest_receipt() {
        assert_eq!(
            preferred_executable_digest(
                Some("sha256:current".into()),
                Some("sha256:stale-manifest".into())
            ),
            Some("sha256:current".into())
        );
        assert_eq!(
            preferred_executable_digest(None, Some("sha256:manifest-fallback".into())),
            Some("sha256:manifest-fallback".into())
        );
    }

    #[test]
    fn accepts_safe_instance_names() {
        for value in ["dev", "alice", "agent-2", "a123"] {
            assert!(validate_name(value), "{value}");
        }
    }

    #[test]
    fn rejects_unsafe_instance_names() {
        for value in [
            "",
            "Dev",
            "-dev",
            "dev_2",
            "../dev",
            "dev/other",
            "abcdefghijklmnopqrstuvwxyz1234567",
        ] {
            assert!(!validate_name(value), "{value}");
        }
    }

    #[test]
    fn executable_provenance_accepts_only_qualified_sha256() {
        assert!(valid_sha256_digest(&format!("sha256:{}", "a".repeat(64))));
        assert!(valid_sha256_digest(&format!("sha256:{}", "F".repeat(64))));
        assert!(!valid_sha256_digest(&"a".repeat(64)));
        assert!(!valid_sha256_digest(&format!("sha256:{}", "g".repeat(64))));
        assert!(!valid_sha256_digest("sha256:short"));
    }

    #[test]
    fn executable_provenance_hashes_runtime_bytes() {
        assert_eq!(
            sha256_digest(std::io::Cursor::new(b"abc")).unwrap(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // --- identity resolution -------------------------------------------------

    #[test]
    fn a_descriptor_alone_names_the_instance() {
        let identity = resolve_identity(
            Some(Ok(descriptor("alpha", "/tmp/instances/alpha/data", DEV_BUNDLE))),
            &EnvIdentity::default(),
            Some(DEV_BUNDLE),
        )
        .unwrap();
        assert_eq!(identity.source, IdentitySource::Descriptor);
        assert_eq!(identity.instance.as_deref(), Some("alpha"));
        assert_eq!(
            identity.data_root.as_deref(),
            Some(Path::new("/tmp/instances/alpha/data"))
        );
        assert_eq!(identity.bundle_id.as_deref(), Some(DEV_BUNDLE));
    }

    #[test]
    fn a_descriptor_for_another_bundle_is_refused() {
        let refusal = resolve_identity(
            Some(Ok(descriptor("alpha", "/tmp/x", "com.synth.desktop.v07.dev.alpha"))),
            &EnvIdentity::default(),
            Some("com.synth.desktop.v07.dev.beta"),
        )
        .unwrap_err();
        assert_eq!(refusal.code(), "descriptor_foreign");
    }

    #[test]
    fn a_dev_bundle_with_neither_descriptor_nor_env_refuses_the_canonical_profile() {
        let refusal = resolve_identity(None, &EnvIdentity::default(), Some(DEV_BUNDLE)).unwrap_err();
        assert_eq!(refusal.code(), "dev_bundle_without_identity");
        assert!(refusal.to_string().contains("canonical"), "{refusal}");

        // An instance name without a data root is still no data root.
        let refusal = resolve_identity(None, &env(Some("alpha"), None, None), Some(DEV_BUNDLE))
            .unwrap_err();
        assert_eq!(refusal.code(), "dev_bundle_without_identity");
    }

    #[test]
    fn the_canonical_bundle_and_a_bare_binary_still_open_the_canonical_profile() {
        for plist in [Some("com.synth.desktop"), None] {
            let identity = resolve_identity(None, &EnvIdentity::default(), plist).unwrap();
            assert_eq!(identity.source, IdentitySource::Canonical);
            assert!(identity.data_root.is_none());
            assert!(identity.instance.is_none());
        }
    }

    #[test]
    fn an_unusable_descriptor_is_a_refusal_not_a_fallback() {
        let refusal = resolve_identity(
            Some(Err("instance.json: expected value".into())),
            &EnvIdentity::default(),
            Some("com.synth.desktop"),
        )
        .unwrap_err();
        assert_eq!(refusal.code(), "descriptor_unusable");

        let refusal = resolve_identity(
            Some(Ok(descriptor("Not Valid", "/tmp/x", DEV_BUNDLE))),
            &EnvIdentity::default(),
            Some(DEV_BUNDLE),
        )
        .unwrap_err();
        assert_eq!(refusal.code(), "descriptor_unusable");
    }

    #[test]
    fn a_descriptor_file_is_read_through_the_shared_contract() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Alpha.app");
        let path = paths::descriptor_path(&bundle);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        assert_eq!(paths::read_descriptor(&path).unwrap(), None);

        fs::write(
            &path,
            serde_json::json!({
                "schemaVersion": DESCRIPTOR_SCHEMA_VERSION,
                "instance_id": "alpha",
                "instance_root": "/tmp/instances/alpha",
                "config_path": "/tmp/instances/alpha/config.toml",
                "data_root": "/tmp/instances/alpha/data",
                "bundle_id": DEV_BUNDLE,
                "release_line": "v07",
                "source_revision": "94a589b7",
                "generated_at": "2026-08-21T00:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        let read = paths::read_descriptor(&path).unwrap().unwrap();
        assert_eq!(read.instance_id, "alpha");
        assert_eq!(read.data_root, PathBuf::from("/tmp/instances/alpha/data"));

        fs::write(&path, r#"{"schemaVersion":"synth.desktop.instance-descriptor.v9","instance_id":"a","data_root":"/x","bundle_id":"b"}"#).unwrap();
        assert!(paths::read_descriptor(&path).is_err());
        fs::write(&path, "{not json").unwrap();
        assert!(paths::read_descriptor(&path).is_err());
    }

    #[test]
    fn the_bundle_identifier_is_read_from_the_info_plist() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Alpha.app");
        fs::create_dir_all(bundle.join("Contents")).unwrap();
        assert_eq!(info_plist_bundle_identifier(&bundle), None);
        fs::write(
            bundle.join("Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\n\
                 <key>CFBundleExecutable</key><string>alpha</string>\n\
                 <key>CFBundleIdentifier</key>\n  <string>{DEV_BUNDLE}</string>\n\
                 </dict></plist>\n"
            ),
        )
        .unwrap();
        assert_eq!(
            info_plist_bundle_identifier(&bundle).as_deref(),
            Some(DEV_BUNDLE)
        );
        assert_eq!(
            bundle_root(&bundle.join("Contents/MacOS/alpha")).as_deref(),
            Some(bundle.as_path())
        );
        assert_eq!(
            bundle_root(&bundle.join("Contents/MacOS/synth-visuals-mcp")).as_deref(),
            Some(bundle.as_path())
        );
    }

    // --- instance lock ----------------------------------------------------------

    fn record(pid: u32, start: &str, epoch: &str) -> InstanceLockRecord {
        InstanceLockRecord {
            schema_version: LOCK_SCHEMA_VERSION.into(),
            pid,
            process_start_identity: start.into(),
            boot_epoch: epoch.into(),
            bundle_id: Some(DEV_BUNDLE.into()),
            exe: "/tmp/Alpha.app/Contents/MacOS/alpha".into(),
            acquired_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn current_record(epoch: &str) -> InstanceLockRecord {
        let process = ProcessIdentity::current();
        record(process.pid, &process.start, epoch)
    }

    #[test]
    fn the_current_process_has_a_start_identity() {
        let identity = ProcessIdentity::current();
        assert_eq!(identity.pid, std::process::id());
        assert!(!identity.start.is_empty());
        assert!(identity.is_still_running());
        assert!(pid_exists(identity.pid));
    }

    #[test]
    fn a_second_acquisition_of_a_live_lock_is_refused_with_the_holder_named() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_path(&dir.path().join("data"));
        let held = acquire_instance_lock_at(&path, current_record("inst_first")).unwrap();
        assert_eq!(held.record.boot_epoch, "inst_first");
        assert_eq!(read_lock_record_at(&path).unwrap().boot_epoch, "inst_first");

        let error = acquire_instance_lock_at(&path, current_record("inst_second")).unwrap_err();
        let LockError::Held { holder: Some(holder), .. } = &error else {
            panic!("expected Held, got {error:?}");
        };
        assert_eq!(holder.pid, std::process::id());
        assert_eq!(holder.boot_epoch, "inst_first");
        let message = error.to_string();
        assert!(
            message.starts_with(&format!(
                "instance_locked pid={} epoch=inst_first",
                std::process::id()
            )),
            "{message}"
        );
        // The loser left the winner's record untouched and wrote no stale file.
        assert_eq!(read_lock_record_at(&path).unwrap().boot_epoch, "inst_first");
        assert!(!stale_lock_path(&path, std::process::id()).exists());

        drop(held);
        acquire_instance_lock_at(&path, current_record("inst_third")).unwrap();
    }

    #[test]
    fn a_record_left_by_a_dead_process_is_quarantined_and_the_lock_is_taken() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_path(&dir.path().join("data"));
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        let dead_pid = child.id();
        child.wait().unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            serde_json::to_vec(&record(dead_pid, "Thu Jan  1 00:00:00 1970", "inst_dead")).unwrap(),
        )
        .unwrap();

        let lock = acquire_instance_lock_at(&path, current_record("inst_live")).unwrap();
        assert_eq!(lock.record.boot_epoch, "inst_live");
        let stale = stale_lock_path(&path, dead_pid);
        assert!(stale.exists(), "{} should hold the quarantined record", stale.display());
        assert_eq!(read_lock_record_at(&stale).unwrap().boot_epoch, "inst_dead");
        assert_eq!(read_lock_record_at(&path).unwrap().boot_epoch, "inst_live");
    }

    #[test]
    fn a_locked_file_whose_holder_is_gone_is_quarantined_and_reacquired() {
        // An inherited descriptor: the flock is held, but the pid in the record
        // is dead. The file must be moved aside so a fresh inode can be locked.
        let dir = tempfile::tempdir().unwrap();
        let path = lock_path(&dir.path().join("data"));
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        let dead_pid = child.id();
        child.wait().unwrap();
        let inherited = acquire_instance_lock_at(&path, record(dead_pid, "gone", "inst_orphan")).unwrap();

        let lock = acquire_instance_lock_at(&path, current_record("inst_live")).unwrap();
        assert_eq!(lock.record.boot_epoch, "inst_live");
        assert_eq!(
            read_lock_record_at(&stale_lock_path(&path, dead_pid))
                .unwrap()
                .boot_epoch,
            "inst_orphan"
        );
        drop(inherited);
    }

    #[test]
    fn a_reused_pid_with_a_different_start_identity_is_stale() {
        // A live pid whose start identity is not the recorded one is a reuse
        // of the number, not the recorded process.
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let reused = ProcessIdentity {
            pid: child.id(),
            start: "Thu Jan  1 00:00:00 1970".into(),
            exe: String::new(),
        };
        assert!(pid_exists(child.id()));
        assert!(!reused.is_still_running());
        let real = ProcessIdentity {
            start: process_start_identity(child.id()).unwrap(),
            ..reused
        };
        assert!(real.is_still_running());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn stale_quarantine_keeps_only_recent_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_path(dir.path());
        fs::create_dir_all(dir.path()).unwrap();
        for pid in 1..=(STALE_LOCKS_KEPT as u32 + 3) {
            fs::write(stale_lock_path(&path, pid), b"{}").unwrap();
        }
        prune_stale_locks(&path);
        let remaining = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(remaining, STALE_LOCKS_KEPT);
    }

    // --- one resolver for the app and every adapter ---------------------------

    /// ID-R-10 lock: the `SYNTH_DESKTOP_DATA_ROOT → ~/Library/Application
    /// Support/Synth Desktop` fallback lives in `instance_paths.rs` and nowhere
    /// else. An adapter that grows its own copy fails here.
    #[test]
    fn every_mcp_adapter_resolves_its_instance_through_the_shared_paths_module() {
        let bin_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin");
        let mut adapters = 0;
        for entry in fs::read_dir(&bin_dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("synth_") && name.ends_with("_mcp.rs")) {
                continue;
            }
            adapters += 1;
            let source = fs::read_to_string(entry.path()).unwrap();
            assert!(
                source.contains("#[path = \"../instance_paths.rs\"]"),
                "{name} must include the shared instance_paths module"
            );
            for forbidden in ["\"Synth Desktop\"", "data_dir()", "SYNTH_DESKTOP_DATA_ROOT"] {
                assert!(
                    !source.contains(forbidden),
                    "{name} carries its own instance fallback ({forbidden}); use instance_paths"
                );
            }
        }
        assert_eq!(adapters, 10, "expected the ten stdio MCP adapters under {}", bin_dir.display());
    }
}
