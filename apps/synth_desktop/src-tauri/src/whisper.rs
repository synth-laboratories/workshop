//! Local Whisper speech-to-text: catalog, download/select/clear via the same
//! a dedicated Python environment, and file-path transcription.
//!
//! Mirrors `laguna.rs`'s model lifecycle shape (list/select/clear/download)
//! but for a small hardcoded catalog of OpenAI Whisper checkpoints instead of
//! Laguna's single pinned model.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, State};

const SELECTED_FILE: &str = "selected";
const COMPLETE_FILE: &str = ".synth-download-complete";

struct WhisperWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Drop for WhisperWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const WHISPER_IDLE_UNLOAD_SECONDS: u64 = crate::limits::WHISPER_IDLE_UNLOAD.as_secs();

#[derive(Default)]
struct WhisperRuntime {
    worker: Option<WhisperWorker>,
    loaded_model: Option<String>,
    last_used_at: Option<u64>,
    generation: u64,
}

/// Injected Whisper worker authority (composition root). No process-global slot.
#[derive(Default)]
pub struct WhisperManager {
    runtime: Mutex<WhisperRuntime>,
}

impl WhisperManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn runtime_status(&self) -> Result<WhisperRuntimeStatus> {
        let now = now_millis();
        let runtime = self
            .runtime
            .lock()
            .map_err(|_| anyhow::anyhow!("Whisper runtime lock was poisoned"))?;
        let loaded = runtime.worker.is_some() && runtime.loaded_model.is_some();
        Ok(WhisperRuntimeStatus {
            phase: if loaded { "ready" } else { "unloaded" }.into(),
            loaded_model: runtime.loaded_model.clone(),
            idle_seconds: runtime
                .last_used_at
                .map(|last| now.saturating_sub(last) / 1_000),
            idle_unload_after_seconds: WHISPER_IDLE_UNLOAD_SECONDS,
            last_used_at: runtime.last_used_at,
            free_at: runtime
                .last_used_at
                .map(|last| last + WHISPER_IDLE_UNLOAD_SECONDS * 1_000),
            updated_at: now,
        })
    }

    fn emit_runtime(&self, app: &AppHandle, phase: &str, model: Option<&str>) {
        let mut payload = self.runtime_status().unwrap_or(WhisperRuntimeStatus {
            phase: phase.into(),
            loaded_model: model.map(str::to_owned),
            idle_seconds: None,
            idle_unload_after_seconds: WHISPER_IDLE_UNLOAD_SECONDS,
            last_used_at: None,
            free_at: None,
            updated_at: now_millis(),
        });
        payload.phase = phase.into();
        if payload.loaded_model.is_none() {
            payload.loaded_model = model.map(str::to_owned);
        }
        let _ = app.emit(crate::contract::events::EventChannel::WHISPER_RUNTIME, payload);
    }

    fn schedule_idle_unload(self: &Arc<Self>, generation: u64) {
        let manager = Arc::clone(self);
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(WHISPER_IDLE_UNLOAD_SECONDS));
            if let Ok(mut runtime) = manager.runtime.lock() {
                if runtime.generation == generation {
                    runtime.worker = None;
                    runtime.loaded_model = None;
                    runtime.last_used_at = None;
                }
            }
        });
    }

    fn warm_runtime(self: &Arc<Self>, python: &Path, model_dir: &Path, model_id: &str) -> Result<()> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| anyhow::anyhow!("Whisper runtime lock was poisoned"))?;
        if runtime.loaded_model.as_deref() != Some(model_id) {
            runtime.worker = None;
            runtime.loaded_model = None;
        }
        if runtime.worker.is_none() {
            runtime.worker = Some(spawn_whisper_worker(python)?);
            runtime
                .worker
                .as_mut()
                .expect("worker initialized")
                .warm(model_dir)?;
            runtime.loaded_model = Some(model_id.to_owned());
        }
        runtime.last_used_at = Some(now_millis());
        runtime.generation = runtime.generation.wrapping_add(1);
        let generation = runtime.generation;
        drop(runtime);
        self.schedule_idle_unload(generation);
        Ok(())
    }

    fn transcribe_with_worker(
        self: &Arc<Self>,
        python: &Path,
        audio_path: &str,
        model_dir: &Path,
    ) -> Result<WhisperTranscription> {
        self.warm_runtime(
            python,
            model_dir,
            model_dir
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("whisper"),
        )?;
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| anyhow::anyhow!("Whisper worker lock was poisoned"))?;
        let result = runtime
            .worker
            .as_mut()
            .expect("worker warmed")
            .request(audio_path, model_dir);
        if result.is_err() {
            runtime.worker = None;
            runtime.loaded_model = None;
        } else {
            runtime.last_used_at = Some(now_millis());
            runtime.generation = runtime.generation.wrapping_add(1);
            let generation = runtime.generation;
            drop(runtime);
            self.schedule_idle_unload(generation);
            return result;
        }
        result
    }

    fn transcribe(self: &Arc<Self>, audio_path: &str) -> Result<WhisperTranscription> {
        let audio = Path::new(audio_path);
        if !audio.is_file() {
            return Err(anyhow::anyhow!("Audio file not found: {audio_path}"));
        }
        let selected = read_selected().ok_or_else(|| {
            anyhow::anyhow!("No Whisper model is selected. Download and select a model first.")
        })?;
        let entry = catalog_entry(&selected)?;
        let dir = model_dir(entry.id);
        if !is_installed(&dir) {
            return Err(anyhow::anyhow!(
                "Selected Whisper model `{}` is not downloaded. Download it again.",
                entry.id
            ));
        }
        let python = python_bin();
        validate_python(&python).context(
            "Whisper transcription needs the same Python environment as model downloads. Download a Whisper model first.",
        )?;
        self.transcribe_with_worker(&python, audio_path, &dir)
    }

    /// Renderer-recorded audio arrives as base64 (no filesystem/path plugin is
    /// wired into the mic capture flow). Decodes it to a temp file, transcribes
    /// via the normal path-based flow, then cleans up regardless of outcome.
    fn transcribe_base64(
        self: &Arc<Self>,
        audio_base64: &str,
        mime_type: &str,
    ) -> Result<WhisperTranscription> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(audio_base64.trim())
            .context("decode base64 audio payload")?;
        let extension = extension_for_mime(mime_type);
        let temp_path = env::temp_dir().join(format!(
            "synth-whisper-{}.{extension}",
            uuid::Uuid::new_v4()
        ));
        fs::write(&temp_path, &bytes)
            .with_context(|| format!("write temporary audio file {}", temp_path.display()))?;
        let result = self.transcribe(&temp_path.to_string_lossy());
        let _ = fs::remove_file(&temp_path);
        result
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperRuntimeStatus {
    pub phase: String,
    pub loaded_model: Option<String>,
    pub idle_seconds: Option<u64>,
    pub idle_unload_after_seconds: u64,
    pub last_used_at: Option<u64>,
    pub free_at: Option<u64>,
    pub updated_at: u64,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug)]
struct WhisperCatalogEntry {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    recommended: bool,
    /// Approximate download size in decimal bytes, as commonly reported for
    /// these checkpoints. Not read from disk: the true size is `installedBytes`.
    download_bytes: u64,
    /// Hugging Face repo id holding the Transformers-format checkpoint.
    hf_repo: &'static str,
}

const CATALOG: &[WhisperCatalogEntry] = &[
    WhisperCatalogEntry {
        id: "tiny",
        title: "Whisper Tiny",
        description: "Fastest and smallest Whisper model. Good for quick drafts; lowest accuracy.",
        recommended: false,
        download_bytes: 74_000_000,
        hf_repo: "mlx-community/whisper-tiny-mlx",
    },
    WhisperCatalogEntry {
        id: "base",
        title: "Whisper Base",
        description: "Balanced speed and accuracy. Recommended default for dictation.",
        recommended: true,
        download_bytes: 141_000_000,
        hf_repo: "mlx-community/whisper-base-mlx",
    },
    WhisperCatalogEntry {
        id: "small",
        title: "Whisper Small",
        description: "Higher accuracy than Base at a moderate size and speed cost.",
        recommended: false,
        download_bytes: 465_000_000,
        hf_repo: "mlx-community/whisper-small-mlx",
    },
    WhisperCatalogEntry {
        id: "large-v3-turbo",
        title: "Whisper Large v3 Turbo",
        description: "Best accuracy and the largest download. Turbo trades a little accuracy for much faster decoding than Large v3.",
        recommended: false,
        download_bytes: 1_549_000_000,
        hf_repo: "mlx-community/whisper-large-v3-turbo",
    },
];

fn catalog_entry(id: &str) -> Result<&'static WhisperCatalogEntry> {
    CATALOG.iter().find(|entry| entry.id == id).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown Whisper model id `{id}`. Known ids: {}",
            CATALOG
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModelHit {
    pub id: String,
    pub title: String,
    pub description: String,
    pub recommended: bool,
    pub multilingual: bool,
    pub download_bytes: u64,
    pub installed_bytes: Option<u64>,
    pub path: Option<String>,
    pub selected: bool,
    pub models_root: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperTranscription {
    pub text: String,
}

/// Whisper owns this environment. In particular, it must never install into
/// Laguna's venv: the two runtimes have independent dependency lifecycles.
fn whisper_home() -> PathBuf {
    env::var_os("SYNTH_WHISPER_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".synth-desktop/whisper")
        })
}

fn python_bin() -> PathBuf {
    whisper_home().join(".venv/bin/python")
}

fn models_root() -> PathBuf {
    env::var_os("SYNTH_WHISPER_MODELS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".synth-desktop/models/whisper")
        })
}

fn model_dir(id: &str) -> PathBuf {
    models_root().join(id)
}

fn selected_path() -> PathBuf {
    models_root().join(SELECTED_FILE)
}

fn read_selected() -> Option<String> {
    fs::read_to_string(selected_path())
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn write_selected(id: &str) -> Result<()> {
    fs::create_dir_all(models_root())?;
    fs::write(selected_path(), format!("{id}\n"))?;
    Ok(())
}

fn clear_selected_if(id: &str) -> Result<()> {
    if read_selected().as_deref() == Some(id) {
        match fs::remove_file(selected_path()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if let Ok(metadata) = entry.metadata() {
                total += if metadata.is_dir() {
                    dir_size(&entry_path)
                } else {
                    metadata.len()
                };
            }
        }
    }
    total
}

fn is_installed(dir: &Path) -> bool {
    if dir.join(COMPLETE_FILE).is_file() {
        return true;
    }

    // Downloads created before COMPLETE_FILE was introduced are still valid.
    // huggingface_hub only moves these files out of its temporary cache after
    // each transfer finishes, so requiring the weights plus runtime metadata
    // distinguishes a complete legacy snapshot from an interrupted download.
    ["weights.safetensors", "config.json"].iter().all(|name| {
        fs::metadata(dir.join(name))
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false)
    })
}

fn hit_for(entry: &WhisperCatalogEntry, selected: Option<&str>) -> WhisperModelHit {
    let dir = model_dir(entry.id);
    let installed = is_installed(&dir);
    WhisperModelHit {
        id: entry.id.into(),
        title: entry.title.into(),
        description: entry.description.into(),
        recommended: entry.recommended,
        multilingual: true,
        download_bytes: entry.download_bytes,
        installed_bytes: installed.then(|| dir_size(&dir)),
        path: installed.then(|| dir.to_string_lossy().into_owned()),
        selected: selected == Some(entry.id),
        models_root: models_root().to_string_lossy().into_owned(),
    }
}

pub fn list_models() -> Vec<WhisperModelHit> {
    let selected = read_selected();
    CATALOG
        .iter()
        .map(|entry| hit_for(entry, selected.as_deref()))
        .collect()
}

fn validate_python(python: &Path) -> Result<()> {
    let status = Command::new(python)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| {
            format!(
                "Whisper's managed Python environment at `{}` could not be started. Download a Whisper model to provision it again.",
                python.display()
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Python interpreter `{}` exited unsuccessfully",
            python.display()
        ))
    }
}

fn bootstrap_python() -> PathBuf {
    env::var_os("SYNTH_WHISPER_BOOTSTRAP_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let laguna = env::var_os("SYNTH_LAGUNA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    dirs::home_dir()
                        .unwrap_or_default()
                        .join(".synth-desktop/laguna")
                })
                .join(".venv/bin/python");
            if laguna.is_file() {
                laguna
            } else if let Some(python) = env::var_os("SYNTH_PYTHON") {
                PathBuf::from(python)
            } else {
                PathBuf::from("/usr/bin/python3")
            }
        })
}

fn deps_available(python: &Path) -> bool {
    Command::new(python)
        .args(["-c", "import huggingface_hub, mlx_whisper"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Creates and provisions Whisper's isolated runtime using only absolute
/// executable paths. `python -m venv` supplies pip inside the new environment;
/// no shell PATH lookup, uv installation, or mutation of Laguna's venv occurs.
fn ensure_whisper_runtime() -> Result<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Err(anyhow::anyhow!(
            "Local Whisper currently requires macOS (Apple MLX)."
        ));
    }
    let python = python_bin();
    if deps_available(&python) {
        return Ok(python);
    }
    let bootstrap = bootstrap_python();
    validate_python(&bootstrap).with_context(|| {
        format!(
            "Whisper needs Python 3 to create its isolated runtime; `{}` is unavailable",
            bootstrap.display()
        )
    })?;
    fs::create_dir_all(whisper_home())?;
    let output = Command::new(&bootstrap)
        .args(["-m", "venv", "--clear"])
        .arg(whisper_home().join(".venv"))
        .stdin(Stdio::null())
        .output()
        .context("create the managed Whisper Python environment")?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to create Whisper's managed environment: {}",
            String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(500)
                .collect::<String>()
        ));
    }
    let output = Command::new(&python)
        .args([
            "-m",
            "pip",
            "install",
            "--quiet",
            "--disable-pip-version-check",
            "huggingface-hub>=0.26,<2",
            "mlx-whisper==0.4.3",
        ])
        .stdin(Stdio::null())
        .output()
        .context("install dependencies into the managed Whisper environment")?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to provision Whisper's managed environment: {}",
            String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(500)
                .collect::<String>()
        ));
    }
    if !deps_available(&python) {
        return Err(anyhow::anyhow!(
            "Whisper's managed environment was created, but its runtime could not be imported."
        ));
    }
    Ok(python)
}

fn download_model_with_progress<F>(id: &str, mut progress: F) -> Result<WhisperModelHit>
where
    F: FnMut(&str, u64, u64),
{
    let entry = catalog_entry(id)?;
    progress("preparing", 0, entry.download_bytes);
    let python = ensure_whisper_runtime()?;
    let dir = model_dir(entry.id);
    fs::create_dir_all(&dir)?;
    let script = r#"from huggingface_hub import snapshot_download
import sys
snapshot_download(repo_id=sys.argv[1], local_dir=sys.argv[2])
"#;
    progress("downloading", dir_size(&dir), entry.download_bytes);
    let mut child = Command::new(&python)
        .arg("-c")
        .arg(script)
        .arg(entry.hf_repo)
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("download {} from Hugging Face", entry.hf_repo))?;

    let status = loop {
        if let Some(status) = child.try_wait().context("check Whisper model download")? {
            break status;
        }
        progress("downloading", dir_size(&dir), entry.download_bytes);
        thread::sleep(Duration::from_millis(250));
    };
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Whisper model `{id}` download failed: {}",
            stderr.trim().chars().take(500).collect::<String>()
        ));
    }
    fs::write(dir.join(COMPLETE_FILE), format!("{}\n", entry.hf_repo))?;
    write_selected(entry.id)?;
    progress("ready", dir_size(&dir), entry.download_bytes);
    Ok(hit_for(entry, Some(entry.id)))
}

pub fn download_model(id: &str) -> Result<WhisperModelHit> {
    download_model_with_progress(id, |_, _, _| {})
}

pub fn set_selected(id: &str) -> Result<WhisperModelHit> {
    let entry = catalog_entry(id)?;
    let dir = model_dir(entry.id);
    if !is_installed(&dir) {
        return Err(anyhow::anyhow!(
            "Whisper model `{id}` is not downloaded yet"
        ));
    }
    write_selected(entry.id)?;
    Ok(hit_for(entry, Some(entry.id)))
}

pub fn clear_model(id: &str) -> Result<()> {
    let entry = catalog_entry(id)?;
    let dir = model_dir(entry.id);
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .with_context(|| format!("remove Whisper model directory {}", dir.display()))?;
    }
    clear_selected_if(entry.id)?;
    Ok(())
}

fn spawn_whisper_worker(python: &Path) -> Result<WhisperWorker> {
    let script = r#"
import json, sys, zlib
import mlx_whisper
import mlx.core as mx
from mlx_whisper.transcribe import ModelHolder

for line in sys.stdin:
    try:
        request = json.loads(line)
        if request.get("action") == "warm":
            # `mlx_whisper.transcribe` defaults to fp16. ModelHolder is keyed by
            # path only, so warming with a different dtype poisons the cache.
            ModelHolder.get_model(request["model_dir"], mx.float16)
            response = {"ready": True}
        else:
            result = mlx_whisper.transcribe(
                request["audio_path"],
                path_or_hf_repo=request["model_dir"],
                verbose=None,
                condition_on_previous_text=False,
                language="en",
            )
            text = result.get("text", "").strip()
            encoded = text.encode("utf-8")
            compression_ratio = len(encoded) / max(1, len(zlib.compress(encoded)))
            if compression_ratio > 2.4:
                response = {"error": "I couldn't understand that recording. Please try again."}
            else:
                response = {"text": text}
    except Exception as exc:
        response = {"error": f"mlx_whisper: {exc}"}
    print(json.dumps(response), flush=True)
"#;
    let mut child = Command::new(python)
        .args(["-u", "-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("start the persistent Whisper transcription worker")?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("Whisper worker stdin was unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .map(BufReader::new)
        .ok_or_else(|| anyhow::anyhow!("Whisper worker stdout was unavailable"))?;
    Ok(WhisperWorker {
        child,
        stdin,
        stdout,
    })
}

impl WhisperWorker {
    fn send_request(&mut self, request: serde_json::Value) -> Result<serde_json::Value> {
        if self.child.try_wait()?.is_some() {
            return Err(anyhow::anyhow!("Whisper worker exited unexpectedly"));
        }
        serde_json::to_writer(&mut self.stdin, &request)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;

        let mut line = String::new();
        if self.stdout.read_line(&mut line)? == 0 {
            return Err(anyhow::anyhow!("Whisper worker returned no response"));
        }
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).with_context(|| {
            format!(
                "Whisper worker returned an unreadable payload: {}",
                line.trim().chars().take(300).collect::<String>()
            )
        })?;
        if let Some(error) = parsed.get("error").and_then(|value| value.as_str()) {
            return Err(anyhow::anyhow!("Whisper runtime failed: {error}"));
        }
        Ok(parsed)
    }

    fn warm(&mut self, model_dir: &Path) -> Result<()> {
        self.send_request(serde_json::json!({"action":"warm", "model_dir":model_dir}))?;
        Ok(())
    }

    fn request(&mut self, audio_path: &str, model_dir: &Path) -> Result<WhisperTranscription> {
        let parsed = self.send_request(serde_json::json!({"action":"transcribe", "audio_path":audio_path, "model_dir":model_dir}))?;
        let text = parsed
            .get("text")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("Whisper transcription returned no text"))?
            .to_string();
        Ok(WhisperTranscription { text })
    }
}

fn selected_runtime() -> Result<(PathBuf, PathBuf, String)> {
    let selected = read_selected().ok_or_else(|| {
        anyhow::anyhow!("No Whisper model is selected. Download and select a model first.")
    })?;
    let entry = catalog_entry(&selected)?;
    let dir = model_dir(entry.id);
    if !is_installed(&dir) {
        return Err(anyhow::anyhow!(
            "Selected Whisper model `{}` is not downloaded. Download it again.",
            entry.id
        ));
    }
    let python = python_bin();
    validate_python(&python)
        .context("Whisper needs its managed runtime. Download a Whisper model first.")?;
    Ok((python, dir, selected))
}

/// Maps a `MediaRecorder` mime type to a file extension Whisper's loaders can
/// sniff correctly. Falls back to `webm`, the renderer's default recording
/// format, for anything unrecognized rather than guessing wrong silently.
fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type.split(';').next().unwrap_or("").trim() {
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/ogg" => "ogg",
        _ => "webm",
    }
}

#[tauri::command]
pub fn whisper_models_list() -> Vec<WhisperModelHit> {
    list_models()
}

#[tauri::command]
pub async fn whisper_model_download(
    app: AppHandle,
    id: String,
) -> std::result::Result<WhisperModelHit, String> {
    let download_id = id.clone();
    let progress_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        download_model_with_progress(&download_id, |phase, downloaded_bytes, total_bytes| {
            let _ = progress_app.emit(
                crate::contract::events::EventChannel::WHISPER_DOWNLOAD,
                serde_json::json!({
                    "phase": phase,
                    "id": download_id,
                    "downloadedBytes": downloaded_bytes,
                    "totalBytes": total_bytes,
                    "detail": if phase == "preparing" {
                        "Preparing the private Whisper runtime…"
                    } else if phase == "ready" {
                        "Whisper model download complete."
                    } else {
                        "Downloading model files…"
                    }
                }),
            );
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string());
    let payload = match &result {
        Ok(hit) => {
            serde_json::json!({"phase":"ready","id":id,"detail":"Whisper model download complete.","path":hit.path})
        }
        Err(error) => serde_json::json!({"phase":"error","id":id,"detail":error}),
    };
    let _ = app.emit(crate::contract::events::EventChannel::WHISPER_DOWNLOAD, payload);
    result
}

#[tauri::command]
pub fn whisper_models_set_selected(id: String) -> std::result::Result<WhisperModelHit, String> {
    set_selected(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn whisper_models_clear(id: String) -> std::result::Result<(), String> {
    clear_model(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn whisper_runtime_status(
    whisper: State<'_, Arc<WhisperManager>>,
) -> std::result::Result<WhisperRuntimeStatus, String> {
    whisper.runtime_status().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn whisper_runtime_warm(
    app: AppHandle,
    whisper: State<'_, Arc<WhisperManager>>,
) -> std::result::Result<WhisperRuntimeStatus, String> {
    let whisper = Arc::clone(whisper.inner());
    let (_, _, model) = selected_runtime().map_err(|error| error.to_string())?;
    whisper.emit_runtime(&app, "warming", Some(&model));
    let warm = Arc::clone(&whisper);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let (python, dir, model) = selected_runtime()?;
        warm.warm_runtime(&python, &dir, &model)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error: anyhow::Error| error.to_string());
    match result {
        Ok(()) => {
            whisper.emit_runtime(&app, "ready", Some(&model));
            whisper.runtime_status().map_err(|error| error.to_string())
        }
        Err(error) => {
            whisper.emit_runtime(&app, "error", Some(&model));
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn whisper_transcribe(
    app: AppHandle,
    whisper: State<'_, Arc<WhisperManager>>,
    audio_path: String,
) -> std::result::Result<WhisperTranscription, String> {
    let whisper = Arc::clone(whisper.inner());
    whisper.emit_runtime(&app, "transcribing", read_selected().as_deref());
    let worker = Arc::clone(&whisper);
    let result = tauri::async_runtime::spawn_blocking(move || worker.transcribe(&audio_path))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string());
    whisper.emit_runtime(
        &app,
        if result.is_ok() { "ready" } else { "error" },
        read_selected().as_deref(),
    );
    result
}

#[tauri::command]
pub async fn whisper_transcribe_base64(
    app: AppHandle,
    whisper: State<'_, Arc<WhisperManager>>,
    audio_base64: String,
    mime_type: String,
) -> std::result::Result<WhisperTranscription, String> {
    let whisper = Arc::clone(whisper.inner());
    whisper.emit_runtime(&app, "transcribing", read_selected().as_deref());
    let worker = Arc::clone(&whisper);
    let result = tauri::async_runtime::spawn_blocking(move || {
        worker.transcribe_base64(&audio_base64, &mime_type)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string());
    whisper.emit_runtime(
        &app,
        if result.is_ok() { "ready" } else { "error" },
        read_selected().as_deref(),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_the_four_specified_models() {
        let ids: Vec<&str> = CATALOG.iter().map(|entry| entry.id).collect();
        assert_eq!(ids, vec!["tiny", "base", "small", "large-v3-turbo"]);
        assert!(
            CATALOG
                .iter()
                .find(|entry| entry.id == "base")
                .unwrap()
                .recommended
        );
        assert_eq!(
            CATALOG.iter().filter(|entry| entry.recommended).count(),
            1,
            "exactly one recommended model"
        );
        assert!(CATALOG
            .iter()
            .all(|entry| entry.hf_repo.starts_with("mlx-community/")));
    }

    #[test]
    fn unknown_id_is_a_clear_error() {
        let error = catalog_entry("does-not-exist").unwrap_err().to_string();
        assert!(error.contains("Unknown Whisper model id"));
        assert!(error.contains("tiny"));
    }

    #[test]
    fn dir_size_sums_nested_files() {
        let root = env::temp_dir().join(format!("whisper-dirsize-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("a.bin"), vec![0u8; 10]).unwrap();
        fs::write(root.join("nested/b.bin"), vec![0u8; 20]).unwrap();
        assert_eq!(dir_size(&root), 30);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mime_types_map_to_expected_extensions() {
        assert_eq!(extension_for_mime("audio/webm;codecs=opus"), "webm");
        assert_eq!(extension_for_mime("audio/wav"), "wav");
        assert_eq!(extension_for_mime("audio/mp4"), "m4a");
        assert_eq!(extension_for_mime("audio/unknown-format"), "webm");
    }

    #[test]
    fn is_installed_requires_a_nonempty_directory() {
        let root = env::temp_dir().join(format!("whisper-installed-test-{}", uuid::Uuid::new_v4()));
        assert!(!is_installed(&root));
        fs::create_dir_all(&root).unwrap();
        assert!(!is_installed(&root));
        fs::write(root.join("config.json"), "{}").unwrap();
        assert!(!is_installed(&root));
        fs::write(root.join(COMPLETE_FILE), "test/model\n").unwrap();
        assert!(is_installed(&root));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recognizes_complete_legacy_snapshot_without_marker() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["weights.safetensors", "config.json"] {
            fs::write(temp.path().join(name), b"present").unwrap();
        }
        assert!(is_installed(temp.path()));
    }
}
