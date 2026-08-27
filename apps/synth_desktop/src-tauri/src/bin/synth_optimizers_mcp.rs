#![recursion_limit = "512"]
//! Stdio MCP adapter for Synth optimizers. Forwards tools through Desktop visuals IPC.

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
};

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
    let mut manifest = json!({"tools":[
        {"name":"optimizer_manage","description":"Operate Synth optimizer runs and the checkpoint catalog. Inline evaluation is the default: draft and inspect the immutable spec, then start it with bounded approval. Catalog recipes are only for an explicit catalog request.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["evaluation_spec_draft","evaluation_spec_validate","evaluation_spec_admit","evaluation_start","reconcile_evaluation_evidence","list_algorithms","list_recipes","start_workflow","prepare","open_visual","await_ready","start","start_recipe","stage_eval_candidates","launch_artifact_inference","inspect_local_mlx","inspect_training_runtime","install_training_runtime","plan_model_install","install_model_or_runtime","create_training_plan","list_training_artifacts","inspect_training_artifact","launch_artifact_eval","export_or_delete_artifact","list_runs","get_run","watch_run","get_state","get_result","reconcile_cloud","cancel_run","cancel","pause_run","resume_run","finalize","list_checkpoints","archive_checkpoint","import_checkpoint","infer_checkpoint","update_checkpoint","publish_checkpoint"]},"arguments":{"type":"object","additionalProperties":true}},"required":["operation","arguments"],"additionalProperties":false}},
        {"name":"optimizer_list_algorithms","description":"List optimizer algorithms and availability","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"optimizer_list_recipes","description":"List workspace-declared recipes for this session plus remaining product training recipes. Task GEPA/eval ids come from workshop.recipe.toml, never a shipped catalog.","inputSchema":{"type":"object","properties":{"session_ref":{"type":"string"}},"additionalProperties":false}},
        {"name":"optimizer_evaluation_spec_draft","description":"Default evaluation path. Construct, validate, pin, and hash an inline execution specification from the requested container, policy, model, seeds, and hard limits. Does not request approval or spend.","inputSchema":{"type":"object","properties":{"containerId":{"type":"string"},"family":{"type":"string"},"policyNamespace":{"type":"string"},"policyName":{"type":"string"},"policyOverrides":{"type":"object"},"provider":{"type":"string"},"modelId":{"type":"string"},"seeds":{"type":"array","items":{"type":"integer"},"minItems":1},"maximumRollouts":{"type":"integer","minimum":1},"maximumModelCallsPerRollout":{"type":"integer","minimum":1},"maximumStepsPerRollout":{"type":"integer","minimum":1},"hardTotalCostUsd":{"type":"number","exclusiveMinimum":0}},"required":["policyNamespace","policyName","provider","modelId","seeds","maximumRollouts","maximumModelCallsPerRollout","maximumStepsPerRollout","hardTotalCostUsd"],"additionalProperties":false}},
        {"name":"optimizer_evaluation_spec_validate","description":"Re-resolve authority and validate an inline evaluation specification. Does not request approval or spend.","inputSchema":{"type":"object","properties":{"containerId":{"type":"string"},"family":{"type":"string"},"policyNamespace":{"type":"string"},"policyName":{"type":"string"},"policyOverrides":{"type":"object"},"provider":{"type":"string"},"modelId":{"type":"string"},"seeds":{"type":"array","items":{"type":"integer"},"minItems":1},"maximumRollouts":{"type":"integer","minimum":1},"maximumModelCallsPerRollout":{"type":"integer","minimum":1},"maximumStepsPerRollout":{"type":"integer","minimum":1},"hardTotalCostUsd":{"type":"number","exclusiveMinimum":0}},"required":["policyNamespace","policyName","provider","modelId","seeds","maximumRollouts","maximumModelCallsPerRollout","maximumStepsPerRollout","hardTotalCostUsd"],"additionalProperties":false}},
        {"name":"optimizer_evaluation_spec_admit","description":"Return the immutable inline execution specification, digest, and exact approval disclosure. Does not spend.","inputSchema":{"type":"object","properties":{"containerId":{"type":"string"},"family":{"type":"string"},"policyNamespace":{"type":"string"},"policyName":{"type":"string"},"policyOverrides":{"type":"object"},"provider":{"type":"string"},"modelId":{"type":"string"},"seeds":{"type":"array","items":{"type":"integer"},"minItems":1},"maximumRollouts":{"type":"integer","minimum":1},"maximumModelCallsPerRollout":{"type":"integer","minimum":1},"maximumStepsPerRollout":{"type":"integer","minimum":1},"hardTotalCostUsd":{"type":"number","exclusiveMinimum":0}},"required":["policyNamespace","policyName","provider","modelId","seeds","maximumRollouts","maximumModelCallsPerRollout","maximumStepsPerRollout","hardTotalCostUsd"],"additionalProperties":false}},
        {"name":"optimizer_evaluation_start","description":"Rebuild and revalidate the inline specification, request digest-bound paid-compute approval, reverify drift, start the exact run, and attach its Workshop visual.","inputSchema":{"type":"object","properties":{"containerId":{"type":"string"},"family":{"type":"string"},"policyNamespace":{"type":"string"},"policyName":{"type":"string"},"policyOverrides":{"type":"object"},"provider":{"type":"string"},"modelId":{"type":"string"},"seeds":{"type":"array","items":{"type":"integer"},"minItems":1},"maximumRollouts":{"type":"integer","minimum":1},"maximumModelCallsPerRollout":{"type":"integer","minimum":1},"maximumStepsPerRollout":{"type":"integer","minimum":1},"hardTotalCostUsd":{"type":"number","exclusiveMinimum":0},"sessionRef":{"type":"string"},"openVisual":{"type":"boolean"}},"required":["policyNamespace","policyName","provider","modelId","seeds","maximumRollouts","maximumModelCallsPerRollout","maximumStepsPerRollout","hardTotalCostUsd"],"additionalProperties":false}},
        {"name":"optimizer_reconcile_evaluation_evidence","description":"For a terminal inline evaluation, re-import its already-sealed Trace V5 bundles and rebuild the authoritative rollout and visual projections. Never starts compute or accesses credentials.","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_start_recipe","description":"Start a workspace-declared or remaining product recipe. Workspace GEPA/eval ids are whatever workshop.recipe.toml declared. Local candidate-comparison eval.* still takes candidate_set_id. Workspace baseline evals take container_id from container_ensure.","inputSchema":{"type":"object","properties":{"recipe_id":{"type":"string"},"session_ref":{"type":"string"},"open_visual":{"type":"boolean"},"base_model":{"type":"string"},"dataset_shard":{"type":"string","enum":["train_a","train_b"]},"candidate_set_id":{"type":"string","description":"Required by pinned local candidate-comparison eval.* recipes. An id returned by optimizer_stage_eval_candidates, never a path."},"training_artifact_id":{"type":"string","description":"Verified local training artifact used as the CISPO warm start."},"container_id":{"type":"string","description":"Registered-container identity from container_ensure. Required when multiple healthy pools advertise the same family."}},"required":["recipe_id"],"additionalProperties":false}},
        {"name":"optimizer_start_workflow","description":"Start one bounded workflow in one call. Workspace task recipes are declared in workshop.recipe.toml. Freshens registered-container capabilities, performs host approval and sidecar admission, creates the run, and opens its chat-owned visual.","inputSchema":{"type":"object","properties":{"recipe_id":{"type":"string"},"session_ref":{"type":"string"},"open_visual":{"type":"boolean"},"base_model":{"type":"string"},"dataset_shard":{"type":"string","enum":["train_a","train_b"]},"candidate_set_id":{"type":"string"},"training_artifact_id":{"type":"string","description":"Verified local training artifact used as the CISPO warm start."},"container_id":{"type":"string"},"plan_override":{"type":"object","description":"Optional trusted narrowing only: candidate_ids, seeds/screening_seeds/confirmation_seeds, and model_efforts."}},"required":["recipe_id"],"additionalProperties":false}},
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
        {"name":"optimizer_open_visual","description":"Create or reuse the optimizer's primary visual and show it live in the current Desktop conversation","inputSchema":{"type":"object","properties":{"optimizer_run_id":{"type":"string"},"session_ref":{"type":"string"}},"required":["optimizer_run_id"],"additionalProperties":false}},
        {"name":"optimizer_list_checkpoints","description":"List This Mac MLX and hosted Tinker LoRA checkpoints in the union catalog. Placement is this_mac, hosted, or all.","inputSchema":{"type":"object","properties":{"search":{"type":"string"},"placement":{"type":"string","enum":["all","this_mac","hosted"]},"scope":{"type":"string","enum":["all","mine","org"]},"provider":{"type":"string"},"checkpoint_kind":{"type":"string","enum":["inference","training"]},"base_model":{"type":"string"},"run_id":{"type":"string"},"optimizer_algorithm":{"type":"string","enum":["sft","cispo","ppo"]},"status":{"type":"string"},"limit":{"type":"integer"},"offset":{"type":"integer"}},"additionalProperties":false}},
        {"name":"optimizer_archive_checkpoint","description":"Archive a catalog LoRA. Local archive hides the row; hosted archive uses the cloud API.","inputSchema":{"type":"object","properties":{"checkpoint_id":{"type":"string"}},"required":["checkpoint_id"],"additionalProperties":false}},
        {"name":"optimizer_import_checkpoint","description":"Import a validated mlx-lora.v1 folder into the This Mac catalog. Path must be a local adapter directory with adapter_config.json and adapters.safetensors.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}},
        {"name":"optimizer_infer_checkpoint","description":"Sample an inference-kind catalog LoRA with a native OpenAI family body. family is chat_completions or responses. Never wrap a {message, reply} helper and never name mlx-rl, Tinker, or loopback ports.","inputSchema":{"type":"object","properties":{"checkpoint_id":{"type":"string"},"family":{"type":"string","enum":["chat_completions","responses"]},"body":{"type":"object","additionalProperties":true}},"required":["checkpoint_id","family","body"],"additionalProperties":false}},
        {"name":"optimizer_update_checkpoint","description":"Rename, tag, or add notes on a catalog LoRA. Bytes stay immutable.","inputSchema":{"type":"object","properties":{"checkpoint_id":{"type":"string"},"name":{"type":"string"},"description":{"type":"string"},"tags":{"type":"array","items":{"type":"string"}}},"required":["checkpoint_id"],"additionalProperties":false}},
        {"name":"optimizer_publish_checkpoint","description":"Explicitly publish a This Mac MLX adapter into the hosted catalog. Never auto-upload.","inputSchema":{"type":"object","properties":{"checkpoint_id":{"type":"string"}},"required":["checkpoint_id"],"additionalProperties":false}}
    ]});
    let inline_tools = [
        "optimizer_evaluation_spec_draft",
        "optimizer_evaluation_spec_validate",
        "optimizer_evaluation_spec_admit",
        "optimizer_evaluation_start",
    ];
    for tool in manifest["tools"]
        .as_array_mut()
        .expect("optimizer tool manifest is an array")
    {
        if !tool
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| inline_tools.contains(&name))
        {
            continue;
        }
        tool.pointer_mut("/inputSchema/properties")
            .and_then(Value::as_object_mut)
            .expect("inline optimizer tool has a property schema")
            .insert(
                "policySourcePath".into(),
                json!({
                    "type": "string",
                    "description": "Repository-relative policy source read from the container's declared immutable source revision. Workshop never guesses this path."
                }),
            );
    }
    manifest
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
        let allow_path = matches!(
            operation,
            "import_local" | "create_run" | "import_checkpoint"
        );
        reject_secret_keys(&nested, allow_path)?;
        let tool = match operation {
            "evaluation_spec_draft" => "optimizer_evaluation_spec_draft",
            "evaluation_spec_validate" => "optimizer_evaluation_spec_validate",
            "evaluation_spec_admit" => "optimizer_evaluation_spec_admit",
            "evaluation_start" => "optimizer_evaluation_start",
            "reconcile_evaluation_evidence" => "optimizer_reconcile_evaluation_evidence",
            "list_algorithms" => "optimizer_list_algorithms",
            "list_recipes" => "optimizer_list_recipes",
            "start_workflow" => "optimizer_start_workflow",
            "prepare" => "optimizer_prepare",
            "start_recipe" => "optimizer_start_recipe",
            "stage_eval_candidates" => "optimizer_stage_eval_candidates",
            "inspect_local_mlx" => "optimizer_inspect_local_mlx",
            "inspect_training_runtime" => "optimizer_inspect_training_runtime",
            "install_training_runtime" => "optimizer_install_training_runtime",
            "plan_model_install" => "optimizer_plan_model_install",
            "install_model_or_runtime" => "optimizer_install_model_or_runtime",
            "create_training_plan" => "optimizer_create_training_plan",
            "list_training_artifacts" => "optimizer_list_training_artifacts",
            "inspect_training_artifact" => "optimizer_inspect_training_artifact",
            "launch_artifact_inference" => "optimizer_launch_artifact_inference",
            "launch_artifact_eval" => "optimizer_launch_artifact_eval",
            "export_or_delete_artifact" => "optimizer_export_or_delete_artifact",
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
            "list_checkpoints" => "optimizer_list_checkpoints",
            "archive_checkpoint" => "optimizer_archive_checkpoint",
            "import_checkpoint" => "optimizer_import_checkpoint",
            "infer_checkpoint" => "optimizer_infer_checkpoint",
            "update_checkpoint" => "optimizer_update_checkpoint",
            "publish_checkpoint" => "optimizer_publish_checkpoint",
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
        "optimizer_evaluation_spec_draft" => request(
            "POST",
            "/v1/optimizers/evaluations/spec/draft",
            Some(args.clone()),
        ),
        "optimizer_evaluation_spec_validate" => request(
            "POST",
            "/v1/optimizers/evaluations/spec/validate",
            Some(args.clone()),
        ),
        "optimizer_evaluation_spec_admit" => request(
            "POST",
            "/v1/optimizers/evaluations/spec/admit",
            Some(args.clone()),
        ),
        "optimizer_evaluation_start" => request(
            "POST",
            "/v1/optimizers/evaluations/start",
            Some(args.clone()),
        ),
        "optimizer_list_algorithms" => request("GET", "/v1/optimizers/algorithms", None),
        "optimizer_list_recipes" => request("GET", "/v1/optimizers/recipes", None),
        "optimizer_inspect_local_mlx" => request("GET", "/v1/mlx/inspect", None),
        "optimizer_inspect_training_runtime" => request("GET", "/v1/training/mlx-runtime", None),
        "optimizer_install_training_runtime" => request(
            "POST",
            "/v1/training/mlx-runtime/install",
            Some(json!({ "confirm": args.get("confirm") })),
        ),
        "optimizer_plan_model_install" => request(
            "GET",
            "/v1/mlx/install-plan",
            Some(json!({ "model_id": args.get("model_id") })),
        ),
        "optimizer_install_model_or_runtime" => request(
            "POST",
            "/v1/mlx/install",
            Some(json!({
                "model_id": args.get("model_id"),
                "confirm": args.get("confirm")
            })),
        ),
        "optimizer_create_training_plan" => request(
            "POST",
            "/v1/training/plans",
            Some(json!({ "recipe_id": args.get("recipe_id") })),
        ),
        "optimizer_list_training_artifacts" => request("GET", "/v1/training/artifacts", None),
        "optimizer_inspect_training_artifact" => {
            let artifact_id = args
                .get("artifact_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "artifact_id required".to_string())?;
            request(
                "GET",
                &format!("/v1/training/artifacts/{artifact_id}"),
                None,
            )
        }
        "optimizer_launch_artifact_inference" => {
            let artifact_id = args
                .get("artifact_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "artifact_id required".to_string())?;
            request(
                "POST",
                &format!("/v1/training/artifacts/{artifact_id}/chat"),
                Some(json!({
                    "confirm": args.get("confirm"),
                    "message": args.get("message")
                })),
            )
        }
        "optimizer_launch_artifact_eval" => {
            let artifact_id = args
                .get("artifact_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "artifact_id required".to_string())?;
            request(
                "POST",
                &format!("/v1/training/artifacts/{artifact_id}/eval"),
                Some(json!({
                    "confirm": args.get("confirm"),
                    "recipe_id": args.get("recipe_id"),
                    "sessionRef": session_ref(),
                    "openVisual": args.get("open_visual")
                })),
            )
        }
        "optimizer_export_or_delete_artifact" => {
            let artifact_id = args
                .get("artifact_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "artifact_id required".to_string())?;
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| "action required".to_string())?;
            if action != "export" && action != "delete" {
                return Err("action must be export or delete".into());
            }
            request(
                "POST",
                &format!("/v1/training/artifacts/{artifact_id}/{action}"),
                Some(json!({
                    "confirm": args.get("confirm"),
                    "destination": args.get("destination"),
                    "digest": args.get("digest")
                })),
            )
        }
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
                "trainingArtifactId": args.get("training_artifact_id"),
                "containerId": args.get("container_id"),
                "planOverride": args.get("plan_override")
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
                "trainingArtifactId": args.get("training_artifact_id"),
                "containerId": args.get("container_id"),
                "planOverride": args.get("plan_override")
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
        "optimizer_reconcile_evaluation_evidence" => request(
            "POST",
            &format!(
                "/v1/optimizers/runs/{}/reconcile_evidence",
                id()?
            ),
            Some(json!({})),
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
        "optimizer_list_checkpoints" => request(
            "GET",
            "/v1/optimizers/checkpoints",
            Some(json!({
                "search": args.get("search"),
                "placement": args.get("placement"),
                "scope": args.get("scope"),
                "provider": args.get("provider"),
                "checkpointKind": args.get("checkpoint_kind"),
                "baseModel": args.get("base_model"),
                "runId": args.get("run_id"),
                "optimizerAlgorithm": args.get("optimizer_algorithm"),
                "status": args.get("status"),
                "limit": args.get("limit"),
                "offset": args.get("offset")
            })),
        ),
        "optimizer_archive_checkpoint" => request(
            "POST",
            "/v1/optimizers/checkpoints/archive",
            Some(json!({ "checkpointId": args.get("checkpoint_id") })),
        ),
        "optimizer_import_checkpoint" => request(
            "POST",
            "/v1/optimizers/checkpoints/import",
            Some(json!({ "path": args.get("path") })),
        ),
        "optimizer_infer_checkpoint" => request(
            "POST",
            "/v1/optimizers/checkpoints/infer",
            Some(json!({
                "checkpointId": args.get("checkpoint_id"),
                "family": args.get("family"),
                "body": args.get("body")
            })),
        ),
        "optimizer_update_checkpoint" => request(
            "POST",
            "/v1/optimizers/checkpoints/update",
            Some(json!({
                "checkpointId": args.get("checkpoint_id"),
                "name": args.get("name"),
                "description": args.get("description"),
                "tags": args.get("tags")
            })),
        ),
        "optimizer_publish_checkpoint" => request(
            "POST",
            "/v1/optimizers/checkpoints/publish",
            Some(json!({ "checkpointId": args.get("checkpoint_id") })),
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
        assert!(!encoded.contains("gepa.banking77."));
        assert!(!encoded.contains("eval.banking77.baseline.v1"));
        assert!(!encoded.contains("eval.healthbench.smoke.v1"));
        assert!(encoded.contains("recipe_id"));
        assert!(!encoded.contains("sft.hosted.fixture.v1"));
        assert!(encoded.contains("container_id"));
        assert!(encoded.contains("dataset_shard"));
        assert!(encoded.contains("plan_override"));
        assert!(encoded.contains("optimizer_pause_run"));
        assert!(encoded.contains("optimizer_resume_run"));
        assert!(encoded.contains("optimizer_start_workflow"));
        assert!(encoded.contains("start_workflow"));
        assert!(encoded.contains("optimizer_list_checkpoints"));
        assert!(encoded.contains("optimizer_infer_checkpoint"));
        assert!(encoded.contains("optimizer_update_checkpoint"));
        assert!(encoded.contains("optimizer_publish_checkpoint"));
        assert!(encoded.contains("chat_completions"));
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
}
