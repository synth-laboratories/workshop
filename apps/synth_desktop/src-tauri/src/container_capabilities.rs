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

/// A normalized block is one that names the protocol or carries at least one
/// exact normalized operation name. A raw container may independently expose
/// an `operations` object such as `prepare`/`start`/`get`; that is useful
/// service metadata, but it is not permission to bypass the typed live-eval
/// gate or to prevent an explicit GEPA-v2 adapter from being projected.
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
        if names_protocol || has_normalized_operations(block) {
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

fn has_normalized_operations(block: &Value) -> bool {
    block
        .get("operations")
        .and_then(Value::as_object)
        .is_some_and(|operations| {
            operations
                .keys()
                .any(|name| ContainerOperation::parse(name).is_some())
        })
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
        target.operations.entry(name.clone()).or_insert(*state);
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
                if !capabilities
                    .policy_refs
                    .iter()
                    .any(|existing| existing.satisfies(&next))
                {
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

fn push_credential_requirement(capabilities: &mut ContainerCapabilities, role: &Value, lane: &str) {
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

