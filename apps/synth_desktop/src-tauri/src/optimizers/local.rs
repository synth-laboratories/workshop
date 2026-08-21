//! Import local OSS optimizer workspaces (GEPA event feed / GELO events.jsonl).

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use super::models::TrainingJobStatus;

#[derive(Clone, Debug)]
pub struct LocalOptimizerImport {
    pub run_id: String,
    pub algorithm_id: String,
    pub objective: Option<String>,
    pub events: Vec<Value>,
    pub source_path: PathBuf,
    pub execution_bindings: Vec<super::models::OptimizerExecutionBinding>,
    pub input_refs: Vec<super::models::OptimizerResourceRef>,
    pub output_refs: Vec<super::models::OptimizerResourceRef>,
    pub summary: Value,
}

pub fn import_local_path(path: impl AsRef<Path>) -> Result<LocalOptimizerImport> {
    let path = path
        .as_ref()
        .canonicalize()
        .unwrap_or_else(|_| path.as_ref().to_path_buf());
    if path.is_file() {
        return import_events_file(&path, None, None);
    }
    if !path.is_dir() {
        bail!("local optimizer path does not exist: {}", path.display());
    }

    // The local MLX service has its own durable job protocol. Do not pass its
    // raw events through the generic importer: that would guess GEPA and turn
    // a scalar-compute smoke into an optimizer/model-training claim.
    if path.join("job.json").is_file() && path.join("events.jsonl").is_file() {
        return import_mlx_job(&path);
    }

    // Prefer canonical optimizer_event.v1 sidecar when present (GELO, SFT, …).
    let goex_canonical = path.join("artifacts/events.optimizer.jsonl");
    if goex_canonical.is_file() {
        let run_id = path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("optimizer_local")
            .to_string();
        let algorithm = sniff_algorithm(&goex_canonical).unwrap_or_else(|| "go-ex".into());
        return import_events_file(&goex_canonical, Some(run_id), Some(algorithm));
    }

    // Hosted/local GELO layout: runs/<id>/artifacts/events.jsonl
    let goex = path.join("artifacts/events.jsonl");
    if goex.is_file() {
        let run_id = path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("goex_local")
            .to_string();
        let algorithm = sniff_algorithm(&goex).unwrap_or_else(|| "go-ex".into());
        return import_events_file(&goex, Some(run_id), Some(algorithm));
    }

    // Nested workspace: runs/<id>/...
    let runs_dir = path.join("runs");
    if runs_dir.is_dir() {
        let mut candidates = Vec::new();
        for entry in fs::read_dir(&runs_dir)? {
            let entry = entry?;
            let canonical = entry.path().join("artifacts/events.optimizer.jsonl");
            let events = entry.path().join("artifacts/events.jsonl");
            if canonical.is_file() {
                candidates.push(canonical);
            } else if events.is_file() {
                candidates.push(events);
            }
        }
        candidates.sort();
        if let Some(latest) = candidates.pop() {
            let run_id = latest
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .and_then(|v| v.to_str())
                .unwrap_or("goex_local")
                .to_string();
            let algorithm = sniff_algorithm(&latest).unwrap_or_else(|| "go-ex".into());
            return import_events_file(&latest, Some(run_id), Some(algorithm));
        }
    }

    // Local OSS GEPA event feed variants (prefer optimizer sidecar)
    for rel in [
        "events.optimizer.jsonl",
        "artifacts/events.optimizer.jsonl",
        "artifacts/event_feed.jsonl",
        "artifacts/events.jsonl",
        "events.jsonl",
        "event_feed.jsonl",
        "logs/events.jsonl",
    ] {
        let candidate = path.join(rel);
        if candidate.is_file() {
            let run_id = path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("gepa_local")
                .to_string();
            let sniffed = sniff_algorithm(&candidate);
            let algorithm = if rel.contains("event_feed") || rel.contains("optimizer") {
                sniffed.unwrap_or_else(|| {
                    if rel.contains("go") {
                        "go-ex".into()
                    } else {
                        "gepa".into()
                    }
                })
            } else {
                sniffed.unwrap_or_else(|| "gepa".into())
            };
            return import_events_file(&candidate, Some(run_id), Some(algorithm));
        }
    }

    Err(anyhow!(
        "no local optimizer event feed found under {}",
        path.display()
    ))
}

fn import_events_file(
    path: &Path,
    run_id: Option<String>,
    algorithm_id: Option<String>,
) -> Result<LocalOptimizerImport> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read local optimizer events {}", path.display()))?;
    let mut events = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut value: Value =
            serde_json::from_str(line).with_context(|| format!("parse line {}", index + 1))?;
        if let Some(obj) = value.as_object_mut() {
            if !obj.contains_key("_seq")
                && !obj.contains_key("sequence_number")
                && !obj.contains_key("seq")
            {
                obj.insert("_seq".into(), serde_json::json!(index as u64 + 1));
            }
        }
        events.push(value);
    }
    if events.is_empty() {
        bail!("local optimizer event feed is empty: {}", path.display());
    }
    let algorithm_id = algorithm_id
        .or_else(|| sniff_algorithm(path))
        .unwrap_or_else(|| "gepa".into());
    let run_id = run_id.or_else(|| sniff_run_id(&events)).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("local_optimizer")
            .to_string()
    });
    Ok(LocalOptimizerImport {
        run_id,
        algorithm_id,
        objective: Some(format!("imported from {}", path.display())),
        events,
        source_path: path.to_path_buf(),
        execution_bindings: vec![],
        input_refs: vec![],
        output_refs: vec![],
        summary: json!({}),
    })
}

/// Import only the documented synth-mlx-rl durable job shape. The current
/// MLX service intentionally supports a scalar compute smoke backend, not a
/// Qwen/LoRA fine-tune; all of those boundaries are carried into Workshop.
fn import_mlx_job(path: &Path) -> Result<LocalOptimizerImport> {
    let job_path = path.join("job.json");
    let job: Value = serde_json::from_slice(&fs::read(&job_path)?)
        .with_context(|| format!("parse MLX job {}", job_path.display()))?;
    let config = job
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("MLX job omitted config"))?;
    let backend = config
        .get("backend")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("MLX job omitted config.backend"))?;
    if !matches!(backend, "fixture" | "mlx_scalar_smoke") {
        bail!("unsupported local MLX backend `{backend}`")
    }
    let run_id = job
        .get("job_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("MLX job omitted job_id"))?
        .to_string();
    let dataset = config
        .get("dataset")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("MLX job omitted config.dataset"))?;
    let dataset_path = dataset
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("MLX job omitted config.dataset.path"))?;
    let dataset_digest = job
        .get("dataset_sha256")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| format!("sha256:{value}"));
    let config_digest = job
        .get("config_sha256")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| format!("sha256:{value}"));
    let status = job
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let events = mlx_events(&path.join("events.jsonl"), &run_id, backend, status)?;
    let checkpoints = job
        .get("checkpoints")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let output_refs = checkpoints
        .iter()
        .filter_map(|checkpoint| {
            let object = checkpoint.as_object()?;
            let id = object.get("checkpoint_id")?.as_str()?.to_string();
            let path = object.get("path")?.as_str()?.to_string();
            let digest = object.get("sha256")?.as_str()?.to_string();
            Some(super::models::OptimizerResourceRef {
                kind: "local_mlx_checkpoint".into(),
                id,
                digest: Some(format!("sha256:{digest}")),
                role: Some("non_deployable_smoke_checkpoint".into()),
                title: Some("Local MLX scalar smoke checkpoint (non-deployable)".into()),
                metadata: json!({"path": path, "bytes": object.get("bytes")}),
            })
        })
        .collect();
    Ok(LocalOptimizerImport {
        run_id: run_id.clone(),
        algorithm_id: "local-mlx-smoke".into(),
        objective: Some("Local MLX infrastructure smoke — not model fine-tuning".into()),
        events,
        source_path: path.to_path_buf(),
        execution_bindings: vec![super::models::OptimizerExecutionBinding {
            kind: "synth_mlx_rl".into(),
            id: path.display().to_string(),
            label: Some("synth-mlx-rl durable local job".into()),
            status: Some(status.into()),
            metadata: json!({"backend": backend, "jobId": run_id}),
        }],
        input_refs: vec![
            super::models::OptimizerResourceRef {
                kind: "dataset".into(),
                id: dataset_path.into(),
                digest: dataset_digest,
                role: Some("train_input".into()),
                title: Some("Local MLX smoke dataset".into()),
                metadata: json!({}),
            },
            super::models::OptimizerResourceRef {
                kind: "local_mlx_job".into(),
                id: job_path.display().to_string(),
                digest: config_digest,
                role: Some("configuration".into()),
                title: Some("synth-mlx-rl immutable job manifest".into()),
                metadata: json!({"backend": backend}),
            },
        ],
        output_refs,
        summary: json!({
            "localMlx": {
                "schemaVersion": "synth_mlx_rl.job.v1",
                "backend": backend,
                "serviceStatus": status,
                "requestedBaseModel": config.get("base_model").cloned().unwrap_or(Value::Null),
                "modelTrainingClaim": false,
                "checkpointDeployable": false,
                "evaluationStatus": "not_run",
                "automaticResumeSupported": false,
                "reason": "The v0.6 local service records MLX scalar compute only; it does not train Qwen or LoRA/QLoRA."
            }
        }),
    })
}

fn mlx_events(path: &Path, run_id: &str, backend: &str, status: &str) -> Result<Vec<Value>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("read MLX events {}", path.display()))?;
    let mut events = Vec::new();
    let mut expected = 1_u64;
    for (index, line) in text.lines().enumerate() {
        let raw: Value = serde_json::from_str(line)
            .with_context(|| format!("parse MLX event line {}", index + 1))?;
        let sequence = raw
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("MLX event line {} omitted sequence", index + 1))?;
        if sequence != expected {
            bail!(
                "MLX event sequence gap at line {}: expected {expected}, got {sequence}",
                index + 1
            );
        }
        expected += 1;
        let timestamp = raw
            .get("timestamp")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("MLX event line {} omitted timestamp", index + 1))?;
        let source_type = raw
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("MLX event line {} omitted type", index + 1))?;
        let payload = raw.get("payload").cloned().unwrap_or_else(|| json!({}));
        let (event_type, event_status, error) = match source_type {
            "job.configured" => ("optimizer.run.created", Some("configured"), None),
            "job.queued" => ("optimizer.run.queued", Some("queued"), None),
            "job.started" => ("optimizer.run.started", Some("running"), None),
            "job.cancellation_requested" => {
                ("local_mlx.cancellation_requested", Some("cancelling"), None)
            }
            "training.metric" => ("local_mlx.training.metric", None, None),
            "checkpoint.created" => ("local_mlx.checkpoint.created", None, None),
            "job.succeeded" => ("optimizer.run.completed", Some("completed"), None),
            "job.cancelled" => ("optimizer.run.cancelled", Some("cancelled"), None),
            "job.failed" | "job.interrupted" => (
                "optimizer.run.failed",
                Some("failed"),
                Some(json!({"sourceStatus": source_type, "detail": payload})),
            ),
            other => (
                "local_mlx.event",
                None,
                Some(json!({"unknownSourceType": other})),
            ),
        };
        let mut delta = payload.as_object().cloned().unwrap_or_else(Map::new);
        delta.insert("backend".into(), json!(backend));
        if let Some(value) = event_status {
            delta.insert("status".into(), json!(value));
        }
        let artifact_refs = if source_type == "checkpoint.created" {
            vec![json!({
                "kind": "local_mlx_checkpoint",
                "id": payload.get("checkpoint_id").cloned().unwrap_or(Value::Null),
                "digest": payload.get("sha256").map(|value| format!("sha256:{}", value.as_str().unwrap_or_default())),
                "role": "non_deployable_smoke_checkpoint",
                "metadata": {"path": payload.get("path"), "bytes": payload.get("bytes")}
            })]
        } else {
            vec![]
        };
        events.push(json!({
            "schema_version": "optimizer_event.v1",
            "event_id": format!("{run_id}:{sequence}"),
            "type": event_type,
            "sequence_number": sequence,
            "created_at": timestamp,
            "run_id": run_id,
            "algorithm_id": "local-mlx-smoke",
            "delta": Value::Object(delta),
            "artifact_refs": artifact_refs,
            "error": error,
            "raw": raw
        }));
    }
    if events.is_empty() {
        bail!("MLX event feed is empty: {}", path.display());
    }
    // A terminal job must have emitted its matching durable terminal event.
    if TrainingJobStatus::parse(status).is_some_and(TrainingJobStatus::is_terminal) {
        let terminal = events
            .last()
            .and_then(|event| event.get("type"))
            .and_then(Value::as_str);
        if !matches!(
            terminal,
            Some("optimizer.run.completed" | "optimizer.run.cancelled" | "optimizer.run.failed")
        ) {
            bail!("terminal MLX job `{status}` has no matching terminal event");
        }
    }
    Ok(events)
}

fn sniff_algorithm(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let sample = text.lines().take(20).collect::<Vec<_>>().join("\n");
    // Prefer explicit algorithm_id from optimizer_event.v1 / goex_event.v1 payloads.
    if let Some(id) = sniff_algorithm_id_from_sample(&sample) {
        return Some(id);
    }
    if sample.contains("\"sft.")
        || sample.contains("sft.dataset")
        || sample.contains("sft.training")
    {
        return Some("sft".into());
    }
    if sample.contains("event_stream_record.v1") || sample.contains("gepa.") {
        return Some("gepa".into());
    }
    if sample.contains("goex") || sample.contains("go-ex") || sample.contains("theme") {
        return Some("go-ex".into());
    }
    None
}

fn sniff_algorithm_id_from_sample(sample: &str) -> Option<String> {
    for line in sample.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(id) = value
            .get("algorithm_id")
            .or_else(|| value.get("algorithm"))
            .and_then(Value::as_str)
        {
            let normalized = id.trim().to_ascii_lowercase().replace('_', "-");
            match normalized.as_str() {
                "sft" => return Some("sft".into()),
                "go-ex" | "goex" | "go-explore" => return Some("go-ex".into()),
                "gepa" => return Some("gepa".into()),
                other if !other.is_empty() => return Some(other.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn sniff_run_id(events: &[Value]) -> Option<String> {
    for event in events {
        if let Some(id) = event
            .get("optimizer_run_id")
            .or_else(|| event.get("run_id"))
            .or_else(|| event.pointer("/fields/run_id"))
            .or_else(|| event.pointer("/payload/run_id"))
            .and_then(Value::as_str)
        {
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn sniff_and_import_sft_optimizer_sidecar() {
        let dir = tempdir().unwrap();
        let run_dir = dir.path().join("sft_craftax_ws_verify1");
        let artifacts = run_dir.join("artifacts");
        fs::create_dir_all(&artifacts).unwrap();
        let feed = artifacts.join("events.optimizer.jsonl");
        let mut file = fs::File::create(&feed).unwrap();
        writeln!(
            file,
            r#"{{"schema_version":"optimizer_event.v1","type":"optimizer.run.created","sequence_number":1,"created_at":"2026-08-09T20:00:00Z","run_id":"sft_craftax_ws_verify1","optimizer_run_id":"sft_craftax_ws_verify1","algorithm_id":"sft","delta":{{}},"raw":{{"task":"craftax"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"schema_version":"optimizer_event.v1","type":"sft.checkpoint.promoted","sequence_number":2,"created_at":"2026-08-09T20:00:01Z","run_id":"sft_craftax_ws_verify1","optimizer_run_id":"sft_craftax_ws_verify1","algorithm_id":"sft","delta":{{}},"raw":{{"task":"craftax"}}}}"#
        )
        .unwrap();
        let imported = import_local_path(&run_dir).unwrap();
        assert_eq!(imported.algorithm_id, "sft");
        assert_eq!(imported.run_id, "sft_craftax_ws_verify1");
        assert_eq!(imported.events.len(), 2);
    }

    #[test]
    fn imports_real_craftax_sft_smoke_when_present() {
        let path = PathBuf::from(
            "/Users/joshuapurtell/Documents/GitHub/optimizers-beta-sft/.out/sft_craftax_smokes/sft_craftax_ws_verify1",
        );
        if !path.join("artifacts/events.optimizer.jsonl").is_file() {
            return;
        }
        let imported = import_local_path(&path).unwrap();
        assert_eq!(imported.algorithm_id, "sft");
        assert!(imported.events.len() >= 8);
        assert!(imported.events.iter().any(|event| {
            event.get("type").and_then(Value::as_str) == Some("optimizer.run.completed")
        }));
    }

    #[test]
    fn imports_real_craftax_gelo_completed_when_present() {
        let path = PathBuf::from(
            "/Users/joshuapurtell/Documents/GitHub/optimizers-beta-sft/.out/craftax_gelo_cli_runs/craftax_gelo_cli6",
        );
        if !path.join("artifacts/events.optimizer.jsonl").is_file() {
            return;
        }
        let imported = import_local_path(&path).unwrap();
        assert_eq!(imported.algorithm_id, "go-ex");
        assert!(imported.events.len() >= 10);
        assert_eq!(imported.run_id, "craftax_gelo_cli6");
    }

    #[test]
    fn imports_real_craftax_gepa_smoke_when_present() {
        let path = PathBuf::from(
            "/Users/joshuapurtell/Documents/GitHub/optimizers-g1/.out/gepa_craftax_smokes/gepa_craftax_ws_verify1",
        );
        if !path.join("artifacts/events.optimizer.jsonl").is_file() {
            return;
        }
        let imported = import_local_path(&path).unwrap();
        assert_eq!(imported.algorithm_id, "gepa");
        assert!(imported.events.len() >= 10);
        assert!(imported.events.iter().any(|event| {
            event.get("type").and_then(Value::as_str) == Some("gepa.run.finished")
                || event.get("type").and_then(Value::as_str) == Some("optimizer.run.completed")
        }));
    }

    #[test]
    fn imports_real_craftax_gelo_partial_when_present() {
        let path = PathBuf::from(
            "/Users/joshuapurtell/Documents/GitHub/optimizers-beta-sft/.out/local_service_runs/goex_76e77cabf1f94abca8c305f7430e2934/.out/local_service_runs/goex_76e77cabf1f94abca8c305f7430e2934/runs/goex_76e77cabf1f94abca8c305f7430e2934",
        );
        if !path.join("artifacts/events.optimizer.jsonl").is_file() {
            return;
        }
        let imported = import_local_path(&path).unwrap();
        assert_eq!(imported.algorithm_id, "go-ex");
        assert!(!imported.events.is_empty());
    }

    #[test]
    fn imports_mlx_scalar_smoke_without_claiming_model_training() {
        let dir = tempdir().unwrap();
        let job_dir = dir.path().join("mlx-smoke-1");
        fs::create_dir_all(&job_dir).unwrap();
        fs::write(
            job_dir.join("job.json"),
            r#"{
              "job_id":"mlx-smoke-1",
              "status":"succeeded",
              "config_sha256":"configdigest",
              "dataset_sha256":"datasetdigest",
              "config":{"backend":"mlx_scalar_smoke","base_model":"Qwen/Qwen3.5-0.8B","dataset":{"path":"/tmp/dataset.jsonl"}},
              "checkpoints":[{"checkpoint_id":"mlx-smoke-1:step-4","path":"/tmp/checkpoint.json","sha256":"checkpointdigest","bytes":44}]
            }"#,
        )
        .unwrap();
        fs::write(
            job_dir.join("events.jsonl"),
            r#"{"sequence":1,"type":"job.configured","timestamp":"2026-08-18T00:00:00Z","payload":{}}
{"sequence":2,"type":"job.started","timestamp":"2026-08-18T00:00:01Z","payload":{"backend":"mlx_scalar_smoke"}}
{"sequence":3,"type":"training.metric","timestamp":"2026-08-18T00:00:02Z","payload":{"step":1,"loss":0.5}}
{"sequence":4,"type":"checkpoint.created","timestamp":"2026-08-18T00:00:03Z","payload":{"checkpoint_id":"mlx-smoke-1:step-4","path":"/tmp/checkpoint.json","sha256":"checkpointdigest","bytes":44}}
{"sequence":5,"type":"job.succeeded","timestamp":"2026-08-18T00:00:04Z","payload":{"terminal_checkpoint":"mlx-smoke-1:step-4"}}
"#,
        )
        .unwrap();
        let imported = import_local_path(&job_dir).unwrap();
        assert_eq!(imported.algorithm_id, "local-mlx-smoke");
        assert_eq!(imported.run_id, "mlx-smoke-1");
        assert_eq!(
            imported.events.last().unwrap()["type"],
            "optimizer.run.completed"
        );
        assert_eq!(imported.output_refs.len(), 1);
        assert_eq!(
            imported.output_refs[0].role.as_deref(),
            Some("non_deployable_smoke_checkpoint")
        );
        assert_eq!(imported.summary["localMlx"]["modelTrainingClaim"], false);
        assert_eq!(imported.summary["localMlx"]["checkpointDeployable"], false);
    }

    #[test]
    fn rejects_terminal_mlx_job_without_terminal_event() {
        let dir = tempdir().unwrap();
        let job_dir = dir.path().join("mlx-smoke-2");
        fs::create_dir_all(&job_dir).unwrap();
        fs::write(
            job_dir.join("job.json"),
            r#"{"job_id":"mlx-smoke-2","status":"succeeded","config":{"backend":"fixture","dataset":{"path":"/tmp/dataset.jsonl"}},"checkpoints":[]}"#,
        )
        .unwrap();
        fs::write(
            job_dir.join("events.jsonl"),
            r#"{"sequence":1,"type":"job.started","timestamp":"2026-08-18T00:00:00Z","payload":{}}"#,
        )
        .unwrap();
        assert!(import_local_path(&job_dir)
            .unwrap_err()
            .to_string()
            .contains("no matching terminal event"));
    }

    #[test]
    fn imports_real_mlx_service_smoke_when_present() {
        let path = PathBuf::from(
            "/Users/joshuapurtell/Documents/Codex/2026-08-18/re/outputs/mlx-v06-runtime/final/service/m5-mlx-smoke-002",
        );
        if !path.join("job.json").is_file() {
            return;
        }
        let imported = import_local_path(&path).unwrap();
        assert_eq!(imported.run_id, "m5-mlx-smoke-002");
        assert_eq!(imported.algorithm_id, "local-mlx-smoke");
        assert_eq!(imported.events.len(), 10);
        assert_eq!(
            imported
                .output_refs
                .last()
                .and_then(|item| item.digest.as_deref()),
            Some("sha256:1f9141f4a7870d3e8fd2dd176c13d3cb8d3dc754623e6bf545d0f3bd02492298")
        );
        assert_eq!(imported.summary["localMlx"]["evaluationStatus"], "not_run");
    }
}
