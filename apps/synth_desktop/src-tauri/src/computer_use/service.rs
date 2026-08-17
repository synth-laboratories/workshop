//! The Desktop-side service: the one place that turns a planned action into an
//! approved, performed, recorded one.
//!
//! Ordering here is the security property. Plan, then approve, then perform,
//! then record — and record even when the answer was no, because a trajectory
//! that only contains what happened cannot be used to evaluate what an agent
//! tried to do.

use super::allowlist::AppAllowlist;
use super::client::HelperClient;
use super::helper::{self, HelperIdentity};
use super::lock::LockGuard;
use super::permissions::{self, GrantState};
use super::session::{ComputerUseSession, Plan, Refusal};
use super::trajectory::{
    Observation, RunVersion, StateRef, TrajectoryRecorder, TrajectoryStep, RESULT_ERROR, RESULT_OK,
    RESULT_REFUSED,
};
use super::vocabulary::Action;
use crate::plugins::types::{
    PluginNotReady, PluginServiceStatus, PluginStatus, COMPUTER_USE_PLUGIN_ID, PLUGIN_STATUS_SCHEMA,
};
use crate::session::approval::{
    ApprovalBroker, ApprovalDecision, ApprovalKind, ApprovalOrigin, ApprovalScope,
    HostDecisionResolver,
};
use crate::storage::content_store::ContentStore;
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;

/// A computer-use approval must not sit forever, but it also must not expire
/// while the screen is locked — the lock guard suspends expiry, so this is the
/// live-screen ceiling only.
const APPROVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

pub struct ComputerUseService {
    allowlist: Arc<AppAllowlist>,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, ComputerUseSession>,
    recorders: HashMap<String, TrajectoryRecorder>,
    client: Option<HelperClient>,
    identity: Option<HelperIdentity>,
    grants: Vec<(String, GrantState)>,
    /// Set when the helper refused to verify, so status can say why rather than
    /// reporting a bare `error`.
    detail: Option<String>,
}

impl ComputerUseService {
    pub fn new(allowlist: AppAllowlist) -> Self {
        Self {
            allowlist: Arc::new(allowlist),
            inner: Mutex::new(Inner::default()),
        }
    }

    pub fn open_default() -> Self {
        Self::new(AppAllowlist::open_default())
    }

    pub fn allowlist(&self) -> &Arc<AppAllowlist> {
        &self.allowlist
    }

    /// Plugin status, including the permission rows and the refined phase.
    pub async fn status(&self) -> PluginStatus {
        let inner = self.inner.lock().await;
        let installed = helper::helper_bundle_path().exists();
        let base = if !installed {
            "not_installed"
        } else if inner.detail.is_some() && inner.identity.is_none() {
            "error"
        } else if inner.client.is_some() {
            "ready"
        } else {
            "installed"
        };
        let phase = permissions::refine_phase(base, &inner.grants);
        PluginStatus {
            schema_version: PLUGIN_STATUS_SCHEMA.into(),
            plugin_id: COMPUTER_USE_PLUGIN_ID.into(),
            // No registry flag of its own yet: the human-only lifecycle in §4
            // means enable/disable is a Desktop-side decision, not an agent one.
            enabled: true,
            phase: phase.clone(),
            installed_version: inner
                .identity
                .as_ref()
                .map(|identity| identity.version.clone()),
            selected_version: None,
            release_channel: crate::plugins::types::OFFICIAL_RELEASE_CHANNEL.into(),
            catalog_version: inner
                .identity
                .as_ref()
                .map(|identity| identity.version.clone())
                .unwrap_or_else(|| "1.0.0".into()),
            digest: inner
                .identity
                .as_ref()
                .map(|identity| crate::plugins::types::digest_ref(&identity.cdhash)),
            service: PluginServiceStatus {
                phase,
                started_at: None,
                active_runs: inner.sessions.len() as u32,
            },
            capabilities_digest: None,
            algorithms: Vec::new(),
            templates: Vec::new(),
            permissions: permissions::rows(&inner.grants),
            last_action_receipt_id: None,
            detail: inner.detail.clone(),
        }
    }

    /// Refresh grant state from the helper. Cheap and never prompts, so it is
    /// safe to call whenever the UI refreshes.
    pub async fn refresh_grants(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        self.ensure_client(&mut inner).await?;
        let client = inner
            .client
            .as_mut()
            .ok_or_else(|| anyhow!("helper is not running"))?;
        let reported = client
            .call_tool("computer_use_permissions", json!({ "operation": "probe" }))
            .await?;
        inner.grants = read_grants(&reported);
        Ok(())
    }

    /// Verify and launch the helper if it is not already running.
    async fn ensure_client(&self, inner: &mut Inner) -> Result<()> {
        if inner.client.is_some() {
            return Ok(());
        }
        let bundle = helper::helper_bundle_path();
        let team = helper::expected_team_id();
        // A development build without a team id configured is allowed to run
        // unnotarized; a build with one is not. That keeps the loose path
        // available for iteration and impossible to reach by accident in a
        // shipped configuration.
        let require_notarized = team.is_some();
        let identity = match helper::verify(
            &helper::SystemCommands,
            &bundle,
            team.as_deref(),
            require_notarized,
        ) {
            Ok(identity) => identity,
            Err(error) => {
                inner.detail = Some(error.to_string());
                return Err(error);
            }
        };
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let client = HelperClient::spawn(&helper::helper_executable(&bundle), &nonce).await?;
        inner.identity = Some(identity);
        inner.client = Some(client);
        inner.detail = None;
        Ok(())
    }

    /// Begin a computer-use run for an agent session.
    pub async fn begin(&self, session_id: &str, content_store: ContentStore) -> Result<String> {
        let mut inner = self.inner.lock().await;
        self.ensure_client(&mut inner).await?;
        if !permissions::is_ready(&inner.grants) {
            return Err(PluginNotReady::for_plugin(
                COMPUTER_USE_PLUGIN_ID,
                crate::plugins::types::PHASE_NEEDS_PERMISSIONS,
                "grant the listed permissions in System Settings",
            )
            .missing(permissions::missing(&inner.grants))
            .into());
        }
        // The IPC route calls begin lazily before every action. Reuse the
        // existing run or a read followed by a click would create a fresh
        // session here and erase the element snapshot the click depends on.
        if let Some(existing) = inner.sessions.get(session_id) {
            return Ok(existing.run_id().to_owned());
        }
        let run_id = format!("cu_run_{}", uuid::Uuid::new_v4().simple());
        let identity = inner
            .identity
            .as_ref()
            .ok_or_else(|| anyhow!("helper identity is unknown"))?;
        let version = RunVersion {
            helper_version: identity.version.clone(),
            helper_identity: identity.describe(),
            vocabulary_version: "v1".into(),
        };
        inner.sessions.insert(
            session_id.to_owned(),
            ComputerUseSession::new(
                session_id,
                run_id.clone(),
                self.allowlist.clone(),
                LockGuard::with_default_ceiling(),
            ),
        );
        inner.recorders.insert(
            session_id.to_owned(),
            TrajectoryRecorder::new(content_store, version, run_id.clone(), session_id),
        );
        Ok(run_id)
    }

    /// End a run and drop its session-scoped grants. A session grant outliving
    /// its session is an always-grant nobody agreed to.
    pub async fn end(&self, session_id: &str) -> Result<()> {
        let mut inner = self.inner.lock().await;
        inner.sessions.remove(session_id);
        inner.recorders.remove(session_id);
        self.allowlist.clear_session(session_id)?;
        Ok(())
    }

    /// Perform one action end to end.
    pub async fn perform<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        broker: &ApprovalBroker,
        session_id: &str,
        action: Action,
    ) -> Result<Value> {
        let started_at = Utc::now().to_rfc3339();
        let mut inner = self.inner.lock().await;

        let plan = {
            let session = inner
                .sessions
                .get(session_id)
                .ok_or_else(|| anyhow!("no computer-use run is open for this session"))?;
            match session.plan(action.clone()) {
                Ok(plan) => plan,
                Err(refusal) => {
                    let step = record(
                        &mut inner,
                        session_id,
                        &action,
                        StateRef::default(),
                        StateRef::default(),
                        None,
                        None,
                        RESULT_REFUSED,
                        Some(refusal.message.clone()),
                        started_at,
                    );
                    return Ok(json!({
                        "refused": refusal,
                        "step": step,
                    }));
                }
            }
        };

        // Approval can remain open for minutes. Never hold the service mutex
        // while waiting for the operator: doing so lets one abandoned card
        // freeze every Computer Use session, including read-only calls in a
        // newly-created chat. Reacquire it only when we need to record or act.
        drop(inner);

        // Approval, if the plan asked for one.
        let approval = match &plan.approval {
            Some(request) => {
                let outcome = self
                    .authorize(app, broker, session_id, request, &plan)
                    .await?;
                match outcome {
                    Authorization::Rejected => {
                        let mut inner = self.inner.lock().await;
                        let step = record(
                            &mut inner,
                            session_id,
                            &action,
                            StateRef::default(),
                            StateRef::default(),
                            None,
                            plan.hazard.clone(),
                            RESULT_REFUSED,
                            Some("the operator declined this action".into()),
                            started_at,
                        );
                        return Ok(json!({
                            "refused": Refusal {
                                code: "approval_rejected".into(),
                                message: "the operator declined this action".into(),
                                remediation: String::new(),
                            },
                            "step": step,
                        }));
                    }
                    Authorization::Granted { id, scope } => {
                        // Only an app-scope grant is remembered. Hazard actions
                        // offered no remembered scope, so nothing is written.
                        if let Some(scope) = scope {
                            self.allowlist.record(
                                &request.app,
                                &scope,
                                session_id,
                                &id,
                                Utc::now().to_rfc3339(),
                            )?;
                        }
                        Some(id)
                    }
                }
            }
            None => None,
        };

        // Capture before, act, capture after.
        let mut inner = self.inner.lock().await;
        let before = observe(&mut inner, session_id, action.app()).await.0;
        let outcome = self.dispatch(&mut inner, &action).await;
        let (after, canonical_snapshot) = observe(&mut inner, session_id, action.app()).await;

        let (result, error, payload) = match outcome {
            Ok(value) => {
                if let Some(session) = inner.sessions.get_mut(session_id) {
                    session.observe_performed(&action);
                    if let Some(app_id) = action.app() {
                        if action.is_read_only() {
                            session.observe_read(
                                app_id,
                                canonical_snapshot.unwrap_or_else(|| snapshot(&value)),
                                plan.is_full_read,
                            );
                        }
                    }
                }
                (RESULT_OK, None, value)
            }
            Err(error) => (RESULT_ERROR, Some(error.to_string()), json!({})),
        };

        let step = record(
            &mut inner,
            session_id,
            &action,
            before,
            after,
            approval,
            plan.hazard.clone(),
            result,
            error.clone(),
            started_at,
        );
        if let Some(error) = error {
            bail!("{error}");
        }
        Ok(json!({ "result": payload, "step": step }))
    }

    async fn dispatch(&self, inner: &mut Inner, action: &Action) -> Result<Value> {
        let client = inner
            .client
            .as_mut()
            .ok_or_else(|| anyhow!("the computer-use helper is not running"))?;
        client
            .call_tool("computer_use", serde_json::to_value(action)?)
            .await
    }

    async fn authorize<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        broker: &ApprovalBroker,
        session_id: &str,
        request: &super::session::ApprovalRequest,
        plan: &Plan,
    ) -> Result<Authorization> {
        let kind = ApprovalKind::ComputerUse {
            app: request.app.clone(),
            action: request.action.clone(),
            payload: request.payload.clone(),
            hazard: request.hazard,
            element_index: request.element_index,
        };
        let _ = plan;
        let (resolver, receiver) = HostDecisionResolver::pair();
        let origin = ApprovalOrigin {
            session_id: session_id.to_owned(),
            instance_id: format!("computer-use-{}", std::process::id()),
        };
        let approval_id = broker.request(app, origin.clone(), kind, resolver).await?;
        let decision = match tokio::time::timeout(APPROVAL_TIMEOUT, receiver).await {
            Ok(result) => result,
            Err(_) => {
                broker
                    .expire_origin(app, &origin, "approval_timed_out")
                    .await?;
                bail!("computer-use approval timed out");
            }
        }
        .map_err(|_| anyhow!("computer-use approval waiter closed"))?
        .map_err(|reason| anyhow!("computer-use approval expired: {reason}"))?;

        Ok(match decision {
            ApprovalDecision::Reject => Authorization::Rejected,
            ApprovalDecision::Approve { scope } => Authorization::Granted {
                id: approval_id,
                scope: request.remembered_allowed.then_some(scope),
            },
            ApprovalDecision::ApproveWithCap { .. } => Authorization::Granted {
                id: approval_id,
                scope: None,
            },
        })
    }

    /// Apps this session may drive without a fresh card.
    pub async fn allowlisted_apps(&self, session_id: &str) -> Vec<String> {
        let _ = self.inner.lock().await;
        self.allowlist.apps_for(session_id)
    }

    /// Screen locked. Pauses every open run.
    pub async fn on_screen_locked(&self) -> Vec<String> {
        let now = Utc::now();
        let mut inner = self.inner.lock().await;
        let mut paused = Vec::new();
        for (session_id, session) in inner.sessions.iter_mut() {
            if !session.lock_mut().on_lock(now).is_empty() {
                paused.push(session_id.clone());
            }
        }
        paused
    }

    /// Screen unlocked. Resumes runs inside the ceiling and ends the rest.
    pub async fn on_screen_unlocked(&self) -> Vec<String> {
        let now = Utc::now();
        let mut inner = self.inner.lock().await;
        let mut resumed = Vec::new();
        for (session_id, session) in inner.sessions.iter_mut() {
            if !session.lock_mut().on_unlock(now).is_empty() && !session.lock().is_terminal() {
                resumed.push(session_id.clone());
            }
        }
        resumed
    }

    /// Remove the helper and everything it was granted. G7.
    pub async fn remove(&self) -> Result<super::helper::RemovalReport> {
        let mut inner = self.inner.lock().await;
        if let Some(client) = inner.client.take() {
            client.shutdown().await;
        }
        inner.sessions.clear();
        inner.recorders.clear();
        inner.identity = None;
        inner.grants.clear();

        let mut report = helper::remove(&helper::SystemCommands, &helper::helper_bundle_path())?;
        // The allowlist is our own residue and would otherwise survive a
        // reinstall as consent nobody gave again.
        let mut removed = 0u32;
        for entry in self.allowlist.entries() {
            removed += self.allowlist.revoke(&entry.app)? as u32;
        }
        report.allowlist_entries_removed = removed;
        Ok(report)
    }
}

enum Authorization {
    Granted {
        id: String,
        scope: Option<ApprovalScope>,
    },
    Rejected,
}

fn read_grants(reported: &Value) -> Vec<(String, GrantState)> {
    [
        (permissions::ACCESSIBILITY, "accessibility"),
        (permissions::SCREEN_RECORDING, "screenRecording"),
        (permissions::APPLE_EVENTS, "appleEvents"),
    ]
    .into_iter()
    .map(|(id, key)| {
        (
            id.to_owned(),
            GrantState::from_wire(reported.get(key).and_then(Value::as_str).unwrap_or("")),
        )
    })
    .collect()
}

/// Turn a helper `get_app_state` result into the session's element-label map.
fn snapshot(value: &Value) -> super::session::AppSnapshot {
    let mut labels = HashMap::new();
    // The rendered tree is the contract; parsing it back keeps one format
    // rather than two representations that can disagree.
    for line in value
        .get("fullText")
        .or_else(|| value.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .lines()
    {
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((index, tail)) = rest.split_once(']') else {
            continue;
        };
        let Ok(index) = index.trim().parse::<u64>() else {
            continue;
        };
        if let Some(start) = tail.find('"') {
            if let Some(end) = tail[start + 1..].find('"') {
                labels.insert(index, tail[start + 1..start + 1 + end].to_owned());
            }
        }
    }
    super::session::AppSnapshot {
        element_count: value
            .get("elementCount")
            .and_then(Value::as_u64)
            .unwrap_or(labels.len() as u64),
        labels,
    }
}

async fn observe(
    inner: &mut Inner,
    session_id: &str,
    app: Option<&str>,
) -> (StateRef, Option<super::session::AppSnapshot>) {
    let Some(app) = app else {
        return (StateRef::default(), None);
    };
    let Some(client) = inner.client.as_mut() else {
        return (StateRef::default(), None);
    };
    let Ok(state) = client
        .call_tool(
            "computer_use_record_state",
            json!({
                "app": app,
                "include_screenshot": true
            }),
        )
        .await
    else {
        return (StateRef::default(), None);
    };
    let canonical_snapshot = snapshot(&state);
    let observation = Observation {
        ax_text: state
            .get("fullText")
            .and_then(Value::as_str)
            .map(str::to_owned),
        screenshot: state
            .get("screenshotPng")
            .and_then(Value::as_str)
            .and_then(decode_base64),
        element_count: state.get("elementCount").and_then(Value::as_u64),
    };
    let state_ref = inner
        .recorders
        .get(session_id)
        .and_then(|recorder| recorder.capture(&observation).ok())
        .unwrap_or_default();
    (state_ref, Some(canonical_snapshot))
}

#[allow(clippy::too_many_arguments)]
fn record(
    inner: &mut Inner,
    session_id: &str,
    action: &Action,
    before: StateRef,
    after: StateRef,
    approval: Option<String>,
    hazard: Option<super::policy::HazardReason>,
    result: &str,
    error: Option<String>,
    started_at: String,
) -> Option<TrajectoryStep> {
    inner.recorders.get_mut(session_id).map(|recorder| {
        recorder.record(
            action,
            before,
            after,
            approval,
            hazard,
            result,
            error,
            started_at,
            Utc::now().to_rfc3339(),
        )
    })
}

fn decode_base64(encoded: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn status_reports_not_installed_before_anything_is_placed() {
        let dir = tempdir().unwrap();
        let service = ComputerUseService::new(AppAllowlist::open(dir.path().join("a.json")));
        let status = service.status().await;
        assert_eq!(status.plugin_id, COMPUTER_USE_PLUGIN_ID);
        // Three permission rows are always present, even with no helper: a row
        // that disappears reads as "not needed".
        assert_eq!(status.permissions.len(), 3);
        assert_eq!(status.service.active_runs, 0);
    }

    #[test]
    fn helper_grant_names_are_translated_into_desktops_permission_ids() {
        let grants = read_grants(&json!({
            "accessibility": "granted",
            "screenRecording": "denied",
            "appleEvents": "not_applicable"
        }));
        assert_eq!(
            grants[0],
            (permissions::ACCESSIBILITY.into(), GrantState::Granted)
        );
        assert_eq!(
            grants[1],
            (permissions::SCREEN_RECORDING.into(), GrantState::Denied)
        );
        // An absent or unrecognized value must never read as granted.
        let unknown = read_grants(&json!({}));
        assert!(unknown
            .iter()
            .all(|(_, state)| *state == GrantState::NotDetermined));
    }

    /// Hazard classification uses these labels, so parsing the tree back has to
    /// be exact — a missed label silently downgrades a Send button to ordinary.
    #[test]
    fn element_labels_are_recovered_from_the_rendered_tree() {
        let snapshot = snapshot(&json!({
            "elementCount": 3,
            "text": "[0] AXButton \"Send\" actions=[AXPress]\n\
                         [1] AXTextField \"To\" value=\"board@example.com\"\n\
                         [2] AXGroup\n"
        }));
        assert_eq!(snapshot.element_count, 3);
        assert_eq!(snapshot.labels.get(&0).map(String::as_str), Some("Send"));
        assert_eq!(snapshot.labels.get(&1).map(String::as_str), Some("To"));
        // An element with no quoted label simply has none.
        assert!(snapshot.labels.get(&2).is_none());
    }

    #[test]
    fn a_malformed_tree_line_is_skipped_rather_than_shifting_every_index() {
        let snapshot = snapshot(&json!({
            "text": "garbage\n[not-a-number] AXButton \"X\"\n[7] AXButton \"Send\"\n"
        }));
        assert_eq!(snapshot.labels.len(), 1);
        assert_eq!(snapshot.labels.get(&7).map(String::as_str), Some("Send"));
    }
}
