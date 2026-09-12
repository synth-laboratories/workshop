use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadId(pub Uuid);

impl ThreadId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ThreadId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(pub Uuid);

impl MessageId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Human,
    InternSync,
    InternAsync,
    Actor,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Principal {
    pub kind: PrincipalKind,
    pub id: String,
    pub org_id: String,
}

/// Soft labels for list/filter only — never messaging ontology.
/// `run` is intentionally absent: SMR owns `run_id → thread_id` outside MQ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Org,
    Factory,
    Effort,
    Project,
    SyncSession,
    AsyncRuntime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeBinding {
    pub kind: ScopeKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cap {
    Read,
    Publish,
    Invite,
    Close,
}

/// Fixed thread roles. Caps are derived from role at write time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Moderator,
    Member,
    Agent,
    Observer,
    /// Retained membership identity with no authority; reactivation is explicit.
    Revoked,
}

impl Role {
    pub fn caps(self) -> Vec<Cap> {
        match self {
            Role::Owner => vec![Cap::Read, Cap::Publish, Cap::Invite, Cap::Close],
            Role::Moderator => vec![Cap::Read, Cap::Publish, Cap::Invite],
            Role::Member | Role::Agent => vec![Cap::Read, Cap::Publish],
            Role::Observer => vec![Cap::Read],
            Role::Revoked => vec![],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Moderator => "moderator",
            Role::Member => "member",
            Role::Agent => "agent",
            Role::Observer => "observer",
            Role::Revoked => "revoked",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Role::Owner),
            "moderator" => Some(Role::Moderator),
            "member" => Some(Role::Member),
            "agent" => Some(Role::Agent),
            "observer" => Some(Role::Observer),
            "revoked" => Some(Role::Revoked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Ask,
    Answer,
    Steer,
    Notice,
    ActorRuntime,
    Blocker,
    HandoffPing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub thread_id: ThreadId,
    pub org_id: String,
    pub scope: ScopeBinding,
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participant {
    pub principal: Principal,
    pub role: Role,
    /// Derived from [`Role::caps`]; stored for enforcement / display.
    #[serde(default)]
    pub caps: Vec<Cap>,
}

impl Participant {
    pub fn new(principal: Principal, role: Role) -> Self {
        Self {
            principal,
            role,
            caps: role.caps(),
        }
    }

    pub fn normalize(mut self) -> Self {
        self.caps = self.role.caps();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub message_id: MessageId,
    pub thread_id: ThreadId,
    pub seq: u64,
    pub kind: MessageKind,
    pub body: String,
    pub payload: serde_json::Value,
    pub sender: Principal,
    pub idempotency_key: Option<String>,
    pub correlation_id: Option<String>,
    pub parent_message_id: Option<MessageId>,
    pub causation_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateThread {
    pub org_id: String,
    pub scope: ScopeBinding,
    pub title: Option<String>,
    pub participants: Vec<Participant>,
    /// Org-scoped; retries return the same thread (ensure semantics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishMessage {
    pub kind: MessageKind,
    pub body: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub idempotency_key: Option<String>,
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    /// If non-empty, delivery jobs only for these members (directed steer).
    /// Empty = fan-out to all other Read members in the workspace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipients: Vec<Principal>,
}

impl Default for PublishMessage {
    fn default() -> Self {
        Self {
            kind: MessageKind::Notice,
            body: String::new(),
            payload: serde_json::Value::Null,
            idempotency_key: None,
            correlation_id: None,
            parent_message_id: None,
            causation_id: None,
            recipients: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeliveryJobId(pub Uuid);

impl DeliveryJobId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for DeliveryJobId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Dispatched,
    AwaitingPull,
    NotRoutable,
    Delivered,
    DeadLetter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryJob {
    pub job_id: DeliveryJobId,
    pub message_id: MessageId,
    pub thread_id: ThreadId,
    pub recipient: Principal,
    pub status: DeliveryStatus,
    pub attempts: u32,
    #[serde(default)]
    pub lease_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub next_attempt_at: Option<DateTime<Utc>>,
}

/// Immutable acceptance intent. See docs/DELIVERY_DURABILITY.md.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishAcceptance {
    pub message_id: MessageId,
    pub fingerprint: String,
    pub recipients: Vec<Principal>,
}

/// Canonical semantic request, excluding the key itself. Recipient order is not semantic.
pub fn publish_fingerprint(request: &PublishMessage) -> String {
    let mut request = request.clone();
    request.idempotency_key = None;
    if request.payload.is_null() {
        request.payload = serde_json::json!({});
    }
    request
        .recipients
        .sort_by_key(|p| serde_json::to_string(p).expect("principal serialization"));
    request.recipients.dedup();
    serde_json::to_string(&request).expect("publish serialization")
}

pub fn publish_key(thread: ThreadId, sender: &Principal, key: &str) -> (String, String) {
    (
        sender.org_id.clone(),
        serde_json::to_string(&(thread, sender, key)).expect("publish key serialization"),
    )
}

/// Retry delay, capped at five minutes. Worker attempt count also bounds retry count.
pub fn delivery_backoff_seconds(attempt: u32) -> i64 {
    2_i64.pow(attempt.saturating_sub(1).min(8)).min(300)
}
pub const DELIVERY_LEASE_SECONDS: i64 = 30;

/// Authorize a participant mutation against one locked membership snapshot.
pub fn validate_participant_change(
    org: &str,
    actor: &Principal,
    members: &[Participant],
    target: &Participant,
    create: bool,
) -> crate::Result<bool> {
    if actor.org_id != org {
        return Err(crate::Error::NotFound("thread"));
    }
    if target.principal.org_id != org {
        return Err(crate::Error::Forbidden("org_workspace_mismatch"));
    }
    let caller = members
        .iter()
        .find(|p| p.principal == *actor)
        .ok_or(crate::Error::Forbidden("membership_required"))?;
    if !caller.caps.contains(&Cap::Invite) {
        return Err(crate::Error::Forbidden("invite_required"));
    }
    if target.role == Role::Owner {
        return Err(crate::Error::Invalid("cannot_transfer_owner_via_set_role"));
    }
    let existing = members.iter().find(|p| p.principal == target.principal);
    if let Some(existing) = existing {
        if existing.role == Role::Owner {
            return Err(crate::Error::Forbidden("cannot_modify_owner"));
        }
        if caller.role != Role::Owner
            && (existing.role == Role::Moderator || target.role == Role::Moderator)
        {
            return Err(crate::Error::Forbidden("cannot_modify_peer_role"));
        }
        if create && existing.role != target.role {
            return Err(crate::Error::Conflict("participant_role_mismatch"));
        }
        return Ok(existing.role != target.role);
    }
    if !create {
        return Err(crate::Error::NotFound("participant"));
    }
    if caller.role != Role::Owner && target.role == Role::Moderator {
        return Err(crate::Error::Forbidden("cannot_grant_peer_role"));
    }
    Ok(true)
}
