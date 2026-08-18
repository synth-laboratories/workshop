//! Typed live-eval capability projection for registered containers.
//!
//! A healthy `/health` proves liveness, never workflow compatibility. Workshop
//! must be able to tell four states apart before it spends a mutating call:
//!
//! 1. a healthy raw environment engine (SSE, no normalized prepare);
//! 2. a healthy normalized live-policy pool;
//! 3. an unhealthy formerly compatible pool;
//! 4. a record whose capabilities are unknown or stale.
//!
//! Support is therefore tri-state. `Unknown` is not `Unsupported`, and neither
//! is ever inferred from task family, endpoint name, SSE support, or a
//! successful health probe. The only sources are an explicit service
//! advertisement, an operator declaration in `config.toml`, and a narrow
//! compatibility mapping over well-known *explicit* advertisements. No caller
//! can assert its own capabilities: registration metadata reaches Workshop
//! through an agent-callable MCP tool, so any capability claim it carries is
//! stripped rather than trusted.

use crate::data::ContainerDeployment;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::time::Duration;

/// Normalized live-eval protocol identifier services advertise.
pub const LIVE_EVAL_PROTOCOL: &str = "synth.container.live-eval.v1";

/// Normalized GEPA v2 optimizer contract. Workshop projects this onto live-eval
/// operation names; task containers keep owning execution, not campaigns.
pub const GEPA_V2_CONTRACT: &str = "synth_optimizers.gepa.v2";

/// Extra evidence operations the live-eval adapter may project. These are not
/// part of the prepare workflow and must never fabricate frames or forks.
pub const USAGE_GET: &str = "usage.get";
pub const RECORDS_GET: &str = "records.get";
pub const RETENTION_DURABLE: &str = "retention.durable";
pub const EVENTS_SEMANTIC: &str = "events.semantic";
pub const FRAMES_REPLAY: &str = "frames.replay";
pub const CHECKPOINT_RESTORE: &str = "checkpoint.restore";
pub const ROLLOUTS_FORK: &str = "rollouts.fork";

/// Registry metadata key holding the computed projection. Host-owned: the
/// hydration writer always overwrites it, so a caller-supplied value can never
/// survive into the record.
pub const CAPABILITIES_KEY: &str = "capabilities";

/// Registry metadata keys a caller might try to use to assert its own
/// capabilities. `container_register.metadata` is agent-reachable through MCP,
/// so these are stripped rather than read: the only non-service capability
/// authority is `synth_config::container_capability_declaration`, which lives
/// in operator-owned `config.toml`.
const CALLER_ASSERTED_KEYS: [&str; 3] = [
    "declaredCapabilities",
    "declared_capabilities",
    "capabilities",
];

/// Registry metadata key holding the last health observation timestamp.
pub const HEALTH_CHECKED_AT_KEY: &str = "healthCheckedAt";

pub const READY_STATUS: &str = "ready";
pub const UNHEALTHY_STATUS: &str = "unhealthy";

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// One normalized live-eval operation. These are wire names; they are also the
/// exact strings that appear in a `container_capability_mismatch` error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContainerOperation {
    RolloutsPrepare,
    RolloutsStartPrepared,
    RolloutsGet,
    RolloutsPoll,
    RewardGet,
    TraceV5Capture,
}

pub const ALL_OPERATIONS: [ContainerOperation; 6] = [
    ContainerOperation::RolloutsPrepare,
    ContainerOperation::RolloutsStartPrepared,
    ContainerOperation::RolloutsGet,
    ContainerOperation::RolloutsPoll,
    ContainerOperation::RewardGet,
    ContainerOperation::TraceV5Capture,
];

/// The workflow a successful `rollouts.prepare` commits Workshop to: prepare,
/// start the prepared identity, restore it, resume its stream, read its reward.
/// `trace_v5.capture` is added only when the caller promises sealed evidence.
pub const PREPARE_WORKFLOW: [ContainerOperation; 5] = [
    ContainerOperation::RolloutsPrepare,
    ContainerOperation::RolloutsStartPrepared,
    ContainerOperation::RolloutsGet,
    ContainerOperation::RolloutsPoll,
    ContainerOperation::RewardGet,
];

impl ContainerOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RolloutsPrepare => "rollouts.prepare",
            Self::RolloutsStartPrepared => "rollouts.start_prepared",
            Self::RolloutsGet => "rollouts.get",
            Self::RolloutsPoll => "rollouts.poll",
            Self::RewardGet => "reward.get",
            Self::TraceV5Capture => "trace_v5.capture",
        }
    }

    /// Accept the dotted wire name and the underscored alias a service may use
    /// as a JSON key. Nothing else — a near-miss stays unknown.
    fn parse(name: &str) -> Option<Self> {
        let normalized = name.trim().to_ascii_lowercase().replace('_', ".");
        ALL_OPERATIONS
            .into_iter()
            .find(|operation| operation.as_str().replace('_', ".") == normalized)
    }
}

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Tri-state support. `Unknown` fails closed at preflight but is reported as a
/// different remediation than an explicit `Unsupported`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Supported,
    Unsupported,
    #[default]
    Unknown,
}

impl CapabilityState {
    fn from_advertised(value: &Value) -> Self {
        match value.as_bool() {
            Some(true) => Self::Supported,
            Some(false) => Self::Unsupported,
            None => match value.as_str().map(str::to_ascii_lowercase).as_deref() {
                Some("supported") | Some("true") | Some("yes") => Self::Supported,
                Some("unsupported") | Some("false") | Some("no") => Self::Unsupported,
                _ => Self::Unknown,
            },
        }
    }
}

/// Where the projection came from. `Info` is the service's own normalized
/// block, `Metadata` an operator declaration in `config.toml`, `Compatibility`
/// a mapping over well-known explicit advertisements, `None` nothing at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySource {
    Info,
    Metadata,
    Compatibility,
    #[default]
    None,
}

impl CapabilitySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Metadata => "metadata",
            Self::Compatibility => "compatibility",
            Self::None => "none",
        }
    }
}

/// One advertised policy the pool can actually run. A policy reference never
/// implies an operation, and an operation never implies a policy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRef {
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
}

impl PolicyRef {
    fn parse(value: &Value) -> Option<Self> {
        let harness = value.get("harness").and_then(Value::as_str)?.trim();
        if harness.is_empty() {
            return None;
        }
        let field = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        };
        Some(Self {
            harness: harness.to_string(),
            config: field("config"),
            model: field("model"),
            provider: field("provider"),
            auth: field("auth"),
        })
    }

    /// Exact match on every field the caller actually pinned. A caller that
    /// names only `harness` + `config` must not be silently upgraded to a
    /// different model or auth, so unspecified fields are not compared.
    fn satisfies(&self, requested: &Self) -> bool {
        let matches = |advertised: &Option<String>, wanted: &Option<String>| match wanted {
            None => true,
            Some(wanted) => advertised.as_deref() == Some(wanted.as_str()),
        };
        self.harness == requested.harness
            && matches(&self.config, &requested.config)
            && matches(&self.model, &requested.model)
            && matches(&self.provider, &requested.provider)
            && matches(&self.auth, &requested.auth)
    }
}

/// The typed projection stored on `container.metadata.capabilities` and
/// returned by `container_list` / `container_get` / `container_probe`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerCapabilities {
    pub protocol: Option<String>,
    pub operations: BTreeMap<String, CapabilityState>,
    pub policy_refs: Vec<PolicyRef>,
    pub observed_at: Option<String>,
    pub source: CapabilitySource,
    pub complete: bool,
    /// Content digest of the projected operations and policy refs. Probe must
    /// replace this atomically; breaker keys must not strip it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// HealthBench-style policy role: the request may name a provider/model
    /// that is not the default Groq seed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub policy_role_configurable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorer_role: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credential_requirements: Vec<Value>,
}

impl ContainerCapabilities {
    fn empty(source: CapabilitySource) -> Self {
        Self {
            protocol: None,
            operations: ALL_OPERATIONS
                .into_iter()
                .map(|operation| (operation.as_str().to_string(), CapabilityState::Unknown))
                .collect(),
            policy_refs: Vec::new(),
            observed_at: None,
            source,
            complete: false,
            revision: None,
            policy_role_configurable: false,
            scorer_role: None,
            credential_requirements: Vec::new(),
        }
    }

    pub fn state(&self, operation: ContainerOperation) -> CapabilityState {
        self.operations
            .get(operation.as_str())
            .copied()
            .unwrap_or_default()
    }

    fn set(&mut self, operation: ContainerOperation, state: CapabilityState) {
        self.operations
            .insert(operation.as_str().to_string(), state);
    }

    fn recompute_complete(&mut self) {
        self.complete = ALL_OPERATIONS
            .into_iter()
            .all(|operation| self.state(operation) != CapabilityState::Unknown);
    }

    /// Read the projection back off a stored registry record. A record written
    /// before this branch has no block and reads as fully unknown.
    pub fn from_metadata(metadata: &Value) -> Self {
        metadata
            .get(CAPABILITIES_KEY)
            .and_then(|value| serde_json::from_value::<Self>(value.clone()).ok())
            .map(|mut capabilities| {
                for operation in ALL_OPERATIONS {
                    capabilities
                        .operations
                        .entry(operation.as_str().to_string())
                        .or_insert(CapabilityState::Unknown);
                }
                capabilities.recompute_complete();
                capabilities
            })
            .unwrap_or_else(|| Self::empty(CapabilitySource::None))
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Project capabilities from a discovery observation.
///
/// `info` is the `/info` (or `/metadata` fallback) body, `declared` an operator
/// declaration supplied at registration. Nothing here issues a request, and
/// nothing infers an operation from liveness, transport, or family.
pub fn project_capabilities(
    info: Option<&Value>,
    declared: Option<&Value>,
    observed_at: DateTime<Utc>,
) -> ContainerCapabilities {
    if let Some(mut capabilities) = info
        .and_then(normalized_block)
        .map(|block| parse_normalized_block(&block, CapabilitySource::Info))
    {
        capabilities.observed_at = Some(observed_at.to_rfc3339());
        merge_policy_refs(&mut capabilities, info);
        apply_model_roles(&mut capabilities, info);
        capabilities.recompute_complete();
        assign_revision(&mut capabilities);
        return capabilities;
    }
    // Operator-declared, from `config.toml` only — never from a caller.
    if let Some(mut capabilities) = declared
        .and_then(normalized_block)
        .map(|block| parse_normalized_block(&block, CapabilitySource::Metadata))
    {
        capabilities.observed_at = Some(observed_at.to_rfc3339());
        merge_policy_refs(&mut capabilities, info);
        apply_model_roles(&mut capabilities, info);
        capabilities.recompute_complete();
        assign_revision(&mut capabilities);
        return capabilities;
    }
    let mut capabilities = ContainerCapabilities::empty(CapabilitySource::None);
    if let Some(info) = info {
        let mapped = compatibility_operations(info);
        if !mapped.is_empty() {
            capabilities.source = CapabilitySource::Compatibility;
            for (operation, state) in mapped {
                capabilities.set(operation, state);
            }
        }
        if let Some(adapted) = gepa_v2_projection(info) {
            if capabilities.source == CapabilitySource::None {
                capabilities = adapted;
            } else {
                merge_gepa_evidence(&mut capabilities, &adapted);
            }
        }
    }
    // A discovery that found nothing is still an observation: the record was
    // probed and the service advertised no normalized capabilities. Only a
    // record that was never projected has no `observed_at`.
    capabilities.observed_at = Some(observed_at.to_rfc3339());
    merge_policy_refs(&mut capabilities, info);
    apply_model_roles(&mut capabilities, info);
    capabilities.recompute_complete();
    assign_revision(&mut capabilities);
    capabilities
}

/// A normalized block is one that names the protocol or carries an explicit
/// `operations` map. The Craftax pool already ships an unrelated
/// `capabilities: {async_rollout: true, …}` object; that is not this.
fn normalized_block(source: &Value) -> Option<Value> {
    for pointer in [
        "/capabilities",
        "/live_eval",
        "/liveEval",
        "/live_eval/capabilities",
    ] {
        let Some(block) = source.pointer(pointer) else {
            continue;
        };
        let names_protocol = block
            .get("protocol")
            .and_then(Value::as_str)
            .is_some_and(|protocol| protocol == LIVE_EVAL_PROTOCOL);
        let has_operations = block.get("operations").is_some_and(Value::is_object);
        if names_protocol || has_operations {
            return Some(block.clone());
        }
    }
    let names_protocol = source
        .get("protocol")
        .and_then(Value::as_str)
        .is_some_and(|protocol| protocol == LIVE_EVAL_PROTOCOL);
    (names_protocol && source.get("operations").is_some_and(Value::is_object))
        .then(|| source.clone())
}

fn parse_normalized_block(block: &Value, source: CapabilitySource) -> ContainerCapabilities {
    let mut capabilities = ContainerCapabilities::empty(source);
    capabilities.protocol = block
        .get("protocol")
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(operations) = block.get("operations").and_then(Value::as_object) {
        for (name, value) in operations {
            if let Some(operation) = ContainerOperation::parse(name) {
                capabilities.set(operation, CapabilityState::from_advertised(value));
            }
        }
    }
    capabilities.policy_refs = policy_refs_from(block);
    capabilities
}

/// Compatibility mapping for services that advertise operations explicitly but
/// have not adopted the protocol block. Only exact operation names count. SSE,
/// `rollout_stream_sse`, task family, and endpoint names map to nothing.
fn compatibility_operations(info: &Value) -> Vec<(ContainerOperation, CapabilityState)> {
    let mut mapped = Vec::new();
    for pointer in ["/capabilities/operations", "/operations"] {
        if let Some(operations) = info.pointer(pointer).and_then(Value::as_object) {
            for (name, value) in operations {
                if let Some(operation) = ContainerOperation::parse(name) {
                    mapped.push((operation, CapabilityState::from_advertised(value)));
                }
            }
        }
    }
    for pointer in ["/features", "/operations", "/routes", "/endpoints"] {
        let Some(entries) = info.pointer(pointer).and_then(Value::as_array) else {
            continue;
        };
        for entry in entries {
            let Some(text) = entry.as_str() else { continue };
            // `POST /rollouts/prepare` and `rollouts.prepare` both name the
            // operation outright; a bare `/rollouts` route does not.
            let candidate = text
                .rsplit(' ')
                .next()
                .unwrap_or(text)
                .trim_start_matches('/')
                .replace('/', ".");
            if let Some(operation) = ContainerOperation::parse(&candidate) {
                mapped.push((operation, CapabilityState::Supported));
            }
        }
    }
    mapped
}

fn policy_refs_from(source: &Value) -> Vec<PolicyRef> {
    for pointer in ["/policy_refs", "/policyRefs"] {
        if let Some(entries) = source.pointer(pointer).and_then(Value::as_array) {
            let refs: Vec<PolicyRef> = entries.iter().filter_map(PolicyRef::parse).collect();
            if !refs.is_empty() {
                return refs;
            }
        }
    }
    Vec::new()
}

/// Advertised policy references may live outside the capability block (the
/// Craftax pool ships `info.liveEval.policyRefs`). They still describe only
/// which policies exist, never which operations are implemented.
fn merge_policy_refs(capabilities: &mut ContainerCapabilities, info: Option<&Value>) {
    if !capabilities.policy_refs.is_empty() {
        return;
    }
    let Some(info) = info else { return };
    for pointer in ["/liveEval", "/live_eval", "/capabilities", "/metadata", ""] {
        let scope = if pointer.is_empty() {
            Some(info)
        } else {
            info.pointer(pointer)
        };
        if let Some(scope) = scope {
            let refs = policy_refs_from(scope);
            if !refs.is_empty() {
                capabilities.policy_refs = refs;
                return;
            }
        }
    }
}

fn gepa_v2_contract(info: &Value) -> Option<&Value> {
    for pointer in [
        "/metadata/optimizer_contracts/gepa",
        "/optimizer_contracts/gepa",
        "/capabilities/optimizer_contracts/gepa",
    ] {
        let Some(block) = info.pointer(pointer) else {
            continue;
        };
        let version = block.get("version").and_then(Value::as_str).unwrap_or("");
        if version == GEPA_V2_CONTRACT {
            return Some(block);
        }
    }
    None
}

fn gepa_v2_projection(info: &Value) -> Option<ContainerCapabilities> {
    let contract = gepa_v2_contract(info)?;
    let has_rollout = contract
        .get("rollout_route")
        .and_then(Value::as_str)
        .is_some_and(|route| !route.trim().is_empty())
        || contract.get("prepare_route").is_some();
    if !has_rollout {
        return None;
    }
    let mut capabilities = ContainerCapabilities::empty(CapabilitySource::Compatibility);
    capabilities.protocol = Some(GEPA_V2_CONTRACT.into());
    for operation in PREPARE_WORKFLOW {
        capabilities.set(operation, CapabilityState::Supported);
    }
    // Semantic events and terminal records exist; replay frames, restore, and
    // fork do not. Advertise that honestly.
    capabilities.set(
        ContainerOperation::TraceV5Capture,
        CapabilityState::Unsupported,
    );
    for (name, state) in [
        (USAGE_GET, CapabilityState::Supported),
        (RECORDS_GET, CapabilityState::Supported),
        (RETENTION_DURABLE, CapabilityState::Supported),
        (EVENTS_SEMANTIC, CapabilityState::Supported),
        (FRAMES_REPLAY, CapabilityState::Unsupported),
        (CHECKPOINT_RESTORE, CapabilityState::Unsupported),
        (ROLLOUTS_FORK, CapabilityState::Unsupported),
    ] {
        capabilities.operations.insert(name.into(), state);
    }
    merge_policy_refs(&mut capabilities, Some(info));
    apply_model_roles(&mut capabilities, Some(info));
    Some(capabilities)
}

fn merge_gepa_evidence(target: &mut ContainerCapabilities, adapted: &ContainerCapabilities) {
    for (name, state) in &adapted.operations {
        target
            .operations
            .entry(name.clone())
            .or_insert(*state);
    }
    if target.policy_refs.is_empty() {
        target.policy_refs = adapted.policy_refs.clone();
    }
    target.policy_role_configurable |= adapted.policy_role_configurable;
    if target.scorer_role.is_none() {
        target.scorer_role = adapted.scorer_role.clone();
    }
    if target.credential_requirements.is_empty() {
        target.credential_requirements = adapted.credential_requirements.clone();
    }
    if target.protocol.is_none() {
        target.protocol = adapted.protocol.clone();
    }
}

fn apply_model_roles(capabilities: &mut ContainerCapabilities, info: Option<&Value>) {
    let Some(info) = info else { return };
    let roles = info
        .pointer("/metadata/model_roles")
        .or_else(|| info.pointer("/model_roles"));
    let Some(roles) = roles else { return };
    if let Some(policy) = roles.get("policy") {
        let configurable = policy
            .get("configuration_authority")
            .and_then(Value::as_str)
            == Some("policy_ref");
        capabilities.policy_role_configurable |= configurable;
        push_credential_requirement(capabilities, policy, "policy");
        if let Some(harness) = policy
            .get("harness")
            .and_then(Value::as_str)
            .or(Some("chat_completion"))
        {
            if let (Some(provider), Some(model)) = (
                policy.get("provider").and_then(Value::as_str),
                policy.get("model").and_then(Value::as_str),
            ) {
                let next = PolicyRef {
                    harness: harness.to_string(),
                    config: None,
                    model: Some(model.to_string()),
                    provider: Some(provider.to_string()),
                    auth: policy
                        .get("api_key_env")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                if !capabilities.policy_refs.iter().any(|existing| existing.satisfies(&next)) {
                    capabilities.policy_refs.push(next);
                }
            }
        }
    }
    if let Some(scorer) = roles.get("scorer") {
        let mut scorer = scorer.clone();
        if let Some(object) = scorer.as_object_mut() {
            object.remove("api_key");
            object.remove("token");
            object.remove("secret");
        }
        capabilities.scorer_role = Some(scorer.clone());
        push_credential_requirement(capabilities, &scorer, "scorer");
    }
}

fn push_credential_requirement(
    capabilities: &mut ContainerCapabilities,
    role: &Value,
    lane: &str,
) {
    let Some(env) = role.get("api_key_env").and_then(Value::as_str) else {
        return;
    };
    let requirement = json!({
        "lane": lane,
        "variable": env,
        "provider": role.get("provider"),
        "model": role.get("model"),
    });
    if !capabilities.credential_requirements.contains(&requirement) {
        capabilities.credential_requirements.push(requirement);
    }
}

fn assign_revision(capabilities: &mut ContainerCapabilities) {
    let payload = json!({
        "protocol": capabilities.protocol,
        "operations": capabilities.operations,
        "policy_refs": capabilities.policy_refs,
        "source": capabilities.source.as_str(),
        "policy_role_configurable": capabilities.policy_role_configurable,
    });
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in payload.to_string().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    capabilities.revision = Some(format!("{hash:016x}"));
}

fn attach_projection(error: &mut ContainerPreflightError, capabilities: &ContainerCapabilities) {
    error.observed_at = capabilities.observed_at.clone();
    error.capability_source = Some(capabilities.source.as_str().into());
    error.capability_revision = capabilities.revision.clone();
    error.protocol = capabilities.protocol.clone();
}

/// Write the projection (and the health observation time) into a registry
/// metadata object. Every hydration path calls this so `container_list`,
/// `container_get`, and `container_probe` cannot disagree.
///
/// `declared` comes from operator-owned `config.toml`, never from the metadata
/// map: registration metadata arrives through an agent-callable MCP tool, so
/// any capability claim it carries is stripped before the record is stored.
///
/// `info_refreshed` is false when the caller reused a cached `/info` body; the
/// previous `observed_at` is then preserved rather than restamped, so a stale
/// observation cannot be laundered into a fresh one by a health-only probe.
pub fn write_capability_metadata(
    metadata: &mut Map<String, Value>,
    info: Option<&Value>,
    declared: Option<&Value>,
    info_refreshed: bool,
    observed_at: DateTime<Utc>,
) {
    let previous = ContainerCapabilities::from_metadata(&Value::Object(metadata.clone()));
    for key in CALLER_ASSERTED_KEYS {
        metadata.remove(key);
    }
    let mut capabilities = project_capabilities(info, declared, observed_at);
    if !info_refreshed {
        if let Some(previous_observed_at) = previous.observed_at {
            capabilities.observed_at = Some(previous_observed_at);
        }
    }
    metadata.insert(CAPABILITIES_KEY.into(), capabilities.to_json());
    metadata.insert(
        HEALTH_CHECKED_AT_KEY.into(),
        json!(observed_at.to_rfc3339()),
    );
}

/// Registry status from one health observation. HTTP success alone is not
/// readiness: a service that answers `200 {"ok": false}` (or
/// `{"healthy": false}`, or `{"status": "unhealthy"}`) is unhealthy, and a
/// record that reads `ready` would otherwise pass the health half of preflight.
///
/// Only an explicit negative demotes the record — an unfamiliar payload stays
/// `ready` so this cannot invent failures for services that report nothing.
pub fn observed_status(http_status: u16, payload: &Value) -> &'static str {
    if !(200..300).contains(&http_status) {
        return UNHEALTHY_STATUS;
    }
    for key in ["ok", "healthy", "ready"] {
        if payload.get(key).and_then(Value::as_bool) == Some(false) {
            return UNHEALTHY_STATUS;
        }
    }
    let reported = payload
        .get("status")
        .or_else(|| payload.get("state"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    if matches!(
        reported.as_deref(),
        Some("unhealthy" | "error" | "fail" | "failed" | "degraded" | "down" | "stopped")
    ) {
        return UNHEALTHY_STATUS;
    }
    READY_STATUS
}

// ---------------------------------------------------------------------------
// Preflight
// ---------------------------------------------------------------------------

pub const CODE_UNHEALTHY: &str = "container_unhealthy";
pub const CODE_CAPABILITY_MISMATCH: &str = "container_capability_mismatch";
pub const CODE_CAPABILITIES_STALE: &str = "container_capabilities_stale";

/// Structured, actionable preflight failure. It crosses IPC as a JSON body and
/// MCP as a tool failure; it is never a successful result carrying an `error`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContainerPreflightError {
    pub code: String,
    pub container_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_probe_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_probe_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_policy_ref: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_policy_refs: Vec<PolicyRef>,
    pub retryable: bool,
    pub remediation: String,
}

impl ContainerPreflightError {
    fn new(
        code: &str,
        container: &ContainerDeployment,
        remediation: &str,
        retryable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            container_id: container.id.clone(),
            base_url: container.base_url.clone(),
            status: Some(container.status.clone()),
            last_probe_at: container
                .metadata
                .get(HEALTH_CHECKED_AT_KEY)
                .and_then(Value::as_str)
                .map(str::to_string),
            last_probe_error: last_probe_error(container),
            observed_at: None,
            capability_source: None,
            capability_revision: None,
            protocol: None,
            required: Vec::new(),
            missing: Vec::new(),
            unknown: Vec::new(),
            requested_policy_ref: None,
            available_policy_refs: Vec::new(),
            retryable,
            remediation: remediation.into(),
        }
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self)
            .unwrap_or_else(|_| json!({"code": self.code, "container_id": self.container_id}))
    }

    /// One-line agent-facing summary used when the structured body has to be
    /// collapsed into transcript text.
    pub fn summary(&self) -> String {
        let mut summary = self.code.clone();
        if !self.missing.is_empty() {
            summary.push_str(&format!(": missing {}", self.missing.join(", ")));
        }
        summary.push_str(&format!(" — {}", self.remediation));
        summary
    }
}

impl std::fmt::Display for ContainerPreflightError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            &serde_json::to_string(&self.to_json()).unwrap_or_else(|_| self.code.clone()),
        )
    }
}

impl std::error::Error for ContainerPreflightError {}

fn last_probe_error(container: &ContainerDeployment) -> Option<String> {
    container
        .health
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            container
                .health
                .get("status")
                .and_then(Value::as_u64)
                .filter(|status| !(200..300).contains(status))
                .map(|status| format!("health returned HTTP {status}"))
        })
}

/// What a caller promises to do after prepare succeeds.
#[derive(Clone, Debug)]
pub struct PreflightRequest {
    pub required: Vec<ContainerOperation>,
    pub requested_policy_ref: Option<PolicyRef>,
    pub require_trace_v5: bool,
    pub max_probe_age: Duration,
}

impl PreflightRequest {
    /// The `rollouts.prepare` workflow. `require_trace_v5` is opt-in: only a
    /// caller that promises sealed Trace V5 evidence may demand it, and it is
    /// never inferred from SSE or telemetry being enabled.
    pub fn for_prepare(
        requested_policy_ref: Option<PolicyRef>,
        require_trace_v5: bool,
        max_probe_age: Duration,
    ) -> Self {
        let mut required = PREPARE_WORKFLOW.to_vec();
        if require_trace_v5 {
            required.push(ContainerOperation::TraceV5Capture);
        }
        Self {
            required,
            requested_policy_ref,
            require_trace_v5,
            max_probe_age,
        }
    }
}

/// Parse a caller-supplied `policy_ref` argument. An argument that is present
/// but unusable is a caller error, not an absent pin.
pub fn requested_policy_ref(value: Option<&Value>) -> Result<Option<PolicyRef>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => PolicyRef::parse(value)
            .map(Some)
            .ok_or_else(|| "policy_ref requires a non-empty harness".to_string()),
    }
}

/// Reject an unhealthy, stale, or capability-incompatible record **before** any
/// mutating request reaches the container.
///
/// This function never probes, never selects another container, never changes
/// the requested policy, and never downgrades the trace requirement.
pub fn preflight_prepare(
    container: &ContainerDeployment,
    request: &PreflightRequest,
    now: DateTime<Utc>,
) -> Result<ContainerCapabilities, ContainerPreflightError> {
    // 1. Record exists and has a base URL.
    if container
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .is_none()
    {
        return Err(ContainerPreflightError::new(
            CODE_UNHEALTHY,
            container,
            "This record has no base URL. Re-register the container with an explicit URL supplied by the user or workspace.",
            false,
        ));
    }

    // 2. Status is currently healthy. The stored health envelope is checked
    //    too, so a record whose status was written by some other path cannot
    //    present as ready while its last observation says otherwise.
    let health_denies = container.health.get("ok").and_then(Value::as_bool) == Some(false);
    if container.status != READY_STATUS || health_denies {
        return Err(ContainerPreflightError::new(
            CODE_UNHEALTHY,
            container,
            "Start or repair the registered pool at this URL, then call container_probe. Do not switch to another port, a raw engine, or archived evidence.",
            true,
        ));
    }

    let capabilities = ContainerCapabilities::from_metadata(&container.metadata);

    // 3. The last successful health/capability probe is fresh.
    let max_age = ChronoDuration::from_std(request.max_probe_age)
        .unwrap_or_else(|_| ChronoDuration::seconds(900));
    let stale = |timestamp: Option<&str>| match timestamp.and_then(parse_rfc3339) {
        None => true,
        Some(observed) => now.signed_duration_since(observed) > max_age,
    };
    let health_checked_at = container
        .metadata
        .get(HEALTH_CHECKED_AT_KEY)
        .and_then(Value::as_str);
    if stale(health_checked_at) || stale(capabilities.observed_at.as_deref()) {
        let mut error = ContainerPreflightError::new(
            CODE_CAPABILITIES_STALE,
            container,
            "Call container_probe before preparing a rollout.",
            true,
        );
        error.observed_at = capabilities.observed_at.clone();
        error.capability_source = Some(capabilities.source.as_str().into());
        attach_projection(&mut error, &capabilities);
        return Err(error);
    }

    // 4-6. The projection explicitly supports every operation this workflow
    //      commits to. Unknown fails closed and is reported separately from an
    //      explicit unsupported.
    let mut missing = Vec::new();
    let mut unknown = Vec::new();
    for operation in &request.required {
        match capabilities.state(*operation) {
            CapabilityState::Supported => {}
            CapabilityState::Unsupported => missing.push(operation.as_str().to_string()),
            CapabilityState::Unknown => {
                missing.push(operation.as_str().to_string());
                unknown.push(operation.as_str().to_string());
            }
        }
    }
    if !missing.is_empty() {
        let remediation = if unknown.len() == missing.len()
            && capabilities.source == CapabilitySource::None
        {
            "This record advertises no normalized live-eval capabilities. Select a registered pool that advertises the synth.container.live-eval.v1 protocol, or have this service advertise it; do not try raw engines, alternate ports, or prior evidence."
        } else if unknown.is_empty() {
            "Select a normalized live-policy pool; this record does not implement the requested operations."
        } else {
            "This record's capabilities are incomplete for the requested workflow. Select a pool that advertises every required operation; do not probe alternate ports or fall back to a raw engine."
        };
        let mut error =
            ContainerPreflightError::new(CODE_CAPABILITY_MISMATCH, container, remediation, false);
        error.required = request
            .required
            .iter()
            .map(|operation| operation.as_str().to_string())
            .collect();
        error.missing = missing;
        error.unknown = unknown;
        error.observed_at = capabilities.observed_at.clone();
        error.capability_source = Some(capabilities.source.as_str().into());
        attach_projection(&mut error, &capabilities);
        error.requested_policy_ref = request
            .requested_policy_ref
            .as_ref()
            .and_then(|policy_ref| serde_json::to_value(policy_ref).ok());
        error.available_policy_refs = capabilities.policy_refs.clone();
        return Err(error);
    }

    // 7. The requested policy ref matches an advertised one, or the container
    //    declares a configurable policy role (HealthBench) that can register
    //    the requested provider/model before the campaign starts.
    if let Some(requested) = &request.requested_policy_ref {
        let advertised_match = capabilities
            .policy_refs
            .iter()
            .any(|advertised| advertised.satisfies(requested));
        let configurable_match = capabilities.policy_role_configurable
            && (requested.harness == "chat_completion" || requested.harness == "classify");
        if !advertised_match && !configurable_match {
            let mut error = ContainerPreflightError::new(
                CODE_CAPABILITY_MISMATCH,
                container,
                "This pool does not advertise the requested policy_ref. Request one of the advertised refs, register a configurable policy role, or select another registered pool; the host does not substitute a policy.",
                false,
            );
            error.required = request
                .required
                .iter()
                .map(|operation| operation.as_str().to_string())
                .collect();
            attach_projection(&mut error, &capabilities);
            error.requested_policy_ref = serde_json::to_value(requested).ok();
            error.available_policy_refs = capabilities.policy_refs.clone();
            return Err(error);
        }
    }

    Ok(capabilities)
}

/// Render a preflight failure that travelled inside an `anyhow` chain as the
/// structured IPC body. Callers that cannot classify fall back to prose.
pub fn preflight_error_body(error: &anyhow::Error) -> Value {
    error
        .downcast_ref::<ContainerPreflightError>()
        .map(ContainerPreflightError::to_json)
        .unwrap_or_else(|| json!({"error": error.to_string()}))
}

/// The single gate in front of `POST /rollouts/prepare`. Both the direct IPC
/// route and the eval-driver policy-rollout path call this, so neither can
/// spend a mutating call the other would have refused.
///
/// `require_trace_v5` is read from the request because only the caller knows
/// whether this workflow promises sealed evidence; it is never inferred.
pub fn preflight_prepare_request(
    container: &ContainerDeployment,
    body: &Value,
) -> anyhow::Result<ContainerCapabilities> {
    let policy_ref = requested_policy_ref(body.get("policy_ref").or_else(|| body.get("policyRef")))
        .map_err(|error| anyhow::anyhow!(error))?;
    let require_trace_v5 = body
        .get("require_trace_v5")
        .or_else(|| body.get("requireTraceV5"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let request = PreflightRequest::for_prepare(
        policy_ref,
        require_trace_v5,
        crate::limits::CONTAINER_CAPABILITY_MAX_AGE,
    );
    preflight_prepare(container, &request, Utc::now()).map_err(anyhow::Error::new)
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-08-15T12:00:00Z";

    fn now() -> DateTime<Utc> {
        parse_rfc3339(NOW).unwrap()
    }

    fn normalized_info() -> Value {
        json!({
            "service": {"task": "gamebench-craftax-rust-react"},
            "env_family": "craftax",
            "capabilities": {
                "protocol": LIVE_EVAL_PROTOCOL,
                "operations": {
                    "rollouts.prepare": true,
                    "rollouts.start_prepared": true,
                    "rollouts.get": true,
                    "rollouts.poll": true,
                    "reward.get": true,
                    "trace_v5.capture": false
                },
                "policy_refs": [
                    {"harness": "react", "config": "luna_low", "model": "gpt-5.6-luna", "auth": "chatgpt-codex"}
                ]
            }
        })
    }

    fn raw_gold_info() -> Value {
        json!({
            "env_family": "craftax",
            "service": "craftax-rust-gold",
            "rollout_stream_sse": true,
            "features": ["rollout_stream_sse", "interactive_rollout"],
            "capabilities": {"async_rollout": true, "checkpoint_resume": true}
        })
    }

    fn container(status: &str, metadata: Value) -> ContainerDeployment {
        ContainerDeployment {
            id: "ctr_test".into(),
            name: "test".into(),
            location: "local".into(),
            status: status.into(),
            base_url: Some("http://127.0.0.1:8104".into()),
            pool_id: None,
            task_family: Some("craftax".into()),
            last_rollout_id: None,
            health: json!({"ok": status == READY_STATUS}),
            metadata,
            created_at: NOW.into(),
            updated_at: NOW.into(),
        }
    }

    fn hydrated(info: Option<&Value>, at: DateTime<Utc>) -> Value {
        let mut metadata = Map::new();
        write_capability_metadata(&mut metadata, info, None, true, at);
        Value::Object(metadata)
    }

    fn prepare_request() -> PreflightRequest {
        PreflightRequest::for_prepare(None, false, Duration::from_secs(900))
    }

    #[test]
    fn container_normalized_pool_projects_every_advertised_operation() {
        let capabilities = project_capabilities(Some(&normalized_info()), None, now());
        assert_eq!(capabilities.source, CapabilitySource::Info);
        assert_eq!(capabilities.protocol.as_deref(), Some(LIVE_EVAL_PROTOCOL));
        assert_eq!(
            capabilities.state(ContainerOperation::RolloutsPrepare),
            CapabilityState::Supported
        );
        assert_eq!(
            capabilities.state(ContainerOperation::TraceV5Capture),
            CapabilityState::Unsupported
        );
        assert!(capabilities.complete);
        assert_eq!(capabilities.policy_refs.len(), 1);
        assert_eq!(
            capabilities.policy_refs[0].config.as_deref(),
            Some("luna_low")
        );
    }

    #[test]
    fn container_raw_gold_sse_never_implies_prepare() {
        let capabilities = project_capabilities(Some(&raw_gold_info()), None, now());
        for operation in ALL_OPERATIONS {
            assert_eq!(
                capabilities.state(operation),
                CapabilityState::Unknown,
                "{} must stay unknown for a raw engine",
                operation.as_str()
            );
        }
        assert!(!capabilities.complete);
        assert_eq!(capabilities.source, CapabilitySource::None);
    }

    #[test]
    fn container_health_only_record_projects_nothing() {
        let capabilities = project_capabilities(None, None, now());
        assert_eq!(capabilities.source, CapabilitySource::None);
        assert_eq!(
            capabilities.observed_at.as_deref(),
            Some(now().to_rfc3339().as_str())
        );
        assert!(!capabilities.complete);
        for operation in ALL_OPERATIONS {
            assert_eq!(capabilities.state(operation), CapabilityState::Unknown);
        }
    }

    /// `container_register.metadata` is agent-reachable through MCP. A caller
    /// that asserts a full normalized block must not be able to talk its way
    /// past the gate this module exists to hold.
    #[test]
    fn container_caller_asserted_capabilities_are_stripped_not_trusted() {
        let asserted = json!({
            "protocol": LIVE_EVAL_PROTOCOL,
            "operations": {
                "rollouts.prepare": true,
                "rollouts.start_prepared": true,
                "rollouts.get": true,
                "rollouts.poll": true,
                "reward.get": true,
                "trace_v5.capture": true
            },
            "policy_refs": [{"harness": "react", "config": "luna_low"}]
        });
        let mut metadata = Map::new();
        metadata.insert("declaredCapabilities".into(), asserted.clone());
        metadata.insert("declared_capabilities".into(), asserted.clone());
        metadata.insert("capabilities".into(), asserted);
        metadata.insert("note".into(), json!("caller metadata is otherwise kept"));

        write_capability_metadata(&mut metadata, Some(&raw_gold_info()), None, true, now());

        assert!(metadata.get("declaredCapabilities").is_none());
        assert!(metadata.get("declared_capabilities").is_none());
        assert_eq!(metadata["note"], json!("caller metadata is otherwise kept"));
        let capabilities = ContainerCapabilities::from_metadata(&Value::Object(metadata.clone()));
        assert_eq!(capabilities.source, CapabilitySource::None);
        assert_eq!(
            capabilities.state(ContainerOperation::RolloutsPrepare),
            CapabilityState::Unknown
        );

        let record = container(READY_STATUS, Value::Object(metadata));
        let error = preflight_prepare(&record, &prepare_request(), now()).unwrap_err();
        assert_eq!(error.code, CODE_CAPABILITY_MISMATCH);
    }

    /// The operator declaration is honoured, but only when it arrives from
    /// `config.toml` through the `declared` argument.
    #[test]
    fn container_operator_declaration_arrives_only_as_an_explicit_argument() {
        let declared = json!({
            "protocol": LIVE_EVAL_PROTOCOL,
            "operations": {
                "rollouts.prepare": true,
                "rollouts.start_prepared": true,
                "rollouts.get": true,
                "rollouts.poll": true,
                "reward.get": true,
                "trace_v5.capture": false
            },
            "policy_refs": [{"harness": "react", "config": "luna_low"}]
        });
        let mut metadata = Map::new();
        write_capability_metadata(
            &mut metadata,
            Some(&raw_gold_info()),
            Some(&declared),
            true,
            now(),
        );
        let record = container(READY_STATUS, Value::Object(metadata));
        let capabilities = preflight_prepare(&record, &prepare_request(), now()).unwrap();
        assert_eq!(capabilities.source, CapabilitySource::Metadata);
    }

    #[test]
    fn container_health_status_reads_the_payload_not_just_the_http_code() {
        assert_eq!(observed_status(200, &json!({"ok": true})), READY_STATUS);
        assert_eq!(observed_status(200, &json!({"status": "ok"})), READY_STATUS);
        // Nothing reported stays ready: only an explicit negative demotes.
        assert_eq!(observed_status(200, &json!({})), READY_STATUS);
        assert_eq!(
            observed_status(200, &json!({"detail": "warming up"})),
            READY_STATUS
        );
        for payload in [
            json!({"ok": false}),
            json!({"healthy": false}),
            json!({"ready": false}),
            json!({"status": "unhealthy"}),
            json!({"status": "DEGRADED"}),
            json!({"ok": true, "healthy": false}),
        ] {
            assert_eq!(
                observed_status(200, &payload),
                UNHEALTHY_STATUS,
                "{payload} must not read as ready"
            );
        }
        assert_eq!(observed_status(503, &json!({"ok": true})), UNHEALTHY_STATUS);
    }

    #[test]
    fn container_stored_health_denial_overrides_a_ready_status() {
        let mut record = container(READY_STATUS, hydrated(Some(&normalized_info()), now()));
        record.health = json!({"ok": false, "status": 200, "payload": {"ok": false}});
        let error = preflight_prepare(&record, &prepare_request(), now()).unwrap_err();
        assert_eq!(error.code, CODE_UNHEALTHY);
        assert!(error.retryable);
    }

    #[test]
    fn container_never_projected_record_has_no_observation() {
        let capabilities = ContainerCapabilities::from_metadata(&json!({"contractHint": "info"}));
        assert!(capabilities.observed_at.is_none());
        assert_eq!(capabilities.source, CapabilitySource::None);
    }

    #[test]
    fn container_operator_declaration_is_used_only_without_a_service_block() {
        let declared = json!({
            "protocol": LIVE_EVAL_PROTOCOL,
            "operations": {"rollouts.prepare": true},
            "policy_refs": [{"harness": "react", "config": "luna_low"}]
        });
        // `declared` here stands for the operator-owned config.toml entry.
        let capabilities = project_capabilities(Some(&raw_gold_info()), Some(&declared), now());
        assert_eq!(capabilities.source, CapabilitySource::Metadata);
        assert_eq!(
            capabilities.state(ContainerOperation::RolloutsPrepare),
            CapabilityState::Supported
        );
        // The service block wins when both exist.
        let capabilities = project_capabilities(Some(&normalized_info()), Some(&declared), now());
        assert_eq!(capabilities.source, CapabilitySource::Info);
    }

    #[test]
    fn container_compatibility_mapping_needs_an_exact_operation_name() {
        let info = json!({"routes": ["POST /rollouts/prepare", "GET /rollouts"]});
        let capabilities = project_capabilities(Some(&info), None, now());
        assert_eq!(capabilities.source, CapabilitySource::Compatibility);
        assert_eq!(
            capabilities.state(ContainerOperation::RolloutsPrepare),
            CapabilityState::Supported
        );
        assert_eq!(
            capabilities.state(ContainerOperation::RolloutsPoll),
            CapabilityState::Unknown
        );
    }

    #[test]
    fn container_policy_refs_outside_the_block_never_imply_an_operation() {
        let info =
            json!({"liveEval": {"policyRefs": [{"harness": "react", "config": "luna_low"}]}});
        let capabilities = project_capabilities(Some(&info), None, now());
        assert_eq!(capabilities.policy_refs.len(), 1);
        assert_eq!(
            capabilities.state(ContainerOperation::RolloutsPrepare),
            CapabilityState::Unknown
        );
    }

    #[test]
    fn container_healthy_normalized_pool_passes_preflight() {
        let record = container(READY_STATUS, hydrated(Some(&normalized_info()), now()));
        let request = PreflightRequest::for_prepare(
            Some(PolicyRef {
                harness: "react".into(),
                config: Some("luna_low".into()),
                ..Default::default()
            }),
            false,
            Duration::from_secs(900),
        );
        let capabilities = preflight_prepare(&record, &request, now()).expect("preflight passes");
        assert!(capabilities.complete);
    }

    #[test]
    fn container_unhealthy_compatible_pool_fails_before_any_request() {
        let record = container("unhealthy", hydrated(Some(&normalized_info()), now()));
        let error = preflight_prepare(&record, &prepare_request(), now()).unwrap_err();
        assert_eq!(error.code, CODE_UNHEALTHY);
        assert!(error.retryable);
        assert!(error.remediation.contains("container_probe"));
        assert_eq!(error.base_url.as_deref(), Some("http://127.0.0.1:8104"));
    }

    #[test]
    fn container_raw_gold_fails_preflight_with_prepare_missing() {
        let record = container(READY_STATUS, hydrated(Some(&raw_gold_info()), now()));
        let error = preflight_prepare(&record, &prepare_request(), now()).unwrap_err();
        assert_eq!(error.code, CODE_CAPABILITY_MISMATCH);
        assert!(!error.retryable);
        assert!(error.missing.contains(&"rollouts.prepare".to_string()));
        assert!(error.unknown.contains(&"rollouts.prepare".to_string()));
        assert_eq!(error.capability_source.as_deref(), Some("none"));
    }

    #[test]
    fn container_health_only_record_fails_closed() {
        let record = container(READY_STATUS, hydrated(None, now()));
        let error = preflight_prepare(&record, &prepare_request(), now()).unwrap_err();
        assert_eq!(error.code, CODE_CAPABILITY_MISMATCH);
        assert!(!error.retryable);
        assert_eq!(error.missing.len(), PREPARE_WORKFLOW.len());
        assert_eq!(error.unknown.len(), PREPARE_WORKFLOW.len());
        assert!(error.remediation.contains("advertises no normalized"));
    }

    #[test]
    fn container_record_projected_by_an_older_build_asks_for_a_probe() {
        let record = container(
            READY_STATUS,
            json!({"contractHint": "info", "healthCheckedAt": NOW}),
        );
        let error = preflight_prepare(&record, &prepare_request(), now()).unwrap_err();
        assert_eq!(error.code, CODE_CAPABILITIES_STALE);
        assert!(error.retryable);
        assert!(error.remediation.contains("container_probe"));
    }

    #[test]
    fn container_stale_observation_requests_a_probe() {
        let observed = now() - ChronoDuration::seconds(1_800);
        let record = container(READY_STATUS, hydrated(Some(&normalized_info()), observed));
        let error = preflight_prepare(&record, &prepare_request(), now()).unwrap_err();
        assert_eq!(error.code, CODE_CAPABILITIES_STALE);
        assert!(error.retryable);
        assert!(error.remediation.contains("container_probe"));
        assert_eq!(
            error.observed_at.as_deref(),
            Some(observed.to_rfc3339().as_str())
        );
    }

    #[test]
    fn container_wrong_policy_config_reports_available_refs() {
        let record = container(READY_STATUS, hydrated(Some(&normalized_info()), now()));
        let request = PreflightRequest::for_prepare(
            Some(PolicyRef {
                harness: "react".into(),
                config: Some("luna_med".into()),
                ..Default::default()
            }),
            false,
            Duration::from_secs(900),
        );
        let error = preflight_prepare(&record, &request, now()).unwrap_err();
        assert_eq!(error.code, CODE_CAPABILITY_MISMATCH);
        assert_eq!(error.available_policy_refs.len(), 1);
        assert_eq!(
            error.available_policy_refs[0].config.as_deref(),
            Some("luna_low")
        );
        assert_eq!(
            error.requested_policy_ref.as_ref().unwrap()["config"],
            "luna_med"
        );
    }

    #[test]
    fn container_trace_required_request_fails_when_capture_is_unsupported() {
        let record = container(READY_STATUS, hydrated(Some(&normalized_info()), now()));
        let request = PreflightRequest::for_prepare(None, true, Duration::from_secs(900));
        let error = preflight_prepare(&record, &request, now()).unwrap_err();
        assert_eq!(error.code, CODE_CAPABILITY_MISMATCH);
        assert_eq!(error.missing, vec!["trace_v5.capture".to_string()]);
        assert!(error.unknown.is_empty());
    }

    #[test]
    fn container_cached_info_keeps_the_original_observation_time() {
        let observed = now() - ChronoDuration::seconds(200);
        let mut metadata = Map::new();
        write_capability_metadata(
            &mut metadata,
            Some(&normalized_info()),
            None,
            true,
            observed,
        );
        write_capability_metadata(&mut metadata, Some(&normalized_info()), None, false, now());
        let capabilities = ContainerCapabilities::from_metadata(&Value::Object(metadata.clone()));
        assert_eq!(
            capabilities.observed_at.as_deref(),
            Some(observed.to_rfc3339().as_str())
        );
        assert_eq!(
            metadata[HEALTH_CHECKED_AT_KEY].as_str(),
            Some(now().to_rfc3339().as_str())
        );
    }

    #[test]
    fn container_requested_policy_ref_rejects_an_unusable_argument() {
        assert!(requested_policy_ref(None).unwrap().is_none());
        assert!(requested_policy_ref(Some(&json!({"config": "luna_low"}))).is_err());
        let parsed = requested_policy_ref(Some(&json!({"harness": "react", "config": "luna_low"})))
            .unwrap()
            .unwrap();
        assert_eq!(parsed.harness, "react");
    }

    #[test]
    fn container_preflight_error_serializes_the_documented_shape() {
        let record = container("unhealthy", hydrated(Some(&normalized_info()), now()));
        let error = preflight_prepare(&record, &prepare_request(), now()).unwrap_err();
        let body = error.to_json();
        assert_eq!(body["code"], CODE_UNHEALTHY);
        assert_eq!(body["container_id"], "ctr_test");
        assert_eq!(body["base_url"], "http://127.0.0.1:8104");
        assert_eq!(body["retryable"], true);
        assert!(body["remediation"].as_str().is_some());
        assert!(error.summary().contains(CODE_UNHEALTHY));
    }

    fn healthbench_info() -> Value {
        json!({
            "runtime_family": "healthbench",
            "evaluation_plan_ref": "healthbench_eval.v1",
            "policy_refs": [{
                "harness": "chat_completion",
                "config": "groq_llama31_8b",
                "provider": "groq",
                "model": "llama-3.1-8b-instant"
            }],
            "capabilities": {
                "contract_version": "container_contract.v1",
                "rollout_modes": ["blocking"],
                "metadata": {"policy_ready": true}
            },
            "metadata": {
                "model_roles": {
                    "policy": {
                        "purpose": "generate_candidate_response",
                        "configuration_authority": "policy_ref",
                        "usage_lane": "policy",
                        "required": true
                    },
                    "scorer": {
                        "purpose": "score_response_against_physician_rubrics",
                        "provider": "openai",
                        "model": "gpt-4.1-2025-04-14",
                        "api_key_env": "OPENAI_API_KEY",
                        "usage_lane": "grader",
                        "canonical": true,
                        "required": true
                    }
                },
                "optimizer_contracts": {
                    "gepa": {
                        "version": GEPA_V2_CONTRACT,
                        "program_route": "/program",
                        "taskset_route": "/taskset",
                        "rollout_route": "/rollout",
                        "trace_route": "/rollouts/{rollout_id}/events"
                    }
                }
            }
        })
    }

    fn banking77_info() -> Value {
        json!({
            "runtime_family": "banking77",
            "evaluation_plan_ref": "banking77_eval.v1",
            "policy_refs": [
                {"harness": "dataset_gold", "config": "dataset_gold"},
                {"harness": "classify", "config": "classify"}
            ],
            "optimizer_contracts": {
                "gepa": {
                    "version": GEPA_V2_CONTRACT,
                    "program_route": "/program",
                    "taskset_route": "/taskset",
                    "rollout_route": "/rollouts",
                    "prepare_route": "/rollouts/prepare",
                    "trace_route": "/rollouts/{rollout_id}/events"
                }
            }
        })
    }

    #[test]
    fn gepa_v2_healthbench_projects_live_eval_operations_without_fabricating_frames() {
        let capabilities = project_capabilities(Some(&healthbench_info()), None, now());
        assert_eq!(capabilities.source, CapabilitySource::Compatibility);
        assert_eq!(capabilities.protocol.as_deref(), Some(GEPA_V2_CONTRACT));
        assert_eq!(
            capabilities.state(ContainerOperation::RolloutsPrepare),
            CapabilityState::Supported
        );
        assert_eq!(
            capabilities.state(ContainerOperation::RewardGet),
            CapabilityState::Supported
        );
        assert_eq!(
            capabilities.state(ContainerOperation::TraceV5Capture),
            CapabilityState::Unsupported
        );
        assert_eq!(
            capabilities.operations.get(FRAMES_REPLAY).copied(),
            Some(CapabilityState::Unsupported)
        );
        assert_eq!(
            capabilities.operations.get(USAGE_GET).copied(),
            Some(CapabilityState::Supported)
        );
        assert!(capabilities.policy_role_configurable);
        assert_eq!(
            capabilities.scorer_role.as_ref().unwrap()["model"],
            "gpt-4.1-2025-04-14"
        );
        assert!(capabilities
            .credential_requirements
            .iter()
            .any(|requirement| requirement["variable"] == "OPENAI_API_KEY"));
        assert!(capabilities.revision.is_some());
        let record = container(READY_STATUS, hydrated(Some(&healthbench_info()), now()));
        let request = PreflightRequest::for_prepare(
            Some(PolicyRef {
                harness: "chat_completion".into(),
                provider: Some("openai".into()),
                model: Some("gpt-4.1-mini-2025-04-14".into()),
                ..Default::default()
            }),
            false,
            Duration::from_secs(900),
        );
        preflight_prepare(&record, &request, now()).expect("configurable OpenAI policy is accepted");
        assert_eq!(
            capabilities.scorer_role.as_ref().unwrap()["usage_lane"],
            "grader"
        );
        assert_eq!(
            healthbench_info()["metadata"]["model_roles"]["policy"]["usage_lane"],
            "policy"
        );
        assert_ne!(
            capabilities.scorer_role.as_ref().unwrap()["usage_lane"],
            healthbench_info()["metadata"]["model_roles"]["policy"]["usage_lane"]
        );
    }

    #[test]
    fn gepa_v2_banking77_projects_prepare_workflow() {
        let capabilities = project_capabilities(Some(&banking77_info()), None, now());
        assert_eq!(
            capabilities.state(ContainerOperation::RolloutsPrepare),
            CapabilityState::Supported
        );
        assert_eq!(
            capabilities.state(ContainerOperation::RolloutsPoll),
            CapabilityState::Supported
        );
        let record = container(READY_STATUS, hydrated(Some(&banking77_info()), now()));
        preflight_prepare(&record, &prepare_request(), now()).expect("banking77 prepare passes");
        assert_ne!(
            capabilities.revision,
            project_capabilities(Some(&healthbench_info()), None, now()).revision
        );
    }

    #[test]
    fn probe_replaces_capability_revision_used_by_prepare() {
        let stale = now() - ChronoDuration::seconds(1_800);
        let mut metadata = Map::new();
        write_capability_metadata(&mut metadata, Some(&banking77_info()), None, true, stale);
        let stale_revision = ContainerCapabilities::from_metadata(&Value::Object(metadata.clone()))
            .revision
            .clone();
        let error = preflight_prepare(
            &container(READY_STATUS, Value::Object(metadata.clone())),
            &prepare_request(),
            now(),
        )
        .unwrap_err();
        assert_eq!(error.code, CODE_CAPABILITIES_STALE);
        assert_eq!(error.capability_revision, stale_revision);

        write_capability_metadata(&mut metadata, Some(&banking77_info()), None, true, now());
        let fresh = ContainerCapabilities::from_metadata(&Value::Object(metadata.clone()));
        assert_ne!(fresh.observed_at, Some(stale.to_rfc3339()));
        preflight_prepare(
            &container(READY_STATUS, Value::Object(metadata)),
            &prepare_request(),
            now(),
        )
        .expect("fresh probe observation is usable without restart");
    }
}
