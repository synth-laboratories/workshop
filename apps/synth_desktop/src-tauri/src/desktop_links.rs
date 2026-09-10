//! OS URL delivery is separate from the single-instance argv callback on macOS.
//! Browser return only reveals the app; device polling remains the auth authority.

#[cfg(target_os = "macos")]
pub fn open(app: &tauri::AppHandle, raw: &str) {
    use tauri::Emitter;

    if raw != "synth-workshop://auth-return" {
        match crate::instance::parse_workshop_deep_link(raw) {
            Ok(route) => {
                if let Err(error) = app.emit("desktop:deep-link", route) {
                    crate::platform::logging::report("desktop_links", "dispatch", error.to_string());
                }
            }
            Err(error) => {
                crate::platform::logging::report("desktop_links", "refused", error);
                return;
            }
        }
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::platform::desktop_runtime::control(&app, "attach").await {
            crate::platform::logging::report("desktop_links", "attach", error.to_string());
        }
    });
}
