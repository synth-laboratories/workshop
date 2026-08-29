#![recursion_limit = "256"]

//! Stdio MCP adapter for Synth visuals. Forwards tools to CoreRuntime visuals IPC.
//!
//! Usage (Codex home config):
//!   command = "synth-visuals-mcp"
//!   env SYNTH_VISUALS_IPC_FILE = "~/Library/Application Support/Synth Desktop/visuals-ipc.json"

#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

#[path = "../instance_paths.rs"]
mod instance_paths;

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
    instance_paths::ipc_connection_file(&["SYNTH_VISUALS_IPC_FILE"], "visuals-ipc.json")
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
        // A structured failure keeps its shape across this boundary. Prefixing
        // it with the HTTP status turned `{"code": …, "remediation": …}` into an
        // opaque string, so the transcript lost the code and the tool-loop
        // breaker could no longer tell one root cause from another.
        if parsed.get("code").and_then(Value::as_str).is_some() {
            return Err(parsed.to_string());
        }
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
    use super::{
        assert_review_viewport, create_bindings_from_args, managed_tool_name, parse_http_response,
        socket_addr, tools, REVIEW_VIEWPORT_HEIGHT_MAX, REVIEW_VIEWPORT_HEIGHT_MIN,
        REVIEW_VIEWPORT_WIDTH_MAX, REVIEW_VIEWPORT_WIDTH_MIN, VISUAL_OPERATIONS,
    };
    use serde_json::{json, Value};

    /// The compact review that failed acceptance was 390x844 — inside the
    /// public schema, outside an undocumented resolver floor of 640. One
    /// declared bound, and every viewport the schema advertises is capturable.
    #[test]
    fn every_viewport_the_public_schema_advertises_is_accepted() {
        for (width, height) in [(320, 400), (390, 844), (430, 932), (768, 1024), (1440, 900)] {
            assert_review_viewport(width, height).unwrap_or_else(|error| {
                panic!("{width}x{height} must be capturable: {error}");
            });
        }
        assert!(assert_review_viewport(REVIEW_VIEWPORT_WIDTH_MIN - 1, 900).is_err());
        assert!(assert_review_viewport(REVIEW_VIEWPORT_WIDTH_MAX + 1, 900).is_err());
        assert!(assert_review_viewport(1440, REVIEW_VIEWPORT_HEIGHT_MIN - 1).is_err());
        assert!(assert_review_viewport(1440, REVIEW_VIEWPORT_HEIGHT_MAX + 1).is_err());
    }

    /// The tool schema and the runtime check must not be able to disagree,
    /// which is the shape of the defect this replaced.
    #[test]
    fn the_capture_schema_and_the_runtime_bound_agree() {
        let listed = tools();
        let capture = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "visual_capture_review")
            .unwrap();
        let viewport = &capture["inputSchema"]["properties"]["viewport"]["properties"];
        assert_eq!(viewport["width"]["minimum"], REVIEW_VIEWPORT_WIDTH_MIN);
        assert_eq!(viewport["width"]["maximum"], REVIEW_VIEWPORT_WIDTH_MAX);
        assert_eq!(viewport["height"]["minimum"], REVIEW_VIEWPORT_HEIGHT_MIN);
        assert_eq!(viewport["height"]["maximum"], REVIEW_VIEWPORT_HEIGHT_MAX);
    }

    /// A slot marked `multiple` in a template contract has to be expressible,
    /// or authors fall back to hand-built bindings the renderer cannot read.
    #[test]
    fn the_bind_tool_can_express_a_multiple_binding_slot() {
        let listed = tools();
        let bind = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "visual_bind_data_source")
            .unwrap();
        let properties = &bind["inputSchema"]["properties"];
        assert!(properties["bindings"]["type"] == "array");
        assert!(properties["input"]["type"] == "string");
        assert!(properties["slot"]["type"] == "string");
        let modes = properties["mode"]["enum"].as_array().unwrap();
        assert!(modes.iter().any(|mode| mode == "append"));
        assert!(properties["poll_url"]["type"] == "string");
    }

    /// The free-form `bindings` object is where the un-canonical shape got in.
    /// It must at least name the envelope it expects.
    #[test]
    fn the_update_tool_names_the_canonical_bindings_envelope() {
        let listed = tools();
        let update = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "visual_update")
            .unwrap();
        let bindings = &update["inputSchema"]["properties"]["bindings"];
        assert_eq!(
            bindings["properties"]["schemaVersion"]["const"],
            "synth.visual-bindings.v1"
        );
        assert!(bindings["properties"]["inputs"]["type"] == "array");
        assert!(bindings["properties"].get("slots").is_none());
    }

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
        assert_eq!(
            managed_tool_name("create_with_bind").unwrap(),
            "visual_create"
        );
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
        assert_eq!(
            managed_tool_name("experiment_create").unwrap(),
            "experiment_create"
        );
        assert_eq!(
            managed_tool_name("experiment_attach_evidence").unwrap(),
            "experiment_attach_evidence"
        );
        assert_eq!(
            managed_tool_name("experiment_finalize").unwrap(),
            "experiment_finalize"
        );
        assert!(managed_tool_name("delete_everything").is_err());
        assert!(
            managed_tool_name("list_components").is_err(),
            "components stay on list_templates; there is no list_components verb"
        );
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

    #[test]
    fn create_with_bind_copies_inline_data_onto_the_required_slot() {
        let bindings = create_bindings_from_args(&json!({
            "template_id": "experiment.overview.v1",
            "slot": "experiment",
            "kind": "inline",
            "data": {"experimentId": "exp.1", "status": "blocked"}
        }))
        .unwrap();
        assert_eq!(bindings["schemaVersion"], "synth.visual-bindings.v1");
        assert!(bindings.get("slots").is_none());
        assert_eq!(bindings["inputs"][0]["input"], "experiment");
        assert!(bindings["inputs"][0].get("slot").is_none());
        assert_eq!(bindings["inputs"][0]["data"]["experimentId"], "exp.1");
    }

    #[test]
    fn create_with_bind_accepts_input_as_canonical_name() {
        let bindings = create_bindings_from_args(&json!({
            "template_id": "compose.visual.v1",
            "input": "spec",
            "kind": "inline",
            "data": {"placements": []}
        }))
        .unwrap();
        assert_eq!(bindings["inputs"][0]["input"], "spec");
        assert!(bindings["inputs"][0].get("slot").is_none());
        assert!(bindings.get("slots").is_none());
    }

    #[test]
    fn create_with_bind_refuses_when_input_and_slot_disagree() {
        assert!(create_bindings_from_args(&json!({
            "template_id": "compose.visual.v1",
            "input": "spec",
            "slot": "stream",
            "kind": "inline",
            "data": {}
        }))
        .is_err());
    }

    #[test]
    fn create_with_bind_prefers_an_explicit_envelope() {
        let envelope = json!({
            "schemaVersion": "synth.visual-bindings.v1",
            "slots": [{"slot": "spec", "kind": "inline", "data": {"blocks": []}}]
        });
        let bindings = create_bindings_from_args(&json!({
            "template_id": "analysis.visual.v1",
            "bindings": envelope,
            "slot": "experiment"
        }))
        .unwrap();
        assert_eq!(bindings["inputs"][0]["input"], "spec");
        assert!(bindings["inputs"][0].get("slot").is_none());
        assert!(bindings.get("slots").is_none());
    }
}

const CHART_TEMPLATE_ID: &str = "analysis.chart.v1";

const VISUAL_OPERATIONS: &[(&str, &str)] = &[
    ("list_templates", "visual_list_templates"),
    ("import_template", "visual_import_template"),
    ("list", "visual_list"),
    ("get", "visual_get"),
    ("create", "visual_create"),
    ("create_with_bind", "visual_create"),
    ("update", "visual_update"),
    ("bind", "visual_bind_data_source"),
    ("save", "visual_save"),
    ("show", "visual_show"),
    ("render", "visual_render"),
    ("capture_review", "visual_capture_review"),
    ("chart", "visual_chart"),
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
    // Experiment records are a lifecycle concern of the same durable visual
    // evidence surface. Codex receives only this facade, so map the lifecycle
    // actions here instead of advertising aliases the local provider may not
    // register in its compact MCP catalog.
    ("experiment_create", "experiment_create"),
    ("experiment_attach_evidence", "experiment_attach_evidence"),
    ("experiment_finalize", "experiment_finalize"),
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
            {"name":"visual_manage","description":"Use author-synth-diagrams; do not call MCP resources. Chart: operation chart, arguments.spec. Mermaid: arguments.content. Then show/review/revise/mark_ready. Experiments: experiment_create/experiment_attach_evidence/experiment_finalize.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","description":"Visual operation."},"arguments":{"type":"object","description":"Operation arguments.","additionalProperties":true}},"required":["operation","arguments"],"additionalProperties":false}},
            {"name":"visual_list_templates","description":"List Synth visual templates","inputSchema":{"type":"object","properties":{"genre":{"type":"string"}},"additionalProperties":false}},
            {"name":"visual_import_template","description":"Import one networkless template.json + renderer.html package into this Desktop instance's managed visual registry","inputSchema":{"type":"object","properties":{"source_path":{"type":"string","description":"Absolute package directory containing template.json and renderer.html"}},"required":["source_path"],"additionalProperties":false}},
            {"name":"visual_list","description":"List visuals in the local registry","inputSchema":{"type":"object","properties":{"search":{"type":"string"},"status":{"type":"string"},"session_id":{"type":"string"}},"additionalProperties":false}},
            {"name":"visual_get","description":"Get a visual by id","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_create","description":"Create a visual from a registered template. sourced.visual.v1 compiles arguments.content (allowlisted TSX) in the pane. Prefer create_with_bind with input+kind+data for experiment.overview.v1, analysis.visual.v1, and compose.visual.v1. compose.visual.v1 binds spec, then stream (eval) or optimizer_run (GEPA/SFT/CISPO optimizer_event.v1). Do not flatten Harbor/Craftax eval traces into optimizer_run. Hosted RLVR is CISPO, not rlvr.*. Unconstrained fetch/EventSource modules fail closed. For ad-hoc data charts prefer visual_chart.","inputSchema":{"type":"object","properties":{"template_id":{"type":"string"},"title":{"type":"string"},"content":{"type":"string"},"props":{"type":"object"},"bindings":{"type":"object"},"input":{"type":"string","description":"Required input name for create_with_bind, e.g. experiment or spec. slot still binds; new writers use input."},"slot":{"type":"string","description":"Read-only alias of input on stored envelopes; still binds."},"kind":{"type":"string","description":"Binding kind. Inline inputs require data."},"data":{"description":"Required when kind is inline"},"source":{"type":"string"},"poll_url":{"type":"string"},"path":{"type":"string"},"schema":{"type":"string"},"visual_config":{"type":"object"},"presentation":{"type":"string","enum":["canvas","pane"]},"session_id":{"type":"string"},"instance_id":{"type":"string"}},"required":["template_id"],"additionalProperties":false}},
            {"name":"visual_create_from_template","description":"Alias of visual_create","inputSchema":{"type":"object","properties":{"template_id":{"type":"string"},"title":{"type":"string"},"props":{"type":"object"},"instance_id":{"type":"string"}},"required":["template_id"],"additionalProperties":false}},
            {"name":"visual_update","description":"Revise visual bindings, title, trusted-template configuration, or Mermaid/systems/chart content","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"title":{"type":"string"},"content":{"type":"string"},"bindings":{"type":"object","description":"Canonical synth.visual-bindings.v1 envelope: {\"schemaVersion\":\"synth.visual-bindings.v1\",\"inputs\":[{\"input\":...,\"kind\":...,\"source\":...}]}. slot still binds on stored envelopes; new writers emit input/inputs. A slot-keyed map such as {\"stream\":[...]} is legacy, is upgraded with a warning, and will be refused in a later release. Prefer visual_bind_data_source.","properties":{"schemaVersion":{"type":"string","const":"synth.visual-bindings.v1"},"inputs":{"type":"array","items":{"type":"object","properties":{"input":{"type":"string"},"slot":{"type":"string"},"kind":{"type":"string"},"source":{"type":"string"},"poll_url":{"type":"string"},"path":{"type":"string"},"schema":{"type":"string"},"data":{}},"required":["kind"]}}},"required":["schemaVersion"]},"status":{"type":"string"},"visual_config":{"type":"object"},"presentation":{"type":"string","enum":["canvas","pane"]}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_update","description":"Revise visual bindings, title, trusted-template configuration, or Mermaid/systems/chart content","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"title":{"type":"string"},"content":{"type":"string"},"bindings":{"type":"object","description":"Canonical synth.visual-bindings.v1 envelope: {\"schemaVersion\":\"synth.visual-bindings.v1\",\"inputs\":[{\"input\":...,\"kind\":...,\"source\":...}]}. slot still binds on stored envelopes; new writers emit input/inputs. A slot-keyed map such as {\"stream\":[...]} is legacy, is upgraded with a warning, and will be refused in a later release. Prefer visual_bind_data_source.","properties":{"schemaVersion":{"type":"string","const":"synth.visual-bindings.v1"},"inputs":{"type":"array","items":{"type":"object","properties":{"input":{"type":"string"},"slot":{"type":"string"},"kind":{"type":"string"},"source":{"type":"string"},"poll_url":{"type":"string"},"path":{"type":"string"},"schema":{"type":"string"},"data":{}},"required":["kind"]}}},"required":["schemaVersion"]},"status":{"type":"string"},"visual_config":{"type":"object"},"presentation":{"type":"string","enum":["canvas","pane"]}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_bind_data_source","description":"Bind one input on a visual. This is the only supported way to write bindings: it emits the canonical synth.visual-bindings.v1 envelope. Inline inputs require data; other kinds require source. compose.visual.v1 stream is eval SSE; optimizer_run is optimizer_event.v1 (GEPA/SFT/CISPO). Use mode=append with bindings[] to put several sources on one input. slot still binds; new writers use input.","inputSchema":{"type":"object","properties":{"instance_id":{"type":"string"},"input":{"type":"string","description":"Bind-point name, e.g. spec, stream, optimizer_run"},"slot":{"type":"string","description":"Read-only alias of input on stored envelopes; still binds."},"mode":{"type":"string","enum":["replace","append"],"description":"replace (default) drops existing bindings on this input; append adds to them"},"kind":{"type":"string","enum":["trace_v5","local_cas","live_sse","fixture","inline","run_ref","optimizer_run","optimizer_snapshot","query_snapshot"]},"source":{"type":"string"},"data":{"description":"Required when kind is inline"},"poll_url":{"type":"string","description":"Exact normalized poll URL declared beside a live SSE source"},"path":{"type":"string"},"schema":{"type":"string"},"bindings":{"type":"array","description":"Several descriptors for one input. Each is {kind, source, data?, poll_url?, path?, schema?}; the named input is authoritative.","items":{"type":"object","properties":{"kind":{"type":"string"},"source":{"type":"string"},"data":{},"poll_url":{"type":"string"},"path":{"type":"string"},"schema":{"type":"string"}},"required":["kind"],"additionalProperties":false}}},"required":["instance_id"],"additionalProperties":false}},
            {"name":"visual_show","description":"Open a visual in the Desktop right pane","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"session_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"document_show","description":"Open one workspace file in the Desktop right pane. The path is resolved against this conversation's workspace roots; a path outside them is refused, and a file that cannot be typeset (missing, binary, a directory) comes back with the named reason. Read-only: there is no write or delete counterpart.","inputSchema":{"type":"object","properties":{"path":{"type":"string","description":"Absolute path, or a path relative to a workspace root, of the file to open"}},"required":["path"],"additionalProperties":false}},
            {"name":"visual_open_in_pane","description":"Alias of visual_show","inputSchema":{"type":"object","properties":{"instance_id":{"type":"string"}},"required":["instance_id"],"additionalProperties":false}},
            {"name":"visual_fork","description":"Fork a visual","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"title":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_chart","description":"Author or revise an ad-hoc data chart and get the rendered PNG back in one call. Pass a synth.visual.chart-spec.v1 object as spec; omit visual_id to create, pass it to revise. Panels: metrics, series (line/stepped/area with optional band), bars (grouped/stacked, vertical/horizontal), scatter (optional Pareto frontier), histogram, heatmap, table, note. Panels either carry literal values or derive them from bound evidence with a from block — bind a trace digest, fixture, CAS blob, or query snapshot with input/kind/source and the host reads it, so charting a trace does not mean pasting its numbers. Every value channel accepts null for an unmeasured point, which renders as a gap or a hatched cell — never as zero. Renders deterministically without opening the Desktop window.","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string","description":"Revise this chart instead of creating one"},"title":{"type":"string"},"spec":{"type":"object","description":"synth.visual.chart-spec.v1: {version:1, title?, subtitle?, theme?:light|dark, width?:480-2000, panels:[...]}. A panel carries literal values OR a from block: {from:{source:{slot,path?,projection?,transform:[...]}, ...channel mapping}}. Transforms: filter, sort, limit, select, unwind, unpivot, derive, groupAggregate, bin."},
              "slot":{"type":"string","description":"Read-only alias of input on stored envelopes; still binds."},
              "input":{"type":"string","description":"Bind evidence in the same call: the input name a from block reads"},
              "kind":{"type":"string","enum":["inline","fixture","local_cas","trace_v5","query_snapshot","optimizer_run","optimizer_snapshot"],"description":"Binding kind for this input. An optimizer_run may be read before it seals; optimizer_snapshot is immutable imported evidence."},
              "source":{"type":"string","description":"Trace digest, fixture path under visuals/, CAS digest, query snapshot id, or optimizer run id"},
              "data":{"description":"Required when kind is inline"},
              "bindings":{"type":"object","description":"A full synth.visual-bindings.v1 envelope, instead of input/kind/source"},"viewport":{"type":"object","properties":{"width":{"type":"integer","minimum":320,"maximum":2400}},"additionalProperties":false,"description":"Capture width; the height follows the chart so nothing is scaled down"},"capture":{"type":"boolean","description":"Default true; false returns the revision and findings without a PNG"},"presentation":{"type":"string","enum":["canvas","pane"]}},"required":["spec"],"additionalProperties":false}},
            {"name":"experiment_attach_evidence","description":"Attach a saved trace, visual/plot, artifact, or admitted-container reference to an experiment. `kind: container` requires container_id; `artifact` requires artifact_uri. References are local-first and may be materialized just in time when opened. Replaying the same evidence_id is idempotent.","inputSchema":{"type":"object","properties":{"experiment_id":{"type":"string"},"node_id":{"type":"string","description":"Optional experiment node; defaults to the latest result node"},"evidence_id":{"type":"string","description":"Stable caller-chosen idempotency key"},"kind":{"type":"string","enum":["trace","visual","artifact","container"]},"label":{"type":"string"},"digest":{"type":"string"},"container_id":{"type":"string"},"rollout_id":{"type":"string"},"trace_id":{"type":"string"},"visual_id":{"type":"string"},"artifact_uri":{"type":"string"},"metadata":{"type":"object"}},"required":["experiment_id","evidence_id","kind","label"],"additionalProperties":false}},
            {"name":"experiment_create","description":"Create or reopen the current task's saved experiment record. request_id is the stable idempotency key.","inputSchema":{"type":"object","properties":{"request_id":{"type":"string"},"title":{"type":"string"},"task":{"type":"string"},"model":{"type":"string"}},"required":["request_id","title"],"additionalProperties":false}},
            {"name":"experiment_create_child","description":"Create a child experiment linked to a parent. relation is follow_up (default), forked_from, or rerun_of. request_id is the stable idempotency key. Subsequent runs in this chat attach to the child.","inputSchema":{"type":"object","properties":{"parent_experiment_id":{"type":"string"},"request_id":{"type":"string"},"title":{"type":"string"},"task":{"type":"string"},"model":{"type":"string"},"relation":{"type":"string","enum":["follow_up","forked_from","rerun_of"]}},"required":["parent_experiment_id","request_id","title"],"additionalProperties":false}},
            {"name":"experiment_fork","description":"Fork a parent experiment (create_child with relation=forked_from). request_id is the stable idempotency key.","inputSchema":{"type":"object","properties":{"parent_experiment_id":{"type":"string"},"request_id":{"type":"string"},"title":{"type":"string"},"task":{"type":"string"},"model":{"type":"string"}},"required":["parent_experiment_id","request_id","title"],"additionalProperties":false}},
            {"name":"experiment_rerun","description":"Rerun a parent experiment (create_child with relation=rerun_of). request_id is the stable idempotency key.","inputSchema":{"type":"object","properties":{"parent_experiment_id":{"type":"string"},"request_id":{"type":"string"},"title":{"type":"string"},"task":{"type":"string"},"model":{"type":"string"}},"required":["parent_experiment_id","request_id","title"],"additionalProperties":false}},
            {"name":"experiment_relate","description":"Relate two members or two candidates in one experiment. relation is compared_with or promoted_to. Mixed member/candidate fails closed. Candidates are not experiment_edges rows.","inputSchema":{"type":"object","properties":{"experiment_id":{"type":"string"},"relation":{"type":"string","enum":["compared_with","promoted_to"]},"source_kind":{"type":"string","enum":["member","candidate"]},"source_id":{"type":"string"},"target_kind":{"type":"string","enum":["member","candidate"]},"target_id":{"type":"string"}},"required":["experiment_id","relation","source_kind","source_id","target_kind","target_id"],"additionalProperties":false}},
            {"name":"experiment_finalize","description":"Finalize a task-owned experiment with authoritative measured results and an honest assessment. Missing measurements must be null, never zero.","inputSchema":{"type":"object","properties":{"experiment_id":{"type":"string"},"status":{"type":"string","enum":["completed","partial","failed"]},"result":{"type":"object"},"assessment":{"type":"object"}},"required":["experiment_id","status","result"],"additionalProperties":false}},
            {"name":"visual_authoring_context","description":"Get the template contract, example evidence, revision, presentation, and outstanding quality gate for one visual","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_list_annotations","description":"List saved labels for a visual and its current overlay digest","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"}},"required":["visual_id"],"additionalProperties":false}},
            {"name":"visual_annotate","description":"Write a label anchored to one exact visual revision","inputSchema":{"type":"object","properties":{"visual_id":{"type":"string"},"revision":{"type":"integer"},"selector":{"type":"object"},"kind":{"type":"string","enum":["note","bug","highlight","reward","acceptance"]},"body":{"type":"string"},"source_digest":{"type":"string"},"supersedes_id":{"type":"string"}},"required":["visual_id","revision","selector","kind"],"additionalProperties":false}},
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
            {"name":"report_upsert_experiment","description":"Create or update an Experiment Record on a Report. experiment_group_id points at an ExperimentGroup when set.","inputSchema":{"type":"object","properties":{"report_id":{"type":"string"},"experiment_id":{"type":"string"},"experiment_group_id":{"type":"string"},"title":{"type":"string"},"hypothesis":{"type":"string"},"status":{"type":"string"},"protocol_digest":{"type":"string"},"arms":{"type":"array"},"runs":{"type":"array"},"results":{"type":"array"}},"required":["report_id","title"],"additionalProperties":false}},
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

/// The session this MCP server was spawned for, which is the only session it
/// may author into. Desktop writes `SYNTH_SESSION_ID` into each per-conversation
/// server config; without it a created visual would have no owner, and an
/// ownerless visual in an instance-global registry is adoptable by any chat.
fn require_session_identity(session_env: &Option<String>, action: &str) -> Result<String, String> {
    session_env
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            json!({
                "code": "visual_session_identity_missing",
                "error": format!("this visuals server has no bound session, so it cannot {action}"),
                "retryable": false,
                "remediation": "Chat-created visuals are owned by their conversation. Start this MCP server from a Desktop session so SYNTH_SESSION_ID is bound."
            })
            .to_string()
        })
}

fn arg_input_name(args: &Value) -> Result<Option<String>, String> {
    let input = args
        .get("input")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let slot = args
        .get("slot")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (input, slot) {
        (Some(a), Some(b)) if a == b => Ok(Some(a.to_string())),
        (Some(_), Some(_)) => Err("input and slot disagree; send one name".into()),
        (Some(a), None) | (None, Some(a)) => Ok(Some(a.to_string())),
        (None, None) => Ok(None),
    }
}

fn descriptor_name(descriptor: &Value) -> Option<&str> {
    let input = descriptor.get("input").and_then(Value::as_str);
    let slot = descriptor.get("slot").and_then(Value::as_str);
    match (input, slot) {
        (Some(a), Some(b)) if a == b => Some(a),
        (Some(a), None) | (None, Some(a)) => Some(a),
        _ => None,
    }
}

fn existing_descriptors(bindings: &Value) -> Vec<Value> {
    bindings
        .get("inputs")
        .or_else(|| bindings.get("slots"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn upgrade_bindings_for_write(bindings: &Value) -> Result<Value, String> {
    let inputs = bindings.get("inputs").and_then(Value::as_array);
    let slots = bindings.get("slots").and_then(Value::as_array);
    let descriptors = match (inputs, slots) {
        (Some(inputs), Some(slots)) if inputs == slots => inputs.clone(),
        (Some(_), Some(_)) => {
            return Err("bindings inputs and slots disagree; send one array".into());
        }
        (Some(inputs), None) => inputs.clone(),
        (None, Some(slots)) => slots.clone(),
        (None, None) => {
            return Ok(bindings.clone());
        }
    };
    let mut upgraded = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        let mut object = descriptor
            .as_object()
            .cloned()
            .ok_or_else(|| "binding descriptors must be objects".to_string())?;
        let input = object
            .get("input")
            .and_then(Value::as_str)
            .map(str::to_string);
        let slot = object
            .get("slot")
            .and_then(Value::as_str)
            .map(str::to_string);
        match (input, slot) {
            (Some(a), Some(b)) if a != b => {
                return Err("input and slot disagree; send one name".into());
            }
            (Some(name), _) | (None, Some(name)) => {
                object.insert("input".into(), json!(name));
            }
            (None, None) => {}
        }
        object.remove("slot");
        upgraded.push(Value::Object(object));
    }
    let mut envelope = bindings.clone();
    if let Some(object) = envelope.as_object_mut() {
        object.insert("schemaVersion".into(), json!("synth.visual-bindings.v1"));
        object.insert("inputs".into(), json!(upgraded));
        object.remove("slots");
    }
    Ok(envelope)
}

fn create_bindings_from_args(args: &Value) -> Result<Value, String> {
    if let Some(bindings) = args.get("bindings").or_else(|| args.get("props")) {
        if bindings.get("schemaVersion").and_then(Value::as_str) == Some("synth.visual-bindings.v1")
            || bindings.get("slots").is_some()
            || bindings.get("inputs").is_some()
        {
            return upgrade_bindings_for_write(bindings);
        }
    }
    if let Some(name) = arg_input_name(args)? {
        let kind = args.get("kind").and_then(Value::as_str).unwrap_or("inline");
        let mut descriptor = json!({
            "input": name,
            "kind": kind,
        });
        if let Some(object) = descriptor.as_object_mut() {
            for field in ["source", "poll_url", "path", "schema"] {
                if let Some(value) = args.get(field).cloned().filter(|value| !value.is_null()) {
                    object.insert(field.into(), value);
                }
            }
            if let Some(data) = args.get("data").cloned() {
                object.insert("data".into(), data);
            }
        }
        return Ok(json!({
            "schemaVersion": "synth.visual-bindings.v1",
            "inputs": [descriptor],
        }));
    }
    Ok(args
        .get("props")
        .or_else(|| args.get("bindings"))
        .cloned()
        .unwrap_or(json!({})))
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
        "visual_import_template" => {
            let source_path = args
                .get("source_path")
                .and_then(Value::as_str)
                .ok_or("source_path required")?;
            // Importing a package writes renderer code the app compiles at
            // every launch, so the host raises a `visual_template_persist`
            // card before any byte lands — and a card is raised on a
            // conversation. The identity is the server's bound session, never
            // an agent-supplied one, for the same reason `visual_create`
            // refuses to take ownership as an argument.
            let session_id = require_session_identity(&session_env, "import a template")?;
            request(
                "POST",
                "/v1/visuals/templates/import",
                Some(json!({"sourcePath": source_path, "sessionRef": session_id})),
            )
        }
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
            // Ownership is bound by the host, not claimed by the caller. An
            // agent-supplied `session_id` would let one chat author outputs
            // into another chat's rail, and an unowned visual lands in an
            // instance-global registry where every chat can adopt it.
            let session_id = require_session_identity(&session_env, "create a visual")?;
            let bindings = create_bindings_from_args(args)?;
            let body = json!({
                "templateId": args.get("template_id"),
                "title": args.get("title"),
                "bindings": bindings,
                "id": args.get("instance_id"),
                "sessionId": session_id,
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
            let name = arg_input_name(args)?.unwrap_or_else(|| "primary".into());
            // A template input declaring `multiple` accepts several bindings —
            // ten rollout streams on one `stream` input is the documented case.
            // Replace-only could not express it, which is what pushed authors
            // onto hand-built binding objects the renderer could not read.
            let mode = args
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("replace");
            if !matches!(mode, "replace" | "append") {
                return Err(format!(
                    "unsupported bind mode {mode:?}; use replace or append"
                ));
            }
            let authored: Vec<Value> = match args.get("bindings") {
                Some(Value::Array(items)) if !items.is_empty() => items.clone(),
                Some(_) => return Err("bindings must be a non-empty array".into()),
                None => vec![json!({
                    "kind": args.get("kind"),
                    "source": args.get("source"),
                    "data": args.get("data"),
                    "poll_url": args.get("poll_url"),
                    "path": args.get("path"),
                    "schema": args.get("schema"),
                })],
            };
            let mut slots = existing_descriptors(&existing);
            if mode == "replace" {
                slots.retain(|binding| descriptor_name(binding) != Some(name.as_str()));
            }
            for binding in authored {
                let mut binding = binding;
                let entry = binding
                    .as_object_mut()
                    .ok_or("each binding must be an object")?;
                // The named input is authoritative, so a batch cannot scatter
                // bindings across inputs the caller did not name.
                entry.insert("input".into(), json!(name.clone()));
                entry.remove("slot");
                entry.retain(|_, value| !value.is_null());
                slots.push(binding);
            }
            let bindings = json!({
                "schemaVersion": "synth.visual-bindings.v1",
                "inputs": slots,
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
        // The document rail's whole agent surface. It names a path; the host
        // re-resolves it against this conversation's workspace scope and opens
        // the pane through the same durable `visual.show` event a visual takes.
        // No read tool beside it: an agent that wants bytes has its own file
        // tools, and this one is about what the *reader* sees.
        "document_show" => {
            let path = args
                .get("path")
                .or_else(|| args.get("document_path"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or("path required")?;
            let session_id = require_session_identity(&session_env, "open a document")?;
            request(
                "POST",
                "/v1/documents/show",
                Some(json!({"path": path, "sessionRef": session_id})),
            )
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
        // One call per iteration: write the spec, render it, photograph it, and
        // hand back the image with the authoring findings. The agent's loop is
        // look → revise → look, and every extra hop in it is a hop where the
        // agent stops looking.
        "visual_chart" => {
            let spec = args.get("spec").ok_or("spec required")?;
            if !spec.is_object() {
                return Err("spec must be a synth.visual.chart-spec.v1 object".into());
            }
            let content =
                serde_json::to_string(spec).map_err(|error| format!("serialize spec: {error}"))?;
            let existing = args
                .get("visual_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            // A chart's evidence travels with its spec: binding the slot in the
            // same call is what makes "chart this trace" one round trip.
            let binds = args.get("bindings").is_some()
                || args.get("slot").is_some()
                || args.get("input").is_some();
            let bindings = if binds {
                Some(create_bindings_from_args(args)?)
            } else {
                None
            };
            let visual = match existing {
                Some(id) => {
                    let mut body = json!({ "content": content });
                    if let Some(title) = args.get("title").cloned() {
                        body["title"] = title;
                    }
                    if let Some(bindings) = bindings.clone() {
                        body["bindings"] = bindings;
                    }
                    request("POST", &format!("/v1/visuals/{id}"), Some(body))?
                }
                None => {
                    let session_id = require_session_identity(&session_env, "create a chart")?;
                    request(
                        "POST",
                        "/v1/visuals",
                        Some(json!({
                            "templateId": CHART_TEMPLATE_ID,
                            "title": args.get("title").cloned().unwrap_or(json!("Chart")),
                            "sessionId": session_id,
                            "sourceAgentId": "mcp",
                            "content": content,
                            "bindings": bindings.clone().unwrap_or(json!({})),
                            "metadata": {
                                "presentation": args
                                    .get("presentation")
                                    .cloned()
                                    .unwrap_or(json!("pane")),
                                "authoringReviews": []
                            },
                        })),
                    )?
                }
            };
            let id = visual
                .pointer("/visual/id")
                .and_then(Value::as_str)
                .ok_or("visual response missing id")?
                .to_string();
            let render_status = visual
                .pointer("/visual/metadata/renderStatus")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if render_status == "failed" {
                let reason = visual
                    .pointer("/visual/metadata/renderError")
                    .and_then(Value::as_str)
                    .unwrap_or("chart render failed");
                return Err(json!({
                    "code": "visual_chart_render_failed",
                    "visual_id": id,
                    "error": reason,
                    "retryable": true,
                    "remediation": "Fix the named field, then call visual_manage with operation chart and this visual_id."
                })
                .to_string());
            }
            let findings = visual
                .pointer("/visual/metadata/authoringFindings")
                .cloned()
                .unwrap_or(json!([]));
            let provenance = visual
                .pointer("/visual/metadata/dataProvenance")
                .cloned()
                .unwrap_or(Value::Null);
            let width = args
                .pointer("/viewport/width")
                .and_then(Value::as_u64)
                .unwrap_or(1280);
            if args.get("capture").and_then(Value::as_bool).unwrap_or(true) {
                let mut capture = call_tool(
                    "visual_capture_review",
                    &json!({"visual_id": id, "viewport": {"width": width, "height": 900}}),
                )?;
                if let Some(object) = capture.as_object_mut() {
                    object.insert("findings".into(), findings);
                    object.insert("data_provenance".into(), provenance);
                    object.insert(
                        "instruction".into(),
                        json!("Inspect the attached PNG. Revise through visual_manage operation chart with this visual_id until the chart reads correctly; findings list detected defects."),
                    );
                }
                return Ok(capture);
            }
            Ok(json!({
                "visual_id": id,
                "revision": visual.pointer("/visual/currentRevision").cloned(),
                "render_status": render_status,
                "findings": findings,
                "data_provenance": provenance,
            }))
        }
        "experiment_attach_evidence" => {
            let session_id = require_session_identity(&session_env, "attach experiment evidence")?;
            let experiment_id = args
                .get("experiment_id")
                .and_then(Value::as_str)
                .ok_or("experiment_id required")?;
            request(
                "POST",
                &format!("/v1/experiments/{experiment_id}/evidence"),
                Some(json!({
                    "sessionId": session_id, "nodeId": args.get("node_id"), "evidenceId": args.get("evidence_id"), "kind": args.get("kind"),
                    "label": args.get("label"), "digest": args.get("digest"), "containerId": args.get("container_id"),
                    "rolloutId": args.get("rollout_id"), "traceId": args.get("trace_id"), "visualId": args.get("visual_id"),
                    "artifactUri": args.get("artifact_uri"), "metadata": args.get("metadata")
                })),
            )
        }
        "experiment_create" => {
            let session_id = require_session_identity(&session_env, "create an experiment")?;
            request(
                "POST",
                "/v1/experiments",
                Some(
                    json!({"sessionId":session_id,"requestId":args.get("request_id"),"title":args.get("title"),"task":args.get("task"),"model":args.get("model")}),
                ),
            )
        }
        "experiment_create_child" | "experiment_fork" | "experiment_rerun" => {
            let session_id = require_session_identity(&session_env, "create a child experiment")?;
            let parent = args
                .get("parent_experiment_id")
                .and_then(Value::as_str)
                .ok_or("parent_experiment_id required")?;
            let relation = match name {
                "experiment_fork" => json!("forked_from"),
                "experiment_rerun" => json!("rerun_of"),
                _ => args.get("relation").cloned().unwrap_or(json!("follow_up")),
            };
            request(
                "POST",
                &format!("/v1/experiments/{parent}/children"),
                Some(json!({
                    "sessionId": session_id,
                    "requestId": args.get("request_id"),
                    "title": args.get("title"),
                    "task": args.get("task"),
                    "model": args.get("model"),
                    "relation": relation
                })),
            )
        }
        "experiment_relate" => {
            let _session_id = require_session_identity(&session_env, "relate experiment members")?;
            let experiment_id = args
                .get("experiment_id")
                .and_then(Value::as_str)
                .ok_or("experiment_id required")?;
            request(
                "POST",
                &format!("/v1/experiments/{experiment_id}/relate"),
                Some(json!({
                    "relation": args.get("relation"),
                    "sourceKind": args.get("source_kind"),
                    "sourceId": args.get("source_id"),
                    "targetKind": args.get("target_kind"),
                    "targetId": args.get("target_id")
                })),
            )
        }
        "experiment_finalize" => {
            let session_id = require_session_identity(&session_env, "finalize an experiment")?;
            let id = args
                .get("experiment_id")
                .and_then(Value::as_str)
                .ok_or("experiment_id required")?;
            request(
                "POST",
                &format!("/v1/experiments/{id}/finalize"),
                Some(
                    json!({"sessionId":session_id,"status":args.get("status"),"result":args.get("result"),"assessment":args.get("assessment")}),
                ),
            )
        }
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
            // A fork is the explicit way to take another chat's visual as your
            // own work, so the copy is owned by *this* session and keeps a
            // record of what it came from. Adopting the original silently is
            // what this replaces.
            let session_id = require_session_identity(&session_env, "fork a visual")?;
            request(
                "POST",
                &format!("/v1/visuals/{id}/fork"),
                Some(json!({"title": args.get("title"), "sessionId": session_id})),
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
    assert_review_viewport(width, height)?;
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
    let mut window_receipt = Value::Null;
    let mut captured = (width, height);
    let capture_mode = if matches!(
        renderer_kind,
        "mermaid" | "systems" | "systems-dynamic" | "chart"
    ) {
        request("POST", &format!("/v1/visuals/{id}/render"), None)?;
        captured = capture_svg_review(id, renderer_kind, width, height, &root, &stem, &png_path)?;
        "deterministic-svg"
    } else {
        window_receipt = capture_desktop_review(id, width, height, &png_path)?;
        "host-webview-snapshot"
    };
    // A rendered observation is evidence about the pane, and only templates
    // that declare an observation contract are certified against it. Requiring
    // one from every Desktop-rendered visual made capture deterministically
    // impossible for contract-free templates such as the Trace inspector, with
    // no path that could ever succeed.
    let observation = if capture_mode == "host-webview-snapshot" {
        let response = request("GET", &format!("/v1/review-observations/{id}"), None)?;
        let observed = response
            .get("observation")
            .cloned()
            .filter(|value| !value.is_null());
        let required = response
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        match observed {
            Some(value) => {
                let rendered = value.get("renderedRevision").and_then(Value::as_i64);
                if rendered != Some(revision) {
                    return Err(json!({
                        "code": "visual_observation_stale",
                        "visual_id": id,
                        "rendered_revision": rendered,
                        "durable_revision": revision,
                        "retryable": true,
                        "remediation": "The open pane is rendering an older revision. Re-show the visual, wait for the pane to settle on the current revision, then capture again."
                    }).to_string());
                }
                Some(value)
            }
            None if required => {
                return Err(json!({
                    "code": "visual_observation_unavailable",
                    "visual_id": id,
                    "revision": revision,
                    "retryable": true,
                    "remediation": "This template declares an observation contract, so the pane must publish a rendered observation. Show the visual in Desktop and let it finish rendering before capturing."
                }).to_string())
            }
            None => None,
        }
    } else {
        None
    };
    let captured_at = chrono::Utc::now().to_rfc3339();
    let observation_path = png_path.with_extension("observations.json");
    fs::write(
        &observation_path,
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": "synth.visual-capture-observation.v1",
            "visualId": id,
            "revision": revision,
            "screenshotPath": png_path.to_string_lossy(),
            "captureTime": captured_at,
            "observation": observation,
            "window": window_receipt,
        }))
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let png = fs::read(&png_path).map_err(|error| error.to_string())?;
    Ok(json!({
        "visual_id": id,
        "revision": revision,
        "renderer_kind": renderer_kind,
        "capture_mode": capture_mode,
        "viewport": {"width":captured.0,"height":captured.1},
        "requested_viewport": {"width":width,"height":height},
        "screenshot_path": png_path.to_string_lossy(),
        "capture_time": captured_at,
        "observations": observation,
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
) -> Result<(u64, u64), String> {
    CapturePlatform::current().require_macos("SVG review capture")?;
    let is_chart = renderer_kind == "chart";
    // A chart declares its own theme in the spec; asking for one here would let
    // the capture disagree with the pane. Diagrams keep their fixed pairing.
    let theme_request = if is_chart {
        json!({"size":"pane"})
    } else {
        json!({
            "theme": if renderer_kind == "mermaid" { "light" } else { "dark" },
            "size":"pane"
        })
    };
    let rendition = request(
        "GET",
        &format!("/v1/visuals/{id}/renditions/svg"),
        Some(theme_request),
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
    // A chart is a document, not a poster: scaling a tall stack of panels into
    // a fixed box shrinks its labels below reading size, which is exactly the
    // defect review is meant to catch. Keep the requested width, take the whole
    // height at that width, and report what was actually photographed.
    let (capture_width, capture_height, background) = if is_chart {
        let natural_w = rendition
            .pointer("/rendition/widthPx")
            .and_then(Value::as_u64)
            .unwrap_or(width)
            .max(1);
        let natural_h = rendition
            .pointer("/rendition/heightPx")
            .and_then(Value::as_u64)
            .unwrap_or(height)
            .max(1);
        let scaled = (width as f64 * natural_h as f64 / natural_w as f64).round() as u64;
        let theme = rendition
            .pointer("/rendition/theme")
            .and_then(Value::as_str)
            .unwrap_or("light");
        let background = if theme == "dark" {
            "#0D0F13"
        } else {
            "#FFFFFF"
        };
        (width, scaled.clamp(120, 12_000), background)
    } else {
        (width, height, "#090A0C")
    };
    render_svg_with_webkit(
        &svg_path,
        capture_width,
        capture_height,
        png_path,
        background,
    )?;
    assert_non_blank_png(png_path)?;
    Ok((capture_width, capture_height))
}

#[cfg(target_os = "macos")]
fn render_svg_with_webkit(
    svg_path: &std::path::Path,
    width: u64,
    height: u64,
    png_path: &std::path::Path,
    background: &str,
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
let background = CommandLine.arguments[5]
_ = NSApplication.shared
let webView = WKWebView(frame: NSRect(x: 0, y: 0, width: width, height: height))
let navigation = Navigation()
webView.navigationDelegate = navigation
let source = try String(contentsOf: input, encoding: .utf8)
let encoded = Data(source.utf8).base64EncodedString()
let html = """
<!doctype html><meta charset="utf-8"><style>
html,body{margin:0;width:100%;height:100%;overflow:hidden;background:\(background)}
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
        .arg(background)
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
    _background: &str,
) -> Result<(), String> {
    CapturePlatform::current().require_macos("SVG review capture")
}

/// Capture one review viewport and report exactly what was photographed.
///
/// The returned receipt is written beside the PNG. Reconstructing which window
/// a review captured, at what bounds, and whether the user's window was put
/// back should not require reading an agent transcript.
fn capture_desktop_review(
    id: &str,
    width: u64,
    height: u64,
    png_path: &std::path::Path,
) -> Result<Value, String> {
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
) -> Result<Value, String> {
    // Template visuals are React surfaces, not SVG renditions. Show the exact
    // visual, then ask the host to resize its own window, snapshot its own
    // WKWebView, and restore — one call, one process. The host photographs its
    // own surface, so the capture needs no Screen Recording TCC grant, no
    // window-identity resolution, and works while the app is occluded. A
    // helper that dies mid-capture can no longer strand the user's window at
    // the review size, because resize and restore never leave the host.
    request(
        "POST",
        &format!("/v1/visuals/{id}/show"),
        Some(json!({"presentation":"pane"})),
    )?;
    let receipt = request(
        "POST",
        "/v1/review-window/capture",
        Some(json!({
            "visualId": id,
            "width": width,
            "height": height,
            "outputPath": png_path.to_string_lossy(),
        })),
    )?;
    if !png_path.is_file() {
        return Err(
            "WebViewSnapshotFailed: host capture reported success but wrote no image".into(),
        );
    }
    assert_non_blank_png(png_path)?;
    Ok(json!({
        "schemaVersion": "synth.visual-capture-window.v1",
        "captureMode": "host-webview-snapshot",
        "requestedViewport": {"width": width, "height": height},
        "resizedViewport": receipt.get("current").cloned(),
        "previousViewport": receipt.get("previous").cloned(),
        "imageSize": {"width": receipt.get("width").cloned(), "height": receipt.get("height").cloned()},
        "scaleFactor": receipt.get("scaleFactor").cloned(),
        "processId": receipt.get("processId").cloned(),
        "windowLabel": receipt.get("windowLabel").cloned(),
        "restored": receipt.get("restored").cloned(),
    }))
}

/// The smallest review viewport the public capture schema accepts.
///
/// One declared bound, used by the schema, the resize endpoint, and this
/// module. Window *identity* never consults it: a window is the right window
/// or it is not, and how large it happens to be is a separate question with a
/// separate answer. Conflating the two rejected a legal 390-wide review of the
/// correct app. See: docs/contracts/desktop_review_capture.md.
const REVIEW_VIEWPORT_WIDTH_MIN: u64 = 320;
const REVIEW_VIEWPORT_HEIGHT_MIN: u64 = 400;
const REVIEW_VIEWPORT_WIDTH_MAX: u64 = 2400;
const REVIEW_VIEWPORT_HEIGHT_MAX: u64 = 1800;

fn assert_review_viewport(width: u64, height: u64) -> Result<(), String> {
    if !(REVIEW_VIEWPORT_WIDTH_MIN..=REVIEW_VIEWPORT_WIDTH_MAX).contains(&width)
        || !(REVIEW_VIEWPORT_HEIGHT_MIN..=REVIEW_VIEWPORT_HEIGHT_MAX).contains(&height)
    {
        return Err(format!(
            "ReviewViewportOutOfRange: requested {width}x{height}; the review viewport is \
             {REVIEW_VIEWPORT_WIDTH_MIN}x{REVIEW_VIEWPORT_HEIGHT_MIN} to \
             {REVIEW_VIEWPORT_WIDTH_MAX}x{REVIEW_VIEWPORT_HEIGHT_MAX}"
        ));
    }
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

        render_svg_with_webkit(&svg_path, 320, 180, &png_path, "#090A0C")
            .expect("render SVG through WebKit");
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
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    );
}
