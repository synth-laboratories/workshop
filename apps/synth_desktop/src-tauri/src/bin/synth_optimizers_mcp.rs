//! Stdio MCP adapter for Synth optimizers. Forwards tools through Desktop visuals IPC.

#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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

fn display_err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn request(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    request_inner(method, path, body).map_err(display_err)
}

fn request_inner(
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<Value, synth_desktop_lib::error::AppError> {
    let connection: Connection = serde_json::from_str(
        &fs::read_to_string(connection_file()).map_err(synth_desktop_lib::error::AppError::from)?,
    )
    .map_err(synth_desktop_lib::error::AppError::from)?;
    let payload = body
        .map(|v| serde_json::to_vec(&v).unwrap_or_default())
        .unwrap_or_default();
    let addr = connection
        .url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .parse::<std::net::SocketAddr>()
        .map_err(synth_desktop_lib::error::AppError::from)?;
    let mut stream =
        std::net::TcpStream::connect(addr).map_err(synth_desktop_lib::error::AppError::from)?;
    let wire = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        connection.token,
        payload.len()
    );
    stream
        .write_all(wire.as_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(synth_desktop_lib::error::AppError::from)?;
    let mut response = String::new();
    io::Read::read_to_string(&mut stream, &mut response)
        .map_err(synth_desktop_lib::error::AppError::from)?;
    serde_json::from_str(
        response
            .split("\r\n\r\n")
            .nth(1)
            .ok_or_else(|| synth_desktop_lib::error::AppError::message("empty IPC response"))?,
    )
    .map_err(synth_desktop_lib::error::AppError::from)
}

fn tools() -> Value {
    json!({"tools":[
        {"name":"optimizer_manage","description":"Operate Synth optimizer runs. Load the use-synth-optimizers skill for operation arguments and safe recipe sequencing.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["list_algorithms","list_recipes","start_recipe","list_runs","get_run","watch_run","get_state","reconcile_cloud","cancel_run","open_visual"]},"arguments":{"type":"object","additionalProperties":true}},"required":["operation","arguments"],"additionalProperties":false}},
        {"name":"optimizer_list_algorithms","description":"List optimizer algorithms and availability","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"optimizer_list_recipes","description":"List product-owned bounded optimizer recipes and their hard limits","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"optimizer_start_recipe","description":"Start an allowlisted optimizer recipe. No commands, paths, credentials, or arbitrary config are accepted. Optional base_model must be an id from docs/sft_tinker_base_models.toml.","inputSchema":{"type":"object","properties":{"recipe_id":{"type":"string","enum":["gepa.banking77.luna.v1","gepa.banking77.sol.v1","gelo.craftax.hosted.v1","sft.craftax.gpt-oss.smoke.v1","sft.hosted.fixture.v1","sft.craftax.nemotron-nano.tinker.v1"]},"session_ref":{"type":"string"},"open_visual":{"type":"boolean"},"base_model":{"type":"string"}},"required":["recipe_id"],"additionalProperties":false}},
        {"name":"optimizer_list_runs","description":"List local optimizer run mirrors","inputSchema":{"type":"object","properties":{"status":{"type":"string"},"algorithm_id":{"type":"string"},"source":{"type":"string"},"search":{"type":"string"}},"additionalProperties":false}},
        {"name":"optimizer_get_run","description":"Get one optimizer run mirror","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_create_run","description":"Create an optimizer run (local stub, cloud-hosted, fixture, or local path import)","inputSchema":{"type":"object","properties":{"algorithm_id":{"type":"string"},"objective":{"type":"string"},"session_ref":{"type":"string"},"source":{"type":"string","enum":["local","cloud"]},"local_path":{"type":"string"},"seed_fixture":{"type":"string"},"cloud_config":{"type":"object"},"open_visual":{"type":"boolean"}},"required":["algorithm_id"],"additionalProperties":false}},
        {"name":"optimizer_import_local","description":"Import a local OSS GEPA or optimizers-beta GELO run directory / events.jsonl","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"session_ref":{"type":"string"},"open_visual":{"type":"boolean"}},"required":["path"],"additionalProperties":false}},
        {"name":"optimizer_list_cloud","description":"List optimizer runs from Synth Cloud (optimizers-beta / hosted GEPA/GELO)","inputSchema":{"type":"object","properties":{"algorithm":{"type":"string"},"status":{"type":"string"},"limit":{"type":"integer"}},"additionalProperties":false}},
        {"name":"optimizer_reconcile_cloud","description":"Pull cloud-hosted run metadata + events into the local mirror","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"},"after_seq":{"type":"integer"},"open_visual":{"type":"boolean"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_get_state","description":"Read one or more projected state slices","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"},"slice_id":{"type":"string"},"slices":{"type":"array","items":{"type":"string"}},"at_seq":{"type":"integer"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_watch_run","description":"Read optimizer events after a sequence","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"},"after_seq":{"type":"integer"},"limit":{"type":"integer"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_cancel_run","description":"Cancel an optimizer run when capability allows","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_open_visual","description":"Open or create the linked optimizer visual","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"}},"required":["optimizer_run_id"],"additionalProperties":false}}
    ]})
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    if name == "optimizer_manage" {
        let operation = args
            .get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| "operation required".to_string())?;
        let nested = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let tool = match operation {
            "list_algorithms" => "optimizer_list_algorithms",
            "list_recipes" => "optimizer_list_recipes",
            "start_recipe" => "optimizer_start_recipe",
            "list_runs" => "optimizer_list_runs",
            "get_run" => "optimizer_get_run",
            "watch_run" => "optimizer_watch_run",
            "get_state" => "optimizer_get_state",
            "reconcile_cloud" => "optimizer_reconcile_cloud",
            "cancel_run" => "optimizer_cancel_run",
            "open_visual" => "optimizer_open_visual",
            other => return Err(format!("unknown optimizer operation {other}")),
        };
        return call_tool(tool, &nested);
    }
    let id = || {
        args.get("optimizer_run_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "optimizer_run_id required".to_string())
    };
    match name {
        "optimizer_list_algorithms" => request("GET", "/v1/optimizers/algorithms", None),
        "optimizer_list_recipes" => request("GET", "/v1/optimizers/recipes", None),
        "optimizer_start_recipe" => request(
            "POST",
            "/v1/optimizers/recipes/run",
            Some(json!({
                "recipeId": args.get("recipe_id"),
                "sessionRef": args.get("session_ref"),
                "openVisual": args.get("open_visual").cloned().unwrap_or(json!(true)),
                "baseModel": args.get("base_model")
            })),
        ),
        "optimizer_list_runs" => request(
            "GET",
            "/v1/optimizers/runs",
            Some(json!({
                "status": args.get("status"),
                "algorithmId": args.get("algorithm_id"),
                "source": args.get("source"),
                "search": args.get("search")
            })),
        ),
        "optimizer_get_run" => request("GET", &format!("/v1/optimizers/runs/{}", id()?), None),
        "optimizer_create_run" => request(
            "POST",
            "/v1/optimizers/runs",
            Some(json!({
                "algorithmId": args.get("algorithm_id"),
                "objective": args.get("objective"),
                "sessionRef": args.get("session_ref"),
                "source": args.get("source"),
                "localPath": args.get("local_path"),
                "seedFixture": args.get("seed_fixture"),
                "cloudConfig": args.get("cloud_config"),
                "openVisual": args.get("open_visual").cloned().unwrap_or(json!(true))
            })),
        ),
        "optimizer_import_local" => request(
            "POST",
            "/v1/optimizers/import_local",
            Some(json!({
                "path": args.get("path"),
                "sessionRef": args.get("session_ref"),
                "openVisual": args.get("open_visual").cloned().unwrap_or(json!(true))
            })),
        ),
        "optimizer_list_cloud" => request(
            "GET",
            "/v1/optimizers/cloud/runs",
            Some(json!({
                "algorithm": args.get("algorithm"),
                "status": args.get("status"),
                "limit": args.get("limit")
            })),
        ),
        "optimizer_reconcile_cloud" => request(
            "POST",
            "/v1/optimizers/reconcile_cloud",
            Some(json!({
                "optimizerRunId": args.get("optimizer_run_id"),
                "afterSeq": args.get("after_seq"),
                "openVisual": args.get("open_visual")
            })),
        ),
        "optimizer_get_state" => {
            if let Some(slice) = args.get("slice_id").and_then(Value::as_str) {
                request(
                    "GET",
                    &format!("/v1/optimizers/runs/{}/state/{}", id()?, slice),
                    Some(json!({ "at_seq": args.get("at_seq") })),
                )
            } else {
                request(
                    "GET",
                    &format!("/v1/optimizers/runs/{}/state/batch", id()?),
                    Some(json!({
                        "slices": args.get("slices"),
                        "at_seq": args.get("at_seq")
                    })),
                )
            }
        }
        "optimizer_watch_run" => request(
            "GET",
            &format!("/v1/optimizers/runs/{}/events", id()?),
            Some(json!({
                "after_seq": args.get("after_seq").cloned().unwrap_or(json!(0)),
                "limit": args.get("limit")
            })),
        ),
        "optimizer_cancel_run" => request(
            "POST",
            &format!("/v1/optimizers/runs/{}/cancel", id()?),
            None,
        ),
        "optimizer_open_visual" => request(
            "POST",
            &format!("/v1/optimizers/runs/{}/open_visual", id()?),
            None,
        ),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-optimizers-mcp",
            version: "0.1.0",
        },
        tools,
        call_tool,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_compact_manager_and_canonical_recipe() {
        let catalog = tools();
        let tools = catalog["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["name"] == "optimizer_manage"));
        let encoded = catalog.to_string();
        assert!(encoded.contains("gepa.banking77.luna.v1"));
        assert!(encoded.contains("gepa.banking77.sol.v1"));
        assert!(encoded.contains("sft.craftax.gpt-oss.smoke.v1"));
        assert!(encoded.contains("sft.hosted.fixture.v1"));
        assert!(encoded.contains("sft.craftax.nemotron-nano.tinker.v1"));
        assert!(!encoded.contains("api_key"));
    }
}
