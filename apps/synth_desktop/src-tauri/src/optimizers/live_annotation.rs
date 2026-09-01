//! Live incremental annotation protocols for container evals (lane C).
//!
//! A workspace recipe may declare `[live_annotation]`: a digest-pinned,
//! stdlib-only protocol file the container runs *beside* every rollout in an
//! isolated process. The protocol tails the rollout's own event stream as it
//! grows and publishes provisional findings (achievements, milestones, failure
//! modes, bounded model judgments) on a sibling stream the container declares
//! in the rollout descriptor. Workshop relays that stream into the run journal
//! as `eval.trial.annotation` events so the live viewer can show the summary
//! layer over the underlying rollout events, and so the evidence survives the
//! container.
//!
//! The protocol is installed the way policy code is: `GET /annotation-protocol`
//! to read the installed identity, `PUT /annotation-protocol` only on mismatch,
//! read back, and refuse to run without a `protocol_revision_id`. Every
//! rollout then pins that revision. Findings are observe-only and provisional:
//! they never change reward, achievements, terminal status, or the sealed
//! trace, and the post-hoc `[annotation]` stage remains the evidence authority.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::OptimizerService;
use crate::error::StructuredFailure;

/// Container-advertised capability flags this lane depends on.
pub(crate) const ANNOTATION_LIVE_CAPABILITY: &str = "annotation.live";
pub(crate) const ANNOTATION_PROTOCOL_PUT_CAPABILITY: &str = "annotation.protocol.put";
pub(crate) const PROTOCOL_STATE_SCHEMA: &str = "synth.container-annotation-protocol.v1";
pub(crate) const PROTOCOL_CODE_SCHEMA: &str = "synth.live-annotation-protocol.v1";

const SPEC_KEYS: &[&str] = &[
    "enabled",
    "protocol_id",
    "protocol_source",
    "configuration",
    "model",
];
const MODEL_KEYS: &[&str] = &[
    "model",
    "base_url",
    "api_key_env",
    "max_calls",
    "max_output_tokens",
    "timeout_seconds",
];
const MAX_PROTOCOL_SOURCE_BYTES: usize = 512 * 1024;

fn failure(
    code: &'static str,
    message: impl Into<String>,
    remediation: impl Into<String>,
) -> anyhow::Error {
    anyhow::Error::new(StructuredFailure::new(code, message, remediation))
}

// ---------------------------------------------------------------------------
// Recipe spec
// ---------------------------------------------------------------------------

/// The `[live_annotation]` table of a workspace recipe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct LiveAnnotationSpec {
    /// Identity the protocol file declares as `PROTOCOL_ID`; the container
    /// refuses an install whose declared id disagrees.
    pub protocol_id: String,
    /// Workspace-relative path of the protocol source. Read at run start, like
    /// `policy_source`, so a recipe can be listed before the file exists.
    pub protocol_source: String,
    /// Protocol configuration handed to the child's `Protocol(config)`.
    pub configuration: Map<String, Value>,
    /// Optional judge settings the container uses for `model_request`
    /// emissions. Never a credential: the container reads its own key.
    pub model: Option<Map<String, Value>>,
}

impl LiveAnnotationSpec {
    /// Parse the `[live_annotation]` table. Bounds are checked here, where the
    /// table is read, so a bad recipe is refused at load time.
    pub(crate) fn parse(recipe_id: &str, table: &toml::value::Table) -> Result<Option<Self>> {
        for key in table.keys() {
            if !SPEC_KEYS.contains(&key.as_str()) {
                bail!("recipe `{recipe_id}` live_annotation.{key} is not an admitted option");
            }
        }
        match table.get("enabled") {
            None | Some(toml::Value::Boolean(true)) => {}
            Some(toml::Value::Boolean(false)) => return Ok(None),
            Some(_) => bail!("recipe `{recipe_id}` live_annotation.enabled must be a boolean"),
        }
        let protocol_id = table
            .get("protocol_id")
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("recipe `{recipe_id}` live_annotation.protocol_id is required"))?
            .to_string();
        let protocol_source = table
            .get("protocol_source")
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                anyhow!("recipe `{recipe_id}` live_annotation.protocol_source is required")
            })?
            .to_string();
        let configuration = match table.get("configuration") {
            None => Map::new(),
            Some(toml::Value::Table(inner)) => json_object(inner)
                .with_context(|| format!("recipe `{recipe_id}` live_annotation.configuration"))?,
            Some(_) => bail!("recipe `{recipe_id}` live_annotation.configuration must be a table"),
        };
        if contains_secret_key(&Value::Object(configuration.clone())) {
            bail!("recipe `{recipe_id}` live_annotation.configuration must not carry credentials");
        }
        let model = match table.get("model") {
            None => None,
            Some(toml::Value::Table(inner)) => {
                for key in inner.keys() {
                    if !MODEL_KEYS.contains(&key.as_str()) {
                        bail!("recipe `{recipe_id}` live_annotation.model.{key} is not an admitted option");
                    }
                }
                let model = json_object(inner)
                    .with_context(|| format!("recipe `{recipe_id}` live_annotation.model"))?;
                if !model
                    .get("model")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
                {
                    bail!("recipe `{recipe_id}` live_annotation.model.model is required when a model table is declared");
                }
                if contains_secret_key(&Value::Object(model.clone())) {
                    bail!("recipe `{recipe_id}` live_annotation.model must not carry credentials");
                }
                Some(model)
            }
            Some(_) => bail!("recipe `{recipe_id}` live_annotation.model must be a table"),
        };
        Ok(Some(Self {
            protocol_id,
            protocol_source,
            configuration,
            model,
        }))
    }

    /// The configuration object the container stores: protocol options plus
    /// the judge block under `model`, which the container's runner reads.
    pub(crate) fn container_configuration(&self) -> Map<String, Value> {
        let mut merged = self.configuration.clone();
        if let Some(model) = self.model.as_ref() {
            merged.insert("model".into(), Value::Object(model.clone()));
        }
        merged
    }

    pub(crate) fn summary_json(&self) -> Value {
        json!({
            "protocolId": self.protocol_id,
            "protocolSource": self.protocol_source,
            "model": self.model.as_ref().and_then(|model| model.get("model").cloned()),
            "mode": "observe_only",
            "findings": "provisional",
        })
    }
}

fn json_object(table: &toml::value::Table) -> Result<Map<String, Value>> {
    serde_json::to_value(table)
        .context("encode table")?
        .as_object()
        .cloned()
        .context("table did not encode as an object")
}

/// Same refusal shape as `PUT /policy` and `PUT /annotation-protocol`: identity
/// and configuration travel, credentials never do.
pub(crate) fn contains_secret_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, child)| {
            let normalized = key.replace(['_', '-'], "").to_ascii_lowercase();
            normalized == "credential"
                || normalized.ends_with("apikey")
                || normalized.ends_with("secret")
                || normalized.ends_with("token")
                || normalized.ends_with("password")
                || contains_secret_key(child)
        }),
        Value::Array(items) => items.iter().any(contains_secret_key),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Resolved source
// ---------------------------------------------------------------------------

/// Protocol bytes read from the workspace, plus the identity the container
/// will report back after an install. Everything needed to decide "already
/// installed" without re-sending source.
#[derive(Clone, Debug)]
pub(crate) struct LiveAnnotationSource {
    pub spec: LiveAnnotationSpec,
    pub code: String,
    /// `sha256:<hex>` of the exact source bytes; sent as `source_revision`.
    pub source_revision: String,
    /// Canonical-JSON digest of [`LiveAnnotationSpec::container_configuration`],
    /// in the container's own `configuration_digest` form.
    pub configuration_digest: String,
}

impl LiveAnnotationSource {
    pub(crate) fn resolve(spec: &LiveAnnotationSpec, workspace: &Path) -> Result<Self> {
        let path = super::workspace_recipe::resolve_workspace_path(workspace, &spec.protocol_source)
            .map_err(|error| {
                failure(
                    "live_annotation_source_unavailable",
                    format!(
                        "live annotation protocol source `{}` could not be resolved: {error:#}",
                        spec.protocol_source
                    ),
                    "point live_annotation.protocol_source at a protocol file inside the workspace",
                )
            })?;
        let code = std::fs::read_to_string(&path)
            .with_context(|| format!("read live annotation protocol {}", path.display()))?;
        Self::from_code(spec.clone(), code)
    }

    pub(crate) fn from_code(spec: LiveAnnotationSpec, code: String) -> Result<Self> {
        if code.trim().is_empty() {
            bail!("live annotation protocol `{}` is empty", spec.protocol_source);
        }
        if code.len() > MAX_PROTOCOL_SOURCE_BYTES {
            bail!(
                "live annotation protocol `{}` exceeds {MAX_PROTOCOL_SOURCE_BYTES} bytes",
                spec.protocol_source
            );
        }
        if !code.contains(PROTOCOL_CODE_SCHEMA) {
            bail!(
                "live annotation protocol `{}` does not declare PROTOCOL = {PROTOCOL_CODE_SCHEMA:?}",
                spec.protocol_source
            );
        }
        let source_revision = format!("sha256:{:x}", Sha256::digest(code.as_bytes()));
        let configuration_digest =
            super::admission::CanonicalJson::new(Value::Object(spec.container_configuration()))?
                .digest()
                .as_str()
                .to_string();
        Ok(Self {
            spec,
            code,
            source_revision,
            configuration_digest,
        })
    }

    /// Body for `PUT /annotation-protocol`.
    pub(crate) fn install_body(&self) -> Value {
        json!({
            "code": self.code,
            "protocol_id": self.spec.protocol_id,
            "configuration": self.spec.container_configuration(),
            "source_revision": self.source_revision,
        })
    }
}

// ---------------------------------------------------------------------------
// Container protocol state
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct ContainerProtocolState {
    schema_version: String,
    status: String,
    protocol_id: Option<String>,
    protocol_revision_id: Option<String>,
    source_revision: Option<String>,
    configuration_digest: Option<String>,
    credential_state: String,
}

pub(crate) async fn read_protocol_state(
    client: &reqwest::Client,
    base: &str,
) -> Result<ContainerProtocolState> {
    let response = client
        .get(format!("{base}/annotation-protocol"))
        .send()
        .await
        .context("GET /annotation-protocol")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        bail!("annotation protocol inspection failed: {status} {text}");
    }
    let state = response
        .json::<ContainerProtocolState>()
        .await
        .context("decode synth.container-annotation-protocol.v1")?;
    if state.schema_version != PROTOCOL_STATE_SCHEMA {
        bail!(
            "unsupported annotation protocol schema `{}`; expected {PROTOCOL_STATE_SCHEMA}",
            state.schema_version
        );
    }
    if state.credential_state != "not_exposed" {
        bail!("container annotation-protocol endpoint violated credential non-disclosure contract");
    }
    Ok(state)
}

pub(crate) fn installed_matches(state: &ContainerProtocolState, source: &LiveAnnotationSource) -> bool {
    state.status == "installed"
        && state
            .protocol_revision_id
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        && state.protocol_id.as_deref() == Some(source.spec.protocol_id.as_str())
        && state.source_revision.as_deref() == Some(source.source_revision.as_str())
        && state.configuration_digest.as_deref() == Some(source.configuration_digest.as_str())
}

/// The container must advertise the lane before any rollout binds a protocol.
/// `Unknown` fails closed: a recipe that asked for live annotation must not
/// silently run without it.
pub(crate) fn require_advertised(info: &Value) -> Result<()> {
    let advertised = |name: &str| {
        info.pointer(&format!("/capabilities/operations/{name}"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    if advertised(ANNOTATION_LIVE_CAPABILITY) && advertised(ANNOTATION_PROTOCOL_PUT_CAPABILITY) {
        return Ok(());
    }
    Err(failure(
        "live_annotation_unsupported",
        format!(
            "this container does not advertise `{ANNOTATION_LIVE_CAPABILITY}` and `{ANNOTATION_PROTOCOL_PUT_CAPABILITY}`"
        ),
        "rebuild the container on a synth-containers release with the live annotation lane, or remove [live_annotation] from the recipe",
    ))
}

/// Read, install on mismatch, read back, and pin. Mirrors the NanoHorizon
/// policy pin: the run refuses to start without a container-issued revision.
pub(crate) async fn register_protocol_pin(
    client: &reqwest::Client,
    base: &str,
    source: &LiveAnnotationSource,
) -> Result<Value> {
    let mut state = read_protocol_state(client, base).await?;
    let mut installed_now = false;
    if !installed_matches(&state, source) {
        let response = client
            .put(format!("{base}/annotation-protocol"))
            .json(&source.install_body())
            .send()
            .await
            .context("PUT /annotation-protocol")?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(failure(
                "live_annotation_install_refused",
                format!("annotation protocol install failed: {status} {text}"),
                "the container refused the protocol source; fix the protocol file (it must boot in an isolated stdlib-only process and declare the recipe's protocol_id)",
            ));
        }
        installed_now = true;
        state = read_protocol_state(client, base).await?;
    }
    if !installed_matches(&state, source) {
        bail!("live_annotation_installation_mismatch: container did not report the requested protocol pin");
    }
    let revision = state
        .protocol_revision_id
        .context("installed annotation protocol omitted protocol_revision_id")?;
    Ok(json!({
        "protocolId": source.spec.protocol_id,
        "protocolRevisionId": revision,
        "sourceRevision": source.source_revision,
        "configurationDigest": source.configuration_digest,
        "protocolSource": source.spec.protocol_source,
        "model": source.spec.model.as_ref().and_then(|model| model.get("model").cloned()),
        "installedByThisRun": installed_now,
        "immutable": true,
        "mode": "observe_only",
        "authority": PROTOCOL_STATE_SCHEMA,
    }))
}

/// Pin JSON from an installed container state, for a mid-run update that has
/// no workspace spec behind it (the caller supplied the source directly).
pub(crate) fn pin_from_state(state: &ContainerProtocolState, protocol_source: Option<&str>) -> Result<Value> {
    let revision = state
        .protocol_revision_id
        .clone()
        .context("installed annotation protocol omitted protocol_revision_id")?;
    Ok(json!({
        "protocolId": state.protocol_id,
        "protocolRevisionId": revision,
        "sourceRevision": state.source_revision,
        "configurationDigest": state.configuration_digest,
        "protocolSource": protocol_source,
        "installedByThisRun": true,
        "immutable": true,
        "mode": "observe_only",
        "authority": PROTOCOL_STATE_SCHEMA,
        "updatedMidRun": true,
    }))
}

pub(crate) async fn persist_protocol_pin(
    service: &OptimizerService,
    run_id: &str,
    pin: &Value,
) -> Result<()> {
    let pin = pin.clone();
    service
        .patch_run(run_id.to_string(), move |run| {
            let mut summary = run.summary.as_object().cloned().unwrap_or_default();
            summary.insert("liveAnnotationPin".into(), pin.clone());
            run.summary = Value::Object(summary);
            Ok(())
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> toml::value::Table {
        toml::from_str::<toml::Value>(text)
            .unwrap()
            .as_table()
            .unwrap()
            .get("live_annotation")
            .unwrap()
            .as_table()
            .unwrap()
            .clone()
    }

    #[test]
    fn parses_a_protocol_with_configuration_and_model() {
        let spec = LiveAnnotationSpec::parse(
            "r",
            &table(
                r#"
[live_annotation]
protocol_id = "craftax.live.v1"
protocol_source = "domains/craftax/annotations/live_protocol.py"

[live_annotation.configuration]
judge_every_calls = 2

[live_annotation.model]
model = "gpt-5.6-luna"
max_calls = 10
"#,
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(spec.protocol_id, "craftax.live.v1");
        assert_eq!(spec.configuration.get("judge_every_calls"), Some(&json!(2)));
        let merged = spec.container_configuration();
        assert_eq!(merged["model"]["model"], json!("gpt-5.6-luna"));
        assert_eq!(merged["model"]["max_calls"], json!(10));
        assert_eq!(spec.summary_json()["model"], json!("gpt-5.6-luna"));
    }

    #[test]
    fn refuses_unknown_keys_missing_identity_and_credentials() {
        let unknown = LiveAnnotationSpec::parse(
            "r",
            &table("[live_annotation]\nprotocol_id = \"x\"\nprotocol_source = \"p.py\"\ncadence = 1\n"),
        )
        .unwrap_err();
        assert!(unknown.to_string().contains("live_annotation.cadence"), "{unknown:#}");
        let missing = LiveAnnotationSpec::parse("r", &table("[live_annotation]\nprotocol_source = \"p.py\"\n"))
            .unwrap_err();
        assert!(missing.to_string().contains("protocol_id"), "{missing:#}");
        let secret = LiveAnnotationSpec::parse(
            "r",
            &table(
                "[live_annotation]\nprotocol_id = \"x\"\nprotocol_source = \"p.py\"\n[live_annotation.model]\nmodel = \"m\"\napi_key = \"sk\"\n",
            ),
        )
        .unwrap_err();
        assert!(secret.to_string().contains("model.api_key"), "{secret:#}");
        let disabled = LiveAnnotationSpec::parse(
            "r",
            &table("[live_annotation]\nenabled = false\nprotocol_id = \"x\"\nprotocol_source = \"p.py\"\n"),
        )
        .unwrap();
        assert!(disabled.is_none());
    }

    #[test]
    fn source_identity_matches_the_container_digest_shape() {
        let spec = LiveAnnotationSpec::parse(
            "r",
            &table("[live_annotation]\nprotocol_id = \"t\"\nprotocol_source = \"p.py\"\n[live_annotation.configuration]\na = 1\n"),
        )
        .unwrap()
        .unwrap();
        let code = format!("PROTOCOL = {PROTOCOL_CODE_SCHEMA:?}\nclass Protocol: pass\n");
        let source = LiveAnnotationSource::from_code(spec, code.clone()).unwrap();
        assert_eq!(
            source.source_revision,
            format!("sha256:{:x}", Sha256::digest(code.as_bytes()))
        );
        // The container computes "sha256:" + sha256(json.dumps(configuration,
        // sort_keys=True, separators=(",", ":"))). CanonicalJson is that
        // encoding, which is why the NanoHorizon policy pin already round-trips.
        let expected = format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_string(&json!({"a": 1})).unwrap().as_bytes())
        );
        assert_eq!(source.configuration_digest, expected);
        let body = source.install_body();
        assert_eq!(body["protocol_id"], json!("t"));
        assert_eq!(body["configuration"], json!({"a": 1}));
        assert_eq!(body["source_revision"], json!(source.source_revision));

        let bad = LiveAnnotationSource::from_code(source.spec.clone(), "print(1)\n".into()).unwrap_err();
        assert!(bad.to_string().contains("does not declare PROTOCOL"), "{bad:#}");
    }

    #[test]
    fn installed_state_must_match_every_identity_axis() {
        let spec = LiveAnnotationSpec::parse(
            "r",
            &table("[live_annotation]\nprotocol_id = \"t\"\nprotocol_source = \"p.py\"\n"),
        )
        .unwrap()
        .unwrap();
        let source = LiveAnnotationSource::from_code(
            spec,
            format!("PROTOCOL = {PROTOCOL_CODE_SCHEMA:?}\n"),
        )
        .unwrap();
        let state = |revision: Option<&str>, protocol_id: &str, source_revision: &str| ContainerProtocolState {
            schema_version: PROTOCOL_STATE_SCHEMA.into(),
            status: "installed".into(),
            protocol_id: Some(protocol_id.into()),
            protocol_revision_id: revision.map(str::to_string),
            source_revision: Some(source_revision.into()),
            configuration_digest: Some(source.configuration_digest.clone()),
            credential_state: "not_exposed".into(),
        };
        assert!(installed_matches(&state(Some("anprev_1"), "t", &source.source_revision), &source));
        assert!(!installed_matches(&state(None, "t", &source.source_revision), &source));
        assert!(!installed_matches(&state(Some("anprev_1"), "other", &source.source_revision), &source));
        assert!(!installed_matches(&state(Some("anprev_1"), "t", "sha256:stale"), &source));
    }

    #[test]
    fn capability_gate_fails_closed() {
        assert!(require_advertised(&json!({})).is_err());
        assert!(require_advertised(&json!({"capabilities": {"operations": {"annotation.live": true}}})).is_err());
        assert!(require_advertised(&json!({"capabilities": {"operations": {
            "annotation.live": true, "annotation.protocol.put": true
        }}}))
        .is_ok());
        let error = require_advertised(&json!({})).unwrap_err();
        let structured = error.downcast_ref::<StructuredFailure>().unwrap();
        assert_eq!(structured.code, "live_annotation_unsupported");
    }
}
