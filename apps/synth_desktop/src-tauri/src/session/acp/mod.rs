//! ACP v1 host adapter. Session/run truth and human consent remain owned by
//! Workshop's existing services. Agent protocol details stay in journal payloads.
pub mod commands;
pub mod config;
mod transport;

use crate::{
    core_runtime::CoreRuntime,
    domain::{RunCreate, RunStatus, RuntimeTarget, SessionCreate, SessionKind, SessionStatus},
    session::approval::{
        ApprovalBroker, ApprovalDecision, ApprovalKind, ApprovalOrigin, HostDecisionResolver,
    },
    storage::{EventAppend, EventSource},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::Mutex;
use transport::Peer;

#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartRequest {
    pub backend_id: String,
    pub title: String,
    pub parent_session_id: Option<String>,
}

struct Attachment {
    peer: Arc<Peer>,
    backend: config::Backend,
    remote_id: String,
    generation: String,
    active_run: Mutex<Option<String>>,
    cancelling: std::sync::atomic::AtomicBool,
    permissions: Arc<tokio::sync::Semaphore>,
}

pub struct Manager {
    core: Arc<CoreRuntime>,
    app: tauri::AppHandle,
    approvals: Arc<ApprovalBroker>,
    attachments: Mutex<HashMap<String, Arc<Attachment>>>,
    lifecycle: Mutex<()>,
}

impl Manager {
    pub fn new(
        core: Arc<CoreRuntime>,
        app: tauri::AppHandle,
        approvals: Arc<ApprovalBroker>,
    ) -> Self {
        Self {
            core,
            app,
            approvals,
            attachments: Mutex::new(HashMap::new()),
            lifecycle: Mutex::new(()),
        }
    }

    /// Reap only a retained process whose kernel start identity still matches.
    /// No remote turn is replayed after a crash; resumption is explicit.
    pub fn reconcile(&self) -> Result<()> {
        self.core.storage().database().with_conn(|conn| {
            let leases: Vec<(u32, String)> = {
                let mut statement = conn.prepare("SELECT process_id, process_start FROM agent_attachments WHERE process_id IS NOT NULL AND process_start IS NOT NULL")?;
                let rows = statement.query_map([], |row| Ok((row.get(0)?,row.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
                rows
            };
            for (pid, start) in leases {
                if pid > 1 && pid != std::process::id() && crate::instance::process_start_identity(pid).as_deref() == Some(start.as_str()) {
                    #[cfg(unix)] unsafe { if libc::getpgid(pid as i32) == pid as i32 && libc::getpgrp() != pid as i32 { libc::kill(-(pid as i32), libc::SIGKILL); } }
                }
            }
            conn.execute("UPDATE agent_attachments SET process_id=NULL, process_start=NULL", [])?;
            Ok(())
        })
    }

    pub fn backends(&self) -> Result<Vec<config::Backend>> {
        config::read(
            self.core
                .storage()
                .database()
                .path()
                .parent()
                .context("missing instance root")?,
        )
    }

    async fn event(&self, session_id: &str, kind: &str, payload: Value) -> Result<()> {
        let mut append = EventAppend::system(kind, payload);
        append.session_id = Some(session_id.to_owned());
        append.source = EventSource::Acp;
        self.core.append_and_emit(&self.app, append).await?;
        Ok(())
    }

    async fn status(&self, id: &str, status: SessionStatus) -> Result<()> {
        let mutation = self
            .core
            .sessions()
            .transition(id.into(), status, EventSource::Acp, json!({}))
            .await?;
        self.core.broadcast_committed(mutation.event);
        Ok(())
    }

    pub async fn start(self: &Arc<Self>, request: StartRequest) -> Result<Value> {
        let _lock = self.lifecycle.lock().await;
        anyhow::ensure!(
            !request.title.trim().is_empty() && request.title.len() <= 512,
            "title must be 1–512 bytes"
        );
        let backend = self
            .backends()?
            .into_iter()
            .find(|backend| backend.id == request.backend_id)
            .context("backend is not registered; configure it locally first")?;
        anyhow::ensure!(
            self.attachments.lock().await.len() < 16,
            "instance concurrent agent limit reached"
        );
        let count = self
            .attachments
            .lock()
            .await
            .values()
            .filter(|entry| entry.backend.id == backend.id)
            .count();
        anyhow::ensure!(
            count < backend.max_sessions as usize,
            "backend concurrent session limit reached"
        );
        let depth = if let Some(parent) = &request.parent_session_id {
            anyhow::ensure!(
                self.attachments.lock().await.contains_key(parent),
                "parent must be attached"
            );
            let (depth, workspace): (u32, String) = self
                .core
                .storage()
                .database()
                .with_conn(|conn| {
                    Ok(conn.query_row(
                        "SELECT depth, workspace FROM agent_attachments WHERE session_id = ?1",
                        [parent],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )?)
                })
                .context("parent is not a retained ACP session")?;
            anyhow::ensure!(
                backend.workspace.starts_with(PathBuf::from(workspace)),
                "child workspace must be inside the parent workspace"
            );
            anyhow::ensure!(depth < 3, "agent delegation depth limit reached");
            depth + 1
        } else {
            0
        };
        let id = format!("session_acp_{}", uuid::Uuid::new_v4().simple());
        let mutation = self
            .core
            .sessions()
            .create_or_update(SessionCreate {
                id: id.clone(),
                title: request.title,
                kind: SessionKind::Acp,
                target: RuntimeTarget::AgentRuntime {
                    backend_id: backend.id.clone(),
                },
                project_id: None,
                remote_id: None,
                codex_thread_id: None,
                status: SessionStatus::Created,
                state_generation: None,
                metadata: json!({}),
                source: EventSource::Acp,
            })
            .await?;
        self.core.broadcast_committed(mutation.event);
        let result = self
            .attach(&id, backend, request.parent_session_id, depth, None)
            .await;
        if result.is_err() {
            self.status(&id, SessionStatus::Failed).await?;
        }
        result
    }

    async fn attach(
        self: &Arc<Self>,
        id: &str,
        backend: config::Backend,
        parent: Option<String>,
        depth: u32,
        resume: Option<String>,
    ) -> Result<Value> {
        let mut command = tokio::process::Command::new(&backend.command);
        command
            .args(&backend.args)
            .current_dir(&backend.workspace)
            .env_clear();
        for key in [
            "PATH",
            "HOME",
            "USER",
            "LANG",
            "LC_ALL",
            "TMPDIR",
            "SystemRoot",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        if let Some(path) = &backend.env_file {
            anyhow::ensure!(
                std::fs::metadata(path)?.len() <= 1024 * 1024,
                "project env file is too large"
            );
            let values = crate::secrets::parse_dotenv(&std::fs::read_to_string(path)?)?;
            command.envs(values);
        }
        let (peer, mut events) = Peer::spawn(command)?;
        let pid = peer
            .process_id()
            .await
            .context("ACP child exited during startup")?;
        let start = crate::instance::process_start_identity(pid)
            .context("cannot establish ACP child identity")?;
        if let Err(error) = self.core.storage().database().with_conn(|conn| {
            conn.execute("INSERT INTO agent_attachments (session_id, backend_id, workspace, generation, parent_session_id, depth, process_id, process_start) VALUES (?1,?2,?3,'starting',?4,?5,?6,?7) ON CONFLICT(session_id) DO UPDATE SET process_id=excluded.process_id, process_start=excluded.process_start",
                rusqlite::params![id,backend.id,backend.workspace.to_string_lossy(),parent,depth,pid,start])?;
            Ok(())
        }) { peer.stop().await?; return Err(error); }
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
        let startup_peer = peer.clone();
        let startup_core = self.core.clone();
        let startup_app = self.app.clone();
        let startup_id = id.to_owned();
        let startup_pump = tokio::spawn(async move {
            tokio::pin!(ready_rx);
            loop {
                tokio::select! {
                    _ = &mut ready_rx => return events,
                    message = events.recv() => match message {
                        Some(envelope) if envelope.message.get("id").is_some() => {
                            let message = &envelope.message;
                            let reply = if message["method"] == "session/request_permission" {
                                json!({"jsonrpc":"2.0","id":message["id"],"result":{"outcome":{"outcome":"cancelled"}}})
                            } else {json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32601,"message":"Client capability not supported"}})};
                            let _ = startup_peer.write(reply).await;
                        },
                        Some(envelope) if envelope.message["method"] == "session/update" => {
                            let message = &envelope.message;
                            let mut event = EventAppend::system("agent.update", message["params"]["update"].clone());
                            event.session_id = Some(startup_id.clone()); event.source = EventSource::Acp;
                            let _ = startup_core.append_and_emit(&startup_app, event).await;
                        },
                        Some(_) => {}, None => return events,
                    }
                }
            }
        });
        let generation = uuid::Uuid::new_v4().to_string();
        let startup = async {
            let initialized = peer.request("initialize", json!({"protocolVersion":1,
                "clientInfo":{"name":"workshop","version":env!("CARGO_PKG_VERSION")},
                "clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false}}), Duration::from_secs(15)).await?;
            anyhow::ensure!(initialized["protocolVersion"] == 1, "backend does not support Workshop's ACP v1 transport");
            if resume.is_some() { anyhow::ensure!(initialized.pointer("/agentCapabilities/loadSession") == Some(&Value::Bool(true)), "backend does not support session/load"); }
            let executable = std::env::current_exe()?.parent().context("missing runtime directory")?.join(if cfg!(windows) {"workshop.exe"} else {"workshop"});
            anyhow::ensure!(executable.is_file(), "packaged Workshop MCP bridge is missing");
            let root = self.core.storage().database().path().parent().context("missing data root")?.to_path_buf();
            let mut params = json!({"cwd":backend.workspace,"mcpServers":[{"name":"workshop","command":executable,
                "args":["mcp","--data-root",root],"env":[]}]});
            let method = if let Some(remote) = &resume { params["sessionId"] = json!(remote); "session/load" } else { "session/new" };
            let created = peer.request(method, params, Duration::from_secs(25)).await?;
            let remote_id = resume.or_else(|| created["sessionId"].as_str().map(str::to_owned)).context("backend returned no session ID")?;
            anyhow::ensure!(!remote_id.is_empty() && remote_id.len() <= 512, "invalid ACP session ID");
            let capabilities = initialized["agentCapabilities"].clone();
            self.core.storage().database().with_conn(|conn| {
                conn.execute("INSERT INTO agent_attachments (session_id, backend_id, backend_session_id, workspace, generation, parent_session_id, depth, capabilities_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(session_id) DO UPDATE SET backend_session_id=excluded.backend_session_id, generation=excluded.generation, capabilities_json=excluded.capabilities_json",
                    rusqlite::params![id, backend.id, remote_id, backend.workspace.to_string_lossy(), generation, parent, depth, capabilities.to_string()])?;
                Ok(())
            })?;
            self.status(id, SessionStatus::Ready).await?;
            Ok::<_, anyhow::Error>((remote_id, capabilities))
        }.await;
        let _ = ready_tx.send(());
        let mut events = startup_pump.await?;
        let (remote_id, capabilities) = match startup {
            Ok(result) => result,
            Err(error) => {
                peer.stop().await?;
                return Err(error);
            }
        };
        let attachment = Arc::new(Attachment {
            peer,
            backend,
            remote_id: remote_id.clone(),
            generation,
            active_run: Mutex::new(None),
            cancelling: std::sync::atomic::AtomicBool::new(false),
            permissions: Arc::new(tokio::sync::Semaphore::new(8)),
        });
        self.attachments
            .lock()
            .await
            .insert(id.into(), attachment.clone());
        let manager = Arc::downgrade(self);
        let session_id = id.to_owned();
        tokio::spawn(async move {
            while let Some(envelope) = events.recv().await {
                let message = envelope.message.clone();
                let Some(manager) = manager.upgrade() else {
                    break;
                };
                if message.get("method").and_then(Value::as_str)
                    == Some("session/request_permission")
                    && message.get("id").is_some()
                {
                    let manager = Arc::downgrade(&manager);
                    let attachment = attachment.clone();
                    let session_id = session_id.clone();
                    tokio::spawn(async move {
                        Self::permission(manager, attachment, session_id, message).await;
                    });
                } else if message["method"] == "session/update"
                    && message.pointer("/params/sessionId").and_then(Value::as_str)
                        == Some(&attachment.remote_id)
                {
                    if manager
                        .event(
                            &session_id,
                            "agent.update",
                            message["params"]["update"].clone(),
                        )
                        .await
                        .is_err()
                    {
                        let _ = attachment.peer.stop().await;
                        break;
                    }
                } else if message.get("id").is_some() {
                    let _ = attachment.peer.write(json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32601,"message":"Client capability not supported"}})).await;
                }
            }
            if let Some(manager) = manager.upgrade() {
                let _lock = manager.lifecycle.lock().await;
                if manager.current(&session_id, &attachment).await {
                    let _ = manager
                        .approvals
                        .expire_origin(
                            &manager.app,
                            &ApprovalOrigin {
                                session_id: session_id.clone(),
                                instance_id: attachment.generation.clone(),
                            },
                            "ACP connection closed",
                        )
                        .await;
                    let _ = manager
                        .finish_active(
                            &session_id,
                            &attachment,
                            RunStatus::Failed,
                            "ACP connection closed",
                        )
                        .await;
                    let _ = manager.status(&session_id, SessionStatus::Failed).await;
                    let _ = attachment.peer.stop().await;
                    manager.attachments.lock().await.remove(&session_id);
                }
            }
        });
        self.event(
            id,
            "agent.attached",
            json!({"backendSessionId":remote_id,"capabilities":capabilities}),
        )
        .await?;
        Ok(json!({"sessionId":id,"backendSessionId":remote_id,"capabilities":capabilities}))
    }

    async fn current(&self, id: &str, attachment: &Arc<Attachment>) -> bool {
        self.attachments
            .lock()
            .await
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, attachment))
    }

    async fn permission(
        manager: Weak<Self>,
        attachment: Arc<Attachment>,
        session_id: String,
        message: Value,
    ) {
        let Some(manager) = manager.upgrade() else {
            return;
        };
        let permit = attachment.permissions.clone().try_acquire_owned();
        if permit.is_err() {
            let _ = attachment.peer.write(json!({"jsonrpc":"2.0","id":message["id"],"result":{"outcome":{"outcome":"cancelled"}}})).await;
            return;
        }
        let guard = manager.lifecycle.lock().await;
        let params = &message["params"];
        let run = attachment.active_run.lock().await.clone();
        let valid = params["sessionId"].as_str() == Some(&attachment.remote_id)
            && manager.current(&session_id, &attachment).await
            && run.is_some()
            && !attachment
                .cancelling
                .load(std::sync::atomic::Ordering::SeqCst);
        let option = |kind: &str| {
            params["options"]
                .as_array()
                .and_then(|options| options.iter().find(|option| option["kind"] == kind))
                .and_then(|option| option["optionId"].as_str())
                .map(str::to_owned)
        };
        let allow = option("allow_once");
        let reject = option("reject_once");
        let decision = if valid && allow.is_some() {
            let (resolver, receiver) = HostDecisionResolver::pair();
            let requested = manager
                .approvals
                .request(
                    &manager.app,
                    ApprovalOrigin {
                        session_id: session_id.clone(),
                        instance_id: attachment.generation.clone(),
                    },
                    ApprovalKind::ShellCommand {
                        request_method: "ACP session/request_permission".into(),
                        detail: params
                            .pointer("/toolCall/title")
                            .and_then(Value::as_str)
                            .unwrap_or("ACP agent requests permission")
                            .chars()
                            .take(2048)
                            .collect(),
                        scope: Some(attachment.backend.workspace.display().to_string()),
                        always_supported: false,
                    },
                    resolver,
                )
                .await;
            requested.ok().map(|_| receiver)
        } else {
            None
        };
        drop(guard);
        let decision = match decision {
            Some(receiver) => receiver.await.ok().and_then(Result::ok),
            None => None,
        };
        // A decision is valid only for the original active turn. Serialize its
        // delivery with completion/cancel/close, including a subsequent turn
        // on the same backend attachment.
        let _guard = manager.lifecycle.lock().await;
        let still_active = valid
            && manager.current(&session_id, &attachment).await
            && *attachment.active_run.lock().await == run
            && !attachment
                .cancelling
                .load(std::sync::atomic::Ordering::SeqCst);
        let selected = if still_active {
            match decision {
                Some(ApprovalDecision::Approve { .. }) => allow,
                Some(ApprovalDecision::Reject) => reject,
                _ => None,
            }
        } else {
            None
        };
        let outcome = selected
            .map(|id| json!({"outcome":"selected","optionId":id}))
            .unwrap_or_else(|| json!({"outcome":"cancelled"}));
        let _ = attachment
            .peer
            .write(json!({"jsonrpc":"2.0","id":message["id"],"result":{"outcome":outcome}}))
            .await;
    }

    pub async fn send(self: &Arc<Self>, id: String, text: String) -> Result<Value> {
        let _lock = self.lifecycle.lock().await;
        anyhow::ensure!(
            !text.trim().is_empty() && text.len() <= 256 * 1024,
            "prompt must be 1–262144 bytes"
        );
        let attachment = self
            .attachments
            .lock()
            .await
            .get(&id)
            .cloned()
            .context("agent is detached; resume it explicitly")?;
        let mut active = attachment.active_run.lock().await;
        anyhow::ensure!(
            active.is_none(),
            "agent already has an active turn; cancel or wait"
        );
        let run_id = format!("run_acp_{}", uuid::Uuid::new_v4().simple());
        self.core.claim_turn(id.clone(), run_id.clone(), Some(attachment.generation.clone())).await?;
        let run = self
            .core
            .runs()
            .start(RunCreate {
                id: run_id.clone(),
                session_id: id.clone(),
                mode: "acp".into(),
                model: None,
                adapter: Some(attachment.backend.id.clone()),
                metadata: json!({}),
                source: EventSource::Acp,
            })
            .await;
        let run = match run {
            Ok(run) => run,
            Err(error) => {
                self.core.release_turn(id.clone()).await?;
                return Err(error);
            }
        };
        self.core.broadcast_committed(run.event);
        self.status(&id, SessionStatus::Running).await?;
        self.event(&id, "agent.prompt", json!({"runId":run_id,"text":text}))
            .await?;
        attachment
            .cancelling
            .store(false, std::sync::atomic::Ordering::SeqCst);
        *active = Some(run_id.clone());
        drop(active);
        let manager = self.clone();
        let turn_id = run_id.clone();
        let session_id = id.clone();
        tokio::spawn(async move {
            let request = attachment.peer.request("session/prompt", json!({"sessionId":attachment.remote_id,"prompt":[{"type":"text","text":text}]}),
                Duration::from_secs(u64::from(attachment.backend.max_turn_seconds)));
            tokio::pin!(request);
            let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
            let result = loop {
                tokio::select! {
                    result = &mut request => break result,
                    _ = heartbeat.tick() => {
                        // The host still owns the bounded request, including human waits.
                        // EOF, timeout and cancellation settle it through the same path.
                        let _lock = manager.lifecycle.lock().await;
                        if !manager.current(&session_id, &attachment).await
                            || attachment.active_run.lock().await.as_deref() != Some(turn_id.as_str()) {
                            break Err(anyhow::anyhow!("ACP turn attachment changed"));
                        }
                        if let Err(error) = manager.core.heartbeat_turn(session_id.clone()).await {
                            break Err(error);
                        }
                    }
                }
            }.and_then(|result| {
                    anyhow::ensure!(matches!(result["stopReason"].as_str(), Some("end_turn" | "max_tokens" | "max_turn_requests" | "refusal" | "cancelled")), "invalid ACP stop reason");
                    Ok(result)
                });
            let _lock = manager.lifecycle.lock().await;
            if !manager.current(&session_id, &attachment).await {
                return;
            }
            let cancelled = result
                .as_ref()
                .ok()
                .is_some_and(|value| value["stopReason"] == "cancelled");
            let status = if result.is_err() {
                RunStatus::Failed
            } else if cancelled {
                RunStatus::Interrupted
            } else {
                RunStatus::Completed
            };
            let detail = match result {
                Ok(value) => value,
                Err(error) => json!({"error":error.to_string(),"outcomeUncertain":true}),
            };
            if let Ok(mutation) = manager
                .core
                .runs()
                .transition(
                    turn_id.clone(),
                    status,
                    Some(detail.clone()),
                    EventSource::Acp,
                )
                .await
            {
                manager.core.broadcast_committed(mutation.event);
            }
            let _ = manager
                .event(
                    &session_id,
                    "agent.turn_completed",
                    json!({"runId":turn_id,"result":detail}),
                )
                .await;
            let _ = manager
                .status(
                    &session_id,
                    if status == RunStatus::Failed {
                        SessionStatus::Failed
                    } else {
                        SessionStatus::Ready
                    },
                )
                .await;
            attachment
                .cancelling
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = manager
                .approvals
                .expire_origin(
                    &manager.app,
                    &ApprovalOrigin {
                        session_id: session_id.clone(),
                        instance_id: attachment.generation.clone(),
                    },
                    "ACP turn completed",
                )
                .await;
            *attachment.active_run.lock().await = None;
            let _ = manager.core.release_turn(session_id.clone()).await;
            if status == RunStatus::Failed {
                let _ = attachment.peer.stop().await;
            }
        });
        Ok(json!({"sessionId":id,"runId":run_id,"accepted":true}))
    }

    pub async fn cancel(self: &Arc<Self>, id: &str) -> Result<()> {
        let _lock = self.lifecycle.lock().await;
        let attachment = self
            .attachments
            .lock()
            .await
            .get(id)
            .cloned()
            .context("agent is detached")?;
        attachment
            .cancelling
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let run = attachment.active_run.lock().await.clone();
        let weak = Arc::downgrade(self);
        let cancel_attachment = attachment.clone();
        let cancel_id = id.to_owned();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if let Some(manager) = weak.upgrade() {
                let _lock = manager.lifecycle.lock().await;
                if run.is_some()
                    && manager.current(&cancel_id, &cancel_attachment).await
                    && *cancel_attachment.active_run.lock().await == run
                {
                    let _ = manager
                        .finish_active(
                            &cancel_id,
                            &cancel_attachment,
                            RunStatus::Interrupted,
                            "ACP cancellation deadline exceeded",
                        )
                        .await;
                    let _ = cancel_attachment.peer.stop().await;
                    manager.attachments.lock().await.remove(&cancel_id);
                }
            }
        });
        attachment
            .peer
            .notify("session/cancel", json!({"sessionId":attachment.remote_id}))
            .await?;
        self.approvals
            .expire_origin(
                &self.app,
                &ApprovalOrigin {
                    session_id: id.into(),
                    instance_id: attachment.generation.clone(),
                },
                "ACP turn cancelled",
            )
            .await?;
        Ok(())
    }

    async fn finish_active(
        &self,
        id: &str,
        attachment: &Arc<Attachment>,
        status: RunStatus,
        reason: &str,
    ) -> Result<()> {
        if let Some(run) = attachment.active_run.lock().await.take() {
            let mutation = self
                .core
                .runs()
                .transition(
                    run,
                    status,
                    Some(json!({"reason":reason})),
                    EventSource::Acp,
                )
                .await?;
            self.core.broadcast_committed(mutation.event);
        }
        self.core.release_turn(id.to_owned()).await?;
        self.event(id, "agent.detached", json!({"reason":reason}))
            .await?;
        Ok(())
    }

    pub async fn resume(self: &Arc<Self>, id: &str) -> Result<Value> {
        let _lock = self.lifecycle.lock().await;
        anyhow::ensure!(
            !self.attachments.lock().await.contains_key(id),
            "agent is already attached"
        );
        let (backend_id, remote, workspace, parent, depth): (String, String, String, Option<String>, u32) = self.core.storage().database().with_conn(|conn| {
            Ok(conn.query_row("SELECT backend_id, backend_session_id, workspace, parent_session_id, depth FROM agent_attachments WHERE session_id=?1", [id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))?)
        })?;
        let backend = self
            .backends()?
            .into_iter()
            .find(|backend| backend.id == backend_id)
            .context("backend is no longer registered")?;
        anyhow::ensure!(
            backend.workspace == PathBuf::from(workspace),
            "resume workspace changed"
        );
        let attachments = self.attachments.lock().await;
        anyhow::ensure!(
            attachments.len() < 16
                && attachments
                    .values()
                    .filter(|entry| entry.backend.id == backend.id)
                    .count()
                    < backend.max_sessions as usize,
            "agent concurrency limit reached"
        );
        if let Some(parent) = &parent {
            anyhow::ensure!(attachments.contains_key(parent), "resume the parent first");
        }
        drop(attachments);
        self.attach(id, backend, parent, depth, Some(remote)).await
    }

    pub async fn list(&self) -> Result<Value> {
        let attached: Vec<String> = self.attachments.lock().await.keys().cloned().collect();
        self.core.storage().database().with_conn(|conn| {
            let mut query = conn.prepare("SELECT a.session_id, a.backend_id, a.backend_session_id, a.parent_session_id, s.title, s.status FROM agent_attachments a JOIN sessions s ON s.id=a.session_id ORDER BY s.created_at DESC LIMIT 500")?;
            let rows = query.query_map([], |row| {
                let id: String = row.get(0)?;
                Ok(json!({"sessionId":id,"backendId":row.get::<_,String>(1)?,"backendSessionId":row.get::<_,Option<String>>(2)?,"parentSessionId":row.get::<_,Option<String>>(3)?,"title":row.get::<_,String>(4)?,"status":row.get::<_,String>(5)?,"attached":attached.contains(&id)}))
            })?.collect::<Result<Vec<_>, _>>()?;
            Ok(json!({"sessions":rows}))
        })
    }

    pub async fn close(&self, id: &str) -> Result<()> {
        let _lock = self.lifecycle.lock().await;
        let descendants: Vec<String> = self.core.storage().database().with_conn(|conn| {
            let mut stmt = conn.prepare("WITH RECURSIVE tree(session_id,depth) AS (SELECT session_id,0 FROM agent_attachments WHERE session_id=?1 UNION ALL SELECT a.session_id,t.depth+1 FROM agent_attachments a JOIN tree t ON a.parent_session_id=t.session_id WHERE t.depth<3) SELECT session_id FROM tree ORDER BY depth DESC")?;
            let rows = stmt.query_map([id], |row| row.get(0))?.collect::<Result<Vec<_>,_>>()?;
            Ok(rows)
        })?;
        anyhow::ensure!(!descendants.is_empty(), "unknown ACP session");
        for child in descendants {
            self.close_one(&child).await?;
        }
        Ok(())
    }

    async fn close_one(&self, id: &str) -> Result<()> {
        let attachment = self.attachments.lock().await.remove(id);
        if let Some(attachment) = attachment {
            attachment
                .cancelling
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.finish_active(
                id,
                &attachment,
                RunStatus::Interrupted,
                "ACP session closed",
            )
            .await?;
            self.approvals
                .expire_origin(
                    &self.app,
                    &ApprovalOrigin {
                        session_id: id.into(),
                        instance_id: attachment.generation.clone(),
                    },
                    "ACP session closed",
                )
                .await?;
            attachment.peer.stop().await?;
        }
        self.status(id, SessionStatus::Closed).await?;
        Ok(())
    }
}

impl crate::services::ManagedService for Manager {
    fn name(&self) -> &'static str {
        "acp-agents"
    }
    fn stop(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let ids: Vec<String> = self.attachments.lock().await.keys().cloned().collect();
            for id in ids {
                self.close(&id).await?;
            }
            Ok(())
        })
    }
}
