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

/// One observation of the main window during a fullscreen transition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct WindowSample {
    pub fullscreen: bool,
    /// Physical inner size.
    pub size: (u32, u32),
    /// Physical size of the monitor the window is on, when that is known.
    pub monitor: Option<(u32, u32)>,
}

/// Whether a transition toward `target` has actually settled.
///
/// macOS animates the transition and sets the style-mask bit when the animation
/// *starts*, so `is_fullscreen()` on its own reports intent, not arrival. Worse,
/// an operation that arrives mid-animation can make AppKit abandon the
/// transition, leaving the window at its old size with the bit cleared again —
/// which is how a present call acknowledged success and a capture taken
/// afterwards found a 1280x900 window reporting `windowFullscreen=false`.
///
/// Requiring the geometry to agree with the flag, and to have stopped moving,
/// is what separates a settled window from one still in flight.
pub(crate) fn fullscreen_settled(
    target: bool,
    sample: WindowSample,
    previous: Option<WindowSample>,
) -> bool {
    // The flag must say what we asked for, and the window must have held still
    // for a whole interval: mid-animation every sample differs from the last.
    if sample.fullscreen != target || previous != Some(sample) {
        return false;
    }
    match sample.monitor {
        // Entering: the content area has to cover substantially all of its
        // monitor. AppKit's fullscreen inner size excludes the display's
        // camera/menu-bar safe area (66 physical pixels on the acceptance
        // machine), so requiring exact monitor height rejects a window that
        // has actually arrived. Integer arithmetic keeps this deterministic.
        Some(monitor) if target => {
            u64::from(sample.size.0) * 100 >= u64::from(monitor.0) * 95
                && u64::from(sample.size.1) * 100 >= u64::from(monitor.1) * 95
        }
        // Leaving: it has to have stopped covering substantially all of the
        // monitor under the same safe-area-aware definition.
        Some(monitor) => {
            u64::from(sample.size.0) * 100 < u64::from(monitor.0) * 95
                || u64::from(sample.size.1) * 100 < u64::from(monitor.1) * 95
        }
        // Nothing to compare against; the flag plus stability is all there is.
        None => true,
    }
}

fn sample_window(window: &tauri::WebviewWindow) -> anyhow::Result<WindowSample> {
    let size = window.inner_size()?;
    Ok(WindowSample {
        fullscreen: window.is_fullscreen()?,
        size: (size.width, size.height),
        monitor: window
            .current_monitor()?
            .map(|monitor| (monitor.size().width, monitor.size().height)),
    })
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
                // Poll until the window has arrived, not until it has agreed to
                // leave. Sampling twice as fast as the settle check needs means
                // a stable pair is two consecutive readings of a still window.
                let settled = tokio::time::timeout(std::time::Duration::from_secs(8), async {
                    let mut previous = None;
                    loop {
                        let sample = sample_window(&window)?;
                        if fullscreen_settled(request.fullscreen, sample, previous) {
                            return Ok::<_, anyhow::Error>(sample);
                        }
                        previous = Some(sample);
                        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
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
                    // Observed after the transition settled, so a caller does
                    // not have to take the acknowledgement's word for it.
                    fullscreen: settled.fullscreen,
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
    use super::{fullscreen_settled, WindowSample};

    const MONITOR: (u32, u32) = (3456, 2234);

    fn sample(fullscreen: bool, size: (u32, u32)) -> WindowSample {
        WindowSample { fullscreen, size, monitor: Some(MONITOR) }
    }

    #[test]
    fn the_style_mask_flipping_is_not_arrival() {
        // What the old wait accepted: macOS sets the bit when the animation
        // starts, while the window is still its old size. Acknowledging here is
        // what let a later capture find a window that was never fullscreen.
        let starting = sample(true, (1728, 1084));
        assert!(!fullscreen_settled(true, starting, Some(starting)));
    }

    #[test]
    fn a_window_still_growing_has_not_settled() {
        let first = sample(true, (2400, 1600));
        let second = sample(true, (3000, 1900));
        assert!(!fullscreen_settled(true, second, Some(first)));
    }

    #[test]
    fn covering_the_monitor_and_holding_still_is_settled() {
        let arrived = sample(true, MONITOR);
        // One reading is never enough; the pair is what proves it stopped.
        assert!(!fullscreen_settled(true, arrived, None));
        assert!(fullscreen_settled(true, arrived, Some(arrived)));
    }

    #[test]
    fn fullscreen_content_safe_area_is_still_settled() {
        // Native acceptance on a 3456x2234 display observes a 3456x2168
        // fullscreen content surface: AppKit reserves 66 physical pixels for
        // the display safe area even though the style mask is fullscreen.
        let arrived = sample(true, (3456, 2168));
        assert!(fullscreen_settled(true, arrived, Some(arrived)));
    }

    #[test]
    fn leaving_fullscreen_settles_only_once_the_window_shrinks() {
        let still_covering = sample(false, MONITOR);
        assert!(!fullscreen_settled(false, still_covering, Some(still_covering)));
        let shrunk = sample(false, (1728, 1084));
        assert!(fullscreen_settled(false, shrunk, Some(shrunk)));
    }

    #[test]
    fn an_abandoned_transition_is_never_reported_as_settled() {
        // The observed failure: AppKit gives up, the bit clears and the window
        // sits at a size an earlier capture left behind.
        let reverted = sample(false, (1280, 900));
        assert!(!fullscreen_settled(true, reverted, Some(reverted)));
    }

    #[test]
    fn without_a_monitor_the_flag_and_stability_are_all_there_is() {
        let unknown = WindowSample { fullscreen: true, size: (1280, 900), monitor: None };
        assert!(fullscreen_settled(true, unknown, Some(unknown)));
        assert!(!fullscreen_settled(false, unknown, Some(unknown)));
    }

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
