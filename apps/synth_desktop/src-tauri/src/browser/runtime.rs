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
}

pub fn policy_path() -> PathBuf {
    crate::storage::app_data_root().join("browser/policy.json")
}

pub fn profile_root() -> PathBuf {
    crate::storage::app_data_root().join("browser-profiles")
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

pub fn runtime_status() -> BrowserRuntimeStatus {
    let backend = backend_script_path();
    let backend_present = backend.is_file();
    let node = env::var_os("SYNTH_BROWSER_NODE").unwrap_or_else(|| "node".into());
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
        let probe = Command::new(&node)
            .args([
                "--input-type=module",
                "-e",
                "import fs from 'node:fs'; import { chromium } from 'playwright'; process.stdout.write(JSON.stringify({chromium: fs.existsSync(chromium.executablePath())}));",
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
        default_local_origins: LOCAL_ORIGINS
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
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

