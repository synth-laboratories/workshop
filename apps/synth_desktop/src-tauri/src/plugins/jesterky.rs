//! Optional, on-demand Jesterky runtime. The catalog, never MCP arguments,
//! supplies download locations and digests. Analysis evidence outlives removal.
use super::types::*;
use super::{PluginRegistry, PluginStatus};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const VERSION: &str = "0.1.4";
pub const ID: &str = "jesterky";
/// Saved independently of the removable runtime; changing scope never starts work.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationScope {
    #[default]
    SelectedRollouts,
    SelectedEvidence,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalysisSettings {
    pub annotation_scope: AnnotationScope,
}
fn settings_at(path: &Path, update: Option<AnalysisSettings>) -> Result<AnalysisSettings> {
    if let Some(settings) = update {
        fs::create_dir_all(path.parent().context("settings directory missing")?)?;
        let staged = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            fs::write(&staged, serde_json::to_vec_pretty(&settings)?)?;
            fs::rename(&staged, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(staged);
        }
        result?;
        return Ok(settings);
    }
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(AnalysisSettings::default())
        }
        Err(error) => Err(error.into()),
    }
}
pub fn analysis_settings(update: Option<AnalysisSettings>) -> Result<AnalysisSettings> {
    settings_at(
        &crate::storage::app_data_root().join("plugins/jesterky-analysis.json"),
        update,
    )
}
const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;
static INSTALL_PHASE: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);
struct PhaseGuard;
impl Drop for PhaseGuard {
    fn drop(&mut self) {
        if let Ok(mut phase) = INSTALL_PHASE.lock() {
            *phase = None;
        }
    }
}
fn phase(value: &'static str) {
    if let Ok(mut phase) = INSTALL_PHASE.lock() {
        *phase = Some(value);
    }
}
const TEMPLATES: &[&str] = &["trace.catalog.v1", "trace.rollout_inspector.v1"];
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub version: String,
    pub target: String,
    pub sha256: String,
    pub size: u64,
    pub url: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Release {
    schema_version: String,
    artifacts: Vec<Artifact>,
}
fn target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}
fn root() -> PathBuf {
    crate::storage::app_data_root().join("plugins/jesterky-runtime")
}
fn dev_root() -> Option<PathBuf> {
    dirs::home_dir().map(|p| {
        p.join(format!(
            ".synth-desktop/dev-builds/jesterky/{VERSION}/current"
        ))
    })
}
fn artifact(channel: &str) -> Result<Artifact> {
    if channel == DEV_RELEASE_CHANNEL {
        let dir = dev_root().context("home directory unavailable")?;
        let a: Artifact = serde_json::from_slice(
            &fs::read(dir.join("artifact.json"))
                .context("register a Jesterky development build first")?,
        )?;
        validate(&a)?;
        return Ok(a);
    }
    let r: Release = serde_json::from_str(include_str!("../../resources/jesterky-release.json"))?;
    if r.schema_version != "synth.jesterky-release.v1" {
        bail!("unsupported Jesterky release manifest");
    }
    let a = r
        .artifacts
        .into_iter()
        .find(|a| a.version == VERSION && a.target == target())
        .context("No verified Jesterky release is published for this platform yet")?;
    validate(&a)?;
    Ok(a)
}
fn validate(a: &Artifact) -> Result<()> {
    if a.version != VERSION || a.target != target() {
        bail!("Jesterky artifact version/platform mismatch");
    }
    if a.sha256.len() != 64
        || !a.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || a.size == 0
        || a.size > MAX_BINARY_BYTES
    {
        bail!("invalid Jesterky artifact digest/size");
    }
    Ok(())
}
fn verify(bytes: &[u8], a: &Artifact) -> Result<()> {
    validate(a)?;
    if bytes.len() as u64 != a.size || format!("{:x}", Sha256::digest(bytes)) != a.sha256 {
        bail!("Jesterky artifact digest/size mismatch");
    }
    Ok(())
}
fn installed() -> Result<Artifact> {
    let a: Artifact = serde_json::from_slice(&fs::read(root().join("artifact.json"))?)?;
    validate(&a)?;
    if fs::metadata(root().join("jesterky"))?.len() != a.size {
        bail!("Jesterky runtime is incomplete; reinstall it");
    }
    verify(&fs::read(root().join("jesterky"))?, &a)?;
    Ok(a)
}
pub fn available() -> bool {
    PluginRegistry::for_plugin(ID).is_enabled() && installed().is_ok()
}
pub fn executable() -> Result<PathBuf> {
    if !PluginRegistry::for_plugin(ID).is_enabled() {
        return Err(PluginNotReady::for_plugin(ID, "disabled", "enable").into());
    }
    installed().map_err(|_| PluginNotReady::for_plugin(ID, "not_installed", "install"))?;
    Ok(root().join("jesterky"))
}
pub fn status() -> PluginStatus {
    let registry = PluginRegistry::for_plugin(ID);
    let a = installed().ok();
    let channel = registry.release_channel();
    let transient = INSTALL_PHASE.lock().ok().and_then(|phase| *phase);
    let phase = if let Some(transient) = transient {
        transient
    } else if !registry.is_enabled() {
        "disabled"
    } else if a.is_some() {
        "ready"
    } else {
        "not_installed"
    };
    registry.apply_to_status(PluginStatus {
        schema_version: PLUGIN_STATUS_SCHEMA.into(),
        plugin_id: ID.into(),
        enabled: registry.is_enabled(),
        phase: phase.into(),
        installed_version: a.as_ref().map(|a| a.version.clone()),
        selected_version: Some(VERSION.into()),
        release_channel: channel.clone(),
        catalog_version: VERSION.into(),
        digest: a.as_ref().map(|a| digest_ref(&a.sha256)),
        service: PluginServiceStatus {
            phase: phase.into(),
            started_at: None,
            active_runs: 0,
        },
        capabilities_digest: None,
        algorithms: vec![],
        templates: if a.is_some() {
            TEMPLATES.iter().map(|s| s.to_string()).collect()
        } else {
            vec![]
        },
        permissions: vec![],
        last_action_receipt_id: None,
        detail: if a.is_none() {
            artifact(&channel).err().map(|e| e.to_string())
        } else {
            Some("Runs on demand. Default analysis model: gpt-5.6-luna; effort: low.".into())
        },
    })
}
pub fn capabilities() -> Result<Value> {
    let executable = executable()?;
    Ok(
        json!({"schemaVersion":"synth.jesterky-capabilities.v1","pluginId":ID,"version":VERSION,
        "executable":executable,"defaultModel":"gpt-5.6-luna","defaultEffort":"low",
        "skills":["use-synth-jesterky"],"tools":["plugin_manage","jesterky_prepare","jesterky_settings","annotation_manage","trace_manage"],
        "compatibleTemplateIds":TEMPLATES,"execution":"on_demand","analysisSettings":analysis_settings(None)?,"requiresAnnotatedEvalJob":false,"containerRunner":"Requires Jesterky in the target container; host installation does not modify remote containers"}),
    )
}
pub fn catalog(version: Option<&str>, channel: &str) -> Result<CatalogEntry> {
    if version.is_some_and(|v| v != VERSION) {
        bail!("unknown Jesterky catalog version");
    }
    let a = artifact(channel).ok();
    Ok(CatalogEntry {
        plugin_id: ID.into(),
        release_channel: channel.into(),
        version: VERSION.into(),
        publisher: PLUGIN_PUBLISHER.into(),
        package: "jesterky-cli".into(),
        network_host: if channel == DEV_RELEASE_CHANNEL {
            "local development build".into()
        } else {
            "github.com".into()
        },
        download_size_bytes: a.map(|a| a.size).unwrap_or(0),
        workshop_compat: ">=0.7.0".into(),
        payload: CatalogPayload::Jesterky {
            model: "gpt-5.6-luna".into(),
            effort: "low".into(),
            skills: vec!["use-synth-jesterky".into()],
            templates: TEMPLATES.iter().map(|s| s.to_string()).collect(),
        },
    })
}
pub async fn execute(action: &str) -> Result<()> {
    static MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = MUTATION.lock().await;
    let registry = PluginRegistry::for_plugin(ID);
    match action {
        "enable" | "start" | "restart" => {
            if action != "enable" {
                installed()?;
            }
            registry.set_enabled(true)?;
        }
        "disable" | "stop" => {
            registry.set_enabled(false)?;
        }
        "remove" => {
            if root().exists() {
                fs::remove_dir_all(root())?;
            }
        }
        "install" | "update" => {
            let _phase_guard = PhaseGuard;
            phase("downloading");
            let channel = registry.release_channel();
            let a = artifact(&channel)?;
            let bytes = if channel == DEV_RELEASE_CHANNEL {
                let path = dev_root()
                    .context("home directory unavailable")?
                    .join("jesterky");
                if fs::metadata(&path)?.len() > MAX_BINARY_BYTES {
                    bail!("Jesterky artifact too large");
                }
                fs::read(path)?
            } else {
                let url = reqwest::Url::parse(&a.url)?;
                if url.scheme() != "https"
                    || url.host_str() != Some("github.com")
                    || !url
                        .path()
                        .starts_with("/synth-laboratories/jesterky/releases/download/")
                {
                    bail!("untrusted Jesterky download URL");
                }
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(120))
                    .build()?;
                let mut response = client.get(url).send().await?.error_for_status()?;
                let mut bytes = vec![];
                while let Some(chunk) = response.chunk().await? {
                    if bytes.len() + chunk.len() > a.size as usize {
                        bail!("Jesterky download exceeds pinned size");
                    }
                    bytes.extend_from_slice(&chunk);
                }
                bytes
            };
            phase("verifying");
            verify(&bytes, &a)?;
            materialize(&root(), &a, &bytes)?;
        }
        other => bail!("unknown Jesterky action {other}"),
    }
    Ok(())
}
fn materialize(destination: &Path, a: &Artifact, bytes: &[u8]) -> Result<()> {
    verify(bytes, a)?;
    let parent = destination.parent().context("runtime parent missing")?;
    fs::create_dir_all(parent)?;
    let stage = parent.join(format!("jesterky-stage-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&stage)?;
    let result = (|| -> Result<()> {
        fs::write(stage.join("jesterky"), bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(stage.join("jesterky"), fs::Permissions::from_mode(0o755))?;
        }
        fs::write(stage.join("artifact.json"), serde_json::to_vec_pretty(a)?)?;
        let backup = parent.join(format!("jesterky-backup-{}", uuid::Uuid::new_v4()));
        if destination.exists() {
            fs::rename(destination, &backup)?;
        }
        if let Err(e) = fs::rename(&stage, destination) {
            if backup.exists() {
                let _ = fs::rename(&backup, destination);
            }
            return Err(e.into());
        }
        if backup.exists() {
            fs::remove_dir_all(backup)?;
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(stage);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bad_download_does_not_replace_runtime() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("runtime");
        let bytes = b"test binary";
        let a = Artifact {
            version: VERSION.into(),
            target: target(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            size: bytes.len() as u64,
            url: String::new(),
        };
        materialize(&dest, &a, bytes).unwrap();
        assert!(materialize(&dest, &a, b"corrupt").is_err());
        assert_eq!(fs::read(dest.join("jesterky")).unwrap(), bytes);
    }
    #[test]
    fn official_manifest_never_invents_a_download() {
        let r: Release =
            serde_json::from_str(include_str!("../../resources/jesterky-release.json")).unwrap();
        assert_eq!(r.schema_version, "synth.jesterky-release.v1");
        for a in r.artifacts {
            assert!(a
                .url
                .starts_with("https://github.com/synth-laboratories/jesterky/releases/download/"));
        }
    }
}

/// Free, reproducible materialization. Starting paid jobs remains the annotation
/// service's responsibility, including runner capability and reservation checks.
pub fn prepare(
    snapshot: &crate::trace_query::QuerySnapshot,
    ids: &[String],
    scope: Option<AnnotationScope>,
) -> Result<Value> {
    executable()?;
    let scope = scope.unwrap_or(analysis_settings(None)?.annotation_scope);
    prepare_selection(snapshot, ids, scope)
}
fn prepare_selection(
    snapshot: &crate::trace_query::QuerySnapshot,
    ids: &[String],
    scope: AnnotationScope,
) -> Result<Value> {
    let mut preparation = crate::trace_research::annotation_selection(snapshot, ids)?;
    preparation["schemaVersion"] = json!("synth.jesterky-preparation.v1");
    preparation["requiredRunner"] = json!("jesterky");
    preparation["annotationScope"] = json!(scope);
    preparation["requiresAnnotatedEvalJob"] = json!(false);
    preparation["analysisDefaults"] = json!({"model":"gpt-5.6-luna","effort":"low"});
    for target in preparation["targets"].as_array_mut().unwrap() {
        let selectors = target["selectors"].as_array().cloned().unwrap_or_default();
        if scope == AnnotationScope::SelectedEvidence
            && !selectors.is_empty()
            && selectors.len() == target["resultIds"].as_array().unwrap().len()
            && selectors
                .iter()
                .all(|s| s["kind"] == "event" && s["entity_id"].is_string())
        {
            let ids: std::collections::BTreeSet<String> = selectors
                .iter()
                .filter_map(|s| s["entity_id"].as_str().map(str::to_owned))
                .collect();
            target["analysisScope"] = json!("selected_events");
            target["metadata"] = json!({"jesterky_event_ids":ids});
        } else {
            if scope == AnnotationScope::SelectedEvidence {
                bail!("Selected evidence must contain event rows; select event results or use selected_rollouts");
            }
            target["analysisScope"] = json!("trace");
        }
    }

    // Explicit sealed refs allow post-hoc campaigns across source eval jobs.
    // A campaign is per container; it never inherits the active eval's plan.
    if scope == AnnotationScope::SelectedRollouts {
        let mut batches: std::collections::BTreeMap<String, Vec<Value>> = Default::default();
        for target in preparation["targets"].as_array().unwrap() {
            batches.entry(target["container_id"].as_str().unwrap().to_owned()).or_default()
                .push(json!({"kind":"trace_v5","id":target["trace_id"],"digest":target["trace_digest"]}));
        }
        preparation["campaignSelections"] = json!(batches.into_iter().map(|(container, traces)|
            json!({"container_id":container,"traces":traces,"estimate_only":true})).collect::<Vec<_>>());
    }
    Ok(preparation)
}

#[cfg(test)]
mod analysis_tests {
    use super::*;
    fn snapshot() -> crate::trace_query::QuerySnapshot {
        let row = |job: &str, trace: &str, event: &str| {
            json!({
                "jobId":job,"traceDigest":format!("sha256:{trace}"),"traceId":trace,
                "containerId":"container","traceAvailability":"available",
                "selector":{"trace_id":trace,"kind":"event","entity_id":event}
            })
        };
        crate::trace_query::QuerySnapshot {
            schema_version: "synth.trace-query-result.v2".into(),
            snapshot_id: "saved".into(),
            domain: "traces".into(),
            query_schema_version: crate::trace_research::SCHEMA.into(),
            query_ast: json!({"evalJobIds":["old-eval","other-eval"]}),
            result_ids: vec!["a".into(), "b".into(), "c".into()],
            result_count: 3,
            facets: json!({"rows":[row("old-eval","t1","e1"),row("old-eval","t1","e2"),row("other-eval","t2","e3")]}),
            result_digest: "original-result".into(),
            queried_at: "now".into(),
            truncated: false,
        }
    }
    #[test]
    fn independent_rollouts_create_explicit_deduplicated_campaign_selections() {
        let result = prepare_selection(
            &snapshot(),
            &["a".into(), "b".into(), "c".into()],
            AnnotationScope::SelectedRollouts,
        )
        .unwrap();
        assert_eq!(result["requiresAnnotatedEvalJob"], false);
        assert_eq!(result["startsCompute"], false);
        assert_eq!(result["resultDigest"], "original-result");
        assert_eq!(
            result["analysisDefaults"],
            json!({"model":"gpt-5.6-luna","effort":"low"})
        );
        assert_eq!(result["targets"].as_array().unwrap().len(), 2);
        assert_eq!(result["targets"][0]["analysisScope"], "trace");
        assert!(result["targets"][0].get("metadata").is_none());
        assert_eq!(
            result["campaignSelections"][0]["traces"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(result["campaignSelections"][0].get("run_id").is_none());
        assert_eq!(result["campaignSelections"][0]["estimate_only"], true);
    }
    #[test]
    fn event_scope_is_explicit_and_never_expands_to_a_whole_rollout() {
        let mut source = snapshot();
        let result = prepare_selection(
            &source,
            &["a".into(), "b".into()],
            AnnotationScope::SelectedEvidence,
        )
        .unwrap();
        assert_eq!(
            result["targets"][0]["metadata"]["jesterky_event_ids"],
            json!(["e1", "e2"])
        );
        assert!(result.get("campaignSelections").is_none());
        source.facets["rows"][0]["selector"] = Value::Null;
        assert!(prepare_selection(
            &source,
            &["a".into(), "b".into()],
            AnnotationScope::SelectedEvidence
        )
        .is_err());
        source.facets["rows"][0]["traceAvailability"] = json!("unavailable");
        assert!(
            prepare_selection(&source, &["a".into()], AnnotationScope::SelectedRollouts).is_err()
        );
    }
    #[test]
    fn scope_is_durable_without_installing_or_running_jesterky() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins/jesterky-analysis.json");
        assert_eq!(
            settings_at(&path, None).unwrap().annotation_scope,
            AnnotationScope::SelectedRollouts
        );
        let value = AnalysisSettings {
            annotation_scope: AnnotationScope::SelectedEvidence,
        };
        settings_at(&path, Some(value.clone())).unwrap();
        assert_eq!(settings_at(&path, None).unwrap(), value);
        assert!(!dir.path().join("plugins/jesterky-runtime").exists());
        assert!(serde_json::from_value::<AnalysisSettings>(
            json!({"annotationScope":"all_traces"})
        )
        .is_err());
    }
}
