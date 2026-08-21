//! CoreRuntime composition root: storage, journal, visuals, and post-commit broadcast.

use crate::cloud::intern::{
    InternProviderManager, InternRuntime, InternSessionBinding, PollerConfig, RuntimeKind,
};
use crate::contract::events::{origin_for_source_and_kind, tag_event, EventChannel};
use crate::data::{ContainerDeployment, ContainerRegisterRequest, DataStore};
use crate::domain::{RunService, RunStatus, SessionKind, SessionService, SessionStatus};
use crate::optimizers::OptimizerService;
use crate::plugins::PluginService;
use crate::reports::ReportRegistry;
use crate::storage::{
    AppEvent, ContentStore, CoreDiagnostics, EventAppend, EventJournal, EventSource, SessionRecord,
    Storage,
};
use crate::visuals::VisualRegistry;
use anyhow::{Context, Result};
use chrono::Utc;
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
    reports: ReportRegistry,
    data: DataStore,
    optimizers: OptimizerService,
    plugins: PluginService,
    computer_use: Arc<crate::computer_use::service::ComputerUseService>,
    diagnostics: Arc<crate::diagnostics::DiagnosticsService>,
    intern: Arc<InternRuntime>,
    intern_provider: Arc<InternProviderManager>,
    sessions: SessionService,
    runs: RunService,
    events_tx: broadcast::Sender<AppEvent>,
    secrets: Arc<crate::secrets::SecretsService>,
}

impl CoreRuntime {
    pub fn open(root: impl Into<std::path::PathBuf>) -> Result<Self> {
        let storage = Storage::open(root)?;
        // Bindings written before the canonical envelope was enforced still
        // render an empty pane, so bring them forward at open. Storage cannot
        // do this itself: deciding what a legacy shape meant is domain logic.
        let backfill = storage
            .database()
            .with_conn(crate::visuals::canonicalize_persisted_bindings)
            .context("canonicalize persisted visual bindings")?;
        if backfill.changed() {
            eprintln!(
                "synth-desktop: visual bindings backfill scanned {}, upgraded {}, refused {}",
                backfill.scanned, backfill.upgraded, backfill.refused
            );
        }
        // Reconcile before returning, not in a spawned task. Everything that can
        // read a session — `listSessions`, the Codex record cache, the eval
        // driver — goes through a CoreRuntime that already exists, so a task
        // scheduled here would race the first read and let a dead `running` row
        // reach the UI as Working. This is the boundary that must hold.
        let recovered = storage
            .database()
            .transaction(|conn| {
                crate::recovery::reconcile_orphaned_turns(
                    conn,
                    crate::instance::boot_epoch(),
                    Utc::now(),
                )
            })
            .context("reconcile abandoned turns at startup")?;
        if !recovered.is_empty() {
            eprintln!(
                "synth-desktop: recovered {} abandoned turn(s) from a previous run ({})",
                recovered.len(),
                recovered
                    .iter()
                    .map(|notice| notice.session_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
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
        let reports = ReportRegistry::new(
            storage.database().clone(),
            journal.clone(),
            content.clone(),
            visuals.clone(),
        );
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
            optimizer_manager.clone(),
        );
        let plugin_path = storage
            .content_root()
            .parent()
            .unwrap_or_else(|| storage.content_root())
            .join("plugins/optimizers.json");
        let plugins = PluginService::new(crate::plugins::PluginRegistry::with_path(plugin_path));
        // The allowlist lives beside the plugin registry, per instance, so a
        // second Desktop instance does not inherit the first one's grants.
        let computer_use = Arc::new(crate::computer_use::service::ComputerUseService::new(
            crate::computer_use::allowlist::AppAllowlist::open(
                storage
                    .content_root()
                    .parent()
                    .unwrap_or_else(|| storage.content_root())
                    .join("computer-use/allowlist.json"),
            ),
        ));
        // Diagnostics share the journal's database and live beside the content
        // store, one directory per instance. The service is constructed here
        // but starts nothing: its writer and its index sidecar are started
        // deliberately, after the main window is interactive.
        let diagnostics_root = storage
            .content_root()
            .parent()
            .unwrap_or_else(|| storage.content_root())
            .join("diagnostics");
        let diagnostics = crate::diagnostics::DiagnosticsService::new(
            storage.database().clone(),
            journal.clone(),
            diagnostics_root,
        );
        optimizers.attach_diagnostics(diagnostics.clone());
        visuals.attach_optimizer_runs(optimizers.clone());
        visuals.attach_diagnostics(diagnostics.clone());
        optimizer_manager.attach_diagnostics(diagnostics.clone());
        let secrets = Arc::new(crate::secrets::SecretsService::new(
            storage.database().clone(),
        ));
        let _ = secrets.load_configured_env_sources();
        Self {
            storage,
            journal,
            content,
            visuals,
            reports,
            data,
            optimizers,
            plugins,
            computer_use,
            diagnostics,
            intern,
            intern_provider,
            sessions,
            runs,
            events_tx,
            secrets,
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

    pub fn reports(&self) -> &ReportRegistry {
        &self.reports
    }

    pub fn data(&self) -> &DataStore {
        &self.data
    }

    pub fn optimizers(&self) -> &OptimizerService {
        &self.optimizers
    }

    pub fn computer_use(&self) -> &crate::computer_use::service::ComputerUseService {
        &self.computer_use
    }

    pub fn plugins(&self) -> &PluginService {
        &self.plugins
    }

    /// Local diagnostics. Named apart from [`Self::diagnostics`], which
    /// reports storage health and predates this system.
    pub fn diagnostics_service(&self) -> &Arc<crate::diagnostics::DiagnosticsService> {
        &self.diagnostics
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

    pub fn secrets(&self) -> &Arc<crate::secrets::SecretsService> {
        &self.secrets
    }

    pub fn broadcast_committed(&self, event: Option<AppEvent>) {
        if let Some(event) = event {
            let _ = self.events_tx.send(event);
        }
    }

    /// Take the live claim on a turn this process is about to run, and clear any
    /// recovery notice the session was still carrying. Returns the attempt
    /// number, so a restarted turn is auditable as one.
    pub async fn claim_turn(
        &self,
        session_id: String,
        run_id: String,
        attachment_id: Option<String>,
    ) -> Result<i64> {
        let instance_id = crate::instance::boot_epoch().to_owned();
        self.storage
            .database()
            .run_transaction(move |conn| {
                let previous_attempt =
                    crate::recovery::clear_recovery_metadata(conn, &session_id)?.unwrap_or(0);
                crate::recovery::ownership::claim(
                    conn,
                    &session_id,
                    &run_id,
                    &instance_id,
                    attachment_id.as_deref(),
                    previous_attempt,
                    Utc::now(),
                )?;
                Ok(previous_attempt)
            })
            .await
    }

    /// Refresh the lease from live provider activity. Rate-limited against the
    /// stored heartbeat so a chatty stream does not turn into a write storm.
    pub async fn heartbeat_turn(&self, session_id: String) -> Result<bool> {
        let instance_id = crate::instance::boot_epoch().to_owned();
        self.storage
            .database()
            .run(move |conn| {
                let now = Utc::now();
                let Some(claim) = crate::recovery::ownership::load(conn, &session_id)? else {
                    return Ok(false);
                };
                if claim.owner_instance_id != instance_id
                    || !crate::recovery::ownership::heartbeat_due(&claim, now)
                {
                    return Ok(false);
                }
                crate::recovery::ownership::heartbeat(conn, &session_id, &instance_id, None, now)
            })
            .await
    }

    pub async fn release_turn(&self, session_id: String) -> Result<()> {
        self.storage
            .database()
            .run(move |conn| crate::recovery::ownership::release(conn, &session_id))
            .await
    }

    /// Sessions this process can honestly present as Working right now.
    pub async fn live_turn_sessions(&self) -> Result<Vec<String>> {
        let instance_id = crate::instance::boot_epoch().to_owned();
        self.storage
            .database()
            .run(move |conn| {
                let now = Utc::now();
                Ok(
                    crate::recovery::ownership::owned_sessions(conn, &instance_id)?
                        .into_iter()
                        .filter(|claim| claim.is_live(&instance_id, now))
                        .map(|claim| claim.session_id)
                        .collect(),
                )
            })
            .await
    }

    /// Run one lease-expiry sweep and broadcast what it recovered.
    ///
    /// The renderer's watchdogs die with the window; this one does not. It also
    /// covers the case the renderer cannot see at all — a turn whose owner is
    /// this process but whose event pump has stopped refreshing the claim.
    pub async fn sweep_expired_leases(&self) -> Result<usize> {
        // Cheap read first: an idle Workshop must not take a write lock every
        // five seconds just to discover it has nothing to do.
        if !self
            .storage
            .database()
            .run(crate::recovery::has_reconcilable_turns)
            .await?
        {
            return Ok(0);
        }
        let instance_id = crate::instance::boot_epoch().to_owned();
        let notices = self
            .storage
            .database()
            .run_transaction(move |conn| {
                crate::recovery::reconcile_orphaned_turns(conn, &instance_id, Utc::now())
            })
            .await?;
        let recovered = notices.len();
        for notice in notices {
            eprintln!(
                "synth-desktop: lease expired for session {} ({})",
                notice.session_id, notice.reason
            );
            let _ = self
                .append_and_broadcast(EventAppend {
                    event_id: None,
                    session_id: Some(notice.session_id.clone()),
                    run_id: notice.run_id.clone(),
                    source: EventSource::System,
                    kind: "session/unhealthy".into(),
                    payload: json!({
                        "reason": notice.reason,
                        "message": "This task stopped proving it was still running.",
                        "recovery": notice.to_json(),
                    }),
                    remote_sequence: None,
                    command_id: None,
                    created_at: None,
                })
                .await;
        }
        Ok(recovered)
    }

    /// Backend-owned liveness. Starting this is what makes the invariant hold
    /// with no window open at all.
    pub fn spawn_lease_watchdog(self: &Arc<Self>) {
        let core = self.clone();
        tauri::async_runtime::spawn(async move {
            let interval = crate::recovery::HEARTBEAT_INTERVAL
                .to_std()
                .unwrap_or(Duration::from_secs(5));
            loop {
                tokio::time::sleep(interval).await;
                if let Err(error) = core.sweep_expired_leases().await {
                    eprintln!("lease watchdog sweep failed: {error}");
                }
            }
        });
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
        self.optimizers.restore_hosted_sft_mirrors().await;
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
