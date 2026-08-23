//! Product-owned Craftax SFT recipe. Callers select the recipe but cannot
//! provide commands, paths, hyperparameters, seeds, or credential values.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, sync::watch, time::sleep};

use super::{
    ingest,
    models::{
        OptimizerCapabilities, OptimizerCreateRequest, OptimizerEventEnvelope,
        OptimizerExecutionBinding, OptimizerRecipeRunRequest, OptimizerResourceRef,
        OPTIMIZER_EVENT_SCHEMA_VERSION,
    },
    OptimizerService,
};

pub const CRAFTAX_SFT_SMOKE_RECIPE: &str = "sft.craftax.gpt-oss.smoke.v1";
const COLLECT_SEEDS: &str = "101,102,103,104";
const EVAL_SEEDS: &str = "501,502";
const TRAIN_STEPS: u64 = 4;
const BATCH_SIZE: u64 = 2;
const LORA_RANK: u64 = 8;
const MAX_TEACHER_ROLLOUTS: u64 = 4;
const MAX_EVAL_ROLLOUTS: u64 = 4;

pub fn recipe_catalog() -> Value {
    let availability = match prerequisites() {
        Ok(_) => "available",
        Err(_) => "unavailable",
    };
    json!({
        "id": CRAFTAX_SFT_SMOKE_RECIPE,
        "title": "Craftax GPT-OSS SFT smoke",
        "algorithmId": "sft",
        "task": "craftax",
        "availability": availability,
        "limits": limits(),
        "credentialInputs": [],
        "prerequisites": ["craftax-gamebench-rust façade (or SYNTH_CRAFTAX_GOLD_BIN + image PYTHONPATH)", "Trusted SFT bridge runtime", "GROQ_API_KEY", "TINKER_API_KEY"],
    })
}

fn limits() -> Value {
    json!({
        "collectSeeds": [101, 102, 103, 104],
        "evalSeeds": [501, 502],
        "maxTeacherRollouts": MAX_TEACHER_ROLLOUTS,
        "maxEvalRollouts": MAX_EVAL_ROLLOUTS,
        "maxTotalEnvironmentRollouts": MAX_TEACHER_ROLLOUTS + MAX_EVAL_ROLLOUTS,
        "trainSteps": TRAIN_STEPS,
        "batchSize": BATCH_SIZE,
        "loraRank": LORA_RANK,
        "baseModel": "openai/gpt-oss-20b",
        "teacherModel": "openai/gpt-oss-120b",
        "costCeilingUsd": null,
        "costNotice": "Provider charges apply; this recipe is bounded by rollouts and training steps, not dollars."
    })
}

pub async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let (script, python, craftax) = prerequisites()?;
    let groq = resolve_secret("GROQ_API_KEY")?;
    let tinker = resolve_secret("TINKER_API_KEY")?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("craftax_sft_smoke_{}", &suffix[..8]);
    let run_dir = script
        .parent()
        .ok_or_else(|| anyhow!("invalid SFT script path"))?
        .join("runs")
        .join(&run_id);
    fs::create_dir_all(&run_dir).context("create Craftax SFT run directory")?;

    let capabilities = OptimizerCapabilities {
        cancel: true,
        stream_events: true,
        state_slices: true,
        checkpoints: true,
        checkpoint_evaluations: true,
        ..OptimizerCapabilities::default()
    };
    let create = OptimizerCreateRequest {
        algorithm_id: "sft".into(),
        algorithm_version: Some("craftax-gpt-oss-tinker-v1".into()),
        objective: Some("Craftax GPT-OSS LoRA · bounded SFT smoke".into()),
        source: Some("local".into()),
        project_ref: Some("craftax@gpt-oss-tinker".into()),
        session_ref: request.session_ref,
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "local_process".into(),
            id: run_id.clone(),
            label: Some("Craftax GPT-OSS SFT smoke".into()),
            status: Some("starting".into()),
            metadata: json!({"recipeId": CRAFTAX_SFT_SMOKE_RECIPE}),
        }]),
        input_refs: Some(vec![
            OptimizerResourceRef {
                kind: "dataset".into(),
                id: "craftax-teacher-rollouts".into(),
                digest: None,
                role: Some("train".into()),
                title: Some("Craftax GPT-OSS-120B teacher traces".into()),
                metadata: limits(),
            },
            OptimizerResourceRef {
                kind: "recipe".into(),
                id: CRAFTAX_SFT_SMOKE_RECIPE.into(),
                digest: None,
                role: Some("configuration".into()),
                title: Some("Bounded Craftax GPT-OSS SFT smoke".into()),
                metadata: limits(),
            },
        ]),
        capabilities: Some(capabilities),
        summary: Some(json!({
            "recipeId": CRAFTAX_SFT_SMOKE_RECIPE, "task": "craftax", "limits": limits(),
            "runDirectory": run_dir, "baseModel": "openai/gpt-oss-20b", "backend": "Tinker LoRA"
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run_id.clone(), cancel_tx)
        .await;
    let worker = service.clone();
    tokio::spawn(async move {
        if let Err(error) = run_worker(
            worker.clone(),
            run_id.clone(),
            script,
            python,
            craftax,
            run_dir,
            groq,
            tinker,
            cancel_rx,
        )
        .await
        {
            let _ = append_terminal(&worker, &run_id, true, error.to_string()).await;
        }
        worker.unregister_local_recipe(&run_id).await;
    });
    Ok((run, event))
}

async fn run_worker(
    service: OptimizerService,
    run_id: String,
    script: PathBuf,
    python: PathBuf,
    craftax: PathBuf,
    run_dir: PathBuf,
    groq: String,
    tinker: String,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    append_status(&service, &run_id, "optimizer.run.started", "running").await?;
    let mut owned_craftax = if craftax_ready() {
        None
    } else {
        let stdout = fs::File::create(run_dir.join("craftax.stdout.log"))?;
        let stderr = fs::File::create(run_dir.join("craftax.stderr.log"))?;
        Some(
            Command::new(&python)
                .args(["-m", "craftax_gold", "--port", "8080", "--host", "127.0.0.1"])
                .env("GROQ_API_KEY", &groq)
                .env("SYNTH_CRAFTAX_GOLD_BIN", &craftax)
                .env("PYTHONPATH", craftax_image_pythonpath())
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr))
                .kill_on_drop(true)
                .spawn()
                .context("launch craftax-gamebench-rust façade")?,
        )
    };
    if owned_craftax.is_some() {
        let mut ready = false;
        for _ in 0..120 {
            if craftax_ready() {
                ready = true;
                break;
            }
            sleep(Duration::from_millis(250)).await;
        }
        if !ready {
            bail!("owned Craftax service did not become ready on 127.0.0.1:8080");
        }
    }
    let stdout_path = run_dir.join("workshop.stdout.log");
    let stderr_path = run_dir.join("workshop.stderr.log");
    let stdout = fs::File::create(&stdout_path)?;
    let stderr = fs::File::create(&stderr_path)?;
    let mut child = Command::new(&python)
        .arg(&script)
        .args(["--output-dir"])
        .arg(&run_dir)
        .args([
            "--collect-seeds",
            COLLECT_SEEDS,
            "--eval-seeds",
            EVAL_SEEDS,
            "--train-steps",
            &TRAIN_STEPS.to_string(),
            "--batch-size",
            &BATCH_SIZE.to_string(),
            "--rank",
            &LORA_RANK.to_string(),
            "--lr",
            "0.001",
        ])
        .env("GROQ_API_KEY", groq)
        .env("TINKER_API_KEY", tinker)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true)
        .spawn()
        .context("launch allowlisted Craftax SFT recipe")?;
    loop {
        ingest_stdout(&service, &run_id, &stdout_path).await?;
        tokio::select! {
            status = child.wait() => {
                let status = status.context("wait for Craftax SFT process")?;
                ingest_stdout(&service, &run_id, &stdout_path).await?;
                if !status.success() { bail!("Craftax SFT exited with {status}; see {}", stderr_path.display()); }
                append_artifacts(&service, &run_id, &run_dir).await?;
                if let Some(craftax) = owned_craftax.as_mut() { let _ = craftax.kill().await; }
                append_terminal(&service, &run_id, false, "recipe process completed".into()).await?;
                return Ok(());
            }
            changed = cancel.changed() => {
                if changed.is_ok() && *cancel.borrow() {
                    child.kill().await.context("cancel Craftax SFT process")?;
                    if let Some(craftax) = owned_craftax.as_mut() { let _ = craftax.kill().await; }
                    append_status(&service, &run_id, "optimizer.run.cancelled", "cancelled").await?;
                    return Ok(());
                }
            }
            _ = sleep(Duration::from_millis(750)) => {}
        }
    }
}

async fn ingest_stdout(service: &OptimizerService, run_id: &str, path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let existing = service
        .events_after(run_id.to_string(), 0, Some(2_000))
        .await?;
    let existing_ids = existing
        .iter()
        .filter_map(|e| e.event_id.as_deref())
        .collect::<std::collections::HashSet<_>>();
    let mut hosted = Vec::new();
    let mut legacy = Vec::new();
    for (index, line) in fs::read_to_string(path)?.lines().enumerate() {
        let Ok(raw) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if raw
            .get("schema_version")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("optimizer_event"))
        {
            hosted.push(raw);
            continue;
        }
        let event_id = format!("{run_id}:sft-stdout:{}", index + 1);
        if existing_ids.contains(event_id.as_str()) {
            continue;
        }
        let Some((event_type, item, delta, snapshot, usage)) = canonicalize(&raw) else {
            continue;
        };
        legacy.push((event_id, event_type, item, delta, snapshot, usage, raw));
    }
    if !hosted.is_empty() {
        let mut upstream = existing
            .iter()
            .filter_map(|event| {
                event
                    .raw
                    .get("sourceSequenceNumber")
                    .and_then(Value::as_u64)
            })
            .max()
            .unwrap_or(0);
        hosted.sort_by_key(|event| {
            event
                .get("sequence_number")
                .or_else(|| event.get("sequenceNumber"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        });
        let new_hosted = hosted
            .into_iter()
            .filter(|event| {
                event
                    .get("sequence_number")
                    .or_else(|| event.get("sequenceNumber"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > upstream
            })
            .collect::<Vec<_>>();
        if !new_hosted.is_empty() {
            let page = json!({
                "schema_version": "optimizer_event_page.v1",
                "run_id": run_id,
                "after_sequence": upstream,
                "next_sequence": new_hosted.last().and_then(|event| event.get("sequence_number").and_then(Value::as_u64)).unwrap_or(upstream),
                "terminal": false,
                "events": new_hosted,
            });
            ingest::ingest_event_page(service, run_id, "sft", &page, &mut upstream).await?;
        }
    }
    let mut sequence = service.get(run_id.to_string()).await?.cursor_seq;
    let mut events = Vec::new();
    for (event_id, event_type, item, delta, snapshot, usage, raw) in legacy {
        sequence += 1;
        events.push(OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some(event_id),
            event_type: event_type.into(),
            sequence_number: sequence,
            occurred_at: chrono::Utc::now().to_rfc3339(),
            optimizer_run_id: run_id.into(),
            algorithm_id: "sft".into(),
            level: Some("info".into()),
            item,
            delta,
            snapshot,
            usage_delta: usage,
            artifact_refs: vec![],
            error: None,
            raw,
        });
    }
    if !events.is_empty() {
        service.append_events(run_id.to_string(), events).await?;
    }
    Ok(())
}

fn canonicalize(
    raw: &Value,
) -> Option<(
    &'static str,
    Option<Value>,
    Map<String, Value>,
    Option<Map<String, Value>>,
    Option<Map<String, Value>>,
)> {
    let kind = raw.get("event")?.as_str()?;
    let map = |pairs: &[(&str, Value)]| {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), v.clone()))
            .collect::<Map<_, _>>()
    };
    match kind {
        "collected" => Some((
            "sft.teacher_rollout.completed",
            Some(json!({"kind":"example","id":format!("seed-{}", raw["seed"]),"raw":raw})),
            map(&[(
                "message",
                json!(format!(
                    "Collected teacher rollout for seed {}",
                    raw["seed"]
                )),
            )]),
            None,
            Some(map(&[("rollouts", json!(1))])),
        )),
        "dataset" => Some((
            "sft.dataset.validated",
            None,
            Map::new(),
            Some(map(&[
                (
                    "splits",
                    json!({"train":{"count":raw["n_rows"]},"heldout":{"count":2}}),
                ),
                ("rejected", json!(0)),
            ])),
            None,
        )),
        "train_step" => Some((
            "sft.step.metrics",
            None,
            map(&[
                ("step", raw["step"].clone()),
                ("learning_rate", json!(0.001)),
                (
                    "message",
                    json!(format!("Completed training step {}", raw["step"])),
                ),
            ]),
            None,
            None,
        )),
        "eval_seed" => Some((
            "sft.checkpoint_eval.completed",
            None,
            map(&[
                ("role", json!("heldout")),
                ("measurementOnly", json!(true)),
                (
                    "split",
                    json!(format!("{}-seed-{}", raw["label"], raw["seed"])),
                ),
                ("metric", json!("craftax_reward")),
                ("score", raw["reward"].clone()),
                ("label", raw["label"].clone()),
                ("seed", raw["seed"].clone()),
            ]),
            None,
            Some(map(&[("rollouts", json!(1))])),
        )),
        "summary" => Some((
            "sft.heldout_eval.completed",
            None,
            map(&[
                ("role", json!("heldout")),
                ("measurementOnly", json!(true)),
                ("metric", json!("reward_uplift")),
                ("score", raw["uplift"].clone()),
                ("baseMean", raw["base_mean"].clone()),
                ("sftMean", raw["sft_mean"].clone()),
                ("message", json!(format!("SFT uplift: {}", raw["uplift"]))),
            ]),
            None,
            None,
        )),
        "collect_failed" | "row_skip" | "tls_patch_skipped" => Some((
            "optimizer.recipe.warning",
            None,
            map(&[(
                "message",
                raw.get("error").cloned().unwrap_or_else(|| json!(kind)),
            )]),
            None,
            None,
        )),
        _ => None,
    }
}

async fn append_artifacts(service: &OptimizerService, run_id: &str, dir: &Path) -> Result<()> {
    let mut artifacts = Vec::new();
    for (kind, name, title) in [
        ("dataset", "train.jsonl", "Training dataset"),
        ("checkpoint", "train_result.json", "Tinker training receipt"),
        ("evaluation", "eval_summary.json", "Base vs SFT evaluation"),
        ("log", "workshop.stdout.log", "Process stdout"),
        ("log", "workshop.stderr.log", "Process stderr"),
        ("log", "craftax.stdout.log", "Craftax stdout"),
        ("log", "craftax.stderr.log", "Craftax stderr"),
    ] {
        let path = dir.join(name);
        if path.is_file() {
            artifacts.push(json!({"kind":kind,"id":path,"path":path,"title":title}));
        }
    }
    let run = service.get(run_id.to_string()).await?;
    let mut events = vec![OptimizerEventEnvelope {
        schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
        event_id: Some(format!("{run_id}:artifacts")),
        event_type: "optimizer.recipe.artifacts".into(),
        sequence_number: run.cursor_seq + 1,
        occurred_at: chrono::Utc::now().to_rfc3339(),
        optimizer_run_id: run_id.into(),
        algorithm_id: "sft".into(),
        level: Some("info".into()),
        item: None,
        delta: map_of(
            "message",
            json!(format!("Persisted {} SFT artifacts", artifacts.len())),
        ),
        snapshot: None,
        usage_delta: None,
        artifact_refs: artifacts,
        error: None,
        raw: json!({"source":"craftax_sft_recipe"}),
    }];
    let receipt = dir.join("train_result.json");
    if let Ok(value) = fs::read_to_string(&receipt)
        .and_then(|s| serde_json::from_str::<Value>(&s).map_err(std::io::Error::other))
    {
        let seq = run.cursor_seq + 2;
        events.push(OptimizerEventEnvelope { schema_version:OPTIMIZER_EVENT_SCHEMA_VERSION.into(), event_id:Some(format!("{run_id}:checkpoint")),
            event_type:"sft.checkpoint.created".into(), sequence_number:seq, occurred_at:chrono::Utc::now().to_rfc3339(), optimizer_run_id:run_id.into(), algorithm_id:"sft".into(), level:Some("info".into()),
            item:Some(json!({"kind":"checkpoint","id":value.get("sampler_path").cloned().unwrap_or(json!("adapter")),"step":TRAIN_STEPS,"status":"selected","raw":value})),
            delta:Map::new(), snapshot:None, usage_delta:None, artifact_refs:vec![json!({"kind":"checkpoint","id":receipt,"path":receipt,"title":"Tinker training receipt"})], error:None, raw:json!({}) });
    }
    service.append_events(run_id.to_string(), events).await?;
    Ok(())
}

fn map_of(key: &str, value: Value) -> Map<String, Value> {
    [(key.to_string(), value)].into_iter().collect()
}

async fn append_status(
    service: &OptimizerService,
    run_id: &str,
    event_type: &str,
    status: &str,
) -> Result<()> {
    let run = service.get(run_id.to_string()).await?;
    service
        .append_events(
            run_id.to_string(),
            vec![OptimizerEventEnvelope {
                schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
                event_id: Some(format!("{run_id}:host:{}", run.cursor_seq + 1)),
                event_type: event_type.into(),
                sequence_number: run.cursor_seq + 1,
                occurred_at: chrono::Utc::now().to_rfc3339(),
                optimizer_run_id: run_id.into(),
                algorithm_id: "sft".into(),
                level: None,
                item: None,
                delta: map_of("status", json!(status)),
                snapshot: None,
                usage_delta: None,
                artifact_refs: vec![],
                error: None,
                raw: json!({"source":"craftax_sft_recipe"}),
            }],
        )
        .await?;
    Ok(())
}

async fn append_terminal(
    service: &OptimizerService,
    run_id: &str,
    failed: bool,
    detail: String,
) -> Result<()> {
    let run = service.get(run_id.to_string()).await?;
    if matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
        return Ok(());
    }
    append_status(
        service,
        run_id,
        if failed {
            "optimizer.run.failed"
        } else {
            "optimizer.run.completed"
        },
        if failed { "failed" } else { "completed" },
    )
    .await?;
    if failed {
        let run = service.get(run_id.to_string()).await?;
        let stderr = run
            .summary
            .get("runDirectory")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .map(|p| p.join("workshop.stderr.log"));
        let tail = stderr
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .map(|s| {
                s.chars()
                    .rev()
                    .take(4000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            });
        let error = json!({"message":tail.as_deref().unwrap_or(&detail),"stderrTail":tail,"logPath":stderr});
        let run = service.get(run_id.to_string()).await?;
        service
            .append_events(
                run_id.to_string(),
                vec![OptimizerEventEnvelope {
                    schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
                    event_id: Some(format!("{run_id}:diagnostic")),
                    event_type: "optimizer.recipe.diagnostic".into(),
                    sequence_number: run.cursor_seq + 1,
                    occurred_at: chrono::Utc::now().to_rfc3339(),
                    optimizer_run_id: run_id.into(),
                    algorithm_id: "sft".into(),
                    level: Some("error".into()),
                    item: None,
                    delta: map_of("status", json!("failed")),
                    snapshot: None,
                    usage_delta: None,
                    artifact_refs: vec![],
                    error: Some(error),
                    raw: json!({}),
                }],
            )
            .await?;
    }
    Ok(())
}

fn prerequisites() -> Result<(PathBuf, PathBuf, PathBuf)> {
    Ok((resolve_script()?, resolve_python()?, resolve_craftax()?))
}
fn resolve_script() -> Result<PathBuf> {
    let path = std::env::var_os("SYNTH_CRAFTAX_SFT_SCRIPT")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("SYNTH_CRAFTAX_SFT_SCRIPT is not configured"))?;
    if !path.is_file() {
        bail!("Craftax SFT bridge script unavailable")
    };
    path.canonicalize()
        .context("canonicalize Craftax SFT script")
}
fn resolve_python() -> Result<PathBuf> {
    let path = std::env::var_os("SYNTH_CRAFTAX_SFT_PYTHON")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("SYNTH_CRAFTAX_SFT_PYTHON is not configured"))?;
    if !path.is_file() {
        bail!("Craftax SFT Python unavailable")
    };
    path.canonicalize()
        .context("canonicalize Craftax SFT Python")
}
fn resolve_craftax() -> Result<PathBuf> {
    let path = std::env::var_os("SYNTH_CRAFTAX_GOLD_BIN")
        .or_else(|| std::env::var_os("SYNTH_CRAFTAX_GOLD_PATH"))
        .map(PathBuf::from)
        .ok_or_else(|| {
            anyhow!("SYNTH_CRAFTAX_GOLD_BIN or SYNTH_CRAFTAX_GOLD_PATH is not configured")
        })?;
    if !path.is_file() {
        bail!("Craftax gold binary unavailable; build craftax_gold or set SYNTH_CRAFTAX_GOLD_BIN")
    }
    path.canonicalize()
        .context("canonicalize Craftax gold binary")
}

fn craftax_ready() -> bool {
    let Ok(address) = "127.0.0.1:8080".parse() else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok()
}

fn craftax_image_pythonpath() -> PathBuf {
    std::env::var_os("SYNTH_CRAFTAX_IMAGE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join("Documents/GitHub/evals/containers/images/craftax-gamebench-rust")
        })
}
fn resolve_secret(name: &str) -> Result<String> {
    if !matches!(name, "GROQ_API_KEY" | "TINKER_API_KEY") {
        bail!("non-allowlisted SFT secret")
    };
    if let Ok(v) = std::env::var(name) {
        if !v.trim().is_empty() {
            return Ok(v);
        }
    }
    let path = std::env::var_os("SYNTH_CRAFTAX_SFT_SECRET_ENV_FILE")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("SYNTH_CRAFTAX_SFT_SECRET_ENV_FILE is not configured"))?;
    let text = fs::read_to_string(path).unwrap_or_default();
    for line in text.lines() {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == name {
                let value = value.trim().trim_matches(['\"', '\'']);
                if !value.is_empty() {
                    return Ok(value.into());
                }
            }
        }
    }
    bail!("Craftax SFT requires {name}; configure it in the Desktop process or the staged SFT secret env file")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_is_bounded_and_named() {
        let c = recipe_catalog();
        assert_eq!(c["id"], CRAFTAX_SFT_SMOKE_RECIPE);
        assert_eq!(c["limits"]["trainSteps"], 4);
        assert_eq!(c["limits"]["maxTotalEnvironmentRollouts"], 8);
    }

    #[test]
    fn stdout_events_map_to_first_class_sft_contract() {
        let (kind, _, delta, _, usage) =
            canonicalize(&json!({"event":"eval_seed","label":"sft","seed":501,"reward":2.0}))
                .unwrap();
        assert_eq!(kind, "sft.checkpoint_eval.completed");
        assert_eq!(delta.get("measurementOnly"), Some(&json!(true)));
        assert_eq!(usage.unwrap().get("rollouts"), Some(&json!(1)));

        let (kind, _, delta, _, _) =
            canonicalize(&json!({"event":"summary","base_mean":0.5,"sft_mean":1.5,"uplift":1.0}))
                .unwrap();
        assert_eq!(kind, "sft.heldout_eval.completed");
        assert_eq!(delta.get("score"), Some(&json!(1.0)));
    }
}
