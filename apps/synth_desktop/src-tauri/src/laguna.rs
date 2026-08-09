use anyhow::{Context, Result};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, RwLock};

const DEFAULT_PORT: u16 = 7333;
const DEFAULT_MODEL: &str = "poolside/Laguna-XS-2.1-NVFP4-mlx";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LagunaStatus {
    pub phase: String,
    pub base_url: Option<String>,
    pub backend: Option<String>,
    pub loaded_model: Option<String>,
    pub detail: Option<String>,
    pub memory_bytes: Option<u64>,
    pub updated_at: u64,
}

pub struct LagunaManager {
    status: RwLock<LagunaStatus>,
    updates: broadcast::Sender<LagunaStatus>,
    client: Client,
}

impl LagunaManager {
    pub fn new() -> Self {
        let (updates, _) = broadcast::channel(32);
        Self {
            status: RwLock::new(LagunaStatus {
                phase: "unknown".into(),
                base_url: None,
                backend: None,
                loaded_model: None,
                detail: None,
                memory_bytes: None,
                updated_at: now_ms(),
            }),
            updates,
            client: Client::new(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LagunaStatus> {
        self.updates.subscribe()
    }
    pub async fn status(&self) -> LagunaStatus {
        self.status.read().await.clone()
    }

    pub async fn set_status(&self, mut status: LagunaStatus) {
        status.updated_at = now_ms();
        *self.status.write().await = status.clone();
        let _ = self.updates.send(status);
    }

    pub async fn set_error(&self, detail: String) {
        let mut status = self.status().await;
        status.phase = "error".into();
        status.detail = Some(detail);
        self.set_status(status).await;
    }

    pub fn api_key(&self) -> Option<String> {
        env::var("SYNTH_LAGUNA_API_KEY").ok()
    }

    pub async fn ensure(&self, workshop_root: &Path) -> Result<Option<String>> {
        if env::var("SYNTH_LAGUNA_AUTO_START").as_deref() == Ok("0") {
            let mut status = self.status().await;
            status.phase = "unavailable".into();
            status.detail = Some("Auto-start disabled (SYNTH_LAGUNA_AUTO_START=0)".into());
            self.set_status(status).await;
            return Ok(env::var("SYNTH_LAGUNA_BASE_URL").ok().map(trim_url));
        }

        let api_key = ensure_api_key()?;
        let base_url = trim_url(
            env::var("SYNTH_LAGUNA_BASE_URL")
                .unwrap_or_else(|_| format!("http://127.0.0.1:{DEFAULT_PORT}")),
        );
        self.set_status(LagunaStatus {
            phase: "starting".into(),
            base_url: Some(base_url.clone()),
            backend: None,
            loaded_model: None,
            detail: Some("Checking Laguna XS…".into()),
            memory_bytes: None,
            updated_at: now_ms(),
        })
        .await;

        if let Some(status) = self.probe(&base_url, &api_key).await {
            if status.phase == "ready" {
                self.set_status(status).await;
                write_env_sh(&api_key, &base_url)?;
                return Ok(Some(base_url));
            }
        }

        let upstream = discover_poolside_upstream(&self.client).await;
        let backend = if upstream.is_some() {
            "external"
        } else if cfg!(target_os = "macos") {
            "mlx_lm"
        } else {
            "auto"
        };
        let mut status = self.status().await;
        status.phase = "loading".into();
        status.backend = Some(backend.into());
        status.detail = Some(
            if upstream.is_some() {
                "Connecting to local Laguna engine…"
            } else {
                "Starting Laguna sidecar…"
            }
            .into(),
        );
        self.set_status(status).await;
        write_env_sh(&api_key, &base_url)?;
        spawn_sidecar(workshop_root, &api_key, backend, upstream.as_ref())?;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
        while tokio::time::Instant::now() < deadline {
            if let Some(status) = self.probe(&base_url, &api_key).await {
                let done = status.phase == "ready" || status.phase == "error";
                self.set_status(status.clone()).await;
                if done {
                    return Ok((status.phase == "ready").then_some(base_url));
                }
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        self.set_error(format!("Timed out waiting for Laguna at {base_url}"))
            .await;
        Ok(None)
    }

    async fn probe(&self, base_url: &str, api_key: &str) -> Option<LagunaStatus> {
        let response = self
            .client
            .get(format!("{base_url}/health"))
            .bearer_auth(api_key)
            .timeout(Duration::from_millis(1200))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body: Value = response.json().await.ok()?;
        let raw = body.get("status").and_then(Value::as_str).unwrap_or("");
        let phase = match raw {
            "ok" | "ready" => "ready",
            "loading" => "loading",
            "error" | "unloaded" => "error",
            _ => "starting",
        };
        Some(LagunaStatus {
            phase: phase.into(),
            base_url: Some(base_url.into()),
            backend: body
                .get("backend")
                .and_then(Value::as_str)
                .map(str::to_owned),
            loaded_model: body
                .get("loadedModel")
                .and_then(Value::as_str)
                .map(str::to_owned),
            detail: Some(if phase == "ready" {
                "Laguna XS ready".into()
            } else {
                format!("sidecar {phase}")
            }),
            memory_bytes: body.get("memoryBytes").and_then(Value::as_u64),
            updated_at: now_ms(),
        })
    }
}

fn home() -> PathBuf {
    env::var_os("SYNTH_LAGUNA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".synth-desktop/laguna")
        })
}
fn models_dir() -> PathBuf {
    if let Some(path) = env::var_os("SYNTH_LAGUNA_MODELS_DIR") {
        return path.into();
    }
    let poolside = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/poolside/models");
    if poolside.join("poolside/Laguna-XS-2.1-NVFP4-mlx").exists() {
        poolside
    } else {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".synth-desktop/models")
    }
}
fn trim_url(value: String) -> String {
    value.trim_end_matches('/').to_owned()
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ensure_api_key() -> Result<String> {
    if let Ok(key) = env::var("SYNTH_LAGUNA_API_KEY") {
        if !key.trim().is_empty() {
            return Ok(key.trim().into());
        }
    }
    fs::create_dir_all(home())?;
    let path = home().join("api_key");
    if let Ok(key) = fs::read_to_string(&path) {
        if !key.trim().is_empty() {
            let key = key.trim().to_owned();
            env::set_var("SYNTH_LAGUNA_API_KEY", &key);
            return Ok(key);
        }
    }
    let mut bytes = [0u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    let key = format!(
        "synth-local-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    write_secret(&path, &key)?;
    env::set_var("SYNTH_LAGUNA_API_KEY", &key);
    Ok(key)
}

fn write_secret(path: &Path, value: &str) -> Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    writeln!(options.open(path)?, "{value}")?;
    Ok(())
}

fn write_env_sh(api_key: &str, base_url: &str) -> Result<()> {
    fs::create_dir_all(home())?;
    let body = format!("export SYNTH_LAGUNA_HOST=\"127.0.0.1\"\nexport SYNTH_LAGUNA_BASE_URL=\"{base_url}\"\nexport SYNTH_LAGUNA_API_KEY=\"{api_key}\"\nexport SYNTH_LAGUNA_BACKEND=\"{}\"\nexport SYNTH_LAGUNA_DEFAULT_MODEL=\"{DEFAULT_MODEL}\"\nexport SYNTH_LAGUNA_MODELS_DIR=\"{}\"\nexport SYNTH_LAGUNA_AUTO_LOAD=\"1\"\nexport PATH=\"$HOME/.synth-desktop/laguna/.venv/bin:$PATH\"\n", env::var("SYNTH_LAGUNA_BACKEND").unwrap_or_else(|_| "auto".into()), models_dir().display());
    fs::write(home().join("env.sh"), body)?;
    Ok(())
}

#[derive(Clone)]
struct Upstream {
    url: String,
    api_key: String,
}

async fn discover_poolside_upstream(client: &Client) -> Option<Upstream> {
    let process_key = Command::new("ps")
        .args(["-axo", "command="])
        .output()
        .ok()
        .and_then(|o| discover_key(&String::from_utf8_lossy(&o.stdout)));
    let saved = fs::read_to_string(home().join("poolside_sidecar_api_key"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let api_key = env::var("SYNTH_LAGUNA_UPSTREAM_API_KEY")
        .ok()
        .or_else(|| env::var("SYNTH_LAGUNA_EXTERNAL_API_KEY").ok())
        .or(saved)
        .or(process_key)?;
    let mut candidates = vec![
        "http://127.0.0.1:63300".into(),
        "http://127.0.0.1:49600".into(),
    ];
    if let Ok(url) = env::var("SYNTH_LAGUNA_EXTERNAL_URL") {
        candidates.insert(0, trim_url(url));
    }
    for url in candidates {
        let healthy = client
            .get(format!("{url}/health"))
            .bearer_auth(&api_key)
            .timeout(Duration::from_millis(1200))
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false);
        if healthy {
            let _ = fs::create_dir_all(home());
            let _ = write_secret(&home().join("poolside_sidecar_api_key"), &api_key);
            return Some(Upstream { url, api_key });
        }
    }
    None
}

fn discover_key(output: &str) -> Option<String> {
    for line in output.lines().filter(|line| line.contains("poolside-mlx")) {
        let parts: Vec<_> = line.split_whitespace().collect();
        for (index, part) in parts.iter().enumerate() {
            if *part == "--api-key" {
                if let Some(value) = parts.get(index + 1) {
                    return Some((*value).into());
                }
            }
            if let Some(value) = part.strip_prefix("--api-key=") {
                return Some(value.into());
            }
        }
    }
    None
}

fn spawn_sidecar(
    root: &Path,
    api_key: &str,
    backend: &str,
    upstream: Option<&Upstream>,
) -> Result<()> {
    fs::create_dir_all(home())?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home().join("desktop-sidecar.log"))?;
    let python = {
        let candidate = home().join(".venv/bin/python");
        if candidate.exists() {
            candidate
        } else {
            PathBuf::from(env::var("SYNTH_PYTHON").unwrap_or_else(|_| "python3".into()))
        }
    };
    validate_python(&python)?;
    let daemon = root.join("services/laguna-daemon");
    let mut command = Command::new(python);
    command
        .args(["-m", "laguna_daemon"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    command.env(
        "PYTHONPATH",
        append_path(&daemon, env::var("PYTHONPATH").ok()),
    );
    command.envs([
        ("SYNTH_LAGUNA_HOST", "127.0.0.1"),
        ("SYNTH_LAGUNA_PORT", "7333"),
        ("SYNTH_LAGUNA_API_KEY", api_key),
        ("SYNTH_LAGUNA_BACKEND", backend),
        ("SYNTH_LAGUNA_DEFAULT_MODEL", DEFAULT_MODEL),
        ("SYNTH_LAGUNA_AUTO_LOAD", "1"),
        ("SYNTH_LAGUNA_REQUIRE_AUTH", "1"),
    ]);
    command
        .env("SYNTH_LAGUNA_MODELS_DIR", models_dir())
        .env("SYNTH_LAGUNA_DATA_DIR", home());
    if let Some(value) = upstream {
        command
            .env("SYNTH_LAGUNA_EXTERNAL_URL", &value.url)
            .env("SYNTH_LAGUNA_UPSTREAM_API_KEY", &value.api_key);
    }
    detach(&mut command);
    command.spawn().context("spawn Laguna sidecar")?;
    Ok(())
}

fn validate_python(python: &Path) -> Result<()> {
    let status = Command::new(python)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| {
            format!(
                "Laguna requires a Python environment, but `{}` could not be started. Create ~/.synth-desktop/laguna/.venv with the Laguna dependencies, or set SYNTH_PYTHON to a usable interpreter.",
                python.display()
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Laguna Python interpreter `{}` exited unsuccessfully",
            python.display()
        ))
    }
}

fn append_path(prefix: &Path, existing: Option<String>) -> String {
    existing
        .map(|v| format!("{}:{v}", prefix.display()))
        .unwrap_or_else(|| prefix.display().to_string())
}
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            libc_detach();
            Ok(())
        });
    }
}
#[cfg(not(unix))]
fn detach(_: &mut Command) {}
#[cfg(unix)]
fn libc_detach() {
    unsafe {
        extern "C" {
            fn setsid() -> i32;
        }
        let _ = setsid();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_poolside_key_forms() {
        assert_eq!(
            discover_key("/x/poolside-mlx --api-key secret"),
            Some("secret".into())
        );
        assert_eq!(
            discover_key("poolside-mlx-sidecar --api-key=token"),
            Some("token".into())
        );
    }
    #[test]
    fn trims_only_trailing_slashes() {
        assert_eq!(
            trim_url("http://127.0.0.1:7333///".into()),
            "http://127.0.0.1:7333"
        );
    }
}
