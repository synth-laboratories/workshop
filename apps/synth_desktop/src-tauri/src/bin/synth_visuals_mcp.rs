//! Stdio MCP adapter for Synth visuals. Forwards tools to CoreRuntime visuals IPC.
//!
//! Usage (Codex home config):
//!   command = "synth-visuals-mcp"
//!   env SYNTH_VISUALS_IPC_FILE = "~/Library/Application Support/Synth Desktop/visuals-ipc.json"

#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

use base64::Engine;
use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{env, fs, io, path::PathBuf, process::Command};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    url: String,
    token: String,
}

fn connection_file() -> PathBuf {
    if let Ok(path) = env::var("SYNTH_VISUALS_IPC_FILE") {
        return PathBuf::from(path);
    }
    env::var_os("SYNTH_DESKTOP_DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Synth Desktop")
        })
        .join("visuals-ipc.json")
}

fn load_connection() -> Result<Connection, String> {
    let path = connection_file();
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("read visuals IPC {}: {error}", path.display()))?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

fn socket_addr(url: &str) -> Result<std::net::SocketAddr, String> {
    url.trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .parse::<std::net::SocketAddr>()
        .map_err(|error| error.to_string())
}

fn request(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let connection = load_connection()?;
    let payload = body
        .as_ref()
        .map(|value| serde_json::to_vec(value).unwrap_or_default())
        .unwrap_or_default();
    let addr = socket_addr(&connection.url)?;
    let mut stream = std::net::TcpStream::connect(addr).map_err(|error| error.to_string())?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
        host = addr,
        token = connection.token,
        len = payload.len(),
        path = if path.starts_with('/') { path } else { "/" }
    );
    use std::io::Write as _;
    stream
        .write_all(request.as_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    io::Read::read_to_string(&mut stream, &mut response).map_err(|error| error.to_string())?;
    let body = response
        .split("\r\n\r\n")
        .nth(1)
        .ok_or_else(|| "empty visuals IPC response".to_string())?;
    serde_json::from_str(body).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{managed_tool_name, socket_addr, tools};

    #[test]
    fn parses_loopback_connection_without_treating_request_path_as_part_of_address() {
        assert_eq!(
            socket_addr("http://127.0.0.1:49262").unwrap().to_string(),
            "127.0.0.1:49262"
        );
        assert_eq!(
            socket_addr("http://127.0.0.1:49262/ignored")
                .unwrap()
                .to_string(),
            "127.0.0.1:49262"
        );
    }

    #[test]
    fn compact_facade_covers_every_canonical_visual_operation() {
        assert_eq!(
            managed_tool_name("list_templates").unwrap(),
            "visual_list_templates"
        );
        assert_eq!(managed_tool_name("list").unwrap(), "visual_list");
        assert_eq!(managed_tool_name("get").unwrap(), "visual_get");
        assert_eq!(managed_tool_name("create").unwrap(), "visual_create");
        assert_eq!(managed_tool_name("update").unwrap(), "visual_update");
        assert_eq!(
            managed_tool_name("bind").unwrap(),
            "visual_bind_data_source"
        );
        assert_eq!(managed_tool_name("show").unwrap(), "visual_show");
        assert_eq!(managed_tool_name("render").unwrap(), "visual_render");
        assert_eq!(
            managed_tool_name("capture_review").unwrap(),
            "visual_capture_review"
        );
        assert_eq!(
            managed_tool_name("authoring_context").unwrap(),
            "visual_authoring_context"
        );
        assert_eq!(managed_tool_name("review").unwrap(), "visual_review");
        assert_eq!(
            managed_tool_name("mark_ready").unwrap(),
            "visual_mark_ready"
        );
        assert_eq!(managed_tool_name("fork").unwrap(), "visual_fork");
        assert_eq!(managed_tool_name("archive").unwrap(), "visual_archive");
        assert!(managed_tool_name("delete_everything").is_err());
    }

    #[test]
    fn server_exposes_the_compact_facade_without_removing_legacy_tools() {
        let listed = tools();
        let names = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"visual_manage"));
        assert!(names.contains(&"visual_create"));
        assert!(names.contains(&"visual_open_in_pane"));
        let bind = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "visual_bind_data_source")
            .unwrap();
        assert!(bind["inputSchema"]["properties"].get("poll_url").is_some());
        let facade = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "visual_manage")
            .unwrap();
        let advertised = serde_json::to_string(&serde_json::json!({
            "name": facade["name"],
            "description": facade["description"],
            "inputSchema": facade["inputSchema"],
        }))
        .unwrap();
        assert!(
            advertised.len() < 900,
            "compact facade grew to {} bytes",
            advertised.len()
        );
        assert!(advertised.contains("author-synth-diagrams"));
        assert!(advertised.contains("do not call MCP resources"));
        assert!(advertised.contains("arguments.content"));
    }
}

fn managed_tool_name(operation: &str) -> Result<&'static str, String> {
    match operation {
        "list_templates" => Ok("visual_list_templates"),
        "list" => Ok("visual_list"),
        "get" => Ok("visual_get"),
        "create" => Ok("visual_create"),
        "update" => Ok("visual_update"),
        "bind" => Ok("visual_bind_data_source"),
        "save" => Ok("visual_save"),
        "show" => Ok("visual_show"),
        "render" => Ok("visual_render"),
        "capture_review" => Ok("visual_capture_review"),
        "authoring_context" => Ok("visual_authoring_context"),
        "review" => Ok("visual_review"),
        "mark_ready" => Ok("visual_mark_ready"),
        "fork" => Ok("visual_fork"),
        "archive" => Ok("visual_archive"),
        other => Err(format!("unknown visual operation {other}")),
    }
}

fn tools() -> Value {
    let mut result = json!({
        "tools": [
            {"name":"visual_manage","description":"Direct tool for Synth visuals; do not call MCP resources or search the filesystem. For a diagram, load author-synth-diagrams, create with arguments.content, show, then capture_review and inspect its returned image before review.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","description":"Operation to run. Diagram path: create, show, capture_review, inspect, review.","enum":["list_templates","list","get","create","update","bind","show","fork","archive","authoring_context","review","mark_ready","render","capture_review"]},"arguments":{"type":"object","description":"capture_review requires visual_id and viewport {width,height}; it returns a PNG image and absolute screenshot_path.","additionalProperties":true}},"required":["operation","arguments"],"additionalProperties":false}},
            {"name":"visual_list_templates","description":"List Synth visual templates","inputSchema":{"type":"object","properties":{"genre":{"type":"string"}},"additionalProperties":false}},
            {"name":"visual_list","description":"List visuals in the local registry","inputSchema":{"type":"object","properties":{"search":{"type":"string"},"status":{"type":"string"},"session_id":{"type":"string"}},"additionalProperties":false}},
            {"name":"visual_get","description":"Get a visual by id","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_create","description":"Create a visual from a trusted registered template. Interactive live viewers are configured templates; arbitrary TSX is not executed.","inputSchema":{"type":"object","properties":{"template_id":{"type":"string"},"title":{"type":"string"},"content":{"type":"string"},"props":{"type":"object"},"visual_config":{"type":"object"},"presentation":{"type":"string","enum":["canvas","pane"]},"session_id":{"type":"string"},"instance_id":{"type":"string"}},"required":["template_id"],"additionalProperties":false}},
            {"name":"visual_create_from_template","description":"Alias of visual_create","inputSchema":{"type":"object","properties":{"template_id":{"type":"string"},"title":{"type":"string"},"props":{"type":"object"},"instance_id":{"type":"string"}},"required":["template_id"],"additionalProperties":false}},
            {"name":"visual_update","description":"Revise visual bindings, title, trusted-template configuration, or Mermaid content","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"title":{"type":"string"},"content":{"type":"string"},"bindings":{"type":"object"},"status":{"type":"string"},"visual_config":{"type":"object"},"presentation":{"type":"string","enum":["canvas","pane"]}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_bind_data_source","description":"Bind a slot on a visual","inputSchema":{"type":"object","properties":{"instance_id":{"type":"string"},"slot":{"type":"string"},"kind":{"type":"string"},"source":{"type":"string"},"poll_url":{"type":"string","description":"Exact normalized poll URL declared beside a live SSE source"},"path":{"type":"string"}},"required":["instance_id","slot","kind","source"],"additionalProperties":false}},
            {"name":"visual_show","description":"Open a visual in the Desktop right pane","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"session_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_open_in_pane","description":"Alias of visual_show","inputSchema":{"type":"object","properties":{"instance_id":{"type":"string"}},"required":["instance_id"],"additionalProperties":false}},
            {"name":"visual_fork","description":"Fork a visual","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"title":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_authoring_context","description":"Get the template contract, example evidence, revision, presentation, and outstanding quality gate for one visual","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_review","description":"Record one rendered-view critique for the current revision. Include viewport and explicit landmark checks.","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"revision":{"type":"integer"},"viewport":{"type":"object"},"checks":{"type":"object"},"findings":{"type":"array","items":{"type":"string"}},"screenshot_path":{"type":"string"}},"required":["visual_id","revision","viewport","checks","findings"],"additionalProperties":false}},
            {"name":"visual_capture_review","description":"Render the current visual revision to a real PNG review image at the requested viewport. Returns the PNG as tool image content and an absolute screenshot_path for visual_review.","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"viewport":{"type":"object","properties":{"width":{"type":"integer","minimum":320,"maximum":2400},"height":{"type":"integer","minimum":400,"maximum":1800}},"required":["width","height"],"additionalProperties":false}},"required":["visual_id","viewport"],"additionalProperties":false}},
            {"name":"visual_mark_ready","description":"Mark the current revision ready after at least two passing rendered-view reviews","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"revision":{"type":"integer"}},"required":["visual_id","revision"],"additionalProperties":false}},
            {"name":"visual_archive","description":"Archive a visual","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}}
        ]
    });
    if let Some(items) = result.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in items {
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let read_only = matches!(
                name.as_str(),
                "visual_list_templates" | "visual_list" | "visual_get" | "visual_authoring_context"
            );
            if let Some(object) = tool.as_object_mut() {
                object.insert(
                    "annotations".into(),
                    json!({
                        "readOnlyHint": read_only,
                        "destructiveHint": name == "visual_archive",
                        "idempotentHint": read_only,
                        "openWorldHint": false
                    }),
                );
            }
        }
    }
    result
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    let session_env = env::var("SYNTH_SESSION_ID").ok();
    match name {
        "visual_manage" => {
            let operation = args
                .get("operation")
                .and_then(Value::as_str)
                .ok_or("operation required")?;
            let arguments = args
                .get("arguments")
                .filter(|value| value.is_object())
                .ok_or("arguments must be an object")?;
            call_tool(managed_tool_name(operation)?, arguments)
        }
        "visual_list_templates" => request("GET", "/v1/visuals/templates", None),
        "visual_list" => request("GET", "/v1/visuals", Some(args.clone())),
        "visual_get" => {
            let id = args
                .get("visual_id")
                .or_else(|| args.get("instance_id"))
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request("GET", &format!("/v1/visuals/{id}"), None)
        }
        "visual_create" | "visual_create_from_template" => {
            let body = json!({
                "templateId": args.get("template_id"),
                "title": args.get("title"),
                "bindings": args.get("props").or_else(|| args.get("bindings")).cloned().unwrap_or(json!({})),
                "id": args.get("instance_id"),
                "sessionId": args.get("session_id").cloned().or_else(|| session_env.clone().map(Value::String)),
                "sourceAgentId": "mcp",
                "content": args.get("content"),
                "metadata": {
                    "presentation": args.get("presentation").cloned().unwrap_or(json!("pane")),
                    "visualConfig": args.get("visual_config").cloned().unwrap_or(json!({})),
                    "authoringReviews": []
                },
            });
            request("POST", "/v1/visuals", Some(body))
        }
        "visual_update" => {
            let id = args
                .get("visual_id")
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            let mut body = args.clone();
            if args.get("visual_config").is_some() || args.get("presentation").is_some() {
                let current = request("GET", &format!("/v1/visuals/{id}"), None)?;
                let mut metadata = current
                    .pointer("/visual/metadata")
                    .cloned()
                    .unwrap_or(json!({}));
                if !metadata.is_object() {
                    metadata = json!({});
                }
                if let Some(value) = args.get("visual_config") {
                    metadata["visualConfig"] = value.clone();
                }
                if let Some(value) = args.get("presentation") {
                    metadata["presentation"] = value.clone();
                }
                body["metadata"] = metadata;
            }
            request("POST", &format!("/v1/visuals/{id}"), Some(body))
        }
        "visual_bind_data_source" => {
            let id = args
                .get("instance_id")
                .and_then(Value::as_str)
                .ok_or("instance_id required")?;
            let current = request("GET", &format!("/v1/visuals/{id}"), None)?;
            let existing = current
                .pointer("/visual/bindings")
                .cloned()
                .unwrap_or(json!({}));
            let slot = args
                .get("slot")
                .and_then(Value::as_str)
                .unwrap_or("primary");
            let mut slots = existing
                .get("slots")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            slots.retain(|binding| binding.get("slot").and_then(Value::as_str) != Some(slot));
            slots.push(json!({
                "slot": slot,
                "kind": args.get("kind"),
                "source": args.get("source"),
                "poll_url": args.get("poll_url"),
                "path": args.get("path"),
                "schema": args.get("schema"),
            }));
            let bindings = json!({
                "schemaVersion": "synth.visual-bindings.v1",
                "slots": slots,
            });
            request(
                "POST",
                &format!("/v1/visuals/{id}"),
                Some(json!({"bindings": bindings})),
            )
        }
        "visual_save" | "visual_save_tsx" => {
            let id = args
                .get("visual_id")
                .or_else(|| args.get("instance_id"))
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request(
                "POST",
                &format!("/v1/visuals/{id}/save"),
                Some(json!({"tsx": args.get("tsx")})),
            )
        }
        "visual_show" | "visual_open_in_pane" => {
            let id = args
                .get("visual_id")
                .or_else(|| args.get("instance_id"))
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            let body = json!({
                "sessionId": args.get("session_id").cloned().or_else(|| session_env.map(Value::String))
            });
            request("POST", &format!("/v1/visuals/{id}/show"), Some(body))
        }
        "visual_render" => {
            let id = args
                .get("visual_id")
                .or_else(|| args.get("instance_id"))
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request("POST", &format!("/v1/visuals/{id}/render"), None)
        }
        "visual_capture_review" => capture_review(args),
        "visual_authoring_context" => {
            let id = args
                .get("visual_id")
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request("GET", &format!("/v1/visuals/{id}/authoring"), None)
        }
        "visual_review" => {
            let id = args
                .get("visual_id")
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request(
                "POST",
                &format!("/v1/visuals/{id}/reviews"),
                Some(args.clone()),
            )
        }
        "visual_mark_ready" => {
            let id = args
                .get("visual_id")
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request(
                "POST",
                &format!("/v1/visuals/{id}/ready"),
                Some(args.clone()),
            )
        }
        "visual_fork" => {
            let id = args
                .get("visual_id")
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request(
                "POST",
                &format!("/v1/visuals/{id}/fork"),
                Some(json!({"title": args.get("title")})),
            )
        }
        "visual_archive" => {
            let id = args
                .get("visual_id")
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request(
                "POST",
                &format!("/v1/visuals/{id}/archive"),
                Some(json!({})),
            )
        }
        other => Err(format!("unknown tool {other}")),
    }
}

fn capture_review(args: &Value) -> Result<Value, String> {
    let id = args
        .get("visual_id")
        .and_then(Value::as_str)
        .ok_or("visual_id required")?;
    let viewport = args
        .get("viewport")
        .and_then(Value::as_object)
        .ok_or("viewport required")?;
    let width = viewport
        .get("width")
        .and_then(Value::as_u64)
        .ok_or("viewport.width required")?;
    let height = viewport
        .get("height")
        .and_then(Value::as_u64)
        .ok_or("viewport.height required")?;
    if !(320..=2400).contains(&width) || !(400..=1800).contains(&height) {
        return Err("review viewport must be within 320x400 and 2400x1800".into());
    }
    let visual_response = request("GET", &format!("/v1/visuals/{id}"), None)?;
    let revision = visual_response
        .pointer("/visual/currentRevision")
        .and_then(Value::as_i64)
        .ok_or("visual response missing revision")?;
    let renderer_kind = visual_response
        .pointer("/visual/rendererKind")
        .and_then(Value::as_str)
        .ok_or("visual response missing renderer kind")?;
    let root = connection_file()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("visual-review-captures");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let stem = format!("{id}-r{revision}-{width}x{height}");
    let png_path = root.join(format!("{stem}.png"));
    let capture_mode = if matches!(renderer_kind, "mermaid" | "systems" | "systemsDynamic") {
        request("POST", &format!("/v1/visuals/{id}/render"), None)?;
        capture_svg_review(id, renderer_kind, width, height, &root, &stem, &png_path)?;
        "deterministic-svg"
    } else {
        capture_desktop_review(id, width, height, &png_path)?;
        "desktop-window"
    };
    let png = fs::read(&png_path).map_err(|error| error.to_string())?;
    Ok(json!({
        "visual_id": id,
        "revision": revision,
        "renderer_kind": renderer_kind,
        "capture_mode": capture_mode,
        "viewport": {"width":width,"height":height},
        "screenshot_path": png_path.to_string_lossy(),
        "instruction": "Inspect the attached PNG image before submitting visual_review. If any collision, truncation, crossing, weak hierarchy, or excessive density is visible, update and capture again.",
        "_mcpImage": {"data":base64::engine::general_purpose::STANDARD.encode(png),"mimeType":"image/png"}
    }))
}

fn capture_svg_review(
    id: &str,
    renderer_kind: &str,
    width: u64,
    height: u64,
    root: &std::path::Path,
    stem: &str,
    png_path: &std::path::Path,
) -> Result<(), String> {
    let rendition = request(
        "GET",
        &format!("/v1/visuals/{id}/renditions/svg"),
        Some(json!({
            "theme": if renderer_kind == "mermaid" { "light" } else { "dark" },
            "size":"pane"
        })),
    )?;
    let svg_b64 = rendition
        .pointer("/rendition/base64")
        .and_then(Value::as_str)
        .ok_or("SVG rendition missing base64")?;
    let svg = base64::engine::general_purpose::STANDARD
        .decode(svg_b64)
        .map_err(|error| error.to_string())?;
    let svg_path = root.join(format!("{stem}.svg"));
    fs::write(&svg_path, &svg).map_err(|error| error.to_string())?;
    // Fit the complete SVG inside the requested viewport. Cropping a square
    // QuickLook thumbnail made valid wide diagrams look clipped and taught the
    // reviewing agent the wrong thing about the actual renderer.
    let svg_text = String::from_utf8_lossy(&svg);
    let view_box = svg_text
        .split("viewBox=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .ok_or("SVG review capture missing viewBox")?;
    let view_box_values = view_box
        .split_whitespace()
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("invalid SVG review viewBox: {error}"))?;
    if view_box_values.len() != 4 || view_box_values[2] <= 0.0 || view_box_values[3] <= 0.0 {
        return Err("SVG review capture has invalid viewBox dimensions".into());
    }
    let scale = (width as f64 / view_box_values[2]).min(height as f64 / view_box_values[3]);
    let fitted_width = (view_box_values[2] * scale).round().max(1.0) as u64;
    let fitted_height = (view_box_values[3] * scale).round().max(1.0) as u64;
    let raster = Command::new("sips")
        .args([
            "-s",
            "format",
            "png",
            "-z",
            &fitted_height.to_string(),
            &fitted_width.to_string(),
        ])
        .arg(&svg_path)
        .args(["--out"])
        .arg(&png_path)
        .status()
        .map_err(|error| format!("launch sips SVG rasterizer: {error}"))?;
    if !raster.success() || !png_path.is_file() {
        return Err("sips failed to rasterize PNG review capture".into());
    }
    if fitted_width != width || fitted_height != height {
        let padded_path = root.join(format!("{stem}.padded.png"));
        let pad = Command::new("sips")
            .args([
                "--padToHeightWidth",
                &height.to_string(),
                &width.to_string(),
                "--padColor",
                "090A0C",
            ])
            .arg(&png_path)
            .args(["--out"])
            .arg(&padded_path)
            .status()
            .map_err(|error| format!("launch sips viewport pad: {error}"))?;
        if !pad.success() || !padded_path.is_file() {
            return Err("sips failed to pad PNG review capture".into());
        }
        fs::rename(&padded_path, &png_path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn capture_desktop_review(
    id: &str,
    width: u64,
    height: u64,
    png_path: &std::path::Path,
) -> Result<(), String> {
    // Template visuals are React surfaces, not SVG renditions. Show the exact
    // visual, locate this named instance's on-screen window, and capture it.
    request(
        "POST",
        &format!("/v1/visuals/{id}/show"),
        Some(json!({"presentation":"pane"})),
    )?;
    std::thread::sleep(std::time::Duration::from_millis(350));
    let app_name = env::var("SYNTH_DESKTOP_APP_NAME")
        .map_err(|_| "template review capture requires SYNTH_DESKTOP_APP_NAME".to_string())?;
    let swift = r#"import CoreGraphics
import Foundation
let wanted = CommandLine.arguments[1]
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)! as! [[String: Any]]
let candidates = windows.compactMap { item -> (Int, Double)? in
  guard (item[kCGWindowOwnerName as String] as? String) == wanted,
        let number = item[kCGWindowNumber as String] as? Int,
        let bounds = item[kCGWindowBounds as String] as? [String: Any],
        let width = bounds["Width"] as? Double,
        let height = bounds["Height"] as? Double,
        width >= 640, height >= 400 else { return nil }
  return (number, width * height)
}
if let best = candidates.max(by: { $0.1 < $1.1 }) { print(best.0) }"#;
    let output = Command::new("swift")
        .args(["-e", swift, &app_name])
        .output()
        .map_err(|error| format!("launch Desktop window resolver: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Desktop window resolver failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let window_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if window_id.is_empty() {
        return Err(format!("no reviewable Desktop window found for {app_name}"));
    }
    let raw_path = png_path.with_extension("window.png");
    let capture = Command::new("/usr/sbin/screencapture")
        .args(["-x", "-l", &window_id])
        .arg(&raw_path)
        .status()
        .map_err(|error| format!("launch Desktop window capture: {error}"))?;
    if !capture.success() || !raw_path.is_file() {
        return Err("Desktop window capture failed".into());
    }
    let resize = Command::new("sips")
        .args([
            "-s",
            "format",
            "png",
            "-z",
            &height.to_string(),
            &width.to_string(),
        ])
        .arg(&raw_path)
        .args(["--out"])
        .arg(png_path)
        .status()
        .map_err(|error| format!("launch Desktop review resize: {error}"))?;
    if !resize.success() || !png_path.is_file() {
        return Err("Desktop review resize failed".into());
    }
    let _ = fs::remove_file(raw_path);
    Ok(())
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-visuals-mcp",
            version: "0.1.0",
        },
        tools,
        call_tool,
    );
}
