//! Stdio MCP adapter for Synth visuals. Forwards tools to CoreRuntime visuals IPC.
//!
//! Usage (Codex home config):
//!   command = "synth-visuals-mcp"
//!   env SYNTH_VISUALS_IPC_FILE = "~/Library/Application Support/Synth Desktop/visuals-ipc.json"

use serde_json::{json, Value};
use std::{
    env, fs,
    io::{self, BufRead, Write},
    path::PathBuf,
};

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
        assert_eq!(managed_tool_name("save").unwrap(), "visual_save");
        assert_eq!(managed_tool_name("show").unwrap(), "visual_show");
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
        "fork" => Ok("visual_fork"),
        "archive" => Ok("visual_archive"),
        other => Err(format!("unknown visual operation {other}")),
    }
}

fn tools() -> Value {
    let mut result = json!({
        "tools": [
            {"name":"visual_manage","description":"Operate Synth visuals. Load the use-synth-visuals skill for operation payloads.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["list_templates","list","get","create","update","bind","save","show","fork","archive"]},"arguments":{"type":"object","additionalProperties":true}},"required":["operation","arguments"],"additionalProperties":false}},
            {"name":"visual_list_templates","description":"List Synth visual templates","inputSchema":{"type":"object","properties":{"genre":{"type":"string"}},"additionalProperties":false}},
            {"name":"visual_list","description":"List visuals in the local registry","inputSchema":{"type":"object","properties":{"search":{"type":"string"},"status":{"type":"string"},"session_id":{"type":"string"}},"additionalProperties":false}},
            {"name":"visual_get","description":"Get a visual by id","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_create","description":"Create a visual. Prefer analysis.visual.v1 for ad-hoc evidence-driven blocks; use blank.canvas.v1 for bespoke sandboxed HTML/SVG/CSS compositions. Choose the visual grammar from the data and never imply a trend without an ordered or temporal x-axis.","inputSchema":{"type":"object","properties":{"template_id":{"type":"string"},"title":{"type":"string"},"props":{"type":"object"},"session_id":{"type":"string"},"instance_id":{"type":"string"}},"required":["template_id"],"additionalProperties":false}},
            {"name":"visual_create_from_template","description":"Alias of visual_create","inputSchema":{"type":"object","properties":{"template_id":{"type":"string"},"title":{"type":"string"},"props":{"type":"object"},"instance_id":{"type":"string"}},"required":["template_id"],"additionalProperties":false}},
            {"name":"visual_update","description":"Update visual bindings/title/status","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"title":{"type":"string"},"bindings":{"type":"object"},"status":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_bind_data_source","description":"Bind a slot on a visual","inputSchema":{"type":"object","properties":{"instance_id":{"type":"string"},"slot":{"type":"string"},"kind":{"type":"string"},"source":{"type":"string"},"path":{"type":"string"}},"required":["instance_id","slot","kind","source"],"additionalProperties":false}},
            {"name":"visual_save","description":"Save visual content and mark saved","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"tsx":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_save_tsx","description":"Alias of visual_save","inputSchema":{"type":"object","properties":{"instance_id":{"type":"string"},"tsx":{"type":"string"}},"required":["instance_id"],"additionalProperties":false}},
            {"name":"visual_show","description":"Open a visual in the Desktop right pane","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"session_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_open_in_pane","description":"Alias of visual_show","inputSchema":{"type":"object","properties":{"instance_id":{"type":"string"}},"required":["instance_id"],"additionalProperties":false}},
            {"name":"visual_fork","description":"Fork a visual","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"title":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
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
                "visual_list_templates" | "visual_list" | "visual_get"
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
            });
            request("POST", "/v1/visuals", Some(body))
        }
        "visual_update" => {
            let id = args
                .get("visual_id")
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request("POST", &format!("/v1/visuals/{id}"), Some(args.clone()))
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

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { continue };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let response = match method {
            "initialize" => {
                json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"synth-visuals-mcp","version":"0.1.0"}}})
            }
            "notifications/initialized" => continue,
            "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":tools()}),
            "tools/call" => {
                let params = req.get("params").cloned().unwrap_or(json!({}));
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match call_tool(name, &args) {
                    Ok(result) => {
                        json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":serde_json::to_string_pretty(&result).unwrap_or_default()}],"structuredContent":result}})
                    }
                    Err(error) => {
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":error}})
                    }
                }
            }
            _ => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("unknown method {method}")}})
            }
        };
        let _ = writeln!(stdout, "{}", response);
        let _ = stdout.flush();
    }
}
