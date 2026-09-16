//! Workshop-owned lifecycle for the pinned public Synth Swarms SDK bridge.
//!
//! Cloud wire contracts stay in `synth-ai`; this module only owns child
//! lifecycle, credential custody, bounded output, and Tauri event forwarding.

use crate::contract::specta::OpaqueJson;
use crate::error::AppError;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use uuid::Uuid;

const MAX_ONESHOT_BYTES: usize = 4 * 1024 * 1024;
pub const EVENT_CHANNEL: &str = "cloud:swarm-event";

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SwarmSdkPayload {
    pub operation: String,
    #[specta(type = specta_typescript::Unknown)]
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SwarmWatchRequest {
    pub subscription_id: Option<String>,
    pub swarm_id: String,
    pub last_event_id: Option<String>,
    pub transcript_cursor: Option<String>,
    pub timeout_seconds: Option<f64>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SwarmWatchStarted {
    pub subscription_id: String,
    pub event_channel: String,
}

#[derive(Debug, Default)]
pub struct SwarmsSdkManager {
    watches: Mutex<HashMap<String, JoinHandle<()>>>,
}

fn allowed_operation(value: &str) -> bool {
    matches!(
        value,
        "capabilities"
            | "preflight"
            | "create"
            | "list"
            | "retrieve"
            | "configuration"
            | "status"
            | "activity"
            | "usage"
            | "evidence"
            | "pause"
            | "resume"
            | "cancel"
    )
}

fn bridge_binary() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SYNTH_SWARMS_SDK_BIN").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        bail!("configured Synth Swarms SDK bridge is unavailable");
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            let packaged = parent.join("synth-swarms-sdk");
            if packaged.is_file() {
                return Ok(packaged);
            }
        }
    }
    let development = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../runtime-distributions/trace/bin/synth-swarms-sdk");
    if development.is_file() {
        return Ok(development);
    }
    bail!("Synth Swarms SDK runtime is not installed")
}

fn bridge_input(payload: Value) -> Result<Vec<u8>> {
    let config = crate::synth_config::resolve().context("resolve Synth Cloud configuration")?;
    let api_key = config
        .api_key
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("Sign in to use Synth Cloud Swarms"))?;
    serde_json::to_vec(&json!({
        "baseUrl": config.backend_url,
        "apiKey": api_key,
        "payload": payload,
    }))
    .context("encode Synth Swarms SDK request")
}

fn spawn_bridge(operation: &str, streaming: bool) -> Result<Child> {
    let mut command = Command::new(bridge_binary()?);
    command
        .arg(operation)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // A long-lived SSE reader must never deadlock because an inherited
        // Python logger filled an unread stderr pipe. One-shot calls retain a
        // bounded stderr buffer for diagnostics.
        .stderr(if streaming {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .kill_on_drop(true);
    command.spawn().context("start Synth Swarms SDK bridge")
}

async fn invoke(operation: &str, payload: Value) -> Result<Value> {
    if !allowed_operation(operation) {
        bail!("unsupported Synth Swarms SDK operation");
    }
    let input = bridge_input(payload)?;
    let mut child = spawn_bridge(operation, false)?;
    child
        .stdin
        .take()
        .context("open SDK bridge stdin")?
        .write_all(&input)
        .await
        .context("write SDK bridge request")?;
    let output = child
        .wait_with_output()
        .await
        .context("wait for SDK bridge")?;
    if output.stdout.len() > MAX_ONESHOT_BYTES || output.stderr.len() > 64 * 1024 {
        bail!("Synth Swarms SDK response exceeded its bounded output limit");
    }
    let line = std::str::from_utf8(&output.stdout)
        .context("SDK bridge returned non-UTF-8 output")?
        .lines()
        .find(|line| !line.trim().is_empty())
        .context("SDK bridge returned no response")?;
    let envelope: Value = serde_json::from_str(line).context("decode SDK bridge response")?;
    if !output.status.success() || envelope.get("ok") != Some(&Value::Bool(true)) {
        let message = envelope
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Synth Swarms SDK request failed");
        bail!("{}", message.chars().take(600).collect::<String>());
    }
    Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
}

#[tauri::command]
#[specta::specta]
pub async fn cloud_swarms_sdk_call(request: SwarmSdkPayload) -> Result<OpaqueJson, AppError> {
    invoke(&request.operation, request.payload)
        .await
        .map(OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn cloud_swarms_watch_start(
    app: AppHandle,
    manager: State<'_, Arc<SwarmsSdkManager>>,
    request: SwarmWatchRequest,
) -> Result<SwarmWatchStarted, AppError> {
    let swarm_id = request.swarm_id.trim().to_owned();
    if swarm_id.is_empty() {
        return Err(AppError::invalid_argument("swarmId is required"));
    }
    let subscription_id = request
        .subscription_id
        .filter(|value| {
            !value.trim().is_empty()
                && value.len() <= 96
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .unwrap_or_else(|| format!("swarm-watch-{}", Uuid::new_v4().simple()));
    let input = bridge_input(json!({
        "swarmId": swarm_id,
        "lastEventId": request.last_event_id,
        "transcriptCursor": request.transcript_cursor,
        "timeoutSeconds": request.timeout_seconds,
    }))
    .map_err(AppError::from)?;
    let mut child = spawn_bridge("watch", true).map_err(AppError::from)?;
    child
        .stdin
        .take()
        .ok_or_else(|| AppError::untyped("Synth Swarms SDK bridge stdin unavailable"))?
        .write_all(&input)
        .await
        .map_err(AppError::from)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::untyped("Synth Swarms SDK bridge stdout unavailable"))?;
    let id_for_task = subscription_id.clone();
    let handle = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.len() > 1024 * 1024 {
                let _ = app.emit(EVENT_CHANNEL, json!({"subscriptionId": id_for_task, "kind": "error", "message": "Swarm event exceeded 1 MiB"}));
                break;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(value) if value.get("ok") == Some(&Value::Bool(true)) => {
                    let _ = app.emit(EVENT_CHANNEL, json!({"subscriptionId": id_for_task, "kind": "event", "event": value.get("event")}));
                }
                Ok(value) => {
                    let _ = app.emit(EVENT_CHANNEL, json!({"subscriptionId": id_for_task, "kind": "error", "message": value.get("message")}));
                }
                Err(_) => {
                    let _ = app.emit(EVENT_CHANNEL, json!({"subscriptionId": id_for_task, "kind": "error", "message": "Invalid SDK event envelope"}));
                    break;
                }
            }
        }
        let status = child.wait().await.ok().and_then(|value| value.code());
        let _ = app.emit(
            EVENT_CHANNEL,
            json!({"subscriptionId": id_for_task, "kind": "closed", "exitCode": status}),
        );
    });
    manager
        .watches
        .lock()
        .await
        .insert(subscription_id.clone(), handle);
    Ok(SwarmWatchStarted {
        subscription_id,
        event_channel: EVENT_CHANNEL.into(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn cloud_swarms_watch_stop(
    manager: State<'_, Arc<SwarmsSdkManager>>,
    subscription_id: String,
) -> Result<bool, AppError> {
    if let Some(handle) = manager.watches.lock().await.remove(&subscription_id) {
        handle.abort();
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_bridge_operations_are_refused() {
        assert!(allowed_operation("preflight"));
        assert!(allowed_operation("cancel"));
        assert!(!allowed_operation("raw_http"));
        assert!(!allowed_operation("watch"));
    }
}
