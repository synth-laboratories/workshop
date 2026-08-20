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

fn resolved_session_ref(args: &Value, fallback: Option<&str>) -> Option<Value> {
    args.get("session_ref")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| fallback.map(str::trim).filter(|value| !value.is_empty()))
        .map(|value| Value::String(value.to_string()))
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
        {"name":"optimizer_manage","description":"Operate Synth optimizer runs. Prefer start_workflow for a bounded product recipe: it performs fresh admission, approval, run creation, and visual opening in one call. Advanced callers may still use prepare, open_visual, await_ready, start. Never install the plugin from this tool.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["list_algorithms","list_recipes","start_workflow","prepare","open_visual","await_ready","start","start_recipe","stage_eval_candidates","launch_artifact_inference","inspect_local_mlx","plan_model_install","install_model_or_runtime","create_training_plan","list_training_artifacts","inspect_training_artifact","launch_artifact_eval","export_or_delete_artifact","list_runs","get_run","watch_run","get_state","get_result","reconcile_cloud","cancel_run","cancel","pause_run","resume_run","finalize"]},"arguments":{"type":"object","additionalProperties":true}},"required":["operation","arguments"],"additionalProperties":false}},
        {"name":"optimizer_list_algorithms","description":"List optimizer algorithms and availability","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"optimizer_list_recipes","description":"List product-owned bounded optimizer recipes and their hard limits","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"optimizer_start_recipe","description":"Prepare an allowlisted paid/plugin recipe. For local eval.* recipes, start the fixed pinned recipe with a candidate_set_id staged by optimizer_stage_eval_candidates. Container baseline evals (Banking77, HealthBench) start from a registered container and do not take a candidate set.","inputSchema":{"type":"object","properties":{"recipe_id":{"type":"string","enum":["gepa.banking77.smoke.v1","gepa.banking77.luna.v1","gepa.banking77.sol.v1","gepa.craftax.smoke.v1","gelo.craftax.hosted.v1","sft.qwen35-0.8b.mlx.v1","sft.craftax.gpt-oss.smoke.v1","sft.hosted.fixture.v1","sft.craftax.nemotron-nano.tinker.v1","sft.banking77.nemotron-lightning.tinker.v1","sft.gsm8k.gpt-oss.smoke.v1","cispo.banking77.mlx.v1","cispo.slime.hosted.v1","eval.fixture.policy-smoke.v1","eval.craftax.code-policy.smoke.v1","eval.gamebench.craftax-code-policy.confirm.v1","eval.craftax.llm-policy.smoke.v1","eval.gamebench.llm-policy.confirm.v1","eval.banking77.baseline.v1","eval.healthbench.smoke.v1","eval.mlx.local-policy.smoke.v1"]},"session_ref":{"type":"string"},"open_visual":{"type":"boolean"},"base_model":{"type":"string"},"dataset_shard":{"type":"string","enum":["train_a","train_b"]},"candidate_set_id":{"type":"string","description":"Required by pinned local eval.* recipes unless training_artifact_id is set. An id returned by optimizer_stage_eval_candidates, never a path."},"training_artifact_id":{"type":"string","description":"Managed training artifact id. Eval stages mlx-lora.v1 from this record and retains identity in the receipt."}},"required":["recipe_id"],"additionalProperties":false}},
        {"name":"optimizer_start_workflow","description":"Start one bounded product workflow in one call. Freshens relevant registered-container capabilities, performs host approval and sidecar admission, creates the run, and opens its chat-owned visual. Craftax policy evals still require a staged candidate_set_id.","inputSchema":{"type":"object","properties":{"recipe_id":{"type":"string","enum":["gepa.banking77.smoke.v1","gepa.banking77.luna.v1","gepa.banking77.sol.v1","gepa.craftax.smoke.v1","gelo.craftax.hosted.v1","sft.qwen35-0.8b.mlx.v1","sft.craftax.gpt-oss.smoke.v1","sft.hosted.fixture.v1","sft.craftax.nemotron-nano.tinker.v1","sft.banking77.nemotron-lightning.tinker.v1","sft.gsm8k.gpt-oss.smoke.v1","cispo.banking77.mlx.v1","cispo.slime.hosted.v1","eval.fixture.policy-smoke.v1","eval.craftax.code-policy.smoke.v1","eval.gamebench.craftax-code-policy.confirm.v1","eval.craftax.llm-policy.smoke.v1","eval.gamebench.llm-policy.confirm.v1","eval.banking77.baseline.v1","eval.healthbench.smoke.v1","eval.mlx.local-policy.smoke.v1"]},"session_ref":{"type":"string"},"open_visual":{"type":"boolean"},"base_model":{"type":"string"},"dataset_shard":{"type":"string","enum":["train_a","train_b"]},"candidate_set_id":{"type":"string"}},"required":["recipe_id"],"additionalProperties":false}},
        {"name":"launch_artifact_inference","description":"Load a managed training artifact into local MLX and chat against that exact snapshot. Fails visibly if the adapter cannot load. Never uses ambient latest.","inputSchema":{"type":"object","properties":{"artifact_id":{"type":"string"},"message":{"type":"string"},"confirm":{"type":"boolean","description":"Must be true; inference is not silent."}},"required":["artifact_id","confirm"],"additionalProperties":false}},
        {"name":"inspect_local_mlx","description":"Read Apple Silicon, managed Qwen pin, revision, license, disk, and whether training weights are present. Never downloads.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"plan_model_install","description":"Preview the one-entry Qwen 3.5 0.8B training install (size, revision, license, already present). Never downloads.","inputSchema":{"type":"object","properties":{"model_id":{"type":"string"}},"additionalProperties":false}},
        {"name":"install_model_or_runtime","description":"Install the managed Qwen training weights after explicit confirm. Never silent. Does not set HF_HUB_OFFLINE off on the MLX child.","inputSchema":{"type":"object","properties":{"model_id":{"type":"string"},"confirm":{"type":"boolean"}},"required":["confirm"],"additionalProperties":false}},
        {"name":"create_training_plan","description":"Return the resolved local SFT or CISPO config without starting a run.","inputSchema":{"type":"object","properties":{"recipe_id":{"type":"string","enum":["sft.qwen35-0.8b.mlx.v1","cispo.banking77.mlx.v1"]}},"required":["recipe_id"],"additionalProperties":false}},
        {"name":"list_training_artifacts","description":"List managed training adapters (base model, producing run, digests, integrity).","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"inspect_training_artifact","description":"Inspect one managed training artifact by id.","inputSchema":{"type":"object","properties":{"artifact_id":{"type":"string"}},"required":["artifact_id"],"additionalProperties":false}},
        {"name":"launch_artifact_eval","description":"Start eval.mlx.local-policy.smoke.v1 against a managed training artifact. Requires confirm.","inputSchema":{"type":"object","properties":{"artifact_id":{"type":"string"},"recipe_id":{"type":"string"},"confirm":{"type":"boolean"}},"required":["artifact_id","confirm"],"additionalProperties":false}},
        {"name":"export_or_delete_artifact","description":"Export (return path) or delete a managed training artifact. Always requires confirm.","inputSchema":{"type":"object","properties":{"artifact_id":{"type":"string"},"operation":{"type":"string","enum":["export","delete"]},"confirm":{"type":"boolean"}},"required":["artifact_id","operation","confirm"],"additionalProperties":false}},
        {"name":"optimizer_stage_eval_candidates","description":"Freeze policy files from the session workspace into one immutable content-addressed candidate set and return its id. Paths are workspace-relative; absolute paths and traversal are refused.","inputSchema":{"type":"object","properties":{"session_ref":{"type":"string","description":"Optional. Defaults to the calling session; an agent has no way to know its own id, so do not guess one."},"candidates":{"type":"array","minItems":1,"maxItems":16,"items":{"type":"object","properties":{"label":{"type":"string"},"path":{"type":"string"},"entrypoint":{"type":"string"},"kind":{"type":"string"},"baseline":{"type":"boolean"}},"required":["label","path"],"additionalProperties":false}}},"required":["candidates"],"additionalProperties":false}},
        {"name":"optimizer_list_runs","description":"List local optimizer run mirrors","inputSchema":{"type":"object","properties":{"status":{"type":"string"},"algorithm_id":{"type":"string"},"source":{"type":"string"},"search":{"type":"string"}},"additionalProperties":false}},
        {"name":"optimizer_get_run","description":"Get one optimizer run mirror","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_create_run","description":"Create an optimizer run (local stub, cloud-hosted, fixture, or local path import)","inputSchema":{"type":"object","properties":{"algorithm_id":{"type":"string"},"objective":{"type":"string"},"session_ref":{"type":"string"},"source":{"type":"string","enum":["local","cloud"]},"local_path":{"type":"string"},"seed_fixture":{"type":"string"},"cloud_config":{"type":"object"},"open_visual":{"type":"boolean"}},"required":["algorithm_id"],"additionalProperties":false}},
        {"name":"optimizer_import_local","description":"Import a local OSS GEPA or optimizers-beta GELO run directory / events.jsonl","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"session_ref":{"type":"string"},"open_visual":{"type":"boolean"}},"required":["path"],"additionalProperties":false}},
        {"name":"optimizer_list_cloud","description":"List optimizer runs from Synth Cloud (optimizers-beta / hosted GEPA/GELO)","inputSchema":{"type":"object","properties":{"algorithm":{"type":"string"},"status":{"type":"string"},"limit":{"type":"integer"}},"additionalProperties":false}},
        {"name":"optimizer_reconcile_cloud","description":"Pull cloud-hosted run metadata + events into the local mirror","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"},"after_seq":{"type":"integer"},"open_visual":{"type":"boolean"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_get_state","description":"Read one or more projected state slices","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"},"slice_id":{"type":"string"},"slices":{"type":"array","items":{"type":"string"}},"at_seq":{"type":"integer"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_watch_run","description":"Read optimizer events after a sequence","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"},"after_seq":{"type":"integer"},"limit":{"type":"integer"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_cancel_run","description":"Cancel an optimizer run when capability allows","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_pause_run","description":"Hold a running optimizer when capability allows. An eval run stops dispatching new trials; trials already running finish and seal, and the run releases its semaphore capacity.","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_resume_run","description":"Resume a paused optimizer run where it left off. A pause changes timing, not evidence.","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_open_visual","description":"Create or reuse the optimizer's primary visual and show it live in the current Desktop conversation","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"},"session_ref":{"type":"string"}},"required":["optimizer_run_id"],"additionalProperties":false}}
    ]})
}

fn reject_secret_keys(args: &Value, allow_path: bool) -> Result<(), String> {
    let Some(object) = args.as_object() else {
        return Ok(());
    };
    for key in object.keys() {
        match key.as_str() {
            "url" | "command" | "env" | "token" | "api_key" | "credential" | "credentials" => {
                return Err(format!("optimizer arguments reject `{key}`"));
            }
            "path" | "local_path" if !allow_path => {
                return Err(format!("optimizer arguments reject `{key}`"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    if name == "optimizer_manage" {
        let operation = args
            .get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| "operation required".to_string())?;
        let nested = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let allow_path = matches!(operation, "import_local" | "create_run");
        reject_secret_keys(&nested, allow_path)?;
        let tool = match operation {
            "list_algorithms" => "optimizer_list_algorithms",
            "list_recipes" => "optimizer_list_recipes",
            "start_workflow" => "optimizer_start_workflow",
            "prepare" => "optimizer_prepare",
            "start_recipe" => "optimizer_start_recipe",
            "launch_artifact_inference" => "launch_artifact_inference",
            "inspect_local_mlx" => "inspect_local_mlx",
            "plan_model_install" => "plan_model_install",
            "install_model_or_runtime" => "install_model_or_runtime",
            "create_training_plan" => "create_training_plan",
            "list_training_artifacts" => "list_training_artifacts",
            "inspect_training_artifact" => "inspect_training_artifact",
            "launch_artifact_eval" => "launch_artifact_eval",
            "export_or_delete_artifact" => "export_or_delete_artifact",
            "stage_eval_candidates" => "optimizer_stage_eval_candidates",
            "start" => "optimizer_start",
            "await_ready" => "optimizer_await_ready",
            "get_result" => "optimizer_get_result",
            "finalize" => "optimizer_get_result",
            "list_runs" => "optimizer_list_runs",
            "get_run" => "optimizer_get_run",
            "watch_run" => "optimizer_watch_run",
            "get_state" => "optimizer_get_state",
            "reconcile_cloud" => "optimizer_reconcile_cloud",
            "cancel_run" | "cancel" => "optimizer_cancel_run",
            "pause_run" => "optimizer_pause_run",
            "resume_run" => "optimizer_resume_run",
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
    let current_session = env::var("SYNTH_SESSION_ID").ok();
    let session_ref = || resolved_session_ref(args, current_session.as_deref());
    match name {
        "optimizer_list_algorithms" => request("GET", "/v1/optimizers/algorithms", None),
        "optimizer_list_recipes" => request("GET", "/v1/optimizers/recipes", None),
        "optimizer_prepare" => request(
            "POST",
            "/v1/optimizers/recipes/prepare",
            Some(json!({
                "recipeId": args.get("recipe_id"),
                "sessionRef": session_ref(),
                "openVisual": args.get("open_visual").cloned().unwrap_or(json!(true))
            })),
        ),
        "optimizer_start_recipe" => request(
            "POST",
            if args
                .get("recipe_id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.starts_with("eval."))
            {
                "/v1/optimizers/recipes/run"
            } else {
                "/v1/optimizers/recipes/prepare"
            },
            Some(json!({
                "recipeId": args.get("recipe_id"),
                "sessionRef": session_ref(),
                "openVisual": args.get("open_visual").cloned().unwrap_or(json!(true)),
                "baseModel": args.get("base_model"),
                "datasetShard": args.get("dataset_shard"),
                "candidateSetId": args.get("candidate_set_id"),
                "trainingArtifactId": args.get("training_artifact_id")
            })),
        ),
        "optimizer_start_workflow" => request(
            "POST",
            "/v1/optimizers/workflows/start",
            Some(json!({
                "recipeId": args.get("recipe_id"),
                "sessionRef": session_ref(),
                "openVisual": args.get("open_visual").cloned().unwrap_or(json!(true)),
                "baseModel": args.get("base_model"),
                "datasetShard": args.get("dataset_shard"),
                "candidateSetId": args.get("candidate_set_id"),
                "trainingArtifactId": args.get("training_artifact_id")
            })),
        ),
        "optimizer_stage_eval_candidates" => request(
            "POST",
            "/v1/optimizers/eval/candidates",
            Some(json!({
                "sessionRef": session_ref(),
                "candidates": args.get("candidates")
            })),
        ),
        "launch_artifact_inference" => {
            let id = args
                .get("artifact_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "artifact_id required".to_string())?;
            if args.get("confirm").and_then(Value::as_bool) != Some(true) {
                return Err("launch_artifact_inference requires confirm=true".into());
            }
            request(
                "POST",
                &format!("/v1/training/artifacts/{id}/chat"),
                Some(json!({
                    "message": args.get("message"),
                    "confirm": true
                })),
            )
        }
        "inspect_local_mlx" => request("GET", "/v1/mlx/inspect", None),
        "plan_model_install" => request(
            "GET",
            "/v1/mlx/install-plan",
            Some(json!({ "model_id": args.get("model_id") })),
        ),
        "install_model_or_runtime" => {
            if args.get("confirm").and_then(Value::as_bool) != Some(true) {
                return Err("install_model_or_runtime requires confirm=true".into());
            }
            request(
                "POST",
                "/v1/mlx/install",
                Some(json!({
                    "model_id": args.get("model_id"),
                    "confirm": true
                })),
            )
        }
        "create_training_plan" => request(
            "POST",
            "/v1/training/plans",
            Some(json!({ "recipe_id": args.get("recipe_id") })),
        ),
        "list_training_artifacts" => request("GET", "/v1/training/artifacts", None),
        "inspect_training_artifact" => {
            let id = args
                .get("artifact_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "artifact_id required".to_string())?;
            request("GET", &format!("/v1/training/artifacts/{id}"), None)
        }
        "launch_artifact_eval" => {
            let id = args
                .get("artifact_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "artifact_id required".to_string())?;
            if args.get("confirm").and_then(Value::as_bool) != Some(true) {
                return Err("launch_artifact_eval requires confirm=true".into());
            }
            request(
                "POST",
                &format!("/v1/training/artifacts/{id}/eval"),
                Some(json!({
                    "recipe_id": args.get("recipe_id"),
                    "confirm": true,
                    "session_ref": session_ref(),
                    "open_visual": args.get("open_visual").cloned().unwrap_or(json!(true))
                })),
            )
        }
        "export_or_delete_artifact" => {
            let id = args
                .get("artifact_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "artifact_id required".to_string())?;
            if args.get("confirm").and_then(Value::as_bool) != Some(true) {
                return Err("export_or_delete_artifact requires confirm=true".into());
            }
            let operation = args
                .get("operation")
                .and_then(Value::as_str)
                .ok_or_else(|| "operation required".to_string())?;
            if operation != "export" && operation != "delete" {
                return Err("export_or_delete_artifact operation must be export or delete".into());
            }
            request(
                "POST",
                &format!("/v1/training/artifacts/{id}/{operation}"),
                Some(json!({ "confirm": true })),
            )
        }
        "optimizer_start" => request(
            "POST",
            "/v1/optimizers/runs/start",
            Some(json!({
                "optimizerRunId": args.get("optimizer_run_id"),
                "preparationDigest": args.get("preparation_digest"),
                "approvalReceiptId": args.get("approval_receipt_id"),
                "sessionRef": session_ref()
            })),
        ),
        "optimizer_await_ready" => request(
            "GET",
            &format!("/v1/optimizers/runs/{}/ready", id()?),
            Some(json!({ "timeout_ms": args.get("timeout_ms") })),
        ),
        "optimizer_get_result" => request(
            "GET",
            &format!("/v1/optimizers/runs/{}/result", id()?),
            None,
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
                "sessionRef": session_ref(),
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
                "sessionRef": session_ref(),
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
        "optimizer_pause_run" => request(
            "POST",
            &format!("/v1/optimizers/runs/{}/pause", id()?),
            None,
        ),
        "optimizer_resume_run" => request(
            "POST",
            &format!("/v1/optimizers/runs/{}/resume", id()?),
            None,
        ),
        "optimizer_open_visual" => request(
            "POST",
            &format!("/v1/optimizers/runs/{}/open_visual", id()?),
            Some(json!({ "sessionRef": session_ref() })),
        ),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-optimizers-mcp",
            version: env!("CARGO_PKG_VERSION"),
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
        assert!(encoded.contains("gepa.banking77.smoke.v1"));
        assert!(encoded.contains("gepa.banking77.luna.v1"));
        assert!(encoded.contains("gepa.banking77.sol.v1"));
        assert!(encoded.contains("gepa.craftax.smoke.v1"));
        assert!(encoded.contains("sft.craftax.gpt-oss.smoke.v1"));
        assert!(encoded.contains("sft.qwen35-0.8b.mlx.v1"));
        assert!(encoded.contains("sft.hosted.fixture.v1"));
        assert!(encoded.contains("cispo.banking77.mlx.v1"));
        assert!(encoded.contains("cispo.slime.hosted.v1"));
        assert!(encoded.contains("sft.craftax.nemotron-nano.tinker.v1"));
        assert!(encoded.contains("sft.banking77.nemotron-lightning.tinker.v1"));
        assert!(encoded.contains("sft.gsm8k.gpt-oss.smoke.v1"));
        assert!(encoded.contains("eval.banking77.baseline.v1"));
        assert!(encoded.contains("eval.healthbench.smoke.v1"));
        assert!(encoded.contains("eval.mlx.local-policy.smoke.v1"));
        assert!(encoded.contains("launch_artifact_inference"));
        assert!(encoded.contains("inspect_local_mlx"));
        assert!(encoded.contains("plan_model_install"));
        assert!(encoded.contains("install_model_or_runtime"));
        assert!(encoded.contains("create_training_plan"));
        assert!(encoded.contains("list_training_artifacts"));
        assert!(encoded.contains("inspect_training_artifact"));
        assert!(encoded.contains("launch_artifact_eval"));
        assert!(encoded.contains("export_or_delete_artifact"));
        assert!(encoded.contains("training_artifact_id"));
        assert!(encoded.contains("dataset_shard"));
        assert!(encoded.contains("optimizer_pause_run"));
        assert!(encoded.contains("optimizer_resume_run"));
        assert!(encoded.contains("optimizer_start_workflow"));
        assert!(encoded.contains("start_workflow"));
        assert!(!encoded.contains("api_key"));
    }

    #[test]
    fn defaults_presentation_to_the_current_conversation() {
        assert_eq!(
            resolved_session_ref(&json!({}), Some("session_current")),
            Some(json!("session_current"))
        );
        assert_eq!(
            resolved_session_ref(
                &json!({"session_ref": "session_explicit"}),
                Some("session_current")
            ),
            Some(json!("session_explicit"))
        );
        assert_eq!(
            resolved_session_ref(&json!({"session_ref": null}), Some("session_current")),
            Some(json!("session_current"))
        );
    }

    #[test]
    fn prepare_and_start_reject_urls_paths_and_credentials() {
        let err = call_tool(
            "optimizer_manage",
            &json!({
                "operation": "prepare",
                "arguments": {
                    "recipe_id": "gepa.banking77.smoke.v1",
                    "url": "https://evil.example"
                }
            }),
        )
        .unwrap_err();
        assert!(err.contains("reject"));
        let err = call_tool(
            "optimizer_manage",
            &json!({
                "operation": "start",
                "arguments": {"optimizer_run_id": "run_1", "path": "/tmp/secret"}
            }),
        )
        .unwrap_err();
        assert!(err.contains("reject"));
        let err = call_tool(
            "optimizer_manage",
            &json!({
                "operation": "get_result",
                "arguments": {"optimizer_run_id": "run_1", "env": {"OPENAI_API_KEY": "x"}}
            }),
        )
        .unwrap_err();
        assert!(err.contains("reject"));
    }

    #[test]
    fn mutating_mlx_capabilities_require_confirm_before_ipc() {
        let err = call_tool(
            "install_model_or_runtime",
            &json!({ "confirm": false }),
        )
        .unwrap_err();
        assert!(err.contains("confirm=true"), "{err}");
        let err = call_tool(
            "launch_artifact_eval",
            &json!({ "artifact_id": "a1", "confirm": false }),
        )
        .unwrap_err();
        assert!(err.contains("confirm=true"), "{err}");
        let err = call_tool(
            "export_or_delete_artifact",
            &json!({ "artifact_id": "a1", "operation": "delete", "confirm": false }),
        )
        .unwrap_err();
        assert!(err.contains("confirm=true"), "{err}");
        let src = include_str!("synth_optimizers_mcp.rs");
        let forbidden = ["training_models", "_download"].concat();
        assert!(
            !src.contains(&forbidden),
            "MCP must not call the Settings download command by name"
        );
    }
}
