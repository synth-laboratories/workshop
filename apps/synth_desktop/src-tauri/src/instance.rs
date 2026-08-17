use serde::Serialize;
use sha2::{Digest, Sha256};
use specta::Type;
use std::{env, fs, io, path::PathBuf, sync::OnceLock};

pub const INSTANCE_ENV: &str = "SYNTH_DESKTOP_INSTANCE";
pub const DATA_ROOT_ENV: &str = "SYNTH_DESKTOP_DATA_ROOT";
pub const MANIFEST_ENV: &str = "SYNTH_DESKTOP_INSTANCE_MANIFEST";
pub const APP_NAME_ENV: &str = "SYNTH_DESKTOP_APP_NAME";
pub const BUNDLE_ID_ENV: &str = "SYNTH_DESKTOP_BUNDLE_ID";

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

pub fn name() -> Option<String> {
    env::var(INSTANCE_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| validate_name(value))
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

/// Product-owned durable data. Named development instances always set the
/// override; the unset path preserves the canonical installed-app location.
pub fn data_root() -> PathBuf {
    env::var_os(DATA_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Synth Desktop")
        })
}

/// User configuration, secrets, Codex homes, and default workspaces. For an
/// isolated instance this intentionally collapses into its private data root.
pub fn state_root() -> PathBuf {
    env::var_os(DATA_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".synth-desktop"))
}

pub fn display_name() -> String {
    if let Ok(value) = env::var(APP_NAME_ENV) {
        let value = value.trim();
        if !value.is_empty() {
            return value.to_owned();
        }
    }
    name()
        .map(|value| format!("Synth Desktop · {value}"))
        .unwrap_or_else(|| "Synth Desktop".into())
}

pub fn bundle_id() -> Option<String> {
    env::var(BUNDLE_ID_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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
        manifest: env::var(MANIFEST_ENV).ok(),
    }
}

fn executable_digest() -> Option<String> {
    manifest_executable_digest().or_else(current_executable_digest)
}

fn manifest_executable_digest() -> Option<String> {
    let path = env::var_os(MANIFEST_ENV).map(PathBuf::from)?;
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
    let Some(path) = env::var_os(MANIFEST_ENV).map(PathBuf::from) else {
        return;
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let diagnostics = diagnostics();
    manifest["runtime"] = serde_json::json!({
        "status": "running",
        "pid": diagnostics.process_id,
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

#[cfg(test)]
mod tests {
    use super::{sha256_digest, valid_sha256_digest, validate_name};

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
}
