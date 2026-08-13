//! Shared MCP stdio JSON-RPC line transport for the Desktop MCP bins.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

#[derive(Clone, Debug)]
pub struct McpServerInfo {
    pub name: &'static str,
    pub version: &'static str,
}

/// Drive the NDJSON MCP stdio loop. `tools` returns the `tools/list` payload
/// object (must contain a `tools` array). `call_tool` handles `tools/call`.
pub fn run_stdio_server(
    info: McpServerInfo,
    tools: impl Fn() -> Value,
    call_tool: impl Fn(&str, &Value) -> Result<Value, String>,
) {
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
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": {
                            "name": info.name,
                            "version": info.version,
                        }
                    }
                })
            }
            "notifications/initialized" | "notifications/cancelled" => continue,
            "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":tools()}),
            "tools/call" => {
                let params = req.get("params").cloned().unwrap_or(json!({}));
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match call_tool(name, &args) {
                    Ok(mut result) => {
                        let image = result
                            .as_object_mut()
                            .and_then(|object| object.remove("_mcpImage"));
                        let mut content = vec![json!({
                            "type": "text",
                            "text": serde_json::to_string_pretty(&result).unwrap_or_default()
                        })];
                        if let Some(image) = image {
                            if let (Some(data), Some(mime_type)) = (
                                image.get("data").and_then(Value::as_str),
                                image.get("mimeType").and_then(Value::as_str),
                            ) {
                                content.push(json!({
                                    "type": "image",
                                    "data": data,
                                    "mimeType": mime_type,
                                }));
                            }
                        }
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": content,
                                "structuredContent": result
                            }
                        })
                    }
                    Err(error) => {
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":error}})
                    }
                }
            }
            _ => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": format!("unknown method {method}")}
                })
            }
        };
        let _ = writeln!(stdout, "{response}");
        let _ = stdout.flush();
    }
}
