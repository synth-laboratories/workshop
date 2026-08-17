//! MCP adapter for the backend-neutral Workshop Browser Protocol.
//!
//! This process is deliberately a thin authenticated proxy. Desktop owns the
//! browser child, lifecycle, origin policy, and exact-action approval broker.

#[allow(dead_code)]
#[path = "../instance_paths.rs"]
mod instance_paths;

use serde_json::{json, Value};
use std::{
    env, fs,
    io::{self, BufRead, Write},
    path::PathBuf,
    time::Duration,
};
use synth_desktop_lib::browser::{DEFAULT_MAX_CHARS, HARD_MAX_CHARS};

const IPC_REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(serde::Deserialize)]
struct Connection {
    url: String,
    token: String,
}

fn connection_file() -> PathBuf {
    env::var("SYNTH_DESKTOP_IPC_FILE")
        .or_else(|_| env::var("SYNTH_VISUALS_IPC_FILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            env::var_os("SYNTH_DESKTOP_DATA_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    dirs::data_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("Synth Desktop")
                })
                .join("visuals-ipc.json")
        })
}

fn request(method: &str, path: &str, body: Value) -> Result<Value, String> {
    let connection: Connection = serde_json::from_str(
        &fs::read_to_string(connection_file()).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut body = body;
    if let (Some(object), Ok(session)) = (body.as_object_mut(), env::var("SYNTH_SESSION_ID")) {
        if !session.trim().is_empty() {
            object.insert("sessionRef".into(), json!(session));
        }
    }
    let payload = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
    let addr = connection
        .url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .parse::<std::net::SocketAddr>()
        .map_err(|error| error.to_string())?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, IPC_REQUEST_TIMEOUT)
        .map_err(|error| format!("browser IPC connect failed: {error}"))?;
    stream
        .set_read_timeout(Some(IPC_REQUEST_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(IPC_REQUEST_TIMEOUT))
        .map_err(|error| error.to_string())?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        connection.token,
        payload.len()
    )
    .and_then(|_| stream.write_all(&payload))
    .map_err(|error| format!("browser IPC request failed: {error}"))?;
    let mut response = String::new();
    io::Read::read_to_string(&mut stream, &mut response)
        .map_err(|error| format!("browser IPC response failed: {error}"))?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "browser IPC returned a malformed response".to_owned())?;
    let status = headers.lines().next().unwrap_or("HTTP status unavailable");
    if !status.contains(" 2") {
        return Err(format!("browser IPC returned {status}: {}", body.trim()));
    }
    serde_json::from_str(body)
        .map_err(|error| format!("browser IPC returned invalid JSON: {error}"))
}

fn target_schema(optional: bool) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "ref": {
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"}, "tab_id": {"type": "string"},
                    "document_revision": {"type": "string"}, "element_id": {"type": "string"}
                },
                "required": ["session_id", "tab_id", "document_revision", "element_id"],
                "additionalProperties": false
            },
            "locator": {
                "type": "object",
                "properties": {"role": {"type": "string"}, "name": {"type": "string"}, "exact": {"type": "boolean"}},
                "required": ["role", "name"], "additionalProperties": false
            }
        },
        "oneOf": [{"required": ["ref"]}, {"required": ["locator"]}],
        "additionalProperties": false
    });
    if optional {
        schema["description"] =
            json!("Optional for browser_press; omit to send the key to the page.");
    }
    schema
}

fn base_properties(tab: bool) -> serde_json::Map<String, Value> {
    let mut properties =
        serde_json::Map::from_iter([("session_id".into(), json!({"type":"string"}))]);
    if tab {
        properties.insert("tab_id".into(), json!({"type":"string"}));
    }
    properties
}

fn schema(properties: serde_json::Map<String, Value>, required: &[&str]) -> Value {
    json!({"type":"object", "properties": properties, "required": required, "additionalProperties": false})
}

fn tools() -> Value {
    let bounded = || json!({"type":"integer","minimum":256,"maximum":HARD_MAX_CHARS,"default":DEFAULT_MAX_CHARS});
    let mut items = Vec::new();
    items.push(json!({"name":"browser_status","description":"Check the local managed-browser runtime and list human-approved origins without starting Chromium.","inputSchema":schema(serde_json::Map::new(),&[]),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}));
    items.push(json!({"name":"browser_create_session","description":"Create a visible Workshop-managed Chromium session using a dedicated persistent profile.","inputSchema":schema(serde_json::Map::from_iter([("profile".into(),json!({"type":"string"}))]),&[])}));
    items.push(json!({"name":"browser_claim_chrome","description":"With exact human approval, claim exactly one existing Chrome tab exposed on a loopback CDP endpoint. Disabled unless the operator explicitly enables Chrome claiming; the claimed user tab is never closed by Workshop.","inputSchema":schema(serde_json::Map::from_iter([
        ("cdp_endpoint".into(),json!({"type":"string","default":"http://127.0.0.1:9222"})),
        ("title_contains".into(),json!({"type":"string"})),
        ("url_contains".into(),json!({"type":"string"}))
    ]),&[])}));
    items.push(json!({"name":"browser_close_session","description":"Close only this Workshop-managed session; never touches user browser tabs.","inputSchema":schema(base_properties(false),&["session_id"])}));
    items.push(json!({"name":"browser_list_tabs","description":"List stable tab identities in one managed session.","inputSchema":schema(base_properties(false),&["session_id"])}));
    let mut new_tab = base_properties(false);
    new_tab.insert("url".into(), json!({"type":"string"}));
    items.push(json!({"name":"browser_new_tab","description":"Create a managed tab, optionally navigating to an approved origin.","inputSchema":schema(new_tab,&["session_id"])}));
    items.push(json!({"name":"browser_close_tab","description":"Close one managed tab. Tab IDs are never reused.","inputSchema":schema(base_properties(true),&["session_id","tab_id"])}));
    let mut navigate = base_properties(true);
    navigate.insert("url".into(), json!({"type":"string"}));
    items.push(json!({"name":"browser_navigate","description":"Navigate a managed tab. Unapproved origins fail closed.","inputSchema":schema(navigate,&["session_id","tab_id","url"])}));
    items.push(json!({"name":"browser_back","description":"Navigate one step back in the managed tab history.","inputSchema":schema(base_properties(true),&["session_id","tab_id"])}));
    for (name, description) in [
        (
            "browser_snapshot",
            "Read a bounded semantic snapshot; raw full DOM is never returned.",
        ),
        (
            "browser_query",
            "Query visible semantic elements without reading the full page.",
        ),
    ] {
        let mut props = base_properties(true);
        props.insert("max_chars".into(), bounded());
        props.insert("cursor".into(), json!({"type":"integer","minimum":0}));
        if name == "browser_query" {
            props.insert("role".into(), json!({"type":"string"}));
            props.insert("name".into(), json!({"type":"string"}));
        }
        items.push(json!({"name":name,"description":description,"inputSchema":schema(props,&["session_id","tab_id"])}));
    }
    let mut subtree = base_properties(true);
    subtree.insert("target".into(), target_schema(false));
    subtree.insert("max_chars".into(), bounded());
    subtree.insert("cursor".into(), json!({"type":"integer","minimum":0}));
    items.push(json!({"name":"browser_subtree","description":"Read bounded text beneath a revision-bound ref or unique semantic locator.","inputSchema":schema(subtree,&["session_id","tab_id","target"])}));
    for (name, description) in [("browser_click","Click a unique semantic target; consequential labels fail closed pending host confirmation."),("browser_fill","Fill a unique field without echoing its value."),("browser_upload","Attach explicitly selected files from an operator-approved root."),("browser_download","Click a unique download target and save into the managed profile download directory.")] {
        let mut props=base_properties(true);props.insert("target".into(),target_schema(false));
        if name=="browser_fill" {props.insert("value".into(),json!({"type":"string"}));}
        if name=="browser_upload" {props.insert("file_paths".into(),json!({"type":"array","items":{"type":"string"},"minItems":1}));}
        items.push(json!({"name":name,"description":description,"inputSchema":schema(props,&["session_id","tab_id","target"])}));
    }
    let mut press = base_properties(true);
    press.insert("target".into(), target_schema(true));
    press.insert("key".into(), json!({"type":"string"}));
    items.push(json!({"name":"browser_press","description":"Press a key on a unique target or on the page.","inputSchema":schema(press,&["session_id","tab_id","key"])}));
    let mut scroll = base_properties(true);
    scroll.insert("target".into(), target_schema(true));
    scroll.insert("delta_x".into(), json!({"type":"number"}));
    scroll.insert("delta_y".into(), json!({"type":"number"}));
    items.push(json!({"name":"browser_scroll","description":"Scroll the page or a unique semantic target.","inputSchema":schema(scroll,&["session_id","tab_id"])}));
    let mut shot = base_properties(true);
    shot.insert("full_page".into(), json!({"type":"boolean"}));
    items.push(json!({"name":"browser_screenshot","description":"Capture a screenshot into the managed profile; returns the controlled path.","inputSchema":schema(shot,&["session_id","tab_id"])}));
    items.push(json!({"name":"browser_list_dialogs","description":"List pending browser dialogs without accepting them.","inputSchema":schema(base_properties(true),&["session_id","tab_id"])}));
    let mut dialog = base_properties(true);
    dialog.insert("dialog_id".into(), json!({"type":"string"}));
    dialog.insert("accept".into(), json!({"type":"boolean"}));
    dialog.insert("prompt_text".into(), json!({"type":"string"}));
    items.push(json!({"name":"browser_handle_dialog","description":"Accept or dismiss one pending dialog. Accepting always requires exact human confirmation.","inputSchema":schema(dialog,&["session_id","tab_id","dialog_id","accept"])}));
    let mut audit = base_properties(true);
    audit.insert(
        "limit".into(),
        json!({"type":"integer","minimum":1,"maximum":500,"default":100}),
    );
    items.push(json!({"name":"browser_audit","description":"Read a bounded tail of this managed profile's browser audit events.","inputSchema":schema(audit,&["session_id","tab_id"]),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true}}));
    json!({"tools":items})
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "initialize" => Ok(
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"synth-browser-mcp","version":env!("CARGO_PKG_VERSION")}}),
            ),
            "tools/list" => Ok(tools()),
            "tools/call" => {
                let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                if !tools()["tools"]
                    .as_array()
                    .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == name))
                {
                    Err(format!("unknown browser tool {name}"))
                } else {
                    let arguments = params
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    if name == "browser_status" {
                        request("GET", "/v1/browser/status", json!({}))
                    } else {
                        request(
                            "POST",
                            "/v1/browser/call",
                            json!({ "operation": name, "arguments": arguments }),
                        )
                    }
                }
            }
            _ => Err(format!("unknown method {method}")),
        };
        let payload = match result {
            Ok(value) if method == "tools/call" => {
                json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}})
            }
            Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
            Err(error) if method == "tools/call" => {
                json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":error}],"isError":true}})
            }
            Err(error) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":error}}),
        };
        let _ = writeln!(stdout, "{payload}");
        let _ = stdout.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_is_bounded_and_never_offers_raw_dom() {
        let catalog = tools().to_string();
        assert!(catalog.contains("browser_snapshot"));
        assert!(catalog.contains("browser_status"));
        assert!(catalog.contains("browser_back"));
        assert!(catalog.contains("20000"));
        assert!(!catalog.contains("browser_dom"));
        assert!(!catalog.contains("browser_html"));
        assert_eq!(
            synth_desktop_lib::browser::PROTOCOL_VERSION,
            "workshop.browser.v1"
        );
    }
}
