//! One runtime owner with zero or one desktop views. Window attachment never
//! opens storage or starts another set of services.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

static PAGE_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn page_ready(ready: bool) {
    PAGE_READY.store(ready, std::sync::atomic::Ordering::SeqCst);
}
pub async fn ensure_desktop(app: &AppHandle) -> Result<()> {
    control(app, "attach").await?;
    tokio::time::timeout(std::time::Duration::from_secs(25), async {
        while !PAGE_READY.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .context("desktop_not_ready: native page did not finish loading")?;
    Ok(())
}

pub fn status(app: &AppHandle) -> Value {
    json!({"processId": std::process::id(), "bootId": crate::instance::boot_epoch(),
        "desktopAttached": app.get_webview_window("main").is_some(),
        "headlessSupported": true})
}

pub fn attach(app: &AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .context("desktop configuration has no main window")?
        .clone();
    page_ready(false);
    tauri::WebviewWindowBuilder::from_config(app, &config)?.build()?;
    Ok(())
}

pub async fn control(app: &AppHandle, action: &str) -> Result<Value> {
    match action {
        "attach" => {
            // macOS WebViews must be constructed on the native event thread.
            let (tx, rx) = tokio::sync::oneshot::channel();
            let handle = app.clone();
            app.run_on_main_thread(move || {
                let _ = tx.send(attach(&handle));
            })?;
            rx.await.context("native event thread stopped")??;
        }
        "detach" => {
            page_ready(false);
            if let Some(window) = app.get_webview_window("main") {
                window.close()?;
            }
        }
        "stop" => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                // Allow the accepted-request receipt to leave the local server.
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                handle.exit(0);
            });
        }
        _ => anyhow::bail!("unknown runtime lifecycle action"),
    }
    Ok(json!({"action": action, "requested": true, "runtime": status(app)}))
}
