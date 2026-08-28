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

#[derive(Clone, Debug, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredInstance {
    pub name: String,
    pub display_name: String,
    pub release_line: Option<String>,
    pub bundle_id: String,
    pub app_bundle: String,
    pub status: String,
    pub current: bool,
    pub deep_link: String,
}

#[derive(Clone, Debug, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopDeepLink {
    pub instance: Option<String>,
    pub view: String,
    pub run_id: Option<String>,
}

fn safe_registry_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Read launcher-owned manifests without trusting them as executable input.
/// Only a minimal, validated switcher projection crosses the UI boundary.
pub fn registered_instances_from(root: &Path) -> io::Result<Vec<RegisteredInstance>> {
    let mut instances = Vec::new();
    let current_name = name();
    let current_bundle = bundle_id();
    let Ok(releases) = fs::read_dir(root) else {
        return Ok(instances);
    };
    for release in releases.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(entries) = fs::read_dir(release.path()) else {
            continue;
        };
        for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
            let manifest_path = entry.path().join("instance.json");
            let Ok(bytes) = fs::read(&manifest_path) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            if value
                .get("schemaVersion")
                .and_then(serde_json::Value::as_str)
                != Some("synth.desktop-instance.v1")
            {
                continue;
            }
            let Some(instance_name) = value.get("name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(bundle_id) = value.get("bundleId").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if !safe_registry_component(instance_name) || !safe_registry_component(bundle_id) {
                continue;
            }
            let instance_root = value
                .get("instanceRoot")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(|| entry.path());
            if instance_root != entry.path() {
                continue;
            }
            let Some(app_bundle) = value
                .get("appBundle")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
            else {
                continue;
            };
            if !app_bundle.starts_with(&instance_root)
                || app_bundle.extension().and_then(|ext| ext.to_str()) != Some("app")
            {
                continue;
            }
            let status = value
                .pointer("/runtime/status")
                .and_then(serde_json::Value::as_str)
                .filter(|status| matches!(*status, "running" | "stopped" | "starting" | "failed"))
                .unwrap_or("unknown")
                .to_string();
            let current = current_name.as_deref() == Some(instance_name)
                || current_bundle.as_deref() == Some(bundle_id);
            instances.push(RegisteredInstance {
                name: instance_name.to_string(),
                display_name: value
                    .get("displayName")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(instance_name)
                    .chars()
                    .take(160)
                    .collect(),
                release_line: value
                    .get("releaseLine")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| value.chars().take(32).collect()),
                bundle_id: bundle_id.to_string(),
                app_bundle: app_bundle.display().to_string(),
                status,
                current,
                deep_link: format!("synth-workshop://open?instance={instance_name}"),
            });
        }
    }
    instances.sort_by(|left, right| {
        right
            .current
            .cmp(&left.current)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    Ok(instances)
}

pub fn registered_instances() -> io::Result<Vec<RegisteredInstance>> {
    let root = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".synth-desktop/instances");
    registered_instances_from(&root)
}

fn decode_deep_link_component(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let raw = value.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        match raw[index] {
            b'%' if index + 2 < raw.len() => {
                let hex = std::str::from_utf8(&raw[index + 1..index + 3]).ok()?;
                bytes.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            b'+' => {
                bytes.push(b' ');
                index += 1;
            }
            byte => {
                bytes.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(bytes).ok()
}

pub fn parse_workshop_deep_link(raw: &str) -> Result<WorkshopDeepLink, String> {
    let query = raw
        .strip_prefix("synth-workshop://open")
        .ok_or_else(|| "unsupported Workshop deep link".to_string())?
        .strip_prefix('?')
        .unwrap_or_default();
    let mut instance = None;
    let mut view = "landing".to_string();
    let mut run_id = None;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode_deep_link_component(value)
            .ok_or_else(|| "invalid percent encoding in Workshop deep link".to_string())?;
        match key {
            "instance" if safe_registry_component(&value) => instance = Some(value),
            "view"
                if matches!(
                    value.as_str(),
                    "landing" | "optimizers" | "experiments" | "visuals"
                ) =>
            {
                view = value
            }
            "runId" if safe_registry_component(&value) => run_id = Some(value),
            "instance" | "view" | "runId" => {
                return Err(format!("invalid {key} in Workshop deep link"))
            }
            _ => {}
        }
    }
    if run_id.is_some() {
        view = "optimizers".into();
    }
    Ok(WorkshopDeepLink {
        instance,
        view,
        run_id,
    })
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
    /// The bundle descriptor and compiled binary name different source trees.
    BuildRevisionMismatch { descriptor: String, binary: String },
}

impl IdentityRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Mismatch { .. } => "identity_mismatch",
            Self::ForeignDescriptor { .. } => "descriptor_foreign",
            Self::EnvBundleMismatch { .. } => "bundle_id_mismatch",
            Self::DevBundleWithoutIdentity { .. } => "dev_bundle_without_identity",
            Self::UnusableDescriptor { .. } => "descriptor_unusable",
            Self::BuildRevisionMismatch { .. } => "build_revision_mismatch",
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
            Self::BuildRevisionMismatch { descriptor, binary } => write!(
                f,
                "the bundle descriptor names source revision {descriptor:?}, but the compiled binary names {binary:?}; rebuild the instance before launching it"
            ),
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
    match identity().and_then(|identity| verify_build_provenance(identity, compiled_revision())) {
        Ok(identity) => {
            crate::platform::logging::report("instance", "eprintln", format!(
                "synth-desktop: instance identity source={:?} instance={} data_root={} bundle_id={}",
                identity.source,
                identity.instance.as_deref().unwrap_or("canonical"),
                identity
                    .data_root
                    .as_deref()
                    .unwrap_or(&paths::canonical_data_root())
                    .display(),
                identity.bundle_id.as_deref().unwrap_or("-"),
            ));
            Ok(identity)
        }
        Err(refusal) => {
            crate::platform::logging::report(
                "instance",
                "eprintln",
                format!(
                    "synth-desktop: identity_refused code={} {refusal}",
                    refusal.code()
                ),
            );
            show_refusal_dialog("Synth Workshop cannot start", &refusal.to_string());
            Err(refusal)
        }
    }
}

fn compiled_revision() -> &'static str {
    option_env!("SYNTH_BUILD_REVISION").unwrap_or("unknown")
}

/// A packaged instance carries the launcher's source revision in its bundle.
/// Compare that immutable receipt to the value compiled into the executable so
/// a cached or copied binary cannot start under a newer manifest.
fn verify_build_provenance(
    identity: Identity,
    build_revision: &str,
) -> Result<Identity, IdentityRefusal> {
    let descriptor_revision = identity
        .descriptor
        .as_ref()
        .and_then(|descriptor| descriptor.source_revision.as_deref())
        .filter(|revision| !revision.trim().is_empty());
    if let Some(descriptor_revision) = descriptor_revision {
        if descriptor_revision != build_revision {
            return Err(IdentityRefusal::BuildRevisionMismatch {
                descriptor: descriptor_revision.to_owned(),
                binary: build_revision.to_owned(),
            });
        }
    }
    Ok(identity)
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
    let build_revision = compiled_revision();
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
            crate::platform::logging::report(
                "instance",
                "eprintln",
                format!("synth-desktop: {error}"),
            );
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

