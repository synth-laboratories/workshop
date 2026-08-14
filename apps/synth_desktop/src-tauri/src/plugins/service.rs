//! Product-plugin lifecycle. Mutating operations go through the approval broker.

use super::policy::{auto_decision, compute_kind, plugin_kind};
use super::registry::PluginRegistry;
use super::types::{
    digest_ref, redact_secrets, CatalogEntry, PluginActionReceipt, PluginNotReady,
    PluginServiceStatus, PluginStatus, OPTIMIZERS_PLUGIN_ID, PLUGIN_ACTION_RECEIPT_SCHEMA,
    PLUGIN_STATUS_SCHEMA,
};
use crate::core_runtime::CoreRuntime;
use crate::session::approval::{
    ApprovalBroker, ApprovalDecision, ApprovalKind, ApprovalOrigin, HostDecisionResolver,
};
use crate::synth_config;
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;
use uuid::Uuid;

const RETAIN_RUNTIME: &str =
    "Retains the installed distribution, mirrored runs, imported artifacts, and replay templates.";
const RETAIN_AFTER_STOP: &str =
    "Stops the local service and workers. Installed distribution, run mirror, artifacts, and visual templates are retained.";
const RETAIN_AFTER_REMOVE: &str =
    "Removes the installed runtime only. Mirrored runs, results, artifacts, and retained templates stay.";
const ENABLE_RETENTION: &str =
    "Does not start, stop, install, or delete the optimizer service or retained data.";

#[derive(Clone)]
pub struct PluginService {
    registry: Arc<PluginRegistry>,
}

impl PluginService {
    pub fn new(registry: PluginRegistry) -> Self {
        Self {
            registry: Arc::new(registry),
        }
    }

    pub fn open_default() -> Self {
        Self::new(PluginRegistry::open_default())
    }

    pub fn registry(&self) -> &PluginRegistry {
        &self.registry
    }

    pub async fn status(&self, core: &CoreRuntime) -> PluginStatus {
        let sidecar = core.optimizers().manager().refresh().await;
        let capabilities = core.optimizers().manager().advertised_capabilities();
        let active_runs = core
            .optimizers()
            .manager()
            .active_gepa_run_ids()
            .await
            .len() as u32;
        let enabled = self.registry.is_enabled();
        let phase = map_phase(enabled, &sidecar.phase, sidecar.version.is_some());
        let status = PluginStatus {
            schema_version: PLUGIN_STATUS_SCHEMA.into(),
            plugin_id: OPTIMIZERS_PLUGIN_ID.into(),
            enabled,
            phase,
            installed_version: sidecar.version.clone(),
            selected_version: sidecar.version.clone(),
            digest: sidecar.digest.as_deref().map(digest_ref),
            service: PluginServiceStatus {
                phase: sidecar.phase.clone(),
                started_at: (sidecar.phase == "ready").then(|| Utc::now().to_rfc3339()),
                active_runs,
            },
            capabilities_digest: capabilities
                .get("digest")
                .and_then(Value::as_str)
                .map(digest_ref),
            algorithms: capabilities
                .get("algorithms")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_else(|| vec!["gepa".into()]),
            templates: capabilities
                .get("compatibleTemplateIds")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_else(|| {
                    vec!["optimizer.gepa.live.v1".into(), "optimizer.run.v1".into()]
                }),
            last_action_receipt_id: None,
            detail: sidecar.detail.as_deref().map(redact_secrets),
        };
        self.registry.apply_to_status(status)
    }

    pub async fn capabilities(&self, core: &CoreRuntime) -> Result<Value> {
        let status = self.status(core).await;
        if !matches!(
            status.service.phase.as_str(),
            "ready" | "starting" | "installed" | "stopped"
        ) && status.installed_version.is_none()
        {
            return Err(PluginNotReady::new(&status.phase, "install").into());
        }
        Ok(core.optimizers().manager().advertised_capabilities())
    }

    pub(crate) async fn manage<R: tauri::Runtime>(
        &self,
        core: &CoreRuntime,
        broker: &ApprovalBroker,
        app: &AppHandle<R>,
        session_id: Option<&str>,
        operation: &str,
        arguments: &Value,
    ) -> Result<Value> {
        validate_plugin_arguments(operation, arguments)?;
        let plugin_id = arguments
            .get("plugin_id")
            .and_then(Value::as_str)
            .unwrap_or(OPTIMIZERS_PLUGIN_ID);
        if plugin_id != OPTIMIZERS_PLUGIN_ID {
            bail!("unknown plugin_id `{plugin_id}`");
        }
        let version = arguments
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned);
        match operation {
            "list" => {
                let status = self.status(core).await;
                Ok(json!({ "plugins": [status] }))
            }
            "status" => Ok(serde_json::to_value(self.status(core).await)?),
            "capabilities" => self.capabilities(core).await,
            "enable" | "disable" | "install" | "start" | "stop" | "update" | "remove" => {
                self.mutate(
                    core, broker, app, session_id, operation, version.as_deref(),
                )
                .await
            }
            other => bail!("unknown plugin operation `{other}`"),
        }
    }

    async fn mutate<R: tauri::Runtime>(
        &self,
        core: &CoreRuntime,
        broker: &ApprovalBroker,
        app: &AppHandle<R>,
        session_id: Option<&str>,
        action: &str,
        version: Option<&str>,
    ) -> Result<Value> {
        let started_at = Utc::now().to_rfc3339();
        let catalog = PluginRegistry::catalog_entry(version)?;
        let before = self.status(core).await;
        let active_runs = before.service.active_runs;
        if action == "stop" && active_runs > 0 {
            bail!("refusing to stop Optimizers while {active_runs} run(s) are active");
        }
        if action == "remove" && active_runs > 0 {
            bail!("refusing to remove Optimizers while {active_runs} run(s) are active");
        }
        let retention = match action {
            "enable" | "disable" => ENABLE_RETENTION,
            "stop" => RETAIN_AFTER_STOP,
            "remove" => RETAIN_AFTER_REMOVE,
            _ => RETAIN_RUNTIME,
        };
        let kind = plugin_kind(
            action,
            &catalog,
            active_runs as u64,
            before.digest.clone(),
            retention,
        );
        let approval = self
            .authorize(broker, app, session_id, kind, active_runs as u64)
            .await?;
        if approval.rejected {
            return Ok(json!({
                "schemaVersion": PLUGIN_ACTION_RECEIPT_SCHEMA,
                "result": "approval_rejected",
                "approvalReceiptId": approval.approval_id,
                "pluginId": OPTIMIZERS_PLUGIN_ID,
                "action": action,
            }));
        }
        let outcome = self.execute(core, action, &catalog).await;
        let finished_at = Utc::now().to_rfc3339();
        let status = self.status(core).await;
        let (result, error) = match &outcome {
            Ok(()) => ("ok".into(), None),
            Err(error) => ("error".into(), Some(redact_secrets(&error.to_string()))),
        };
        let receipt = self.registry.record_receipt(PluginActionReceipt {
            schema_version: PLUGIN_ACTION_RECEIPT_SCHEMA.into(),
            receipt_id: format!("plugin_action_{}", Uuid::new_v4().simple()),
            plugin_id: OPTIMIZERS_PLUGIN_ID.into(),
            action: action.into(),
            version: status.installed_version.clone().or(Some(catalog.version.clone())),
            digest: status.digest.clone(),
            approval_receipt_id: Some(approval.approval_id),
            started_at,
            finished_at,
            result,
            retained_data: retention.into(),
            status: Some(status),
            error,
        })?;
        outcome?;
        Ok(serde_json::to_value(receipt)?)
    }

    async fn execute(
        &self,
        core: &CoreRuntime,
        action: &str,
        catalog: &CatalogEntry,
    ) -> Result<()> {
        let manager = core.optimizers().manager();
        match action {
            "enable" => {
                self.registry.set_enabled(true)?;
                Ok(())
            }
            "disable" => {
                self.registry.set_enabled(false)?;
                Ok(())
            }
            "install" | "update" => {
                manager
                    .set_status_phase("downloading", Some("Downloading optimizer distribution…"))
                    .await;
                match manager.install(Some(&catalog.version)) {
                    Ok(_) => {
                        manager
                            .set_status_phase(
                                "verifying",
                                Some("Verifying digest, signature, and offline runtime…"),
                            )
                            .await;
                        if !manager.has_offline_runtime(&catalog.version) {
                            manager
                                .set_status_phase(
                                    "error",
                                    Some("Installed distribution is missing an offline runtime"),
                                )
                                .await;
                            bail!("installed optimizer distribution cannot start offline");
                        }
                        manager
                            .set_status_phase("installed", Some("Optimizer distribution installed"))
                            .await;
                        Ok(())
                    }
                    Err(error) => {
                        manager
                            .set_status_phase("error", Some(&redact_secrets(&error.to_string())))
                            .await;
                        Err(error)
                    }
                }
            }
            "start" => {
                manager.start().await?;
                Ok(())
            }
            "stop" => {
                manager.stop().await?;
                Ok(())
            }
            "remove" => {
                let version = catalog.version.clone();
                manager.uninstall(&version, core.optimizers()).await?;
                Ok(())
            }
            other => bail!("unknown plugin action `{other}`"),
        }
    }

    async fn authorize<R: tauri::Runtime>(
        &self,
        broker: &ApprovalBroker,
        app: &AppHandle<R>,
        session_id: Option<&str>,
        kind: ApprovalKind,
        active_runs: u64,
    ) -> Result<Authorization> {
        let policy = synth_config::desktop_permission_settings()
            .map(|settings| settings.approval_policy)
            .unwrap_or_else(|_| "untrusted".into());
        if let Some(decision) = auto_decision(&policy, &kind, active_runs)? {
            let session_id = session_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("policy-auto");
            let approval_id = broker
                .record_auto(app, session_id, &kind, &decision)
                .await?;
            return Ok(Authorization {
                approval_id,
                rejected: false,
            });
        }
        let session_id = session_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("plugin mutations require a session to request approval"))?;
        let (resolver, rx) = HostDecisionResolver::pair();
        let approval_id = broker
            .request(
                app,
                ApprovalOrigin {
                    session_id: session_id.into(),
                    instance_id: format!("desktop-{}", std::process::id()),
                },
                kind,
                resolver,
            )
            .await?;
        let decision = rx
            .await
            .map_err(|_| anyhow!("plugin approval waiter closed"))?
            .map_err(|reason| anyhow!("plugin approval expired: {reason}"))?;
        Ok(Authorization {
            approval_id,
            rejected: matches!(decision, ApprovalDecision::Reject),
        })
    }

    pub(crate) async fn authorize_compute<R: tauri::Runtime>(
        &self,
        broker: &ApprovalBroker,
        app: &AppHandle<R>,
        session_id: Option<&str>,
        recipe_id: &str,
        preparation_digest: &str,
        max_cost_usd: f64,
        max_rollouts: u64,
        proposer_model: &str,
        timeout_seconds: u64,
    ) -> Result<Authorization> {
        let kind = compute_kind(
            recipe_id,
            preparation_digest,
            max_cost_usd,
            max_rollouts,
            proposer_model,
            timeout_seconds,
        );
        self.authorize(broker, app, session_id, kind, 0).await
    }
}

pub(crate) struct Authorization {
    pub approval_id: String,
    pub rejected: bool,
}

fn map_phase(enabled: bool, sidecar_phase: &str, installed: bool) -> String {
    if !enabled && sidecar_phase != "ready" {
        return "disabled".into();
    }
    match sidecar_phase {
        "unknown" | "" if !installed => "not_installed".into(),
        "unknown" => "installed".into(),
        other => other.into(),
    }
}

fn validate_plugin_arguments(operation: &str, arguments: &Value) -> Result<()> {
    let Some(object) = arguments.as_object() else {
        bail!("plugin arguments must be an object");
    };
    for key in object.keys() {
        if key != "plugin_id" && key != "version" && key != "sessionRef" && key != "session_id" {
            bail!("plugin arguments reject `{key}`");
        }
    }
    if object.contains_key("url")
        || object.contains_key("path")
        || object.contains_key("command")
        || object.contains_key("env")
        || object.contains_key("token")
    {
        bail!("plugin arguments reject URLs, paths, commands, env, and tokens");
    }
    let _ = operation;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_rejects_urls_and_arbitrary_keys() {
        let err = validate_plugin_arguments(
            "install",
            &json!({"plugin_id":"optimizers","url":"https://evil.example"}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("reject"));
        validate_plugin_arguments("status", &json!({"plugin_id":"optimizers"})).unwrap();
        validate_plugin_arguments(
            "install",
            &json!({"plugin_id":"optimizers","version":"0.2.0"}),
        )
        .unwrap();
    }
}
