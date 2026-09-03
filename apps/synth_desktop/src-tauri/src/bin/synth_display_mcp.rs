//! Agent-facing control for Workshop's displayed plugin destinations, and the
//! screenshots of them.
//!
//! Capture lives here because this server already owns "what the user sees".
//! The host photographs its own WKWebView, so a capture needs no Screen
//! Recording grant, no window-identity resolution, and no visibility: it works
//! while Workshop is occluded or backgrounded, which is what makes it usable
//! from an unattended eval.

#[path = "../instance_paths.rs"]
mod instance_paths;
#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

use base64::Engine;
use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    time::Duration,
};

/// Plugin destinations a capture may name. Mirrors the host allowlist; naming
/// it here too keeps the tool schema self-describing to the agent.
const PLUGIN_IDS: [&str; 7] = [
    "visuals",
    "reports",
    "experiments",
    "optimizers",
    "inventory",
    "inference",
    "computer-use",
];

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    url: String,
    token: String,
}

fn connection_file() -> PathBuf {
    instance_paths::ipc_connection_file(
        &["SYNTH_DESKTOP_IPC_FILE", "SYNTH_VISUALS_IPC_FILE"],
        "visuals-ipc.json",
    )
}

fn request(method: &str, path: &str, body: Value) -> Result<Value, String> {
    let connection: Connection =
        serde_json::from_str(&fs::read_to_string(connection_file()).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let addr = connection
        .url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .parse()
        .map_err(|e: std::net::AddrParseError| e.to_string())?;
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(10)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let payload = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let wire = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", connection.token, payload.len());
    stream
        .write_all(wire.as_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or("malformed display IPC response")?;
    if !headers.lines().next().unwrap_or_default().contains(" 2") {
        return Err(body.trim().to_string());
    }
    serde_json::from_str(body).map_err(|e| e.to_string())
}

fn tools() -> Value {
    json!({"tools":[
        {"name":"workshop_display","description":"List Workshop plugin destinations or choose which are visible in the user's sidebar. Use only when the user asks to change the Workshop display.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["list","set_visible"]},"visible_plugin_ids":{"type":"array","items":{"type":"string","enum":PLUGIN_IDS}}},"required":["operation"],"additionalProperties":false}},
        {"name":"workshop_capture","description":"Screenshot Workshop and get the PNG back as tool image content. scope `app` photographs the app exactly as it stands right now; `plugin` routes to one destination first and keeps the surrounding chrome; `visual` isolates one visual the way authoring review does; `element` crops to one data-testid. Omit `viewport` to photograph the window at its current size — pass one only when reviewing a specific breakpoint, because it resizes the user's window and restores it afterwards. Inspect the returned image; do not describe a surface you have not looked at.","inputSchema":{"type":"object","properties":{
            "scope":{"type":"string","enum":["app","plugin","visual","element"],"description":"What to photograph. Defaults to app."},
            "target":{"type":"string","description":"Plugin id for scope=plugin, visual id for scope=visual, data-testid for scope=element. Omitted for scope=app."},
            "viewport":{"type":"object","properties":{"width":{"type":"integer","minimum":320,"maximum":2400},"height":{"type":"integer","minimum":400,"maximum":1800}},"required":["width","height"],"additionalProperties":false,"description":"Optional CSS viewport. Resizes the window for the capture and restores it."},
            "label":{"type":"string","description":"Short slug for the filename, so a sequence of captures is legible on disk."},
            "frames":{"type":"integer","minimum":2,"maximum":600,"description":"Record a filmstrip of this many frames instead of one still. Use to watch a live run: the window is never resized during a recording, so `viewport` is rejected with frames."},
            "interval_ms":{"type":"integer","minimum":250,"maximum":60000,"description":"Milliseconds between filmstrip frames. Default 2000."}
        },"additionalProperties":false}}
    ]})
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "workshop_display" => display(args),
        "workshop_capture" => capture(args),
        other => Err(format!("unknown tool {other}")),
    }
}

/// Slug for a capture filename. Anything outside the allowed set becomes `-`,
/// so a label can never introduce a path separator or escape the capture root.
fn slug(value: &str) -> String {
    let mut cleaned = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            cleaned.push(character.to_ascii_lowercase());
        } else if !cleaned.ends_with('-') {
            // Collapse runs: " · " is three characters and would otherwise
            // become three dashes in every filename.
            cleaned.push('-');
        }
    }
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "capture".into()
    } else {
        trimmed.chars().take(48).collect()
    }
}

fn capture(args: &Value) -> Result<Value, String> {
    let scope = args
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("app");
    let target = args.get("target").and_then(Value::as_str);
    match scope {
        "app" => {}
        "plugin" => {
            let id = target.ok_or("scope `plugin` requires target: a plugin id")?;
            if !PLUGIN_IDS.contains(&id) {
                return Err(format!(
                    "unknown plugin `{id}`; expected one of {}",
                    PLUGIN_IDS.join(", ")
                ));
            }
        }
        "visual" => {
            target.ok_or("scope `visual` requires target: a visual id")?;
        }
        "element" => {
            target.ok_or("scope `element` requires target: a data-testid")?;
        }
        other => {
            return Err(format!(
                "unknown scope `{other}`; expected app, plugin, visual, or element"
            ))
        }
    }

    let mut body = json!({"scope": scope});
    if let Some(target) = target {
        body["target"] = json!(target);
    }
    let viewport = match args.get("viewport") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let object = value.as_object().ok_or("viewport must be an object")?;
            let width = object
                .get("width")
                .and_then(Value::as_u64)
                .ok_or("viewport.width required")?;
            let height = object
                .get("height")
                .and_then(Value::as_u64)
                .ok_or("viewport.height required")?;
            if !(320..=2400).contains(&width) || !(400..=1800).contains(&height) {
                return Err("viewport must be within 320x400 and 2400x1800".into());
            }
            body["width"] = json!(width);
            body["height"] = json!(height);
            Some((width, height))
        }
    };

    // Captures land beside the IPC connection file, inside the instance data
    // root the host will accept writes into.
    let root = connection_file()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("surface-captures");
    let label = args
        .get("label")
        .and_then(Value::as_str)
        .map(slug)
        .unwrap_or_else(|| slug(target.unwrap_or(scope)));
    let size = viewport
        .map(|(width, height)| format!("{width}x{height}"))
        .unwrap_or_else(|| "window".into());

    let frames = args.get("frames").and_then(Value::as_u64);
    if let Some(frames) = frames {
        if viewport.is_some() {
            // Resizing between frames would make the recording a series of
            // relayouts rather than a record of the run.
            return Err("frames and viewport cannot be combined: a recording never resizes the window".into());
        }
        return record(&body, &root, &label, frames, args);
    }

    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ");
    let path = root.join(format!("{scope}-{label}-{size}-{stamp}.png"));
    let mut receipt = capture_once(&body, &path)?;
    let png = fs::read(&path).map_err(|error| error.to_string())?;
    if let Some(object) = receipt.as_object_mut() {
        object.insert(
            "instruction".into(),
            json!("Inspect the attached PNG before describing this surface. `audit.findings` is the machine-checkable subset only; it is not a substitute for looking."),
        );
        object.insert(
            "_mcpImage".into(),
            json!({"data": base64::engine::general_purpose::STANDARD.encode(png), "mimeType": "image/png"}),
        );
    }
    Ok(receipt)
}

fn capture_once(body: &Value, path: &std::path::Path) -> Result<Value, String> {
    let mut body = body.clone();
    body["outputPath"] = json!(path.to_string_lossy());
    request("POST", "/v1/capture", body)
}

/// A filmstrip of the app while something is happening to it.
///
/// The frames are the record; the mp4 is a convenience rendered from them when
/// ffmpeg is present. Keeping the PNGs authoritative means a recording stays
/// inspectable frame by frame, with each frame carrying its own digest, app
/// state, and layout audit — which is what a later review actually reads.
fn record(
    body: &Value,
    root: &std::path::Path,
    label: &str,
    frames: u64,
    args: &Value,
) -> Result<Value, String> {
    let interval = args
        .get("interval_ms")
        .and_then(Value::as_u64)
        .unwrap_or(2000);
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ");
    let dir = root.join(format!("recording-{label}-{stamp}"));
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;

    let mut records = Vec::new();
    let mut findings = 0_u64;
    let mut failures = Vec::new();
    for index in 0..frames {
        if index > 0 {
            std::thread::sleep(Duration::from_millis(interval));
        }
        let path = dir.join(format!("frame-{index:04}.png"));
        match capture_once(body, &path) {
            Ok(receipt) => {
                findings += receipt
                    .pointer("/audit/findings")
                    .and_then(Value::as_array)
                    .map(|list| list.len() as u64)
                    .unwrap_or(0);
                records.push(receipt);
            }
            // One bad frame must not discard the rest of a long recording.
            Err(error) => failures.push(json!({"frame": index, "error": error})),
        }
    }
    if records.is_empty() {
        return Err(format!(
            "no frame was captured: {}",
            serde_json::to_string(&failures).unwrap_or_default()
        ));
    }

    let fps = (1000.0 / interval as f64).clamp(1.0, 30.0);
    let video = mux(&dir, fps);
    let last = fs::read(dir.join(format!("frame-{:04}.png", records.len() - 1)))
        .or_else(|_| fs::read(dir.join("frame-0000.png")))
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "schemaVersion": "synth.surface-recording.v1",
        "directory": dir.to_string_lossy(),
        "frames": records.len(),
        "requestedFrames": frames,
        "intervalMs": interval,
        "video": video,
        "failures": failures,
        "totalFindings": findings,
        "records": records,
        "instruction": "Frames are the record; the mp4 is a rendering of them. Inspect frames where `audit.findings` is non-empty, and compare the first and last frames for live-update stability.",
        "_mcpImage": {"data": base64::engine::general_purpose::STANDARD.encode(last), "mimeType": "image/png"}
    }))
}

/// Best effort. A recording without ffmpeg is still a complete record, so a
/// missing or failing muxer is reported, never fatal.
fn mux(dir: &std::path::Path, fps: f64) -> Value {
    let output = dir.join("recording.mp4");
    let result = std::process::Command::new("ffmpeg")
        .args(["-y", "-framerate", &format!("{fps:.3}"), "-i"])
        .arg(dir.join("frame-%04d.png"))
        // Even dimensions are required by yuv420p, and a captured window is
        // routinely odd-sized on a Retina display.
        .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-pix_fmt", "yuv420p"])
        .arg(&output)
        .output();
    match result {
        Ok(done) if done.status.success() => json!({"path": output.to_string_lossy(), "fps": fps}),
        Ok(done) => json!({
            "unavailable": String::from_utf8_lossy(&done.stderr).lines().last().unwrap_or("ffmpeg failed")
        }),
        Err(error) => json!({"unavailable": error.to_string()}),
    }
}

fn display(args: &Value) -> Result<Value, String> {
    match args.get("operation").and_then(Value::as_str) {
        Some("list") => request("GET", "/v1/display/plugins", json!({})),
        Some("set_visible") => {
            let ids = args
                .get("visible_plugin_ids")
                .and_then(Value::as_array)
                .ok_or("visible_plugin_ids required")?;
            request(
                "POST",
                "/v1/display/plugins/visibility",
                json!({"visiblePluginIds": ids}),
            )
        }
        _ => Err("unsupported operation".into()),
    }
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "workshop-display",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_display_and_capture() {
        let catalog = tools();
        let listed = catalog["tools"].as_array().unwrap();
        let names: Vec<&str> = listed
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["workshop_display", "workshop_capture"]);

        let capture = &listed[1];
        let scopes = capture["inputSchema"]["properties"]["scope"]["enum"]
            .as_array()
            .unwrap();
        assert_eq!(scopes.len(), 4);
        // The viewport is optional on purpose: photographing the app must not
        // begin by resizing the user's window.
        let required = capture["inputSchema"].get("required");
        assert!(required.is_none(), "capture must require no argument: {required:?}");
        let described = capture["description"].as_str().unwrap();
        assert!(described.contains("current size"), "{described}");
    }

    #[test]
    fn capture_refuses_a_scope_it_cannot_point_at() {
        // These fail before any IPC, so they are safe to assert without a host.
        for args in [
            json!({"scope": "plugin"}),
            json!({"scope": "plugin", "target": "settings"}),
            json!({"scope": "visual"}),
            json!({"scope": "element"}),
            json!({"scope": "window"}),
        ] {
            assert!(capture(&args).is_err(), "{args}");
        }
    }

    #[test]
    fn a_label_cannot_escape_the_capture_directory() {
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug("Banking77 · GEPA"), "banking77-gepa");
        assert_eq!(slug("///"), "capture");
        assert_eq!(slug("").len(), "capture".len());
        assert!(slug(&"x".repeat(200)).len() <= 48);
    }
}
