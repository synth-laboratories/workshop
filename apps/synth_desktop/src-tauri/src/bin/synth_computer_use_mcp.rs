//! Stdio MCP adapter for Computer Use.
//!
//! A thin proxy: it forwards to Desktop over loopback IPC and does no policy of
//! its own. Every decision — app class, allowlist, hazard, lock state, index
//! freshness — happens in Desktop, where the approval broker and the operator
//! are. An adapter that decided anything would be a second place to get it
//! wrong, and the one an attacker would target because it runs in the agent's
//! process rather than ours.

#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

#[path = "../instance_paths.rs"]
mod instance_paths;

use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

/// Longer than the plugin adapter's: a single action waits for the app to
/// settle, and a cold action may launch the target app first.
const IPC_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

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

fn request(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let connection: Connection = serde_json::from_str(
        &fs::read_to_string(connection_file()).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut payload_value = body.unwrap_or_else(|| json!({}));
    if let Some(object) = payload_value.as_object_mut() {
        if !object.contains_key("sessionRef") && !object.contains_key("session_id") {
            if let Ok(session) = env::var("SYNTH_SESSION_ID") {
                if !session.trim().is_empty() {
                    object.insert("sessionRef".into(), json!(session));
                }
            }
        }
    }
    let payload = serde_json::to_vec(&payload_value).unwrap_or_default();
    let addr = connection
        .url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .parse::<std::net::SocketAddr>()
        .map_err(|error| error.to_string())?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, IPC_REQUEST_TIMEOUT)
        .map_err(|error| format!("computer-use IPC connect failed: {error}"))?;
    stream
        .set_read_timeout(Some(IPC_REQUEST_TIMEOUT))
        .map_err(|error| format!("computer-use IPC read-timeout setup failed: {error}"))?;
    stream
        .set_write_timeout(Some(IPC_REQUEST_TIMEOUT))
        .map_err(|error| format!("computer-use IPC write-timeout setup failed: {error}"))?;
    let wire = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        connection.token,
        payload.len()
    );
    stream
        .write_all(wire.as_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(|error| format!("computer-use IPC request failed: {error}"))?;
    let mut response = String::new();
    io::Read::read_to_string(&mut stream, &mut response)
        .map_err(|error| format!("computer-use IPC response failed: {error}"))?;
    let (headers, body) = response.split_once("\r\n\r\n").ok_or_else(|| {
        if response.trim().is_empty() {
            "computer-use IPC returned an empty HTTP response".to_string()
        } else {
            format!(
                "computer-use IPC returned a malformed HTTP response: {}",
                response.trim()
            )
        }
    })?;
    let status = headers.lines().next().unwrap_or("HTTP status unavailable");
    if !status.contains(" 2") {
        let body = body.trim();
        return Err(if body.is_empty() {
            format!("computer-use IPC returned {status} with an empty response body")
        } else {
            format!("computer-use IPC returned {status}: {body}")
        });
    }
    serde_json::from_str(body).map_err(|error| {
        format!(
            "computer-use IPC returned invalid JSON ({status}): {error}; body: {}",
            body.trim()
        )
    })
}

/// The vocabulary from `docs/COMPUTER_USE.md` §5, verbatim. Kept in step with
/// `computer_use::vocabulary::ACTION_VERBS` by a test in that module.
fn tools() -> Value {
    json!({"tools":[
        {
            "name": "computer_use",
            "description": "Observe and drive native macOS apps you have been allowed to use. EVERY call requires `verb`. Read with list_apps, bounded get_app_outline, find_elements, get_subtree, or get_app_state. Never use `observe`, `open`, `navigate`, `action`, `app_id`, or a display name such as `Safari`. Use `app` with the exact bundle id returned by list_apps; Safari is `com.apple.Safari`. Prefer find_elements or get_app_outline before a broad state read. After screen unlock use get_app_state with disable_diff=true. Target elements by their unchanged canonical element_index; filtered results never renumber. Re-read after element actions because indexes are invalidated by UI changes. Never sleep; the runtime settles for you. The bundled use-computer-use skill contains the full contract and routing guidance.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "verb": {"type": "string", "enum": [
                        "list_apps", "launch", "get_app_state", "get_app_outline", "find_elements",
                        "get_subtree", "click", "set_value", "type_text",
                        "press_key", "scroll", "select_text", "drag", "perform_secondary_action"
                    ]},
                    "app": {
                        "type": "string",
                        "description": "Exact macOS bundle identifier. Use the id from list_apps, never displayName. Safari is com.apple.Safari.",
                        "examples": ["com.apple.Safari", "com.apple.mail"]
                    },
                    "disable_diff": {"type": "boolean", "description": "Return the full tree instead of a diff. Required after a screen lock."},
                    "scope": {"type": "string", "enum": ["all", "visible"]},
                    "max_chars": {"type": "integer", "minimum": 256, "maximum": 20000, "description": "Bound returned observation text; defaults to 16000."},
                    "cursor": {"type": "integer", "minimum": 0, "description": "Continuation cursor returned by a bounded observation."},
                    "role": {"type": "string"},
                    "name": {"type": "string", "description": "Case-insensitive label fragment for find_elements."},
                    "depth": {"type": "integer", "minimum": 0, "maximum": 24},
                    "element_index": {"type": "integer", "minimum": 0},
                    "x": {"type": "number"}, "y": {"type": "number"},
                    "mouse_button": {"type": "string", "enum": ["left", "right", "middle"]},
                    "click_count": {"type": "integer", "minimum": 1, "maximum": 3},
                    "value": {"type": "string"},
                    "text": {"type": "string"},
                    "key": {"type": "string", "description": "xdotool-style keysym, e.g. Return or cmd+shift+a. App-scoped: cannot invoke global shortcuts."},
                    "direction": {"type": "string", "enum": ["up", "down", "left", "right"]},
                    "pages": {"type": "number"},
                    "prefix": {"type": "string"}, "suffix": {"type": "string"},
                    "selection_type": {"type": "string", "enum": ["exact", "line", "paragraph", "all"]},
                    "from_x": {"type": "number"}, "from_y": {"type": "number"},
                    "to_x": {"type": "number"}, "to_y": {"type": "number"},
                    "action": {"type": "string", "description": "An action the element reported. Do not guess."}
                },
                "required": ["verb"],
                "allOf": [{
                    "if": {
                        "properties": {"verb": {"not": {"const": "list_apps"}}},
                        "required": ["verb"]
                    },
                    "then": {"required": ["app"]}
                }],
                "additionalProperties": false
            }
        },
        {
            "name": "computer_use_status",
            "description": "Report whether Computer Use is installed, permitted, and which apps this session may drive. Read-only: installing, enabling, and removing Computer Use are human-only.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
        }
    ]})
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "computer_use_status" => request("GET", "/v1/computer-use/status", None),
        "computer_use" => {
            // Reject unknown keys here as well as in Desktop. The adapter is not
            // the authority, but forwarding a field nobody validates is how a
            // typo becomes a silently ignored argument.
            if let Some(object) = args.as_object() {
                for key in object.keys() {
                    if !ALLOWED_KEYS.contains(&key.as_str()) {
                        return Err(format!("computer_use rejects `{key}`"));
                    }
                }
            }
            let response = request("POST", "/v1/computer-use/perform", Some(args.clone()))?;
            // Desktop records the trajectory step server-side. The agent-facing
            // tool contract returns the action result itself (AX tree, app
            // list, and so on), not the host's `{result, step}` bookkeeping
            // envelope. Refusals intentionally remain intact and typed.
            Ok(response.get("result").cloned().unwrap_or(response))
        }
        other => Err(format!("unknown tool {other}")),
    }
}

const ALLOWED_KEYS: &[&str] = &[
    "verb",
    "app",
    "disable_diff",
    "scope",
    "max_chars",
    "cursor",
    "role",
    "name",
    "depth",
    "element_index",
    "x",
    "y",
    "mouse_button",
    "click_count",
    "value",
    "text",
    "key",
    "direction",
    "pages",
    "prefix",
    "suffix",
    "selection_type",
    "from_x",
    "from_y",
    "to_x",
    "to_y",
    "action",
    "sessionRef",
    "session_id",
];

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-computer-use-mcp",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lifecycle carve-out in §4: an agent may read status and nothing
    /// else. There is no install, enable, start, or remove on this surface.
    #[test]
    fn the_agent_surface_offers_no_lifecycle_control() {
        let catalog = tools().to_string();
        for forbidden in [
            "install", "enable", "disable", "start", "stop", "update", "remove",
        ] {
            assert!(
                !catalog.contains(&format!("\"{forbidden}\"")),
                "`{forbidden}` must not be reachable from the agent"
            );
        }
    }

    #[test]
    fn the_schema_rejects_paths_commands_and_tokens() {
        let schema = tools()["tools"][0]["inputSchema"].to_string();
        assert!(!schema.contains("additionalProperties\":true"));
        for forbidden in ["\"url\"", "\"path\"", "\"command\"", "\"env\"", "\"token\""] {
            assert!(
                !schema.contains(forbidden),
                "{forbidden} must not be accepted"
            );
        }
    }

    #[test]
    fn unknown_arguments_are_refused_rather_than_forwarded() {
        let error = call_tool(
            "computer_use",
            &json!({"verb": "list_apps", "url": "https://evil"}),
        )
        .unwrap_err();
        assert!(error.contains("rejects"), "{error}");
    }

    #[test]
    fn an_unknown_tool_is_named_in_the_error() {
        assert!(call_tool("computer_use_install", &json!({}))
            .unwrap_err()
            .contains("computer_use_install"));
    }

    /// Every key the schema advertises must also be on the allowlist, or a
    /// documented argument would be refused at the door.
    #[test]
    fn every_advertised_property_is_accepted() {
        let properties = tools()["tools"][0]["inputSchema"]["properties"].clone();
        for key in properties.as_object().unwrap().keys() {
            assert!(
                ALLOWED_KEYS.contains(&key.as_str()),
                "`{key}` is advertised but would be rejected"
            );
        }
    }

    #[test]
    fn every_app_scoped_verb_requires_an_app_in_the_schema() {
        let conditional = &tools()["tools"][0]["inputSchema"]["allOf"][0];
        assert_eq!(
            conditional["if"]["properties"]["verb"]["not"]["const"],
            "list_apps"
        );
        assert_eq!(conditional["then"]["required"], json!(["app"]));
    }

    #[test]
    fn successful_action_envelopes_are_not_part_of_the_agent_contract() {
        let response = json!({
            "result": {"app": "com.example.App", "text": "[1] AXButton"},
            "step": {"id": "step-1"}
        });
        assert_eq!(
            response.get("result").cloned().unwrap_or(response),
            json!({
                "app": "com.example.App",
                "text": "[1] AXButton"
            })
        );
    }
}
