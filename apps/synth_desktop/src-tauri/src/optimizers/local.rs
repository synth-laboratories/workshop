//! Import local OSS optimizer workspaces (GEPA event feed / GELO events.jsonl).

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct LocalOptimizerImport {
    pub run_id: String,
    pub algorithm_id: String,
    pub objective: Option<String>,
    pub events: Vec<Value>,
    pub source_path: PathBuf,
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
    })
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
            "/Users/joshuapurtell/Documents/GitHub/optimizers-beta/.out/sft_craftax_smokes/sft_craftax_ws_verify1",
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
            "/Users/joshuapurtell/Documents/GitHub/optimizers-beta/.out/craftax_gelo_cli_runs/craftax_gelo_cli6",
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
            "/Users/joshuapurtell/Documents/GitHub/optimizers-beta/.out/gepa_craftax_smokes/gepa_craftax_ws_verify1",
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
            "/Users/joshuapurtell/Documents/GitHub/optimizers-beta/.out/local_service_runs/goex_76e77cabf1f94abca8c305f7430e2934/.out/local_service_runs/goex_76e77cabf1f94abca8c305f7430e2934/runs/goex_76e77cabf1f94abca8c305f7430e2934",
        );
        if !path.join("artifacts/events.optimizer.jsonl").is_file() {
            return;
        }
        let imported = import_local_path(&path).unwrap();
        assert_eq!(imported.algorithm_id, "go-ex");
        assert!(!imported.events.is_empty());
    }
}
