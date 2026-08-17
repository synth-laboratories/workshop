use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const LOCAL_ORIGINS: [&str; 2] = ["http://localhost", "http://127.0.0.1"];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPolicy {
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub upload_roots: Vec<String>,
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
    pub upload_roots: Vec<String>,
    pub service_running: bool,
    pub crash_count: u32,
    pub chrome_claim_enabled: bool,
}

pub fn policy_path() -> PathBuf {
    crate::storage::app_data_root().join("browser/policy.json")
}

pub fn profile_root() -> PathBuf {
    crate::storage::app_data_root().join("browser-profiles")
}

pub fn bundled_runtime_root() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let root = exe.parent()?.join("../Resources/browser/runtime");
    root.join("manifest.json").is_file().then_some(root)
}

pub fn browser_node_path() -> PathBuf {
    if let Some(configured) = env::var_os("SYNTH_BROWSER_NODE") {
        return PathBuf::from(configured);
    }
    bundled_runtime_root()
        .map(|root| root.join("node/bin/node"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("node"))
}

pub fn backend_script_path() -> PathBuf {
    if let Some(configured) = env::var_os("SYNTH_BROWSER_BACKEND_SCRIPT") {
        return PathBuf::from(configured);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(macos) = exe.parent() {
            let bundled = macos.join("../Resources/browser/playwright_backend.mjs");
            if bundled.is_file() {
                return bundled;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../browser/playwright_backend.mjs")
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

pub fn allow_upload_root(value: &str) -> Result<BrowserPolicy> {
    let path = fs::canonicalize(value).with_context(|| format!("resolve upload folder {value}"))?;
    if !path.is_dir() {
        return Err(anyhow!("upload root must be a directory"));
    }
    let value = path.display().to_string();
    let mut policy = load_policy()?;
    if !policy.upload_roots.contains(&value) {
        policy.upload_roots.push(value);
        policy.upload_roots.sort();
    }
    save_policy(&policy)?;
    Ok(policy)
}

pub fn revoke_upload_root(value: &str) -> Result<BrowserPolicy> {
    let mut policy = load_policy()?;
    policy.upload_roots.retain(|candidate| candidate != value);
    save_policy(&policy)?;
    Ok(policy)
}

pub fn runtime_status() -> BrowserRuntimeStatus {
    let backend = backend_script_path();
    let backend_present = backend.is_file();
    let node = browser_node_path();
    let node_probe = Command::new(&node).arg("--version").output();
    let node_present = node_probe
        .as_ref()
        .is_ok_and(|output| output.status.success());
    let node_version = node_probe.ok().and_then(|output| {
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    });
    let mut playwright_present = false;
    let mut chromium_present = false;
    if node_present && backend_present {
        let runtime_root = bundled_runtime_root();
        let mut command = Command::new(&node);
        if let Some(root) = runtime_root.as_ref() {
            command
                .env("SYNTH_BROWSER_RUNTIME_ROOT", root)
                .env("PLAYWRIGHT_BROWSERS_PATH", root.join("browsers"));
        }
        let probe = command
            .args([
                "--input-type=module",
                "-e",
                "import fs from 'node:fs'; import path from 'node:path'; import { createRequire } from 'node:module'; const require=createRequire(import.meta.url); const base=process.env.SYNTH_BROWSER_RUNTIME_ROOT; const {chromium}=require(base ? path.join(base,'node_modules/playwright') : 'playwright'); process.stdout.write(JSON.stringify({chromium: fs.existsSync(chromium.executablePath())}));",
            ])
            .current_dir(backend.parent().unwrap_or_else(|| Path::new(".")))
            .output();
        if let Ok(output) = probe {
            playwright_present = output.status.success();
            chromium_present = serde_json::from_slice::<serde_json::Value>(&output.stdout)
                .ok()
                .and_then(|value| value.get("chromium").and_then(serde_json::Value::as_bool))
                .unwrap_or(false);
        }
    }
    let ready = backend_present && node_present && playwright_present && chromium_present;
    let detail = if ready {
        "Playwright and Chromium are available. The current backend opens a separate managed browser window.".to_owned()
    } else if !backend_present {
        "The Workshop Browser backend resource is missing from this build.".to_owned()
    } else if !node_present {
        "Node.js is not available. Production builds must ship a pinned, signed runtime.".to_owned()
    } else if !playwright_present {
        "The Playwright package is unavailable to the managed browser backend.".to_owned()
    } else {
        "Playwright is installed, but its pinned Chromium executable is missing.".to_owned()
    };
    let policy = load_policy().unwrap_or_default();
    BrowserRuntimeStatus {
        phase: if ready { "ready" } else { "not_ready" }.to_owned(),
        detail,
        backend_present,
        node_present,
        playwright_present,
        chromium_present,
        node_version,
        backend_path: backend.display().to_string(),
        profile_root: profile_root().display().to_string(),
        allowed_origins: policy.allowed_origins,
        upload_roots: policy.upload_roots,
        default_local_origins: LOCAL_ORIGINS
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        service_running: false,
        crash_count: 0,
        chrome_claim_enabled: env::var("SYNTH_BROWSER_ENABLE_CHROME_CLAIM").as_deref() == Ok("1"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_reduced_to_explicit_http_origins() {
        assert_eq!(
            normalize_origin("https://example.com").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            normalize_origin("http://localhost:4173").unwrap(),
            "http://localhost:4173"
        );
        assert!(normalize_origin("https://example.com/path").is_err());
        assert!(normalize_origin("file:///tmp/page.html").is_err());
        assert!(normalize_origin("https://user@example.com").is_err());
    }
}
