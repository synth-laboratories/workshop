#![recursion_limit = "256"]

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CapturePlatform {
    MacOs,
    Unsupported(&'static str),
}

impl CapturePlatform {
    fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::MacOs
        }
        #[cfg(target_os = "linux")]
        {
            Self::Unsupported("linux")
        }
        #[cfg(target_os = "windows")]
        {
            Self::Unsupported("windows")
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Self::Unsupported("unknown")
        }
    }

    fn require_macos(self, operation: &str) -> Result<(), String> {
        match self {
            Self::MacOs => Ok(()),
            Self::Unsupported(platform) => Err(format!(
                "UnsupportedCapturePlatform: {operation} is not implemented for {platform}"
            )),
        }
    }
}

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
    parse_http_response(&response)
}

fn parse_http_response(response: &str) -> Result<Value, String> {
    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "invalid visuals IPC HTTP status".to_string())?;
    let body = response
        .split("\r\n\r\n")
        .nth(1)
        .ok_or_else(|| "empty visuals IPC response".to_string())?;
    let parsed: Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    if !(200..300).contains(&status) {
        let detail = parsed
            .get("error")
            .or_else(|| parsed.get("detail"))
            .and_then(|value| value.as_str())
            .unwrap_or(body);
        return Err(format!("visuals IPC HTTP {status}: {detail}"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{managed_tool_name, parse_http_response, socket_addr, tools, VISUAL_OPERATIONS};
    use serde_json::Value;

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
        let listed = tools();
        let advertised = listed["tools"][0]["inputSchema"]["properties"]["operation"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            advertised,
            VISUAL_OPERATIONS
                .iter()
                .map(|(operation, _)| *operation)
                .collect::<Vec<_>>()
        );
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
        assert!(names.contains(&"report_create"));
        assert!(names.contains(&"report_attach_trace"));
        assert!(names.contains(&"report_seal"));
        assert!(names.contains(&"report_get_seal"));
        assert!(
            !names.iter().any(|name| name.contains("share")
                || name.contains("upload")
                || name.contains("promote")),
            "agent MCP must not advertise Report share, upload, or promote: {names:?}"
        );
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

    #[test]
    fn http_errors_preserve_the_actionable_server_detail() {
        let error = parse_http_response(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: 34\r\n\r\n{\"error\":\"template_not_renderable\"}",
        )
        .unwrap_err();
        assert_eq!(error, "visuals IPC HTTP 400: template_not_renderable");
    }
}

const VISUAL_OPERATIONS: &[(&str, &str)] = &[
    ("list_templates", "visual_list_templates"),
    ("list", "visual_list"),
    ("get", "visual_get"),
    ("create", "visual_create"),
    ("update", "visual_update"),
    ("bind", "visual_bind_data_source"),
    ("save", "visual_save"),
    ("show", "visual_show"),
    ("render", "visual_render"),
    ("capture_review", "visual_capture_review"),
    ("authoring_context", "visual_authoring_context"),
    ("list_annotations", "visual_list_annotations"),
    ("annotate", "visual_annotate"),
    ("review", "visual_review"),
    ("mark_ready", "visual_mark_ready"),
    ("seal", "visual_seal"),
    ("list_seals", "visual_list_seals"),
    ("get_seal", "visual_get_seal"),
    ("fork", "visual_fork"),
    ("archive", "visual_archive"),
];

fn managed_tool_name(operation: &str) -> Result<&'static str, String> {
    VISUAL_OPERATIONS
        .iter()
        .find_map(|(candidate, tool)| (*candidate == operation).then_some(*tool))
        .ok_or_else(|| {
            let supported = VISUAL_OPERATIONS
                .iter()
                .map(|(candidate, _)| *candidate)
                .collect::<Vec<_>>()
                .join(", ");
            format!("unknown visual operation {operation}; supported operations: {supported}")
        })
}

fn tools() -> Value {
    let mut result = json!({
        "tools": [
            {"name":"visual_manage","description":"Synth visuals. Use author-synth-diagrams; do not call MCP resources. Create/show, review PNGs wide and compact, revise defects, then mark_ready. Mermaid source goes in arguments.content.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","description":"Visual operation."},"arguments":{"type":"object","description":"Operation arguments. capture_review returns a PNG and screenshot_path; review and mark_ready use the current revision.","additionalProperties":true}},"required":["operation","arguments"],"additionalProperties":false}},
            {"name":"visual_list_templates","description":"List Synth visual templates","inputSchema":{"type":"object","properties":{"genre":{"type":"string"}},"additionalProperties":false}},
            {"name":"visual_list","description":"List visuals in the local registry","inputSchema":{"type":"object","properties":{"search":{"type":"string"},"status":{"type":"string"},"session_id":{"type":"string"}},"additionalProperties":false}},
            {"name":"visual_get","description":"Get a visual by id","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_create","description":"Create a visual from a trusted registered template. Interactive live viewers are configured templates; arbitrary TSX is not executed.","inputSchema":{"type":"object","properties":{"template_id":{"type":"string"},"title":{"type":"string"},"content":{"type":"string"},"props":{"type":"object"},"visual_config":{"type":"object"},"presentation":{"type":"string","enum":["canvas","pane"]},"session_id":{"type":"string"},"instance_id":{"type":"string"}},"required":["template_id"],"additionalProperties":false}},
            {"name":"visual_create_from_template","description":"Alias of visual_create","inputSchema":{"type":"object","properties":{"template_id":{"type":"string"},"title":{"type":"string"},"props":{"type":"object"},"instance_id":{"type":"string"}},"required":["template_id"],"additionalProperties":false}},
            {"name":"visual_update","description":"Revise visual bindings, title, trusted-template configuration, or Mermaid content","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"title":{"type":"string"},"content":{"type":"string"},"bindings":{"type":"object"},"status":{"type":"string"},"visual_config":{"type":"object"},"presentation":{"type":"string","enum":["canvas","pane"]}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_bind_data_source","description":"Bind a slot on a visual","inputSchema":{"type":"object","properties":{"instance_id":{"type":"string"},"slot":{"type":"string"},"kind":{"type":"string","enum":["trace_v5","local_cas","live_sse","fixture"]},"source":{"type":"string"},"poll_url":{"type":"string","description":"Exact normalized poll URL declared beside a live SSE source"},"path":{"type":"string"}},"required":["instance_id","slot","kind","source"],"additionalProperties":false}},
            {"name":"visual_show","description":"Open a visual in the Desktop right pane","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"session_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_open_in_pane","description":"Alias of visual_show","inputSchema":{"type":"object","properties":{"instance_id":{"type":"string"}},"required":["instance_id"],"additionalProperties":false}},
            {"name":"visual_fork","description":"Fork a visual","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"title":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_authoring_context","description":"Get the template contract, example evidence, revision, presentation, and outstanding quality gate for one visual","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_list_annotations","description":"List durable labels for a visual and its current overlay digest","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_annotate","description":"Write a durable label anchored to one exact visual revision","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"revision":{"type":"integer"},"selector":{"type":"object"},"kind":{"type":"string","enum":["note","bug","highlight","reward","acceptance"]},"body":{"type":"string"},"source_digest":{"type":"string"},"supersedes_id":{"type":"string"}},"required":["visual_id","revision","selector","kind"],"additionalProperties":false}},
            {"name":"visual_review","description":"Record one rendered-view critique for the current revision. Include viewport and explicit landmark checks.","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"revision":{"type":"integer"},"viewport":{"type":"object"},"checks":{"type":"object"},"findings":{"type":"array","items":{"type":"string"}},"screenshot_path":{"type":"string"}},"required":["visual_id","revision","viewport","checks","findings"],"additionalProperties":false}},
            {"name":"visual_capture_review","description":"Render the current visual revision to a real PNG review image at the requested viewport. Returns the PNG as tool image content and an absolute screenshot_path for visual_review.","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"viewport":{"type":"object","properties":{"width":{"type":"integer","minimum":320,"maximum":2400},"height":{"type":"integer","minimum":400,"maximum":1800}},"required":["width","height"],"additionalProperties":false}},"required":["visual_id","viewport"],"additionalProperties":false}},
            {"name":"visual_mark_ready","description":"Mark the current revision ready after at least two passing rendered-view reviews","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"revision":{"type":"integer"}},"required":["visual_id","revision"],"additionalProperties":false}},
            {"name":"visual_seal","description":"Compile an E1-ready exact revision into a local immutable ArtifactBundle v1","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"revision":{"type":"integer"}},"required":["visual_id","revision"],"additionalProperties":false}},
            {"name":"visual_list_seals","description":"List local immutable visual seals","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"additionalProperties":false}},
            {"name":"visual_get_seal","description":"Get one local ArtifactBundle v1 by receipt digest","inputSchema":{"type":"object","properties":{"receipt_digest":{"type":"string"}},"required":["receipt_digest"],"additionalProperties":false}},
            {"name":"visual_archive","description":"Archive a visual","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"report_list","description":"List local Reports","inputSchema":{"type":"object","properties":{"search":{"type":"string"},"status":{"type":"string"}},"additionalProperties":false}},
            {"name":"report_get","description":"Get a Report identity and current revision","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"}},"required":["report_id"],"additionalProperties":false}},
            {"name":"report_get_revision","description":"Get one exact Report revision, including ordered blocks","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"revision":{"type":"integer"}},"required":["report_id"],"additionalProperties":false}},
            {"name":"report_create","description":"Create a local Report draft with narrative and appendix blocks","inputSchema":{"type":"object","properties":{"title":{"type":"string"},"summary":{"type":"string"},"authors":{"type":"array","items":{"type":"string"}}},"additionalProperties":false}},
            {"name":"report_update","description":"Update a local Report draft using optimistic revision control. Sealed revisions stay immutable.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"expected_revision":{"type":"integer"},"title":{"type":"string"},"summary":{"type":"string"},"blocks":{"type":"array"},"claims":{"type":"array"},"limitations":{"type":"array"}},"required":["report_id","expected_revision"],"additionalProperties":false}},
            {"name":"report_block_add","description":"Add a block to a Report draft using optimistic revision control.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"expected_revision":{"type":"integer"},"block":{"type":"object"}},"required":["report_id","expected_revision","block"],"additionalProperties":false}},
            {"name":"report_block_update","description":"Replace one block in a Report draft using its block_id and optimistic revision control.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"expected_revision":{"type":"integer"},"block":{"type":"object"}},"required":["report_id","expected_revision","block"],"additionalProperties":false}},
            {"name":"report_block_remove","description":"Remove one block from a Report draft using optimistic revision control.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"expected_revision":{"type":"integer"},"block_id":{"type":"string"}},"required":["report_id","expected_revision","block_id"],"additionalProperties":false}},
            {"name":"report_attach_trace","description":"Attach a Trace V5 digest to a Report. Desktop resolves the local rollout-inspector projection when omitted. The frozen Report reader renders the canonical inspector; missing projections stay —.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"trace_digest":{"type":"string"},"trace_id":{"type":"string"},"label":{"type":"string"},"collection_id":{"type":"string"},"projection":{"type":"object","description":"Optional synth.trace-projection.rollout-inspector.v1 packet. Omit to resolve from local inventory."}},"required":["report_id","trace_digest"],"additionalProperties":false}},
            {"name":"report_seal","description":"Seal one exact Report revision for offline reopen. Does not upload or promote.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"revision":{"type":"integer"}},"required":["report_id","revision"],"additionalProperties":false}},
            {"name":"report_list_seals","description":"List local sealed Report revisions","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"}},"additionalProperties":false}},
            {"name":"report_get_seal","description":"Reopen one local sealed Report by receipt digest","inputSchema":{"type":"object","properties":{"receipt_digest":{"type":"string"}},"required":["receipt_digest"],"additionalProperties":false}},
            {"name":"report_upsert_experiment","description":"Create or update an Experiment Record on a Report","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"experiment_id":{"type":"string"},"title":{"type":"string"},"hypothesis":{"type":"string"},"status":{"type":"string"},"protocol_digest":{"type":"string"},"arms":{"type":"array"},"runs":{"type":"array"},"results":{"type":"array"}},"required":["report_id","title"],"additionalProperties":false}},
            {"name":"report_append_log","description":"Append a Research Log entry. Corrections link earlier entries and never rewrite them.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"entry_kind":{"type":"string"},"title":{"type":"string"},"body":{"type":"string"},"author":{"type":"string"},"actor_kind":{"type":"string","enum":["human","agent"]},"claim_effect":{"type":"string"},"supersedes_entry_id":{"type":"string"},"links":{"type":"array"}},"required":["report_id","entry_kind","title","body"],"additionalProperties":false}},
            {"name":"report_archive","description":"Archive a local Report. This is reversible and never deletes sealed bytes.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"}},"required":["report_id"],"additionalProperties":false}},
            {"name":"report_restore","description":"Restore an archived local Report.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"}},"required":["report_id"],"additionalProperties":false}},
            {"name":"report_request_visibility","description":"Request a human-approved visibility change for one exact sealed Report receipt. This does not share, publish, or unpublish by itself.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"receipt_digest":{"type":"string"},"target":{"type":"string","enum":["private","public","unpublished"]},"slug":{"type":"string"},"reason":{"type":"string"}},"required":["report_id","receipt_digest","target"],"additionalProperties":false}},
            {"name":"report_list_visibility_requests","description":"List visibility requests and their human decision status.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"}},"additionalProperties":false}}
        ]
    });
    if let Some(items) = result.get_mut("tools").and_then(Value::as_array_mut) {
        if let Some(facade) = items
            .iter_mut()
            .find(|tool| tool["name"] == "visual_manage")
        {
            facade["inputSchema"]["properties"]["operation"]["enum"] = Value::Array(
                VISUAL_OPERATIONS
                    .iter()
                    .map(|(operation, _)| Value::String((*operation).into()))
                    .collect(),
            );
        }
        for tool in items {
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let read_only = matches!(
                name.as_str(),
                "visual_list_templates"
                    | "visual_list"
                    | "visual_get"
                    | "visual_authoring_context"
                    | "visual_list_annotations"
                    | "visual_list_seals"
                    | "visual_get_seal"
                    | "report_list"
                    | "report_get"
                    | "report_get_revision"
                    | "report_list_seals"
                    | "report_get_seal"
                    | "report_list_visibility_requests"
            );
            if let Some(object) = tool.as_object_mut() {
                object.insert(
                    "annotations".into(),
                    json!({
                        "readOnlyHint": read_only,
                        "destructiveHint": matches!(name.as_str(), "visual_archive" | "report_archive" | "report_block_remove"),
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
        "visual_list_annotations" => {
            let id = args
                .get("visual_id")
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request("GET", &format!("/v1/visuals/{id}/annotations"), None)
        }
        "visual_annotate" => {
            let id = args
                .get("visual_id")
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            let revision = args
                .get("revision")
                .and_then(Value::as_i64)
                .ok_or("revision required")?;
            let selector = args.get("selector").cloned().ok_or("selector required")?;
            let kind = args.get("kind").cloned().ok_or("kind required")?;
            request(
                "POST",
                &format!("/v1/visuals/{id}/annotations"),
                Some(json!({
                    "visualRevision": revision,
                    "sourceDigest": args.get("source_digest"),
                    "selector": selector,
                    "kind": kind,
                    "body": args.get("body"),
                    "authorId": "mcp",
                    "supersedesId": args.get("supersedes_id")
                })),
            )
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
        "visual_seal" => {
            let id = args
                .get("visual_id")
                .and_then(Value::as_str)
                .ok_or("visual_id required")?;
            request(
                "POST",
                &format!("/v1/visuals/{id}/seal"),
                Some(args.clone()),
            )
        }
        "visual_list_seals" => request("GET", "/v1/seals", Some(args.clone())),
        "visual_get_seal" => {
            let digest = args
                .get("receipt_digest")
                .and_then(Value::as_str)
                .ok_or("receipt_digest required")?;
            request("GET", &format!("/v1/seals/{digest}"), None)
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
        "report_list" => request("GET", "/v1/reports", Some(args.clone())),
        "report_get" => {
            let id = args
                .get("report_id")
                .and_then(Value::as_str)
                .ok_or("report_id required")?;
            request("GET", &format!("/v1/reports/{id}"), None)
        }
        "report_get_revision" => {
            let id = args
                .get("report_id")
                .and_then(Value::as_str)
                .ok_or("report_id required")?;
            request(
                "GET",
                &format!("/v1/reports/{id}/revision"),
                Some(args.clone()),
            )
        }
        "report_create" => request("POST", "/v1/reports", Some(args.clone())),
        "report_attach_trace" => {
            let id = args
                .get("report_id")
                .and_then(Value::as_str)
                .ok_or("report_id required")?;
            request(
                "POST",
                &format!("/v1/reports/{id}/traces"),
                Some(args.clone()),
            )
        }
        "report_update" => {
            let id = args
                .get("report_id")
                .and_then(Value::as_str)
                .ok_or("report_id required")?;
            request("POST", &format!("/v1/reports/{id}"), Some(args.clone()))
        }
        "report_block_add" => report_block_change(args, "add"),
        "report_block_update" => report_block_change(args, "update"),
        "report_block_remove" => report_block_change(args, "remove"),
        "report_seal" => {
            let id = args
                .get("report_id")
                .and_then(Value::as_str)
                .ok_or("report_id required")?;
            request(
                "POST",
                &format!("/v1/reports/{id}/seal"),
                Some(args.clone()),
            )
        }
        "report_list_seals" => request("GET", "/v1/report-seals", Some(args.clone())),
        "report_get_seal" => {
            let digest = args
                .get("receipt_digest")
                .and_then(Value::as_str)
                .ok_or("receipt_digest required")?;
            request("GET", &format!("/v1/report-seals/{digest}"), None)
        }
        "report_upsert_experiment" => {
            let id = args
                .get("report_id")
                .and_then(Value::as_str)
                .ok_or("report_id required")?;
            request(
                "POST",
                &format!("/v1/reports/{id}/experiments"),
                Some(args.clone()),
            )
        }
        "report_append_log" => {
            let id = args
                .get("report_id")
                .and_then(Value::as_str)
                .ok_or("report_id required")?;
            request("POST", &format!("/v1/reports/{id}/log"), Some(args.clone()))
        }
        "report_archive" | "report_restore" => {
            let id = args
                .get("report_id")
                .and_then(Value::as_str)
                .ok_or("report_id required")?;
            let operation = if name == "report_archive" {
                "archive"
            } else {
                "restore"
            };
            request(
                "POST",
                &format!("/v1/reports/{id}/{operation}"),
                Some(json!({})),
            )
        }
        "report_request_visibility" => {
            let id = args
                .get("report_id")
                .and_then(Value::as_str)
                .ok_or("report_id required")?;
            let mut body = args.clone();
            body["requested_by"] = json!("mcp");
            request(
                "POST",
                &format!("/v1/reports/{id}/visibility-requests"),
                Some(body),
            )
        }
        "report_list_visibility_requests" => {
            request("GET", "/v1/report-visibility-requests", Some(args.clone()))
        }
        other => Err(format!("unknown tool {other}")),
    }
}

fn report_block_change(args: &Value, operation: &str) -> Result<Value, String> {
    let id = args
        .get("report_id")
        .and_then(Value::as_str)
        .ok_or("report_id required")?;
    let expected_revision = args
        .get("expected_revision")
        .and_then(Value::as_i64)
        .ok_or("expected_revision required")?;
    let current = request(
        "GET",
        &format!("/v1/reports/{id}/revision"),
        Some(json!({"revision": expected_revision})),
    )?;
    let mut blocks = current
        .pointer("/revision/blocks")
        .and_then(Value::as_array)
        .cloned()
        .ok_or("Report revision response missing blocks")?;
    match operation {
        "add" => {
            let block = args.get("block").cloned().ok_or("block required")?;
            let block_id = block
                .get("block_id")
                .or_else(|| block.get("blockId"))
                .and_then(Value::as_str)
                .ok_or("block.block_id required")?;
            if blocks.iter().any(|row| {
                row.get("blockId")
                    .or_else(|| row.get("block_id"))
                    .and_then(Value::as_str)
                    == Some(block_id)
            }) {
                return Err("block_id already exists".into());
            }
            blocks.push(block);
        }
        "update" => {
            let block = args.get("block").cloned().ok_or("block required")?;
            let block_id = block
                .get("block_id")
                .or_else(|| block.get("blockId"))
                .and_then(Value::as_str)
                .ok_or("block.block_id required")?;
            let existing = blocks
                .iter_mut()
                .find(|row| {
                    row.get("blockId")
                        .or_else(|| row.get("block_id"))
                        .and_then(Value::as_str)
                        == Some(block_id)
                })
                .ok_or("block does not exist")?;
            *existing = block;
        }
        "remove" => {
            let block_id = args
                .get("block_id")
                .and_then(Value::as_str)
                .ok_or("block_id required")?;
            let before = blocks.len();
            blocks.retain(|row| {
                row.get("blockId")
                    .or_else(|| row.get("block_id"))
                    .and_then(Value::as_str)
                    != Some(block_id)
            });
            if blocks.len() == before {
                return Err("block does not exist".into());
            }
        }
        _ => return Err("unsupported block operation".into()),
    }
    request(
        "POST",
        &format!("/v1/reports/{id}"),
        Some(json!({
            "expected_revision": expected_revision,
            "blocks": blocks,
        })),
    )
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
    let capture_mode = if matches!(renderer_kind, "mermaid" | "systems" | "systems-dynamic") {
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
    CapturePlatform::current().require_macos("SVG review capture")?;
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
    render_svg_with_webkit(&svg_path, width, height, png_path)?;
    assert_non_blank_png(png_path)
}

#[cfg(target_os = "macos")]
fn render_svg_with_webkit(
    svg_path: &std::path::Path,
    width: u64,
    height: u64,
    png_path: &std::path::Path,
) -> Result<(), String> {
    // WebKit is the canonical macOS SVG renderer for review captures. Unlike
    // `sips`, it preserves the browser features Synth allows, including
    // foreignObject content, and snapshots the exact requested viewport.
    let swift = r#"import AppKit
import Foundation
import WebKit

final class Navigation: NSObject, WKNavigationDelegate {
    var loaded = false
    var error: Error?
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { loaded = true }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { self.error = error; loaded = true }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { self.error = error; loaded = true }
}

let input = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2])
let width = Double(CommandLine.arguments[3])!
let height = Double(CommandLine.arguments[4])!
_ = NSApplication.shared
let webView = WKWebView(frame: NSRect(x: 0, y: 0, width: width, height: height))
let navigation = Navigation()
webView.navigationDelegate = navigation
let source = try String(contentsOf: input, encoding: .utf8)
let encoded = Data(source.utf8).base64EncodedString()
let html = """
<!doctype html><meta charset="utf-8"><style>
html,body{margin:0;width:100%;height:100%;overflow:hidden;background:#090A0C}
img{display:block;width:100%;height:100%;object-fit:contain}
</style><img src="data:image/svg+xml;base64,\(encoded)">
"""
webView.loadHTMLString(html, baseURL: input.deletingLastPathComponent())
let loadDeadline = Date().addingTimeInterval(15)
while !navigation.loaded && Date() < loadDeadline {
    RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.02))
}
if let error = navigation.error { fputs("\(error)\n", stderr); exit(2) }
if !navigation.loaded { fputs("WebKit SVG load timed out\n", stderr); exit(3) }
var snapshot: NSImage?
var snapshotError: Error?
webView.takeSnapshot(with: nil) { image, error in snapshot = image; snapshotError = error }
let snapshotDeadline = Date().addingTimeInterval(15)
while snapshot == nil && snapshotError == nil && Date() < snapshotDeadline {
    RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.02))
}
if let error = snapshotError { fputs("\(error)\n", stderr); exit(4) }
guard let image = snapshot,
      let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: Int(width), pixelsHigh: Int(height),
        bitsPerSample: 8, samplesPerPixel: 4,
        hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0
      ),
      let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
    fputs("WebKit SVG snapshot produced no PNG\n", stderr); exit(5)
}
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = context
image.draw(in: NSRect(x: 0, y: 0, width: width, height: height),
           from: .zero, operation: .copy, fraction: 1.0)
NSGraphicsContext.restoreGraphicsState()
guard let png = bitmap.representation(using: .png, properties: [:]) else {
    fputs("WebKit SVG snapshot could not encode PNG\n", stderr); exit(6)
}
try png.write(to: output, options: .atomic)
"#;
    let output = Command::new("swift")
        .args(["-e", swift])
        .arg(svg_path)
        .arg(png_path)
        .arg(width.to_string())
        .arg(height.to_string())
        .output()
        .map_err(|error| format!("launch WebKit SVG renderer: {error}"))?;
    if !output.status.success() || !png_path.is_file() {
        return Err(format!(
            "WebKitSvgRenderFailed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn render_svg_with_webkit(
    _svg_path: &std::path::Path,
    _width: u64,
    _height: u64,
    _png_path: &std::path::Path,
) -> Result<(), String> {
    CapturePlatform::current().require_macos("SVG review capture")
}

fn capture_desktop_review(
    id: &str,
    width: u64,
    height: u64,
    png_path: &std::path::Path,
) -> Result<(), String> {
    CapturePlatform::current().require_macos("Desktop template review capture")?;
    #[cfg(target_os = "macos")]
    {
        capture_macos_desktop_review(id, width, height, png_path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (id, width, height, png_path);
        unreachable!("platform gate returned success outside macOS")
    }
}

#[cfg(target_os = "macos")]
fn capture_macos_desktop_review(
    id: &str,
    width: u64,
    height: u64,
    png_path: &std::path::Path,
) -> Result<(), String> {
    // Template visuals are React surfaces, not SVG renditions. Show the exact
    // visual, resize the actual Webview so responsive layout runs at the
    // requested viewport, then capture that named instance's on-screen window.
    // Resizing a bitmap after capture is not a responsive-layout review.
    request(
        "POST",
        &format!("/v1/visuals/{id}/show"),
        Some(json!({"presentation":"pane"})),
    )?;
    let resize = request(
        "POST",
        "/v1/review-window/resize",
        Some(json!({"width":width,"height":height})),
    )?;
    // Do not `?` between here and the restore. The window is already resized;
    // any early return from this span leaves the user's Desktop at the review
    // size with nothing left to put it back.
    let previous = resize.get("previous").cloned();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let result = capture_current_macos_window(width, height, png_path);
    let restore = match previous {
        Some(previous) => request("POST", "/v1/review-window/resize", Some(previous)),
        None => Err(
            "review window resize omitted its previous size, so the Desktop window was left at the review size"
                .to_string(),
        ),
    };
    match (result, restore) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(format!(
            "captured review but failed to restore Desktop window: {error}"
        )),
        (Ok(()), Ok(_)) => Ok(()),
    }
}

#[cfg(target_os = "macos")]
fn capture_current_macos_window(
    width: u64,
    height: u64,
    png_path: &std::path::Path,
) -> Result<(), String> {
    let app_name = env::var("SYNTH_DESKTOP_APP_NAME")
        .map_err(|_| "template review capture requires SYNTH_DESKTOP_APP_NAME".to_string())?;
    let bundle_id = env::var("SYNTH_DESKTOP_BUNDLE_ID").unwrap_or_default();
    let swift = r#"import CoreGraphics
import AppKit
import Foundation
let wantedName = CommandLine.arguments[1]
let wantedBundle = CommandLine.arguments[2]
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)! as! [[String: Any]]
let candidates = windows.compactMap { item -> (Int, Double, String, String)? in
  guard let owner = item[kCGWindowOwnerName as String] as? String,
        let pid = item[kCGWindowOwnerPID as String] as? Int,
        let number = item[kCGWindowNumber as String] as? Int,
        let bounds = item[kCGWindowBounds as String] as? [String: Any],
        let width = bounds["Width"] as? Double,
        let height = bounds["Height"] as? Double,
        width >= 640, height >= 400 else { return nil }
  let bundle = NSRunningApplication(processIdentifier: pid_t(pid))?.bundleIdentifier ?? ""
  guard (!wantedBundle.isEmpty && bundle == wantedBundle) || owner == wantedName else { return nil }
  return (number, width * height, owner, bundle)
}
if let best = candidates.max(by: { $0.1 < $1.1 }) { print(best.0) }
else {
  let observed = windows.compactMap { item -> String? in
    guard let owner = item[kCGWindowOwnerName as String] as? String,
          owner.lowercased().contains("synth"),
          let pid = item[kCGWindowOwnerPID as String] as? Int else { return nil }
    let bundle = NSRunningApplication(processIdentifier: pid_t(pid))?.bundleIdentifier ?? "unknown"
    return "\(owner) [\(bundle)]"
  }
  fputs("observed synth windows: \(observed.joined(separator: ", "))", stderr)
}"#;
    let output = Command::new("swift")
        .args(["-e", swift, &app_name, &bundle_id])
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
        let observed = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "DesktopWindowNotFound: expected app `{app_name}` bundle `{}`; {}",
            if bundle_id.is_empty() { "unavailable" } else { &bundle_id },
            if observed.is_empty() { "no Synth windows were visible" } else { &observed }
        ));
    }
    let raw_path = png_path.with_extension("window.png");
    let capture = Command::new("/usr/sbin/screencapture")
        .args(["-x", "-l", &window_id])
        .arg(&raw_path)
        .status()
        .map_err(|error| format!("launch Desktop window capture: {error}"))?;
    if !capture.success() || !raw_path.is_file() {
        return Err("ScreenRecordingDeniedOrUnavailable: Desktop window capture produced no image; enable Screen Recording for Synth Workshop and retry".into());
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
    assert_non_blank_png(png_path)?;
    Ok(())
}

fn assert_non_blank_png(path: &std::path::Path) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|error| format!("open review PNG: {error}"))?;
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|error| format!("decode review PNG header: {error}"))?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| format!("decode review PNG: {error}"))?;
    let bytes = &buffer[..info.buffer_size()];
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Indexed => return Err("review PNG unexpectedly remained indexed".into()),
    };
    let mut min_luma = u16::MAX;
    let mut max_luma = 0u16;
    let mut sampled = 0usize;
    for pixel in bytes.chunks_exact(channels).step_by(97) {
        if channels == 4 && pixel[3] == 0 || channels == 2 && pixel[1] == 0 {
            continue;
        }
        let (r, g, b) = if channels >= 3 {
            (pixel[0] as u16, pixel[1] as u16, pixel[2] as u16)
        } else {
            let value = pixel[0] as u16;
            (value, value, value)
        };
        let luma = (r * 3 + g * 6 + b) / 10;
        min_luma = min_luma.min(luma);
        max_luma = max_luma.max(luma);
        sampled += 1;
    }
    if sampled < 16 || max_luma.saturating_sub(min_luma) < 8 {
        return Err("BlankReviewCapture: screenshot is uniform/blank; Screen Recording permission may be denied".into());
    }
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod webkit_tests {
    use super::*;

    #[test]
    fn webkit_svg_capture_preserves_foreign_object_at_requested_viewport() {
        let root =
            std::env::temp_dir().join(format!("synth-visuals-webkit-svg-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create SVG capture test directory");
        let svg_path = root.join("foreign-object.svg");
        let png_path = root.join("foreign-object.png");
        fs::write(
            &svg_path,
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180" viewBox="0 0 320 180">
<rect width="320" height="180" fill="#111827"/>
<foreignObject x="40" y="30" width="240" height="120">
  <div xmlns="http://www.w3.org/1999/xhtml" style="width:100%;height:100%;background:#22c55e;color:#ffffff;font:32px sans-serif;display:flex;align-items:center;justify-content:center">WebKit</div>
</foreignObject></svg>"##,
        )
        .expect("write SVG fixture");

        render_svg_with_webkit(&svg_path, 320, 180, &png_path).expect("render SVG through WebKit");
        assert_non_blank_png(&png_path).expect("capture should contain visible geometry");

        let file = fs::File::open(&png_path).expect("open rendered PNG");
        let decoder = png::Decoder::new(file);
        let mut reader = decoder.read_info().expect("read rendered PNG header");
        assert_eq!((reader.info().width, reader.info().height), (320, 180));
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).expect("decode rendered PNG");
        let has_foreign_object_green = buffer[..info.buffer_size()].chunks_exact(4).any(|pixel| {
            let (red, green, blue) = (pixel[0] as u16, pixel[1] as u16, pixel[2] as u16);
            green > 150 && green > red * 2 && green > blue
        });
        assert!(
            has_foreign_object_green,
            "rendered PNG omitted the foreignObject HTML surface"
        );

        let _ = fs::remove_dir_all(root);
    }
}

fn main() {
    if std::env::args().any(|argument| argument == "--dump-tools") {
        println!(
            "{}",
            serde_json::to_string_pretty(&tools()).unwrap_or_default()
        );
        return;
    }
    run_stdio_server(
        McpServerInfo {
            name: "synth-visuals-mcp",
            version: "0.1.0",
        },
        tools,
        call_tool,
    );
}
