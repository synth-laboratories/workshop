//! MCP adapter for the backend-neutral Workshop Browser Protocol.
//!
//! The reference backend is an unprivileged Playwright/Chromium child. It has
//! no Tauri IPC token and therefore cannot turn hostile page content into a
//! privileged Desktop command.

use serde_json::{json, Value};
use std::{
    env,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};
use synth_desktop_lib::browser::{DEFAULT_MAX_CHARS, HARD_MAX_CHARS};

struct Backend {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Backend {
    fn spawn() -> Result<Self, String> {
        let script = browser_script();
        if !script.is_file() {
            return Err(format!(
                "Playwright backend is missing at {}",
                script.display()
            ));
        }
        let mut child =
            Command::new(env::var_os("SYNTH_BROWSER_NODE").unwrap_or_else(|| "node".into()))
                .arg(script)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|error| format!("could not start Playwright backend: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or("browser backend stdin was not piped")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("browser backend stdout was not piped")?;
        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            next_id: 0,
        })
    }

    fn call(&mut self, operation: &str, arguments: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        writeln!(
            self.stdin,
            "{}",
            json!({"id": id, "operation": operation, "arguments": arguments})
        )
        .and_then(|_| self.stdin.flush())
        .map_err(|error| format!("browser backend stopped while sending {operation}: {error}"))?;
        loop {
            let mut line = String::new();
            if self
                .stdout
                .read_line(&mut line)
                .map_err(|error| error.to_string())?
                == 0
            {
                return Err(format!(
                    "browser backend stopped while answering {operation}"
                ));
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if value.get("ok").and_then(Value::as_bool) == Some(true) {
                return Ok(value.get("response").cloned().unwrap_or_else(|| json!({})));
            }
            return Err(value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("browser backend failed")
                .to_owned());
        }
    }
}

fn browser_script() -> PathBuf {
    if let Some(configured) = env::var_os("SYNTH_BROWSER_BACKEND_SCRIPT") {
        return PathBuf::from(configured);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(macos) = exe.parent() {
            let bundled = macos.join("../Resources/browser/playwright_backend.mjs");
            if bundled.is_file() {
                return bundled;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../browser/playwright_backend.mjs")
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
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
    items.push(json!({"name":"browser_create_session","description":"Create a visible Workshop-managed Chromium session using a dedicated persistent profile.","inputSchema":schema(serde_json::Map::from_iter([("profile".into(),json!({"type":"string"}))]),&[])}));
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
    json!({"tools":items})
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut backend: Option<Backend> = None;
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
                    if backend.is_none() {
                        match Backend::spawn() {
                            Ok(service) => backend = Some(service),
                            Err(error) => {
                                let payload = json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":error}],"isError":true}});
                                let _ = writeln!(stdout, "{payload}");
                                let _ = stdout.flush();
                                continue;
                            }
                        }
                    }
                    backend.as_mut().expect("backend initialized").call(
                        name,
                        params
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                    )
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
