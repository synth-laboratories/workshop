//! Shared MCP stdio JSON-RPC line transport for the Desktop MCP bins.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

#[derive(Clone, Debug)]
pub struct McpServerInfo {
    pub name: &'static str,
    pub version: &'static str,
}

const BREAKER_FAILURE_LIMIT: usize = 2;

#[derive(Default)]
struct ToolLoopBreaker {
    failures: HashMap<String, (String, usize)>,
}

impl ToolLoopBreaker {
    fn terminal_error(&self, tool: &str) -> Option<String> {
        let (error, count) = self.failures.get(tool)?;
        (*count >= BREAKER_FAILURE_LIMIT).then(|| {
            format!(
                "ToolLoopBreaker: {tool} stopped before invocation after {count} repeated failures: {error}. Change approach or fix the reported cause before retrying."
            )
        })
    }

    fn record_failure(&mut self, tool: &str, error: &str) {
        let normalized = normalize_error(error);
        match self.failures.get_mut(tool) {
            Some((previous, count)) if previous == &normalized => *count += 1,
            _ => {
                self.failures.insert(tool.to_string(), (normalized, 1));
            }
        }
    }

    fn record_success(&mut self, tool: &str) {
        self.failures.remove(tool);
    }
}

fn normalize_error(error: &str) -> String {
    let mut normalized = String::with_capacity(error.len());
    let mut in_digits = false;
    for character in error.chars() {
        if character.is_ascii_digit() {
            if !in_digits {
                normalized.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            normalized.push(character);
        }
    }
    normalized
}

fn tool_error_result(error: String, terminal: bool) -> Value {
    json!({
        "content": [{"type":"text","text":error}],
        "structuredContent": {"error": error, "terminal": terminal},
        "isError": true
    })
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
    let mut breaker = ToolLoopBreaker::default();
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
                if let Some(error) = breaker.terminal_error(name) {
                    json!({"jsonrpc":"2.0","id":id,"result":tool_error_result(error, true)})
                } else {
                    match call_tool(name, &args) {
                        Ok(mut result) => {
                            breaker.record_success(name);
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
                            breaker.record_failure(name, &error);
                            json!({"jsonrpc":"2.0","id":id,"result":tool_error_result(error, false)})
                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaker_ignores_changing_numeric_arguments_and_resets_on_success() {
        let mut breaker = ToolLoopBreaker::default();
        breaker.record_failure("visual_capture_review", "viewport 640 failed: denied");
        assert!(breaker.terminal_error("visual_capture_review").is_none());
        breaker.record_failure("visual_capture_review", "viewport 800 failed: denied");
        let terminal = breaker.terminal_error("visual_capture_review").unwrap();
        assert!(terminal.contains("stopped before invocation"));
        breaker.record_success("visual_capture_review");
        assert!(breaker.terminal_error("visual_capture_review").is_none());
    }

    #[test]
    fn tool_failures_are_results_not_json_rpc_errors() {
        let result = tool_error_result("denied".into(), true);
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["terminal"], true);
    }
}
