use serde::Serialize;
use specta::Type;
use std::{env, fs, path::PathBuf};

pub const INSTANCE_ENV: &str = "SYNTH_DESKTOP_INSTANCE";
pub const DATA_ROOT_ENV: &str = "SYNTH_DESKTOP_DATA_ROOT";
pub const MANIFEST_ENV: &str = "SYNTH_DESKTOP_INSTANCE_MANIFEST";
pub const APP_NAME_ENV: &str = "SYNTH_DESKTOP_APP_NAME";

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
    pub process_id: u32,
    pub executable: String,
    pub data_root: String,
    pub vite_url: Option<String>,
    pub manifest: Option<String>,
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
        process_id: std::process::id(),
        executable: env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "unknown".into()),
        data_root: data_root().display().to_string(),
        vite_url: env::var("SYNTH_DESKTOP_VITE_URL").ok(),
        manifest: env::var(MANIFEST_ENV).ok(),
    }
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
    use super::validate_name;

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
}
