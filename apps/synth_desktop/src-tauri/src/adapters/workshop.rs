//! Authenticated local Workshop entry point. This adapter assembles domain
//! operations; it never opens a second database or creates a hosted session.

use crate::contract::capabilities::PublicOperation;
use crate::core_runtime::CoreRuntime;
use crate::domains::visuals::operations::*;
use base64::Engine;
use serde_json::{json, Value};
use std::{future::Future, pin::Pin};
use tauri::{AppHandle, Manager};

type Reply<'a> = Pin<Box<dyn Future<Output = anyhow::Result<Value>> + Send + 'a>>;
type Handler = for<'a> fn(&'a CoreRuntime, &'a AppHandle, Value) -> Reply<'a>;

struct Registration {
    name: &'static str,
    descriptor: fn() -> Value,
    handler: Handler,
}

fn register<O: PublicOperation>(handler: Handler) -> Registration {
    Registration {
        name: O::MCP_NAME,
        descriptor: O::mcp_tool,
        handler,
    }
}

fn registrations() -> Vec<Registration> {
    vec![
        register::<crate::domains::runtime::Present>(|core, app, args| {
            Box::pin(async move {
                let request: crate::domains::runtime::PresentRequest = serde_json::from_value(args)?;
                use crate::domains::runtime::Destination;
                match &request.destination {
                    Destination::Chat { chat_id } => { core.sessions().get(chat_id.clone()).await?; },
                    Destination::Sync { session_id } | Destination::Async { session_id } => { core.sessions().get(session_id.clone()).await?; },
                    _ => {}
                }
                crate::platform::desktop_runtime::ensure_desktop(app).await?;
                let request_id = uuid::Uuid::new_v4().to_string();
                let payload = json!({"requestId": request_id, "view": request.destination});
                app.get_webview_window("main").ok_or_else(|| anyhow::anyhow!("desktop unavailable"))?.eval(format!("window.__workshopAppPresentation={payload};window.dispatchEvent(new CustomEvent('workshop:app-present',{{detail:window.__workshopAppPresentation}}));"))?;
                Ok(serde_json::to_value(crate::domains::runtime::PresentResult { request_id, requested: true })?)
            })
        }),
        register::<crate::domains::desktop_state::Update>(|core, _, args| {
            Box::pin(async move { Ok(serde_json::to_value(crate::domains::desktop_state::write(core, serde_json::from_value(args)?, true)?)?) })
        }),
        register::<crate::domains::runtime::Status>(|_, app, args| {
            Box::pin(async move {
                let _: crate::domains::runtime::StatusRequest = serde_json::from_value(args)?;
                Ok(crate::platform::desktop_runtime::status(app))
            })
        }),
        register::<crate::domains::runtime::Control>(|_, app, args| {
            Box::pin(async move {
                let request: crate::domains::runtime::ControlRequest =
                    serde_json::from_value(args)?;
                crate::platform::desktop_runtime::control(app, request.action.as_str()).await
            })
        }),
        register::<crate::domains::runtime::Capture>(|core, app, args| {
            Box::pin(async move {
                let request: crate::domains::runtime::CaptureRequest =
                    serde_json::from_value(args)?;
                crate::platform::desktop_runtime::ensure_desktop(app).await?;
                let output = core
                    .storage()
                    .content_root()
                    .join("exports")
                    .join(format!("app-capture-{}.png", uuid::Uuid::new_v4()));
                let mut body = serde_json::to_value(request)?;
                body["outputPath"] = json!(output);
                let receipt = crate::visuals_ipc::capture_surface(app, &body).await?;
                let bytes = std::fs::read(output)?;
                Ok(
                    json!({"receipt":receipt,"_mcpImage":{"data":base64::engine::general_purpose::STANDARD.encode(bytes),"mimeType":"image/png"}}),
                )
            })
        }),
        register::<ListVisualTemplates>(|core, _, args| {
            Box::pin(async move {
                Ok(serde_json::to_value(ListVisualTemplates::execute(
                    &core.visuals(),
                    serde_json::from_value(args)?,
                )?)?)
            })
        }),
        register::<ListVisuals>(|core, _, args| {
            Box::pin(async move {
                Ok(serde_json::to_value(
                    ListVisuals::execute(&core.visuals(), serde_json::from_value(args)?).await?,
                )?)
            })
        }),
        register::<GetVisual>(|core, _, args| {
            Box::pin(async move {
                Ok(serde_json::to_value(
                    GetVisual::execute(&core.visuals(), serde_json::from_value(args)?).await?,
                )?)
            })
        }),
        register::<CreateVisual>(|core, _, args| {
            Box::pin(async move {
                let (result, event) =
                    CreateVisual::execute(&core.visuals(), serde_json::from_value(args)?).await?;
                core.broadcast_committed(Some(serde_json::from_value(event)?));
                Ok(serde_json::to_value(result)?)
            })
        }),
        register::<UpdateVisual>(|core, _, args| {
            Box::pin(async move {
                let (result, event) =
                    UpdateVisual::execute(&core.visuals(), serde_json::from_value(args)?).await?;
                core.broadcast_committed(Some(serde_json::from_value(event)?));
                Ok(serde_json::to_value(result)?)
            })
        }),
        register::<PresentVisual>(|core, app, args| {
            Box::pin(async move {
                let request: PresentRequest = serde_json::from_value(args)?;
                let visual = core.visuals().get(request.visual_id).await?;
                crate::platform::desktop_runtime::ensure_desktop(app).await?;
                let window = app.get_webview_window("main").ok_or_else(|| {
                    anyhow::anyhow!("presentation_unavailable: Workshop has no main window")
                })?;
                window.set_fullscreen(request.fullscreen)?;
                tokio::time::timeout(std::time::Duration::from_secs(8), async {
                    loop {
                        if window.is_fullscreen()? == request.fullscreen {
                            return Ok::<_, anyhow::Error>(());
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                    }
                })
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "presentation_unavailable: fullscreen transition did not settle"
                    )
                })??;
                let intent =
                    json!({"visualId": visual.id, "requestId": uuid::Uuid::new_v4().to_string()});
                window.eval(format!(
                "window.__workshopVisualPresentation={intent};window.dispatchEvent(new CustomEvent('workshop:visual-present',{{detail:window.__workshopVisualPresentation}}));"
            ))?;
                Ok(serde_json::to_value(PresentResult {
                    visual_id: visual.id,
                    revision: visual.current_revision,
                    requested: true,
                })?)
            })
        }),
        register::<ObserveVisual>(|core, _, args| {
            Box::pin(async move {
                let request: GetRequest = serde_json::from_value(args)?;
                let visual = core.visuals().get(request.visual_id).await?;
                let observation = crate::visuals_ipc::rendered_observation(&visual.id)
                    .ok()
                    .map(serde_json::to_value)
                    .transpose()?;
                Ok(serde_json::to_value(ObserveResult {
                    visual_id: visual.id,
                    revision: visual.current_revision,
                    observation,
                })?)
            })
        }),
        register::<CaptureVisual>(|core, app, args| {
            Box::pin(async move {
                let request: GetRequest = serde_json::from_value(args)?;
                let before = core.visuals().get(request.visual_id).await?;
                let output = core
                    .storage()
                    .content_root()
                    .join("exports")
                    .join(format!("visual-capture-{}.png", uuid::Uuid::new_v4()));
                let receipt = crate::visuals_ipc::capture_surface(
                    app,
                    &json!({
                        "scope":"visual", "target": before.id, "outputPath": output
                    }),
                )
                .await?;
                let after = core.visuals().get(before.id.clone()).await?;
                anyhow::ensure!(
                    before.current_revision == after.current_revision,
                    "visual_revision_changed_during_capture"
                );
                anyhow::ensure!(receipt.pointer("/appState/mountedVisuals").and_then(Value::as_array)
                    .is_some_and(|visuals| visuals.iter().any(|visual| visual["visualId"] == before.id
                        && visual["revision"] == before.current_revision)), "capture_revision_mismatch: renderer did not mount the requested visual revision");
                let bytes = std::fs::read(output)?;
                let mut result = serde_json::to_value(CaptureResult {
                    visual_id: before.id,
                    revision: before.current_revision,
                    receipt,
                })?;
                result["_mcpImage"] = json!({"data":base64::engine::general_purpose::STANDARD.encode(bytes),"mimeType":"image/png"});
                Ok(result)
            })
        }),
    ]
}

fn build_tools() -> Value {
    let mut tools: Vec<Value> = registrations()
        .iter()
        .map(|entry| (entry.descriptor)())
        .collect();
    let generated: Value = serde_json::from_str(include_str!("../contract/desktop_tools.json"))
        .expect("generated desktop tool schemas must be valid JSON");
    for mut tool in generated["tools"].as_array().unwrap().iter().cloned() {
        let name = tool["name"].as_str().unwrap();
        if crate::contract::desktop_policy::internal(name) {
            continue;
        }
        if let Some(surface) = crate::contract::desktop_policy::human_surface(name) {
            tool["description"] = json!(format!("This action requires a human in {surface}. The agent may explain the requested action but cannot supply this decision or human evidence through MCP."));
            tool["_meta"]["workshop/humanSurface"] = json!(surface);
        }
        tools.push(tool);
    }
    for adapter in crate::adapters::mcp::operations::registrations() {
        for mut tool in (adapter.tools)()["tools"].as_array().expect("adapter tools").iter().cloned() {
            let name = tool["name"].as_str().expect("named tool").to_owned();
            // This facade is fully expanded into named operations below. Keeping
            // it would bypass the typed visual revision and ownership contracts.
            if name == "visual_manage" || tools.iter().any(|entry| entry["name"] == name) {
                continue;
            }
            tool["_meta"]["workshop/operationId"] = json!(format!("{}.{}.v1", adapter.domain, name));
            tool["_meta"]["workshop/owner"] = json!(adapter.domain);
            tools.push(tool);
        }
    }
    tools.extend(crate::browser::operations::tools()["tools"].as_array().expect("browser tools").iter().cloned());
    json!({"tools": tools})
}

pub fn tools() -> Value {
    static TOOLS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    TOOLS.get_or_init(build_tools).clone()
}

/// Scope is explicit even though this first connection grants the whole local
/// instance. A descriptor copied from another instance must fail closed.
pub async fn dispatch(
    core: &CoreRuntime,
    app: &AppHandle,
    route: &str,
    body: Value,
) -> anyhow::Result<Value> {
    let root = core
        .storage()
        .database()
        .path()
        .parent()
        .ok_or_else(|| anyhow::anyhow!("instance root unavailable"))?
        .canonicalize()?;
    let requested = body
        .get("data_root")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("data_root is required"))?;
    anyhow::ensure!(
        std::path::Path::new(requested).canonicalize()? == root,
        "instance_scope_mismatch"
    );
    anyhow::ensure!(
        body.get("contract_version").and_then(Value::as_u64) == Some(1),
        "unsupported Workshop client contract"
    );
    let workspace_id: String = core.storage().database().with_conn(|conn| {
        Ok(conn.query_row(
            "SELECT id FROM workshop_workspaces WHERE local_instance = 1",
            [],
            |row| row.get(0),
        )?)
    })?;
    match route {
        "/v1/workshop/describe" => Ok(json!({
            "contractVersion": 1, "runtimeVersion": env!("CARGO_PKG_VERSION"),
            "dataRoot": root, "workspaceId": workspace_id, "scope": "local-instance", "tools": tools()["tools"],
            "features": {"visuals": true, "fullscreen": true, "agentWake": false, "headless": true, "acpHosting": true, "desktopOperationCoverage": true, "fullProductCoverage": true},
            "runtime": crate::platform::desktop_runtime::status(app)
        })),
        "/v1/workshop/call" => {
            let name = body.get("name").and_then(Value::as_str).unwrap_or_default();
            let entry = registrations().into_iter().find(|entry| entry.name == name);
            let args = body.get("arguments").cloned().unwrap_or_else(|| json!({}));
            if let Some(entry) = entry {
                return (entry.handler)(core, app, args).await;
            }
            anyhow::ensure!(
                !crate::contract::desktop_policy::internal(name),
                "renderer_callback_required: {name} is not an agent action"
            );
            if let Some(surface) = crate::contract::desktop_policy::human_surface(name) {
                anyhow::bail!("human_action_required: {name} must be completed by the user in {surface}; no action or approval was performed");
            }
            if crate::contract::desktop_dispatch::NAMES.contains(&name) {
                return crate::contract::desktop_dispatch::invoke(app, name, args).await;
            }
            if crate::browser::operations::contains(name) {
                return app.state::<std::sync::Arc<crate::browser::operations::Manager>>().call(name, args).await;
            }
            for adapter in crate::adapters::mcp::operations::registrations() {
                if name != "visual_manage" && (adapter.tools)()["tools"].as_array().is_some_and(|tools| tools.iter().any(|tool| tool["name"] == name)) {
                    crate::adapters::mcp::operations::check_agent_call(name, &args)?;
                    if matches!(name, "visual_chart" | "visual_capture_review" | "visual_show" | "visual_open_in_pane" | "workshop_display" | "workshop_capture" | "document_show") || (name == "human_annotation_manage" && args["operation"] == "human_annotation_show") {
                        crate::platform::desktop_runtime::ensure_desktop(app).await?;
                    }
                    let owned_name = name.to_owned();
                    return tokio::task::spawn_blocking(move || (adapter.call)(&owned_name, &args))
                        .await?.map_err(anyhow::Error::msg);
                }
            }
            anyhow::bail!("unknown Workshop operation: {name}")
        }
        _ => anyhow::bail!("unsupported Workshop route"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_discovered_operation_has_a_unique_executable_registration() {
        let entries = super::registrations();
        let names: std::collections::HashSet<_> = entries.iter().map(|entry| entry.name).collect();
        assert_eq!(names.len(), entries.len());
        for entry in entries {
            assert_eq!((entry.descriptor)()["name"], entry.name);
            assert!((entry.descriptor)()["outputSchema"].is_object());
        }
        let tools = super::tools();
        let names: std::collections::HashSet<_> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), tools["tools"].as_array().unwrap().len());
        for adapter in crate::adapters::mcp::operations::registrations() {
            for descriptor in (adapter.tools)()["tools"].as_array().unwrap() {
                let name = descriptor["name"].as_str().unwrap();
                assert!(name == "visual_manage" || names.contains(name), "missing {} operation {name}", adapter.domain);
            }
        }
        // The compact legacy facade is expanded into named operations. These
        // two aliases use the existing generated desktop contract.
        for name in ["visuals_save", "visuals_render", "visual_bind_data_source", "desktop_state_update", "app_present"] {
            assert!(names.contains(name), "missing facade/UI semantic operation {name}");
        }
        for descriptor in crate::browser::operations::tools()["tools"].as_array().unwrap() {
            assert!(names.contains(descriptor["name"].as_str().unwrap()));
        }
        for name in crate::contract::desktop_dispatch::NAMES {
            assert_eq!(
                names.contains(name),
                !crate::contract::desktop_policy::internal(name)
            );
        }
    }
}
