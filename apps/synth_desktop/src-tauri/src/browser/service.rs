use super::client::BrowserClient;
use crate::session::approval::{ApprovalBroker, ApprovalKind};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;

const ACTION_OPERATIONS: [&str; 7] = [
    "browser_click",
    "browser_fill",
    "browser_press",
    "browser_upload",
    "browser_download",
    "browser_scroll",
    "browser_handle_dialog",
];

#[derive(Default)]
pub struct BrowserService {
    client: Mutex<Option<BrowserClient>>,
    crashes: AtomicU64,
}

impl BrowserService {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn runtime_status(&self) -> super::BrowserRuntimeStatus {
        let mut status = super::runtime_status();
        status.crash_count = self.crashes.load(Ordering::Relaxed).min(u32::MAX.into()) as u32;
        status.service_running = self.client.lock().await.is_some();
        status
    }

    async fn with_client(&self, operation: &str, arguments: Value) -> Result<Value> {
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            *guard = Some(BrowserClient::spawn().await?);
        }
        let result = guard
            .as_mut()
            .expect("browser client initialized")
            .call(operation, arguments)
            .await;
        let transport_failed = result.as_ref().err().is_some_and(|error| {
            let message = error.to_string();
            [
                "backend crashed",
                "backend timed out",
                "write browser request",
                "flush browser request",
            ]
            .iter()
            .any(|marker| message.contains(marker))
        });
        if transport_failed {
            self.crashes.fetch_add(1, Ordering::Relaxed);
            *guard = None;
        }
        result
    }

    pub(crate) async fn call<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        broker: &ApprovalBroker,
        agent_session_id: Option<&str>,
        operation: &str,
        arguments: Value,
    ) -> Result<Value> {
        if operation == "browser_status" {
            return Ok(serde_json::to_value(self.runtime_status().await)?);
        }
        if operation == "browser_claim_chrome" {
            let agent_session_id = agent_session_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("claiming a Chrome tab requires an agent session"))?;
            let endpoint = arguments
                .get("cdp_endpoint")
                .or_else(|| arguments.get("cdpEndpoint"))
                .and_then(Value::as_str)
                .unwrap_or("http://127.0.0.1:9222");
            let kind = ApprovalKind::ComputerUse {
                app: "browser:claimed-chrome".to_owned(),
                action: operation.to_owned(),
                payload: json!({
                    "cdpEndpoint": endpoint,
                    "titleContains": arguments.get("title_contains").or_else(|| arguments.get("titleContains")),
                    "urlContains": arguments.get("url_contains").or_else(|| arguments.get("urlContains")),
                }),
                hazard: true,
                element_index: None,
            };
            broker
                .authorize_host(app, Some(agent_session_id), kind)
                .await?;
            return self.with_client(operation, arguments).await;
        }
        if !ACTION_OPERATIONS.contains(&operation) {
            return self.with_client(operation, arguments).await;
        }

        let prepared = self
            .with_client(
                "browser_prepare_action",
                json!({ "operation": operation, "arguments": arguments }),
            )
            .await?;
        let result = prepared.get("result").unwrap_or(&prepared);
        let token = result
            .get("actionToken")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("browser did not return an action token"))?;
        if result
            .get("consequential")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let agent_session_id = agent_session_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("consequential browser actions require an agent session"))?;
            let name = result
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unnamed target");
            let role = result
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("element");
            let origin = result
                .get("origin")
                .and_then(Value::as_str)
                .unwrap_or("unknown origin");
            let kind = ApprovalKind::ComputerUse {
                app: format!("browser:{origin}"),
                action: operation.to_owned(),
                payload: json!({
                    "origin": origin,
                    "role": role,
                    "name": name,
                    "tab": result.get("tabId"),
                    "documentRevision": result.get("documentRevision"),
                    "actionDetails": result.get("actionDetails"),
                }),
                hazard: true,
                element_index: None,
            };
            broker
                .authorize_host(app, Some(agent_session_id), kind)
                .await?;
        }
        self.with_client("browser_commit_action", json!({ "action_token": token }))
            .await
    }

    pub async fn stop(&self) {
        if let Some(client) = self.client.lock().await.take() {
            client.stop().await;
        }
    }
}

impl crate::services::ManagedService for BrowserService {
    fn name(&self) -> &'static str {
        "browser"
    }

    fn probe(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>> {
        Box::pin(async move { Ok(super::runtime_status().phase == "ready") })
    }

    fn stop(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            BrowserService::stop(self).await;
            Ok(())
        })
    }
}
