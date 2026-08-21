//! Sidecar-owned `synth-mlx-rl` child: probe, start, dispatch HTTP.
//!
//! `OptimizerService` never calls this. Training recipes talk to the sidecar
//! proxy; the proxy is the only Workshop code allowed to dial MLX loopback.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::sleep;

const MLX_DEFAULT_URL: &str = "http://127.0.0.1:8787";
const TRAINING_MODEL_ID: &str = "Qwen/Qwen3.5-0.8B";
const HEALTH_TRIES: u32 = 40;
const HEALTH_WAIT: Duration = Duration::from_millis(250);

static SUPERVISOR: OnceLock<Mutex<MlxSupervisor>> = OnceLock::new();

struct MlxSupervisor {
    base_url: Option<String>,
    child: Option<Child>,
}

#[derive(Clone)]
pub struct MlxLoopback {
    pub base_url: String,
    http: reqwest::Client,
}

impl MlxLoopback {
    fn client(base_url: String) -> Result<Self> {
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
        })
    }

    pub fn from_env() -> Result<Self> {
        let url = supervisor()
            .lock()
            .ok()
            .and_then(|guard| guard.base_url.clone())
            .unwrap_or_else(configured_mlx_url);
        Self::client(url)
    }

    /// Probe an already-running service, or start `synth-mlx-rl serve` as a
    /// sidecar child when the operator did not pin a URL.
    pub async fn ensure() -> Result<Self> {
        if let Some(url) = probe_url(&configured_mlx_url()).await {
            remember_url(&url);
            return Self::client(url);
        }
        if std::env::var("SYNTH_MLX_RL_URL").is_ok() {
            bail!("SYNTH_MLX_RL_URL is set but synth-mlx-rl is not reachable");
        }
        start_child().await
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        decode_mlx(
            self.http
                .get(format!("{}{path}", self.base_url))
                .send()
                .await?,
            path,
        )
        .await
    }

    pub async fn post(&self, path: &str, body: Option<&Value>) -> Result<Value> {
        self.post_timed(path, body, Duration::from_secs(10)).await
    }

    async fn post_timed(&self, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Value> {
        let mut request = self
            .http
            .post(format!("{}{path}", self.base_url))
            .timeout(timeout);
        if let Some(value) = body {
            request = request.json(value);
        }
        decode_mlx(request.send().await?, path).await
    }

    pub async fn chat(&self, message: &str, policy_snapshot_id: &str) -> Result<String> {
        let snapshot_id = policy_snapshot_id.trim();
        if snapshot_id.is_empty() {
            bail!("inference requires a policy_snapshot_id; ambient latest is refused");
        }
        let response = self
            .post_timed(
                "/v1/chat/completions",
                Some(&json!({
                    "messages": [{"role": "user", "content": message}],
                    "policy_snapshot_id": snapshot_id,
                })),
                Duration::from_secs(120),
            )
            .await?;
        if let Some(error) = response.pointer("/error/message").and_then(Value::as_str) {
            bail!("MLX refused the pinned adapter: {error}");
        }
        response
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                anyhow::anyhow!("MLX chat completion omitted choices[0].message.content")
            })
    }

    pub async fn register_policy(
        &self,
        policy_dir: &Path,
        snapshot_id: &str,
        artifact_digest: Option<&str>,
    ) -> Result<String> {
        let mut body = json!({
            "policy_dir": policy_dir,
            "snapshot_id": snapshot_id,
        });
        if let Some(digest) = artifact_digest {
            body["artifact_digest"] = json!(digest);
        }
        let response = self
            .post_timed("/v1/synth/policies/register", Some(&body), Duration::from_secs(120))
            .await?;
        response
            .get("policy_snapshot_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                anyhow::anyhow!("MLX policy registration omitted policy_snapshot_id")
            })
    }
}

pub fn configured_mlx_url() -> String {
    std::env::var("SYNTH_MLX_RL_URL").unwrap_or_else(|_| MLX_DEFAULT_URL.into())
}

pub fn mlx_serve_command(port: u16, root: &Path) -> Result<Command> {
    let model_path = require_training_model()?;
    mlx_serve_command_with_model(port, root, &model_path)
}

fn mlx_serve_command_with_model(port: u16, root: &Path, model_path: &Path) -> Result<Command> {
    let mut command = Command::new(mlx_bin()?);
    command
        .arg("serve")
        .args(["--host", "127.0.0.1"])
        .args(["--port", &port.to_string()])
        .args(["--root", &root.display().to_string()])
        .args(["--model", &model_path.display().to_string()]);
    command.env("SYNTH_MLX_RL_MODEL_PATH", model_path);
    command.env("HF_HUB_OFFLINE", "1");
    command.kill_on_drop(true);
    Ok(command)
}

pub fn training_model_path() -> PathBuf {
    crate::training_models::training_models_root().join(TRAINING_MODEL_ID)
}

pub fn require_training_model() -> Result<PathBuf> {
    let path = training_model_path();
    if !path.is_dir() {
        bail!(
            "download the training model in Settings → Models → On-device training ({TRAINING_MODEL_ID})"
        );
    }
    Ok(path)
}

fn mlx_bin() -> Result<PathBuf> {
    if let Ok(raw) = std::env::var("SYNTH_MLX_RL_BIN") {
        let path = PathBuf::from(raw.trim());
        if path.as_os_str().is_empty() {
            bail!("SYNTH_MLX_RL_BIN is empty");
        }
        return Ok(path);
    }
    if let Ok(root) = std::env::var("SYNTH_MLX_RL_ROOT") {
        for candidate in [
            PathBuf::from(root.trim()).join(".venv/bin/synth-mlx-rl"),
            PathBuf::from(root.trim()).join(".venv/Scripts/synth-mlx-rl.exe"),
        ] {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        bail!(
            "SYNTH_MLX_RL_ROOT {} has no prepared .venv/bin/synth-mlx-rl",
            root
        );
    }
    Ok(PathBuf::from("synth-mlx-rl"))
}

fn supervisor() -> &'static Mutex<MlxSupervisor> {
    SUPERVISOR.get_or_init(|| {
        Mutex::new(MlxSupervisor {
            base_url: None,
            child: None,
        })
    })
}

fn remember_url(url: &str) {
    if let Ok(mut guard) = supervisor().lock() {
        guard.base_url = Some(url.to_string());
    }
}

async fn start_child() -> Result<MlxLoopback> {
    {
        let cached = supervisor()
            .lock()
            .ok()
            .and_then(|guard| guard.base_url.clone());
        if let Some(url) = cached {
            if probe_url(&url).await.is_some() {
                return MlxLoopback::client(url);
            }
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("bind a loopback port for synth-mlx-rl")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let root = crate::instance::data_root().join("optimizers/mlx-rl");
    std::fs::create_dir_all(&root).context("create synth-mlx-rl artifact root")?;
    let mut command = mlx_serve_command(port, &root)?;
    let child = command
        .spawn()
        .context("start synth-mlx-rl as an Optimizers sidecar child")?;
    let url = format!("http://127.0.0.1:{port}");
    if let Ok(mut guard) = supervisor().lock() {
        guard.child = Some(child);
        guard.base_url = Some(url.clone());
    }
    for _ in 0..HEALTH_TRIES {
        if probe_url(&url).await.is_some() {
            return MlxLoopback::client(url);
        }
        sleep(HEALTH_WAIT).await;
    }
    bail!("sidecar started synth-mlx-rl at {url} but /v1/capabilities never answered");
}

async fn probe_url(raw: &str) -> Option<String> {
    let url = match reqwest::Url::parse(raw.trim()) {
        Ok(url) => url,
        Err(_) => return None,
    };
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if url.scheme() != "http" || !local {
        return None;
    }
    let base = url.as_str().trim_end_matches('/').to_string();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .ok()?;
    let response = client
        .get(format!("{base}/v1/capabilities"))
        .send()
        .await
        .ok()?;
    response.status().is_success().then_some(base)
}

async fn decode_mlx(response: reqwest::Response, operation: &str) -> Result<Value> {
    let status = response.status();
    let text = response.text().await.context("read MLX response")?;
    if !status.is_success() {
        bail!(
            "MLX service {operation} failed with {status}: {}",
            text.trim()
        );
    }
    serde_json::from_str(&text).with_context(|| format!("decode MLX response for {operation}"))
}

#[cfg(test)]
pub fn reset_supervisor_for_tests() {
    if let Ok(mut guard) = supervisor().lock() {
        guard.base_url = None;
        guard.child = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_command_uses_the_pinned_bin_and_loopback_port() {
        let model_path = Path::new("/tmp/managed-training-model");
        std::env::set_var("SYNTH_MLX_RL_BIN", "/tmp/synth-mlx-rl-test-bin");
        let command =
            mlx_serve_command_with_model(9123, Path::new("/tmp/mlx-root"), model_path).unwrap();
        let std_cmd = command.as_std();
        assert_eq!(
            std_cmd.get_program().to_string_lossy(),
            "/tmp/synth-mlx-rl-test-bin"
        );
        let args: Vec<String> = std_cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "serve",
                "--host",
                "127.0.0.1",
                "--port",
                "9123",
                "--root",
                "/tmp/mlx-root",
                "--model",
                "/tmp/managed-training-model"
            ]
        );
        assert_eq!(
            std_cmd
                .get_envs()
                .find(|(key, _)| *key == "SYNTH_MLX_RL_MODEL_PATH")
                .and_then(|(_, value)| value)
                .unwrap(),
            model_path.as_os_str()
        );
        assert_eq!(
            std_cmd
                .get_envs()
                .find(|(key, _)| *key == "HF_HUB_OFFLINE")
                .and_then(|(_, value)| value)
                .unwrap(),
            "1"
        );
        std::env::remove_var("SYNTH_MLX_RL_BIN");
    }

    #[test]
    fn chat_refuses_an_empty_policy_pin() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = MlxLoopback {
            base_url: "http://127.0.0.1:9".into(),
            http: reqwest::Client::new(),
        };
        let error = runtime
            .block_on(client.chat("hello", "  "))
            .unwrap_err()
            .to_string();
        assert!(error.contains("policy_snapshot_id"), "{error}");
        assert!(error.contains("ambient latest"), "{error}");
    }

    #[test]
    fn missing_training_model_preflight_points_to_settings() {
        let path = training_model_path();
        if path.exists() {
            return;
        }
        let error = require_training_model().unwrap_err().to_string();
        assert!(error.contains("Settings → Models → On-device training"));
    }

    #[test]
    fn training_model_path_shares_the_instance_scoped_catalog_root() {
        let isolated = std::env::temp_dir().join(format!(
            "synth-desktop-mlx-model-root-{}",
            std::process::id()
        ));
        let previous = std::env::var_os("SYNTH_DESKTOP_DATA_ROOT");
        std::env::set_var("SYNTH_DESKTOP_DATA_ROOT", &isolated);
        assert_eq!(
            training_model_path(),
            crate::training_models::training_models_root().join(TRAINING_MODEL_ID)
        );
        assert!(
            training_model_path().starts_with(&isolated),
            "training model path {} escaped instance root {}",
            training_model_path().display(),
            isolated.display()
        );
        match previous {
            Some(value) => std::env::set_var("SYNTH_DESKTOP_DATA_ROOT", value),
            None => std::env::remove_var("SYNTH_DESKTOP_DATA_ROOT"),
        }
    }

    #[test]
    fn managed_model_path_is_the_offline_serve_argument() {
        let previous = std::env::var_os("SYNTH_MLX_RL_BIN");
        std::env::set_var("SYNTH_MLX_RL_BIN", "/usr/bin/true");
        let model = Path::new("/managed/Qwen/Qwen3.5-0.8B");
        let command = mlx_serve_command_with_model(57855, Path::new("/jobs"), model).unwrap();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args.windows(2)
                .find(|pair| pair[0] == "--model")
                .map(|pair| pair[1].as_str()),
            Some("/managed/Qwen/Qwen3.5-0.8B")
        );
        match previous {
            Some(value) => std::env::set_var("SYNTH_MLX_RL_BIN", value),
            None => std::env::remove_var("SYNTH_MLX_RL_BIN"),
        }
    }
}
