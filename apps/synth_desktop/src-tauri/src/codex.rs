use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{oneshot, Mutex, RwLock},
};

const EVENT_NAME: &str = "codex:event";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionStartRequest {
    pub session_id: String,
    pub workspace: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub provider_name: Option<String>,
    pub provider_title: Option<String>,
    pub provider_env_key: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox: Option<String>,
    pub thread_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurnStartRequest {
    pub session_id: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionInfo {
    pub session_id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionRecord {
    pub session_id: String,
    pub thread_id: String,
    pub workspace: String,
    pub model: String,
    pub provider_name: String,
    pub provider_title: String,
    pub base_url: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexEvent {
    session_id: String,
    method: String,
    params: Value,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

struct AppServer {
    child: Mutex<Child>,
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    pending: Pending,
    next_id: AtomicU64,
}

impl AppServer {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if let Err(error) = write_message(
            &self.stdin,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )
        .await
        {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) => Err(anyhow!("codex app-server {method} error: {error}")),
            Ok(Err(_)) => Err(anyhow!("codex app-server stopped while handling {method}")),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(anyhow!("codex app-server timed out waiting for {method}"))
            }
        }
    }

    async fn notify(&self, method: &str) -> Result<()> {
        write_message(&self.stdin, &json!({"jsonrpc":"2.0","method":method})).await
    }

    async fn stop(&self) -> Result<()> {
        self.child
            .lock()
            .await
            .kill()
            .await
            .context("stop app-server")
    }
}

struct Session {
    server: Arc<AppServer>,
    thread_id: String,
    turn_id: RwLock<Option<String>>,
    model: String,
    approval_policy: String,
}

pub struct CodexManager {
    sessions: RwLock<HashMap<String, Arc<Session>>>,
    records: Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    state_path: PathBuf,
}

impl CodexManager {
    pub fn new() -> Self {
        let state_path = codex_root().join("threads.json");
        let records = fs::read_to_string(&state_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self {
            sessions: RwLock::new(HashMap::new()),
            records: Arc::new(RwLock::new(records)),
            state_path,
        }
    }

    pub async fn start(
        &self,
        app: AppHandle,
        request: CodexSessionStartRequest,
    ) -> Result<CodexSessionInfo> {
        validate_start(&request)?;
        if let Some(existing) = self.sessions.read().await.get(&request.session_id) {
            return Ok(session_info(&request.session_id, existing).await);
        }
        let home = codex_root()
            .join("homes")
            .join(safe_component(&request.session_id));
        ensure_home(&home, &request)?;
        let server = spawn_server(
            app,
            &request.session_id,
            &home,
            &request,
            self.records.clone(),
            self.state_path.clone(),
        )
        .await?;
        server
            .request(
                "initialize",
                json!({"clientInfo":{"name":"synth-desktop","title":"Synth Desktop","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}),
            )
            .await?;
        server.notify("initialized").await?;
        let remembered = self
            .records
            .read()
            .await
            .get(&request.session_id)
            .map(|record| record.thread_id.clone());
        let requested_thread = request.thread_id.clone().or(remembered);
        let method = if requested_thread.is_some() {
            "thread/resume"
        } else {
            "thread/start"
        };
        let mut params = json!({
            "model": request.model,
            "cwd": request.workspace,
            "approvalPolicy": request.approval_policy.as_deref().unwrap_or("never"),
            "sandbox": request.sandbox.as_deref().unwrap_or("workspace-write")
        });
        if let Some(thread_id) = requested_thread {
            params["threadId"] = Value::String(thread_id);
        }
        let result = server.request(method, params).await?;
        let thread_id = nested_id(&result, "threadId")
            .ok_or_else(|| anyhow!("Codex {method} response missing thread id: {result}"))?;
        let session = Arc::new(Session {
            server,
            thread_id: thread_id.clone(),
            turn_id: RwLock::new(None),
            model: request.model,
            approval_policy: request.approval_policy.unwrap_or_else(|| "never".into()),
        });
        self.sessions
            .write()
            .await
            .insert(request.session_id.clone(), session.clone());
        self.records.write().await.insert(
            request.session_id.clone(),
            CodexSessionRecord {
                session_id: request.session_id.clone(),
                thread_id,
                workspace: request.workspace,
                model: session.model.clone(),
                provider_name: request.provider_name.unwrap_or_else(|| "custom".into()),
                provider_title: request
                    .provider_title
                    .unwrap_or_else(|| "Synth Responses Provider".into()),
                base_url: request.base_url.trim_end_matches('/').to_owned(),
                status: "ready".into(),
            },
        );
        self.persist_records().await?;
        Ok(session_info(&request.session_id, &session).await)
    }

    pub async fn start_turn(&self, request: CodexTurnStartRequest) -> Result<CodexSessionInfo> {
        if request.prompt.trim().is_empty() {
            return Err(anyhow!("prompt must not be empty"));
        }
        let session = self.session(&request.session_id).await?;
        let result = session
            .server
            .request(
                "turn/start",
                json!({
                    "threadId": session.thread_id,
                    "model": session.model,
                    "input":[{"type":"text","text":request.prompt,"textElements":[]}],
                    "approvalPolicy": session.approval_policy
                }),
            )
            .await?;
        let turn_id = nested_id(&result, "turnId")
            .ok_or_else(|| anyhow!("Codex turn/start response missing turn id: {result}"))?;
        *session.turn_id.write().await = Some(turn_id);
        self.set_status(&request.session_id, "running").await?;
        Ok(session_info(&request.session_id, &session).await)
    }

    pub async fn interrupt(&self, session_id: &str) -> Result<()> {
        let session = self.session(session_id).await?;
        let turn_id = session
            .turn_id
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("session has no active turn"))?;
        session
            .server
            .request(
                "turn/interrupt",
                json!({"threadId":session.thread_id,"turnId":turn_id}),
            )
            .await?;
        Ok(())
    }

    pub async fn close(&self, session_id: &str) -> Result<()> {
        let session = self.sessions.write().await.remove(session_id);
        if let Some(session) = session {
            session.server.stop().await?;
        }
        self.set_status(session_id, "closed").await?;
        Ok(())
    }

    pub async fn list(&self) -> Vec<CodexSessionRecord> {
        let mut records: Vec<_> = self.records.read().await.values().cloned().collect();
        records.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        records
    }

    async fn session(&self, id: &str) -> Result<Arc<Session>> {
        self.sessions
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("Codex session not started: {id}"))
    }

    async fn set_status(&self, session_id: &str, status: &str) -> Result<()> {
        if let Some(record) = self.records.write().await.get_mut(session_id) {
            record.status = status.into();
        }
        self.persist_records().await
    }

    async fn persist_records(&self) -> Result<()> {
        persist_records(&self.records, &self.state_path).await
    }
}

async fn spawn_server(
    app: AppHandle,
    session_id: &str,
    home: &Path,
    request: &CodexSessionStartRequest,
    records: Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    state_path: PathBuf,
) -> Result<Arc<AppServer>> {
    let binary = env::var("SYNTH_CODEX_BIN").unwrap_or_else(|_| "codex".into());
    let env_key = request
        .provider_env_key
        .as_deref()
        .unwrap_or("SYNTH_LAGUNA_API_KEY");
    let mut command = Command::new(binary);
    command
        .args(["app-server", "--listen", "stdio://"])
        .current_dir(&request.workspace)
        .env("CODEX_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if !request.api_key.is_empty() {
        command.env(env_key, &request.api_key);
    }
    let mut child = command.spawn().context("spawn codex app-server")?;
    let stdin = Arc::new(Mutex::new(
        child.stdin.take().context("capture app-server stdin")?,
    ));
    let stdout = child.stdout.take().context("capture app-server stdout")?;
    let stderr = child.stderr.take().context("capture app-server stderr")?;
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let server = Arc::new(AppServer {
        child: Mutex::new(child),
        stdin: stdin.clone(),
        pending: pending.clone(),
        next_id: AtomicU64::new(1),
    });
    let sid = session_id.to_owned();
    tauri::async_runtime::spawn(read_stdout(
        app.clone(),
        sid.clone(),
        stdout,
        stdin,
        pending,
        request
            .approval_policy
            .clone()
            .unwrap_or_else(|| "never".into()),
        PersistenceContext {
            records,
            state_path,
        },
    ));
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = app.emit(
                EVENT_NAME,
                CodexEvent {
                    session_id: sid.clone(),
                    method: "app-server/stderr".into(),
                    params: json!({"line":line}),
                },
            );
        }
    });
    Ok(server)
}

struct PersistenceContext {
    records: Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    state_path: PathBuf,
}

async fn read_stdout(
    app: AppHandle,
    session_id: String,
    stdout: tokio::process::ChildStdout,
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    pending: Pending,
    approval_policy: String,
    persistence: PersistenceContext,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if message.get("method").is_none() {
            if let Some(id) = message.get("id").and_then(Value::as_u64) {
                if let Some(sender) = pending.lock().await.remove(&id) {
                    let response = match message.get("error") {
                        Some(error) => Err(error.to_string()),
                        None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let _ = sender.send(response);
                }
            }
            continue;
        }
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let _ = app.emit(
            EVENT_NAME,
            CodexEvent {
                session_id: session_id.clone(),
                method: method.clone(),
                params: params.clone(),
            },
        );
        if matches!(
            method.as_str(),
            "turn/completed" | "turn/failed" | "turn/interrupted"
        ) {
            let status = match method.as_str() {
                "turn/completed" => "ready",
                "turn/failed" => "failed",
                _ => "interrupted",
            };
            if let Some(record) = persistence.records.write().await.get_mut(&session_id) {
                record.status = status.into();
            }
            let _ = persist_records(&persistence.records, &persistence.state_path).await;
        }
        if let Some(id) = message.get("id").cloned() {
            if is_approval_method(&method) && approval_policy != "never" {
                let _ = app.emit(
                    EVENT_NAME,
                    CodexEvent {
                        session_id: session_id.clone(),
                        method: "approval/pending".into(),
                        params: json!({"requestMethod":method,"request":params}),
                    },
                );
            }
            let response = approval_response(&method, &params, id);
            let _ = write_message(&stdin, &response).await;
        }
    }
    let mut pending = pending.lock().await;
    for (_, sender) in pending.drain() {
        let _ = sender.send(Err("codex app-server stdout closed".into()));
    }
}

fn approval_response(method: &str, params: &Value, id: Value) -> Value {
    if is_approval_method(method) {
        let available = params.get("availableDecisions").and_then(Value::as_array);
        let decision = available
            .and_then(|values| {
                ["decline", "reject", "deny", "cancel"]
                    .iter()
                    .find(|candidate| values.iter().any(|value| value.as_str() == Some(candidate)))
            })
            .copied();
        if decision.is_none() {
            return json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"No supported approval decision"}});
        }
        return json!({"jsonrpc":"2.0","id":id,"result":{"decision":decision}});
    }
    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("Unsupported server request: {method}")}})
}

fn is_approval_method(method: &str) -> bool {
    matches!(
        method,
        "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "commandExecution/requestApproval"
            | "applyPatch/requestApproval"
            | "fileChange/requestApproval"
            | "permissions/request"
            | "execCommandApproval"
    )
}

async fn write_message(
    stdin: &Arc<Mutex<tokio::process::ChildStdin>>,
    value: &Value,
) -> Result<()> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    let mut stdin = stdin.lock().await;
    stdin.write_all(&encoded).await?;
    stdin.flush().await?;
    Ok(())
}

fn ensure_home(home: &Path, request: &CodexSessionStartRequest) -> Result<()> {
    fs::create_dir_all(home.join("sessions"))?;
    let provider = request.provider_name.as_deref().unwrap_or("custom");
    let title = request
        .provider_title
        .as_deref()
        .unwrap_or("Synth Responses Provider");
    let env_key = request
        .provider_env_key
        .as_deref()
        .unwrap_or("SYNTH_LAGUNA_API_KEY");
    let config = format!(
        "model = \"{}\"\nmodel_provider = \"{}\"\napproval_policy = \"{}\"\nsandbox_mode = \"{}\"\nservice_tier = \"default\"\n\n[model_providers.{}]\nname = \"{}\"\nbase_url = \"{}\"\nenv_key = \"{}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\n\n[features]\ntool_call_mcp_elicitation = true\nshell_tool = true\nunified_exec = true\n",
        toml_string(&request.model), toml_string(provider), toml_string(request.approval_policy.as_deref().unwrap_or("never")), toml_string(request.sandbox.as_deref().unwrap_or("workspace-write")), toml_key(provider), toml_string(title), toml_string(request.base_url.trim_end_matches('/')), toml_string(env_key)
    );
    fs::write(home.join("config.toml"), config)?;
    let auth = home.join("auth.json");
    if !auth.exists() {
        fs::write(
            auth,
            "{\n  \"OPENAI_API_KEY\": \"synth-desktop-provider\"\n}\n",
        )?;
    }
    Ok(())
}

fn validate_start(request: &CodexSessionStartRequest) -> Result<()> {
    if request.session_id.trim().is_empty() || request.model.trim().is_empty() {
        return Err(anyhow!("sessionId and model are required"));
    }
    if !Path::new(&request.workspace).is_dir() {
        return Err(anyhow!("workspace must be an existing directory"));
    }
    if !(request.base_url.starts_with("http://127.0.0.1:")
        || request.base_url.starts_with("http://localhost:")
        || request.base_url.starts_with("https://"))
    {
        return Err(anyhow!("baseUrl must be local HTTP or HTTPS"));
    }
    Ok(())
}

fn nested_id(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            payload
                .get(key.trim_end_matches("Id"))
                .and_then(|nested| nested.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

async fn session_info(id: &str, session: &Session) -> CodexSessionInfo {
    CodexSessionInfo {
        session_id: id.into(),
        thread_id: session.thread_id.clone(),
        turn_id: session.turn_id.read().await.clone(),
    }
}

fn codex_root() -> PathBuf {
    env::var_os("SYNTH_CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".synth-desktop/codex")
        })
}

async fn persist_records(
    records: &Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    state_path: &Path,
) -> Result<()> {
    if let Some(parent) = state_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let data = serde_json::to_vec_pretty(&*records.read().await)?;
    let temporary = state_path.with_extension("json.tmp");
    tokio::fs::write(&temporary, data).await?;
    tokio::fs::rename(temporary, state_path).await?;
    Ok(())
}
fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}
fn toml_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
fn toml_key(value: &str) -> String {
    format!("\"{}\"", toml_string(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_flat_and_nested_ids() {
        assert_eq!(
            nested_id(&json!({"threadId":"a"}), "threadId").as_deref(),
            Some("a")
        );
        assert_eq!(
            nested_id(&json!({"thread":{"id":"b"}}), "threadId").as_deref(),
            Some("b")
        );
    }
    #[test]
    fn approvals_always_fail_closed() {
        let denied = approval_response(
            "permissions/request",
            &json!({"availableDecisions":["decline","acceptForSession"]}),
            json!(7),
        );
        assert_eq!(
            denied.pointer("/result/decision").and_then(Value::as_str),
            Some("decline")
        );
        let rejected = approval_response(
            "permissions/request",
            &json!({"availableDecisions":["accept","acceptForSession"]}),
            json!(8),
        );
        assert_eq!(
            rejected.pointer("/error/code").and_then(Value::as_i64),
            Some(-32602)
        );
    }
    #[test]
    fn sanitizes_session_home_component() {
        assert_eq!(safe_component("a/b c"), "a_b_c");
    }
    #[test]
    fn escapes_toml_values() {
        assert_eq!(toml_string("a\"b\\c\n"), "a\\\"b\\\\c\\n");
    }
}
