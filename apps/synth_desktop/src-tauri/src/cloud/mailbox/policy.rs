//! Participant access presets, handler limits and the restricted tool gate.
//!
//! A message is data, never authority. Nothing a message says can widen what
//! the explicit participant grant allows: the gate below is the enforcement
//! point for every file, artifact, tool, spend or side effect a mailbox
//! handler attempts, and it refuses access expansion, peer invitations,
//! unrelated work, spending and deployment unconditionally.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// Access preset chosen when the local session is connected (spec §8).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// Read status/evidence; never publish or schedule work.
    Observe,
    /// Publish questions/answers under the session's existing authority.
    /// Requests wait for an operator answer; nothing runs automatically.
    Collaborate,
    /// Bounded automatic handlers with explicit tools/data and cost policy.
    Respond,
}

impl Preset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Collaborate => "collaborate",
            Self::Respond => "respond",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "observe" => Self::Observe,
            "collaborate" => Self::Collaborate,
            "respond" => Self::Respond,
            _ => bail!("unknown participant preset"),
        })
    }
    pub fn may_publish(self) -> bool {
        !matches!(self, Self::Observe)
    }
    pub fn automatic_handlers(self) -> bool {
        matches!(self, Self::Respond)
    }
}

/// Bounds for automatic handling. Every field has a hard ceiling; a policy
/// outside them is refused at connection time rather than clamped.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HandlerLimits {
    pub max_concurrent: u32,
    pub max_per_minute: u32,
    pub deadline_secs: u32,
    /// Per-message cost ceiling. Zero means no paid work may run.
    pub max_cost_usd_micros: u64,
    /// Automatic replies per causal chain (hop count). Loop guard.
    pub max_causal_depth: u32,
}

impl Default for HandlerLimits {
    fn default() -> Self {
        Self {
            max_concurrent: 1,
            max_per_minute: 6,
            deadline_secs: 600,
            max_cost_usd_micros: 0,
            max_causal_depth: 2,
        }
    }
}

pub const MAX_CONCURRENT: u32 = 4;
pub const MAX_PER_MINUTE: u32 = 30;
pub const MAX_DEADLINE_SECS: u32 = 3600;
pub const MAX_COST_USD_MICROS: u64 = 5_000_000;
pub const MAX_CAUSAL_DEPTH: u32 = 8;

impl HandlerLimits {
    pub fn validate(&self) -> Result<()> {
        if !(1..=MAX_CONCURRENT).contains(&self.max_concurrent)
            || !(1..=MAX_PER_MINUTE).contains(&self.max_per_minute)
            || !(30..=MAX_DEADLINE_SECS).contains(&self.deadline_secs)
            || self.max_cost_usd_micros > MAX_COST_USD_MICROS
            || !(1..=MAX_CAUSAL_DEPTH).contains(&self.max_causal_depth)
        {
            bail!("handler limits are outside the supported bounds");
        }
        Ok(())
    }
}

/// The only tools a restricted mailbox turn can be granted. Shell, file
/// writes, network, deployment, spawning and grant administration are not in
/// the catalog, so no policy can name them.
pub const RESTRICTED_TOOL_CATALOG: &[&str] =
    &["read_allowed_file", "read_allowed_artifact", "mailbox_status"];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParticipantPolicy {
    #[serde(default)]
    pub allowed_tools: BTreeSet<String>,
    /// Absolute directory or file prefixes, compared after canonicalization.
    #[serde(default)]
    pub allowed_files: Vec<String>,
    #[serde(default)]
    pub allowed_artifacts: BTreeSet<String>,
    #[serde(default)]
    pub limits: HandlerLimits,
}

impl ParticipantPolicy {
    pub fn validate(&self, preset: Preset) -> Result<()> {
        self.limits.validate()?;
        if self.allowed_tools.len() > RESTRICTED_TOOL_CATALOG.len()
            || self
                .allowed_tools
                .iter()
                .any(|tool| !RESTRICTED_TOOL_CATALOG.contains(&tool.as_str()))
        {
            bail!("policy names a tool outside the restricted catalog");
        }
        if self.allowed_files.len() > 32 || self.allowed_artifacts.len() > 64 {
            bail!("policy data boundary is too large");
        }
        for path in &self.allowed_files {
            let path = Path::new(path);
            if !path.is_absolute()
                || path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
            {
                bail!("allowed files must be normalized absolute paths");
            }
        }
        if self
            .allowed_artifacts
            .iter()
            .any(|id| id.is_empty() || id.len() > 256 || id.chars().any(char::is_control))
        {
            bail!("invalid artifact identifier");
        }
        if preset == Preset::Observe
            && (!self.allowed_tools.is_empty()
                || !self.allowed_files.is_empty()
                || !self.allowed_artifacts.is_empty()
                || self.limits.max_cost_usd_micros != 0)
        {
            bail!("observe-only participants cannot hold tools, data or budget");
        }
        Ok(())
    }
}

/// Every side effect a mailbox handler can attempt, as seen by the gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolRequest {
    Tool { name: String },
    ReadFile { path: PathBuf },
    WriteFile { path: PathBuf },
    Artifact { id: String },
    Spend { usd_micros: u64 },
    Deploy,
    InvitePeer,
    ExpandAccess,
    SpawnWork,
}

impl ToolRequest {
    fn label(&self) -> String {
        match self {
            Self::Tool { name } => format!("tool:{name}"),
            Self::ReadFile { .. } => "read_file".into(),
            Self::WriteFile { .. } => "write_file".into(),
            Self::Artifact { id } => format!("artifact:{id}"),
            Self::Spend { usd_micros } => format!("spend:{usd_micros}"),
            Self::Deploy => "deploy".into(),
            Self::InvitePeer => "invite_peer".into(),
            Self::ExpandAccess => "expand_access".into(),
            Self::SpawnWork => "spawn_work".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GateDecision {
    pub request: String,
    pub allowed: bool,
    pub reason: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
pub struct GateRefusal(pub &'static str);
impl std::fmt::Display for GateRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "restricted mailbox gate refused: {}", self.0)
    }
}
impl std::error::Error for GateRefusal {}

/// The tool proxy for one restricted turn. The executor receives only this
/// gate; it holds no Synth key, MQ credential or general session authority.
pub struct ToolGate {
    policy: ParticipantPolicy,
    roots: Vec<PathBuf>,
    remaining_cost: Mutex<u64>,
    audit: Mutex<Vec<GateDecision>>,
}

impl ToolGate {
    /// `cost_cap` is already the minimum of the policy ceiling and the
    /// session's existing work authorization.
    pub fn new(policy: ParticipantPolicy, cost_cap: u64) -> Self {
        // Roots that do not canonicalize are dropped: fail closed.
        let roots = policy
            .allowed_files
            .iter()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .collect();
        Self {
            policy,
            roots,
            remaining_cost: Mutex::new(cost_cap),
            audit: Mutex::new(Vec::new()),
        }
    }

    pub fn authorize(&self, request: &ToolRequest) -> std::result::Result<(), GateRefusal> {
        let verdict = self.decide(request);
        if let Ok(mut audit) = self.audit.lock() {
            audit.push(GateDecision {
                request: request.label(),
                allowed: verdict.is_ok(),
                reason: match &verdict {
                    Ok(()) => "allowed",
                    Err(refusal) => refusal.0,
                },
            });
        }
        verdict
    }

    fn decide(&self, request: &ToolRequest) -> std::result::Result<(), GateRefusal> {
        match request {
            ToolRequest::Tool { name } => {
                if self.policy.allowed_tools.contains(name) {
                    Ok(())
                } else {
                    Err(GateRefusal("tool_not_granted"))
                }
            }
            ToolRequest::ReadFile { path } => {
                if !self.policy.allowed_tools.contains("read_allowed_file") {
                    return Err(GateRefusal("tool_not_granted"));
                }
                let resolved = std::fs::canonicalize(path).map_err(|_| GateRefusal("file_unresolvable"))?;
                if self.roots.iter().any(|root| resolved.starts_with(root)) {
                    Ok(())
                } else {
                    Err(GateRefusal("file_outside_grant"))
                }
            }
            ToolRequest::WriteFile { .. } => Err(GateRefusal("writes_not_permitted")),
            ToolRequest::Artifact { id } => {
                if self.policy.allowed_tools.contains("read_allowed_artifact")
                    && self.policy.allowed_artifacts.contains(id)
                {
                    Ok(())
                } else {
                    Err(GateRefusal("artifact_outside_grant"))
                }
            }
            ToolRequest::Spend { usd_micros } => {
                let mut remaining = self
                    .remaining_cost
                    .lock()
                    .map_err(|_| GateRefusal("budget_unavailable"))?;
                if *usd_micros > *remaining {
                    return Err(GateRefusal("exceeds_work_authorization"));
                }
                *remaining -= *usd_micros;
                Ok(())
            }
            ToolRequest::Deploy => Err(GateRefusal("message_cannot_authorize_deploy")),
            ToolRequest::InvitePeer => Err(GateRefusal("message_cannot_invite_peers")),
            ToolRequest::ExpandAccess => Err(GateRefusal("message_cannot_expand_access")),
            ToolRequest::SpawnWork => Err(GateRefusal("message_cannot_spawn_work")),
        }
    }

    pub fn remaining_cost(&self) -> u64 {
        self.remaining_cost.lock().map(|value| *value).unwrap_or(0)
    }

    pub fn audit(&self) -> Vec<GateDecision> {
        self.audit.lock().map(|audit| audit.clone()).unwrap_or_default()
    }

    /// Host-side bounded read through the gate.
    pub fn read_allowed_file(&self, path: &Path, max_bytes: usize) -> Result<String> {
        self.authorize(&ToolRequest::ReadFile { path: path.to_owned() })?;
        let bytes = std::fs::read(path)?;
        let bytes = &bytes[..bytes.len().min(max_bytes)];
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// Actions a message may name; anything else requested is refused. This is
/// an early decline for well-formed requests. The gate still refuses the
/// same effects if a handler attempts them anyway.
const PERMITTED_REQUEST_ACTIONS: &[&str] = &["answer", "status", "summarize", "review"];

/// Map a message's declared requested actions onto refusal reasons.
pub fn refused_requested_actions(payload: &Value) -> Vec<&'static str> {
    let mut requested: Vec<&str> = Vec::new();
    if let Some(items) = payload.get("requested_actions").and_then(Value::as_array) {
        requested.extend(items.iter().filter_map(Value::as_str));
    }
    if let Some(action) = payload.get("action").and_then(Value::as_str) {
        requested.push(action);
    }
    let mut refused: Vec<&'static str> = requested
        .into_iter()
        .filter(|action| !PERMITTED_REQUEST_ACTIONS.contains(action))
        .map(|action| match action {
            "deploy" | "deployment" | "release" => "message_cannot_authorize_deploy",
            "spend" | "budget" | "paid_compute" | "purchase" => "message_cannot_authorize_spend",
            "invite" | "add_participant" | "add_peer" => "message_cannot_invite_peers",
            "grant" | "expand_access" | "share" | "upload" => "message_cannot_expand_access",
            "spawn" | "start_run" | "launch" | "run" => "message_cannot_spawn_work",
            _ => "unsupported_requested_action",
        })
        .collect();
    refused.sort_unstable();
    refused.dedup();
    refused
}

/// How an inbound message is handled. Only `WorkRequest` can ever reach a
/// restricted executor; every other intent is handled without a model call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundIntent {
    Heartbeat,
    Notice,
    StatusRequest,
    Answer,
    WorkRequest,
}

pub fn classify(message: &mq_core::Message) -> InboundIntent {
    use mq_core::MessageKind;
    match message.kind {
        MessageKind::HandoffPing | MessageKind::ActorRuntime => InboundIntent::Heartbeat,
        MessageKind::Notice => InboundIntent::Notice,
        MessageKind::Answer => InboundIntent::Answer,
        MessageKind::Ask
            if message.payload.get("request").and_then(Value::as_str) == Some("status") =>
        {
            InboundIntent::StatusRequest
        }
        MessageKind::Ask | MessageKind::Steer | MessageKind::Blocker => InboundIntent::WorkRequest,
    }
}

/// Hop metadata carried in `payload.synth.hop`, bounded.
pub fn message_hop(payload: &Value) -> u32 {
    payload
        .pointer("/synth/hop")
        .and_then(Value::as_u64)
        .map(|hop| hop.min(64) as u32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy(dir: &Path) -> ParticipantPolicy {
        ParticipantPolicy {
            allowed_tools: ["read_allowed_file".to_owned(), "read_allowed_artifact".to_owned()].into(),
            allowed_files: vec![dir.join("shared").display().to_string()],
            allowed_artifacts: ["trace-18".to_owned()].into(),
            limits: HandlerLimits { max_cost_usd_micros: 1_000, ..HandlerLimits::default() },
        }
    }

    #[test]
    fn gate_refuses_everything_outside_the_explicit_grant() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("shared")).unwrap();
        std::fs::write(dir.path().join("shared/notes.md"), "allowed").unwrap();
        std::fs::write(dir.path().join("private.md"), "secret").unwrap();
        // A symlink inside the grant pointing outside it must not escape.
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("private.md"), dir.path().join("shared/escape.md")).unwrap();
        let gate = ToolGate::new(policy(dir.path()), 600);
        assert_eq!(gate.read_allowed_file(&dir.path().join("shared/notes.md"), 64).unwrap(), "allowed");
        assert!(gate.read_allowed_file(&dir.path().join("private.md"), 64).is_err());
        #[cfg(unix)]
        assert_eq!(
            gate.authorize(&ToolRequest::ReadFile { path: dir.path().join("shared/escape.md") }),
            Err(GateRefusal("file_outside_grant"))
        );
        assert!(gate
            .authorize(&ToolRequest::ReadFile { path: dir.path().join("shared/../private.md") })
            .is_err());
        assert_eq!(
            gate.authorize(&ToolRequest::WriteFile { path: dir.path().join("shared/notes.md") }),
            Err(GateRefusal("writes_not_permitted"))
        );
        assert!(gate.authorize(&ToolRequest::Artifact { id: "trace-18".into() }).is_ok());
        assert!(gate.authorize(&ToolRequest::Artifact { id: "trace-19".into() }).is_err());
        assert!(gate.authorize(&ToolRequest::Tool { name: "shell".into() }).is_err());
        for forbidden in [ToolRequest::Deploy, ToolRequest::InvitePeer, ToolRequest::ExpandAccess, ToolRequest::SpawnWork] {
            assert!(gate.authorize(&forbidden).is_err());
        }
        assert!(gate.authorize(&ToolRequest::Spend { usd_micros: 500 }).is_ok());
        assert_eq!(
            gate.authorize(&ToolRequest::Spend { usd_micros: 101 }),
            Err(GateRefusal("exceeds_work_authorization"))
        );
        assert_eq!(gate.remaining_cost(), 100);
        let audit = gate.audit();
        assert!(audit.iter().any(|decision| decision.request == "deploy" && !decision.allowed));
        assert!(audit.iter().all(|decision| !decision.request.contains("secret")));
    }

    #[test]
    fn policies_cannot_name_tools_outside_the_catalog_or_exceed_bounds() {
        let mut shell = ParticipantPolicy::default();
        shell.allowed_tools.insert("shell".into());
        assert!(shell.validate(Preset::Respond).is_err());
        let mut relative = ParticipantPolicy::default();
        relative.allowed_files.push("../home".into());
        assert!(relative.validate(Preset::Respond).is_err());
        let mut expensive = ParticipantPolicy::default();
        expensive.limits.max_cost_usd_micros = MAX_COST_USD_MICROS + 1;
        assert!(expensive.validate(Preset::Respond).is_err());
        let mut observer = ParticipantPolicy::default();
        observer.allowed_tools.insert("mailbox_status".into());
        assert!(observer.validate(Preset::Observe).is_err());
        assert!(ParticipantPolicy::default().validate(Preset::Observe).is_ok());
        assert!(serde_json::from_value::<ParticipantPolicy>(json!({"allowed_tools":[],"extra":true})).is_err());
    }

    #[test]
    fn requested_expansions_are_always_refused() {
        let refused = refused_requested_actions(&json!({
            "requested_actions": ["answer", "deploy", "spend", "invite", "grant", "spawn", "rm -rf"]
        }));
        assert_eq!(refused, vec![
            "message_cannot_authorize_deploy",
            "message_cannot_authorize_spend",
            "message_cannot_expand_access",
            "message_cannot_invite_peers",
            "message_cannot_spawn_work",
            "unsupported_requested_action",
        ]);
        assert!(refused_requested_actions(&json!({"requested_actions":["answer","status"]})).is_empty());
        assert_eq!(message_hop(&json!({"synth":{"hop":3}})), 3);
        assert_eq!(message_hop(&json!({"synth":{"hop":100000}})), 64);
    }
}
