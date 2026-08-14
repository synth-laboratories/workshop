//! CoreRuntime composition root: storage, journal, visuals, and post-commit broadcast.

use crate::cloud::intern::{
    InternProviderManager, InternRuntime, InternSessionBinding, PollerConfig, RuntimeKind,
};
use crate::contract::events::{origin_for_source_and_kind, tag_event, EventChannel};
use crate::data::{ContainerDeployment, ContainerRegisterRequest, DataStore};
use crate::domain::{RunService, RunStatus, SessionKind, SessionService, SessionStatus};
use crate::optimizers::OptimizerService;
use crate::plugins::PluginService;
use crate::storage::{
    AppEvent, ContentStore, CoreDiagnostics, EventAppend, EventJournal, EventSource, SessionRecord,
    Storage,
};
use crate::visuals::VisualRegistry;
use anyhow::{Context, Result};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

pub const RUNTIME_EVENT_CHANNEL: &str = EventChannel::RUNTIME;
pub const VISUAL_SHOW_CHANNEL: &str = EventChannel::VISUAL_SHOW;

#[derive(Clone)]
pub struct CoreRuntime {
    storage: Storage,
    journal: EventJournal,
    content: ContentStore,
    visuals: VisualRegistry,
    data: DataStore,
    optimizers: OptimizerService,
    plugins: PluginService,
    intern: Arc<InternRuntime>,
    intern_provider: Arc<InternProviderManager>,
    sessions: SessionService,
    runs: RunService,
    events_tx: broadcast::Sender<AppEvent>,
}

impl CoreRuntime {
    pub fn open(root: impl Into<std::path::PathBuf>) -> Result<Self> {
        let storage = Storage::open(root)?;
        let backend = crate::synth_config::resolve().context("resolve Synth backend")?;
        let intern = Arc::new(match backend.api_key {
            Some(api_key) => InternRuntime::configured(
                &backend.backend_url,
                api_key,
                crate::limits::INTERN_HTTP_TIMEOUT,
            )
            .context("configure Rust Intern runtime")?,
            None => InternRuntime::unconfigured(),
        });
        Ok(Self::from_parts(storage, intern))
    }

    fn from_parts(storage: Storage, intern: Arc<InternRuntime>) -> Self {
        let journal = EventJournal::new(storage.database().clone());
        let content = ContentStore::new(storage.content_root());
        let visuals =
            VisualRegistry::new(storage.database().clone(), journal.clone(), content.clone());
        let data = DataStore::new(storage.database().clone(), content.clone());
        let intern_provider = Arc::new(InternProviderManager::new(
            intern.clone(),
            storage.database().clone(),
        ));
        let sessions = SessionService::new(storage.database().clone());
        let runs = RunService::new(storage.database().clone());
        let (events_tx, _) = broadcast::channel(512);
        let optimizer_manager = Arc::new(crate::optimizers::OptimizerManager::new());
        let optimizers = OptimizerService::new_with_manager(
            storage.database().clone(),
            journal.clone(),
            visuals.clone(),
            events_tx.clone(),
            optimizer_manager,
        );
        let plugin_path = storage
            .content_root()
            .parent()
            .unwrap_or_else(|| storage.content_root())
            .join("plugins/optimizers.json");
        let plugins = PluginService::new(crate::plugins::PluginRegistry::with_path(plugin_path));
        Self {
            storage,
            journal,
            content,
            visuals,
            data,
            optimizers,
            plugins,
            intern,
            intern_provider,
            sessions,
            runs,
            events_tx,
        }
    }

    #[cfg(test)]
    pub(crate) fn open_with_intern(
        root: impl Into<std::path::PathBuf>,
        intern: InternRuntime,
    ) -> Result<Self> {
        Ok(Self::from_parts(Storage::open(root)?, Arc::new(intern)))
    }

    pub fn open_default() -> Result<Self> {
        Self::open(crate::storage::app_data_root())
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    pub fn journal(&self) -> &EventJournal {
        &self.journal
    }

    pub fn content(&self) -> &ContentStore {
        &self.content
    }

    pub fn visuals(&self) -> &VisualRegistry {
        &self.visuals
    }

    pub fn data(&self) -> &DataStore {
        &self.data
    }

    pub fn optimizers(&self) -> &OptimizerService {
        &self.optimizers
    }

    pub fn plugins(&self) -> &PluginService {
        &self.plugins
    }

    pub fn intern(&self) -> &Arc<InternRuntime> {
        &self.intern
    }

    pub fn sessions(&self) -> &SessionService {
        &self.sessions
    }

    pub fn runs(&self) -> &RunService {
        &self.runs
    }

    pub fn broadcast_committed(&self, event: Option<AppEvent>) {
        if let Some(event) = event {
            let _ = self.events_tx.send(event);
        }
    }

    pub async fn start_intern_provider(
        &self,
        session_id: String,
        runtime_id: String,
        runtime_kind: RuntimeKind,
        title: Option<String>,
    ) -> Result<bool> {
        let (committed_tx, mut committed_rx) = tokio::sync::mpsc::channel(128);
        let binding = InternSessionBinding {
            session_id,
            runtime_id,
            runtime_kind,
            title,
        };
        let started = match runtime_kind {
            RuntimeKind::Sync => {
                self.intern_provider
                    .start_sync(binding, committed_tx, PollerConfig::default())
                    .await?
            }
            RuntimeKind::Async => {
                self.intern_provider
                    .start_async(binding, committed_tx, PollerConfig::default())
                    .await?
            }
        };
        if started {
            let events_tx = self.events_tx.clone();
            tokio::spawn(async move {
                while let Some(event) = committed_rx.recv().await {
                    let _ = events_tx.send(event);
                }
            });
        }
        Ok(started)
    }

    #[cfg(test)]
    pub(crate) async fn stop_intern_providers_for_test(&self) -> Result<()> {
        self.intern_provider.shutdown().await
    }

    /// Reattach durable Intern sessions after application restart. Remote
    /// cursors are loaded by the ingestion adapter before polling resumes.
    pub async fn resume_intern_providers(&self) -> Result<usize> {
        self.resume_intern_providers_inner(true).await
    }

    async fn resume_intern_providers_inner(&self, reconcile_restart: bool) -> Result<usize> {
        let mut resumed = 0;
        for session in self.sessions.list(2_000).await? {
            if session.status == SessionStatus::Closed.as_str() {
                continue;
            }
            if session.kind != SessionKind::Intern.as_str() {
                continue;
            }
            let Some(runtime_id) = session.remote_id.clone() else {
                continue;
            };
            let Some(mode) = session.target.intern_mode() else {
                continue;
            };
            let runtime_kind = match mode {
                crate::domain::InternMode::Sync => RuntimeKind::Sync,
                crate::domain::InternMode::Async => RuntimeKind::Async,
            };
            if reconcile_restart {
                self.reconcile_intern_active_run(&session).await?;
            }
            if self
                .start_intern_provider(session.id, runtime_id, runtime_kind, Some(session.title))
                .await?
            {
                resumed += 1;
            }
        }
        Ok(resumed)
    }

    /// A command may have reached the remote runtime immediately before the
    /// desktop process exited. There is no command-status endpoint to prove
    /// its final outcome on restart, so fail the local receipt closed and mark
    /// the abandoned local run interrupted before mailbox polling resumes.
    async fn reconcile_intern_active_run(&self, session: &SessionRecord) -> Result<()> {
        let Some(run_id) = session.active_run_id.clone() else {
            return Ok(());
        };
        let Some(run) = self.runs.get(run_id.clone()).await? else {
            return Ok(());
        };
        if let Some(command_id) = run
            .metadata
            .get("commandId")
            .and_then(|value| value.as_str())
        {
            if self
                .runs
                .command_receipt(command_id.to_owned())
                .await?
                .is_some_and(|receipt| receipt.status == "accepted")
            {
                let receipt = self
                    .runs
                    .resolve_command(
                        command_id.to_owned(),
                        "failed".into(),
                        json!({"error":"desktop restarted before command receipt was persisted"}),
                        None,
                    )
                    .await?;
                self.broadcast_committed(receipt.event);
            }
        }
        let interrupted = self
            .runs
            .transition(
                run_id,
                RunStatus::Interrupted,
                Some(json!({"reason":"desktop_restart_reconciliation"})),
                EventSource::Intern,
            )
            .await?;
        self.broadcast_committed(interrupted.event);
        Ok(())
    }

    /// Rotate the cloud endpoint and credential without leaving pollers on the
    /// previous identity alive. Missing credentials deliberately fail closed.
    pub async fn reload_intern_config(&self) -> Result<()> {
        let backend = crate::synth_config::resolve().context("resolve Synth backend")?;
        self.intern_provider.shutdown().await?;
        match backend.api_key {
            Some(api_key) => {
                self.intern
                    .reconfigure(
                        &backend.backend_url,
                        api_key,
                        crate::limits::INTERN_HTTP_TIMEOUT,
                    )
                    .await
                    .context("reconfigure Rust Intern runtime")?;
                self.resume_intern_providers_inner(false)
                    .await
                    .context("resume Intern providers after reconfiguration")?;
                Ok(())
            }
            None => {
                self.intern.disable().await;
                Ok(())
            }
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.events_tx.subscribe()
    }

    pub fn diagnostics(&self) -> Result<CoreDiagnostics> {
        self.storage.diagnostics()
    }

    pub async fn append_and_broadcast(&self, input: EventAppend) -> Result<AppEvent> {
        let event = self.journal.append(input).await?;
        let _ = self.events_tx.send(event.clone());
        Ok(event)
    }

    pub async fn append_and_emit<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        input: EventAppend,
    ) -> Result<AppEvent> {
        let _ = app;
        self.append_and_broadcast(input).await
    }

    pub async fn publish_event<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        event: AppEvent,
    ) -> Result<()> {
        let _ = app;
        let _ = self.events_tx.send(event);
        Ok(())
    }

    /// Persist an inventory health mutation and its journal record atomically,
    /// then make the committed event visible to live subscribers.
    pub async fn update_container_health(
        &self,
        id: String,
        status: String,
        health: serde_json::Value,
    ) -> Result<ContainerDeployment> {
        let (container, event) = self
            .data
            .update_container_health(id, status, health)
            .await?;
        let _ = self.events_tx.send(event);
        Ok(container)
    }

    pub async fn register_container(
        &self,
        request: ContainerRegisterRequest,
        status: String,
        health: serde_json::Value,
        metadata: serde_json::Value,
        task_family: Option<String>,
    ) -> Result<ContainerDeployment> {
        let (container, event) = self
            .data
            .upsert_container(request, status, health, metadata, task_family)
            .await?;
        let _ = self.events_tx.send(event);
        Ok(container)
    }

    pub async fn update_container_hydration(
        &self,
        id: String,
        status: String,
        health: serde_json::Value,
        metadata: serde_json::Value,
        task_family: Option<String>,
    ) -> Result<ContainerDeployment> {
        let (container, event) = self
            .data
            .update_container_hydration(id, status, health, metadata, task_family)
            .await?;
        let _ = self.events_tx.send(event);
        Ok(container)
    }

    pub async fn update_container_last_rollout(
        &self,
        id: String,
        rollout_id: String,
    ) -> Result<ContainerDeployment> {
        let (container, event) = self
            .data
            .update_container_last_rollout(id, rollout_id)
            .await?;
        let _ = self.events_tx.send(event);
        Ok(container)
    }

    pub async fn bootstrap(&self, app: &AppHandle) -> Result<()> {
        let diagnostics = self.diagnostics()?;
        self.append_and_emit(
            app,
            EventAppend::system(
                "runtime.ready",
                json!({
                    "schemaVersion": diagnostics.schema_version,
                    "journalHead": diagnostics.journal_head,
                    "databasePath": diagnostics.database_path,
                }),
            ),
        )
        .await
        .context("emit runtime.ready")?;
        Ok(())
    }

    pub fn spawn_forwarder(self: &Arc<Self>, app: AppHandle) {
        let mut rx = self.subscribe();
        tauri::async_runtime::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let tagged = tag_event(
                            origin_for_source_and_kind(event.source.as_str(), &event.kind),
                            event.clone(),
                        );
                        let _ = app.emit(RUNTIME_EVENT_CHANNEL, &tagged);
                        if event.kind == "visual.show" {
                            let _ = app.emit(VISUAL_SHOW_CHANNEL, &event);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn broadcasts_only_after_event_is_committed() {
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        let mut events = core.subscribe();

        let appended = core
            .append_and_broadcast(EventAppend::system("runtime.test", json!({"ok": true})))
            .await
            .unwrap();
        let broadcast = events.recv().await.unwrap();

        assert_eq!(broadcast, appended);
        let persisted = core.journal().events_after(0, 10).await.unwrap();
        assert_eq!(persisted, vec![broadcast]);
    }

    #[tokio::test]
    async fn inventory_mutation_broadcasts_its_committed_event() {
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        core.storage().database().with_conn(|conn| {
            conn.execute("INSERT INTO containers(id,name,location,status,health_json,metadata_json,created_at,updated_at) VALUES('ctr_1','Local','local','starting','{}','{}','2026-01-01','2026-01-01')", [])?;
            Ok(())
        }).unwrap();
        let mut events = core.subscribe();

        let container = core
            .update_container_health("ctr_1".into(), "ready".into(), json!({"ok": true}))
            .await
            .unwrap();
        let event = events.recv().await.unwrap();

        assert_eq!(container.status, "ready");
        assert_eq!(event.kind, "container.health.updated");
        assert_eq!(
            core.journal().events_after(0, 10).await.unwrap(),
            vec![event]
        );
    }
}
