//! Durable MQ participant binding, inbox delivery ladder and outbox.
//!
//! Contract: manderqueue `docs/WORKSHOP_GRANT_CONTRACT.md`. Every method
//! fences on the active scope lease. Native acceptance additionally fences on
//! the exact local session, enrollment incarnation and grant generation, and
//! queued writes fence on the account write fence and grant generation.
use super::*;
use crate::cloud::mailbox::policy::{ParticipantPolicy, Preset};
use mq_core::{Message, MessageKind};

pub const MQ_PUBLISH_OPERATION: &str = "mq.publish";
const MAX_PEERS: usize = 16;
const MAX_MQ_BODY: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerRef {
    pub kind: String,
    pub id: String,
    pub org_id: String,
}

/// Server-issued enrollment facts, already validated against the verified
/// identity by the caller (`cloud::mailbox::grant`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrollmentBinding {
    pub enrollment_id: String,
    pub device_id: String,
    pub incarnation: u64,
    pub principal_id: String,
    pub org_id: String,
    pub mq_endpoint: String,
}

#[derive(Clone, Debug)]
pub struct ParticipantSpec {
    pub thread_id: String,
    pub local_session_id: String,
    pub peers: Vec<PeerRef>,
    pub preset: Preset,
    pub policy: ParticipantPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantLifecycle {
    Active,
    Revoked,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantSnapshot {
    pub grant_id: String,
    pub thread_id: String,
    pub enrollment_id: String,
    pub principal_id: String,
    pub org_id: String,
    pub operations: Vec<String>,
    pub history_after_seq: u64,
    pub expires_at_ms: i64,
    pub incarnation: u64,
    pub generation: u64,
    pub lifecycle: GrantLifecycle,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantRecord {
    pub thread_id: String,
    pub local_session_id: String,
    pub enrollment_id: String,
    pub device_id: String,
    pub incarnation: u64,
    pub principal_id: String,
    pub org_id: String,
    pub mq_endpoint: String,
    pub peers: Vec<PeerRef>,
    pub preset: Preset,
    pub policy: ParticipantPolicy,
    pub grant_id: Option<String>,
    pub grant_generation: Option<u64>,
    pub grant_operations: Vec<String>,
    pub history_after_seq: Option<u64>,
    pub grant_expires_ms: Option<i64>,
    pub state: String,
    pub state_reason: Option<String>,
}

impl ParticipantRecord {
    pub fn can_publish(&self) -> bool {
        self.preset.may_publish() && self.grant_operations.iter().any(|op| op == "publish")
    }
    pub fn subscription_id(&self) -> String {
        format!("enrollment:{}", self.enrollment_id)
    }
}

/// Authorized history skip `(after_seq, through_seq]` (contract §8).
pub type MqSkipped = mq_core::HistorySkip;

/// `GET /v1/threads/{id}/history` page, as typed by the vendored SDK.
pub type MqHistoryPage = mq_core::HistoryPage;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MqHistoryCommit {
    pub committed: usize,
    pub own_reconciled: Vec<String>,
    pub answered_outbound: Vec<String>,
    pub gap: Option<(u64, u64)>,
    pub cursor: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundDisposition {
    Message,
    Answer,
    Decline,
    Expiry,
}
impl OutboundDisposition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Answer => "answer",
            Self::Decline => "decline",
            Self::Expiry => "expiry",
        }
    }
}

#[derive(Clone, Debug)]
pub struct OutboundDraft {
    /// Caller-stable identity. The MQ idempotency key and local command id
    /// derive from it, so a retry or crash recovery reuses the original.
    pub local_message_id: String,
    pub kind: MessageKind,
    pub body: String,
    pub payload: Value,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub parent_message_id: Option<String>,
    pub recipients: Vec<PeerRef>,
    pub disposition: OutboundDisposition,
    pub reply_to_message_id: Option<String>,
    pub causal_depth: u32,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MqOutboxView {
    pub command_id: String,
    pub thread_id: String,
    pub idempotency_key: String,
    pub kind: String,
    pub disposition: String,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub parent_message_id: Option<String>,
    pub reply_to_message_id: Option<String>,
    pub causal_depth: u32,
    /// queued | unknown | accepted | answered | refused | conflict | fenced
    pub status: String,
    pub delivery_state: String,
    pub fenced_reason: Option<String>,
    pub grant_generation: u64,
    pub mq_message_id: Option<String>,
    pub mq_seq: Option<u64>,
    pub answered_by_message_id: Option<String>,
    pub lookup: Option<Value>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MqDeliveryView {
    pub message_id: String,
    pub sequence: u64,
    pub stage: String,
    pub correlation_id: Option<String>,
    pub causal_depth: u32,
    pub deadline_ms: Option<i64>,
    pub delivered_generation: Option<u64>,
    pub delivered_incarnation: Option<u64>,
    pub reply_command_id: Option<String>,
    pub disposition: Option<Value>,
    /// The stored inbound message, for local status views only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
}

/// Exact authority a native consumer must still hold at acceptance time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryFence {
    pub local_session_id: String,
    pub incarnation: u64,
    pub grant_generation: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DeliveryAdmission {
    Observed(MqDeliveryView),
    /// Delivered under an older grant generation; never executed.
    Fenced(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActingAdmission {
    Admitted,
    Refused(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliverySettlement {
    Answered,
    Declined,
    Expired,
}
impl DeliverySettlement {
    fn stage(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Declined => "declined",
            Self::Expired => "expired",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MqSendRequest {
    pub command_id: String,
    pub publish: mq_core::PublishMessage,
    pub grant_generation: u64,
}

#[derive(Clone, Debug)]
pub enum SendAdmission {
    Send(MqSendRequest),
    /// Permanently fenced; visible, never flushed.
    Fenced(String),
    /// Still queued; authority is temporarily unavailable (e.g. expired grant).
    Deferred(String),
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

struct RawParticipant {
    thread_id: String,
    local_session_id: String,
    enrollment_id: String,
    device_id: String,
    incarnation: i64,
    principal_id: String,
    org_id: String,
    mq_endpoint: String,
    peers_json: String,
    preset: String,
    policy_json: String,
    grant_id: Option<String>,
    grant_generation: Option<i64>,
    grant_operations: Option<String>,
    history_after_seq: Option<i64>,
    grant_expires_ms: Option<i64>,
    state: String,
    state_reason: Option<String>,
}

fn participant_conn(conn: &Connection, lease: &ScopeLease, thread_id: &str) -> Result<Option<ParticipantRecord>> {
    let raw = conn.query_row(
        "SELECT thread_id,local_session_id,enrollment_id,device_id,incarnation,principal_id,org_id,mq_endpoint,peers_json,preset,policy_json,grant_id,grant_generation,grant_operations,history_after_seq,grant_expires_ms,state,state_reason FROM cloud_mq_participants WHERE scope_id=?1 AND thread_id=?2",
        params![lease.scope_id, thread_id],
        |r| Ok(RawParticipant {
            thread_id: r.get(0)?, local_session_id: r.get(1)?, enrollment_id: r.get(2)?, device_id: r.get(3)?,
            incarnation: r.get(4)?, principal_id: r.get(5)?, org_id: r.get(6)?, mq_endpoint: r.get(7)?,
            peers_json: r.get(8)?, preset: r.get(9)?, policy_json: r.get(10)?, grant_id: r.get(11)?,
            grant_generation: r.get(12)?, grant_operations: r.get(13)?, history_after_seq: r.get(14)?,
            grant_expires_ms: r.get(15)?, state: r.get(16)?, state_reason: r.get(17)?,
        }),
    ).optional()?;
    raw.map(|raw| {
        Ok(ParticipantRecord {
            thread_id: raw.thread_id,
            local_session_id: raw.local_session_id,
            enrollment_id: raw.enrollment_id,
            device_id: raw.device_id,
            incarnation: u64::try_from(raw.incarnation)?,
            principal_id: raw.principal_id,
            org_id: raw.org_id,
            mq_endpoint: raw.mq_endpoint,
            peers: serde_json::from_str(&raw.peers_json)?,
            preset: Preset::parse(&raw.preset)?,
            policy: serde_json::from_str(&raw.policy_json)?,
            grant_id: raw.grant_id,
            grant_generation: raw.grant_generation.map(u64::try_from).transpose()?,
            grant_operations: raw
                .grant_operations
                .map(|ops| ops.split(',').filter(|op| !op.is_empty()).map(str::to_owned).collect())
                .unwrap_or_default(),
            history_after_seq: raw.history_after_seq.map(u64::try_from).transpose()?,
            grant_expires_ms: raw.grant_expires_ms,
            state: raw.state,
            state_reason: raw.state_reason,
        })
    })
    .transpose()
}

fn require_participant(conn: &Connection, lease: &ScopeLease, thread_id: &str) -> Result<ParticipantRecord> {
    participant_conn(conn, lease, thread_id)?.context("MQ thread has no connected local participant")
}

fn scope_fence_conn(conn: &Connection, lease: &ScopeLease) -> Result<i64> {
    conn.execute("INSERT OR IGNORE INTO cloud_mq_scope_fences(scope_id,fence) VALUES(?1,0)", params![lease.scope_id])?;
    Ok(conn.query_row("SELECT fence FROM cloud_mq_scope_fences WHERE scope_id=?1", params![lease.scope_id], |r| r.get(0))?)
}

fn scope_org(conn: &Connection, lease: &ScopeLease) -> Result<String> {
    Ok(conn.query_row("SELECT org_id FROM cloud_scopes WHERE id=?1", params![lease.scope_id], |r| r.get(0))?)
}

fn validate_mq_origin(endpoint: &str) -> Result<()> {
    let url = reqwest::Url::parse(endpoint)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || url.origin().ascii_serialization() != endpoint
    {
        bail!("MQ endpoint must be a canonical origin");
    }
    Ok(())
}

fn mq_stream(thread_id: &str) -> Stream {
    Stream { adapter: Adapter::Mq, external_id: thread_id.to_owned() }
}

/// Locally fence pending writes chosen by `filter` (a SQL predicate over
/// cloud_mq_outbox aliased `o`). They stay visible as refused/fenced.
fn fence_pending_writes(conn: &Connection, lease: &ScopeLease, thread_id: &str, filter: &str, generation: Option<i64>, reason: &str) -> Result<usize> {
    let sql = format!(
        "SELECT c.command_id,c.local_command_id FROM cloud_command_outbox c JOIN cloud_mq_outbox o ON o.scope_id=c.scope_id AND o.command_id=c.command_id WHERE c.scope_id=?1 AND o.thread_id=?2 AND c.delivery_state='pending' AND ({filter})"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = match generation {
        Some(generation) => statement.query_map(params![lease.scope_id, thread_id, generation], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?,
        None => statement.query_map(params![lease.scope_id, thread_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?,
    };
    let receipt = json!({"localFence": reason});
    for (command_id, local_id) in &rows {
        conn.execute("UPDATE cloud_command_outbox SET delivery_state='refused',receipt_json=?1 WHERE scope_id=?2 AND command_id=?3 AND delivery_state='pending'", params![receipt.to_string(), lease.scope_id, command_id])?;
        conn.execute("UPDATE cloud_mq_outbox SET fenced_reason=?1 WHERE scope_id=?2 AND command_id=?3", params![reason, lease.scope_id, command_id])?;
        conn.execute("UPDATE command_receipts SET status='rejected',response_json=?1,updated_at=?2 WHERE command_id=?3", params![json!({"deliveryState":"refused","localFence":reason}).to_string(), chrono::Utc::now().to_rfc3339(), local_id])?;
    }
    Ok(rows.len())
}

fn fence_open_deliveries(conn: &Connection, lease: &ScopeLease, thread_id: &str, below_generation: Option<i64>, reason: &str) -> Result<usize> {
    let disposition = json!({"fence": reason}).to_string();
    Ok(match below_generation {
        Some(generation) => conn.execute(
            "UPDATE cloud_mq_pending_inputs SET stage='fenced',disposition_json=?1,settled_at=?2 WHERE scope_id=?3 AND external_id=?4 AND stage IN ('delivered','observed','acting') AND (delivered_generation IS NULL OR delivered_generation<?5)",
            params![disposition, chrono::Utc::now().to_rfc3339(), lease.scope_id, thread_id, generation],
        )?,
        None => conn.execute(
            "UPDATE cloud_mq_pending_inputs SET stage='fenced',disposition_json=?1,settled_at=?2 WHERE scope_id=?3 AND external_id=?4 AND stage IN ('delivered','observed','acting')",
            params![disposition, chrono::Utc::now().to_rfc3339(), lease.scope_id, thread_id],
        )?,
    })
}

fn principal_from_peer(peer: &PeerRef) -> Result<mq_core::Principal> {
    Ok(mq_core::Principal {
        kind: serde_json::from_value(json!(peer.kind)).context("unknown peer principal kind")?,
        id: peer.id.clone(),
        org_id: peer.org_id.clone(),
    })
}

fn kind_str(kind: MessageKind) -> Result<String> {
    Ok(serde_json::to_value(kind)?.as_str().context("message kind")?.to_owned())
}

fn enqueue_mq_conn(conn: &Connection, lease: &ScopeLease, participant: &ParticipantRecord, draft: &OutboundDraft) -> Result<MqOutboxView> {
    valid_id(&draft.local_message_id)?;
    if !matches!(participant.state.as_str(), "active" | "expired") {
        bail!("MQ participant cannot queue writes while {}", participant.state);
    }
    if !participant.can_publish() {
        bail!("participant grant does not allow publishing");
    }
    if draft.body.len() > MAX_MQ_BODY {
        bail!("MQ message body exceeds limit");
    }
    for recipient in &draft.recipients {
        if !participant.peers.contains(recipient) {
            bail!("recipients must be named thread participants");
        }
    }
    for id in [&draft.correlation_id, &draft.causation_id, &draft.reply_to_message_id].into_iter().flatten() {
        valid_id(id)?;
    }
    let generation = participant.grant_generation.context("participant grant is not attached")?;
    let command_id = format!("mq-publish:{}", draft.local_message_id);
    let idempotency_key = format!("workshop:{}", draft.local_message_id);
    let mut payload = match &draft.payload {
        Value::Null => json!({}),
        Value::Object(_) => draft.payload.clone(),
        _ => bail!("MQ payload must be an object"),
    };
    let synth = payload.as_object_mut().context("payload object")?.entry("synth").or_insert_with(|| json!({}));
    synth.as_object_mut().context("payload.synth must be an object")?.insert("hop".into(), json!(draft.causal_depth));
    let publish = mq_core::PublishMessage {
        // Both are server-side trusted ingress state, never on the wire.
        expected_grant_generation: None,
        grant_fence: None,
        kind: draft.kind,
        body: draft.body.clone(),
        payload,
        idempotency_key: Some(idempotency_key.clone()),
        correlation_id: draft.correlation_id.clone(),
        parent_message_id: draft.parent_message_id.as_deref().map(uuid::Uuid::parse_str).transpose().context("parent message id")?.map(mq_core::MessageId),
        causation_id: draft.causation_id.clone(),
        recipients: draft.recipients.iter().map(principal_from_peer).collect::<Result<_>>()?,
    };
    let envelope = serde_json::to_vec(&json!({
        "expected_generation": generation,
        "thread_id": participant.thread_id,
        "incarnation": participant.incarnation,
        "publish": publish,
    }))?;
    let existing: Option<String> = conn.query_row("SELECT c.body_sha256 FROM cloud_mq_outbox o JOIN cloud_command_outbox c ON c.scope_id=o.scope_id AND c.command_id=o.command_id WHERE o.scope_id=?1 AND o.command_id=?2", params![lease.scope_id, command_id], |r| r.get(0)).optional()?;
    if let Some(hash) = existing {
        if hash != digest(&envelope) {
            bail!("MQ message identity reused with different content");
        }
        return outbox_view_conn(conn, lease, &command_id);
    }
    let intent = CommandIntent {
        command_id: command_id.clone(),
        stream: mq_stream(&participant.thread_id),
        operation_id: MQ_PUBLISH_OPERATION.into(),
        idempotency_key,
        body: envelope,
        expected_generation: Some(generation),
    };
    enqueue_conn_with_epoch(conn, lease, &intent, lease.epoch)?;
    let fence_value = scope_fence_conn(conn, lease)?;
    conn.execute(
        "INSERT INTO cloud_mq_outbox(scope_id,command_id,thread_id,kind,disposition,correlation_id,causation_id,parent_message_id,reply_to_message_id,causal_depth,grant_generation,incarnation,scope_fence,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![lease.scope_id, command_id, participant.thread_id, kind_str(draft.kind)?, draft.disposition.as_str(), draft.correlation_id, draft.causation_id, draft.parent_message_id, draft.reply_to_message_id, draft.causal_depth, i64::try_from(generation)?, i64::try_from(participant.incarnation)?, fence_value, chrono::Utc::now().to_rfc3339()],
    )?;
    outbox_view_conn(conn, lease, &command_id)
}

const OUTBOX_SELECT: &str = "SELECT o.command_id,o.thread_id,c.idempotency_key,o.kind,o.disposition,o.correlation_id,o.causation_id,o.parent_message_id,o.reply_to_message_id,o.causal_depth,c.delivery_state,o.fenced_reason,o.grant_generation,o.mq_message_id,o.mq_seq,o.answered_by_message_id,o.lookup_json,o.created_at,o.scope_fence FROM cloud_mq_outbox o JOIN cloud_command_outbox c ON c.scope_id=o.scope_id AND c.command_id=o.command_id";

fn outbox_row(conn: &Connection, lease: &ScopeLease, row: &rusqlite::Row<'_>) -> rusqlite::Result<(MqOutboxView, i64)> {
    let _ = (conn, lease);
    let lookup: Option<String> = row.get(16)?;
    Ok((MqOutboxView {
        command_id: row.get(0)?,
        thread_id: row.get(1)?,
        idempotency_key: row.get(2)?,
        kind: row.get(3)?,
        disposition: row.get(4)?,
        correlation_id: row.get(5)?,
        causation_id: row.get(6)?,
        parent_message_id: row.get(7)?,
        reply_to_message_id: row.get(8)?,
        causal_depth: row.get(9)?,
        status: String::new(),
        delivery_state: row.get(10)?,
        fenced_reason: row.get(11)?,
        grant_generation: row.get::<_, i64>(12)? as u64,
        mq_message_id: row.get(13)?,
        mq_seq: row.get::<_, Option<i64>>(14)?.map(|value| value as u64),
        answered_by_message_id: row.get(15)?,
        lookup: lookup.and_then(|text| serde_json::from_str(&text).ok()),
        created_at: row.get(17)?,
    }, row.get(18)?))
}

fn finish_view(mut view: MqOutboxView, row_fence: i64, current_fence: i64) -> MqOutboxView {
    view.status = match view.delivery_state.as_str() {
        "pending" if row_fence != current_fence => {
            view.fenced_reason.get_or_insert_with(|| "account_signed_out".into());
            "fenced"
        }
        "pending" => "queued",
        "outcome_unknown" => "unknown",
        "received" | "delivered" | "applied" if view.answered_by_message_id.is_some() => "answered",
        "received" | "delivered" | "applied" => "accepted",
        "refused" if view.fenced_reason.is_some() => "fenced",
        "refused" => "refused",
        _ => "conflict",
    }
    .into();
    view
}

fn outbox_view_conn(conn: &Connection, lease: &ScopeLease, command_id: &str) -> Result<MqOutboxView> {
    let current = scope_fence_conn(conn, lease)?;
    let (view, fence_value) = conn
        .query_row(&format!("{OUTBOX_SELECT} WHERE o.scope_id=?1 AND o.command_id=?2"), params![lease.scope_id, command_id], |row| outbox_row(conn, lease, row))
        .context("MQ outbox entry not found")?;
    Ok(finish_view(view, fence_value, current))
}

const DELIVERY_SELECT: &str = "SELECT p.remote_event_id,p.sequence,p.stage,p.correlation_id,p.causal_depth,p.deadline_ms,p.delivered_generation,p.delivered_incarnation,p.reply_command_id,p.disposition_json,e.payload_json FROM cloud_mq_pending_inputs p JOIN events e ON e.event_id=p.journal_event_id WHERE p.scope_id=?1 AND p.external_id=?2";

fn delivery_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(MqDeliveryView, String)> {
    let disposition: Option<String> = r.get(9)?;
    Ok((MqDeliveryView {
        message_id: r.get(0)?,
        sequence: r.get::<_, i64>(1)? as u64,
        stage: r.get(2)?,
        correlation_id: r.get(3)?,
        causal_depth: r.get(4)?,
        deadline_ms: r.get(5)?,
        delivered_generation: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
        delivered_incarnation: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        reply_command_id: r.get(8)?,
        disposition: disposition.and_then(|text| serde_json::from_str(&text).ok()),
        message: None,
    }, r.get(10)?))
}

fn with_message((mut view, payload): (MqDeliveryView, String)) -> Result<MqDeliveryView> {
    let envelope: Value = serde_json::from_str(&payload)?;
    view.message = envelope.get("data").cloned().and_then(|data| serde_json::from_value(data).ok());
    Ok(view)
}

/// `filter` is a constant SQL predicate over alias `p`; never user text.
fn delivery_rows(conn: &Connection, lease: &ScopeLease, thread_id: &str, filter: &str, limit: usize) -> Result<Vec<MqDeliveryView>> {
    let mut statement = conn.prepare(&format!("{DELIVERY_SELECT} AND ({filter}) ORDER BY p.sequence LIMIT ?3"))?;
    let rows = statement.query_map(params![lease.scope_id, thread_id, limit as i64], delivery_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(with_message).collect()
}

fn delivery_conn(conn: &Connection, lease: &ScopeLease, thread_id: &str, message_id: &str) -> Result<MqDeliveryView> {
    let row = conn
        .query_row(&format!("{DELIVERY_SELECT} AND p.remote_event_id=?3"), params![lease.scope_id, thread_id, message_id], delivery_row)
        .context("MQ delivery not found")?;
    with_message(row)
}

fn check_delivery_fence(participant: &ParticipantRecord, fence: &DeliveryFence) -> Result<()> {
    if participant.state != "active"
        || participant.local_session_id != fence.local_session_id
        || participant.incarnation != fence.incarnation
        || participant.grant_generation != Some(fence.grant_generation)
    {
        bail!("MQ delivery fence mismatch: session, incarnation or grant generation changed");
    }
    Ok(())
}

/// Pure validation of one granted-history page (contract §8). Any other
/// discontinuity is a real gap and refuses rather than being papered over.
pub fn validate_history_page(page: &MqHistoryPage, thread_id: &str, cursor: u64, grant_floor: u64) -> Result<()> {
    if page.thread_id.0.to_string() != thread_id {
        bail!("history page belongs to another thread");
    }
    if page.requested_after_seq != cursor {
        bail!("history page does not continue the durable cursor");
    }
    if page.history_after_seq != grant_floor {
        bail!("history floor differs from the attached grant; resync");
    }
    if page.effective_after_seq != cursor.max(grant_floor) {
        bail!("history effective cursor is inconsistent");
    }
    match (&page.skipped, cursor < grant_floor) {
        (None, false) => {}
        (Some(skip), true) if skip.after_seq == cursor && skip.through_seq == grant_floor && !skip.reason.is_empty() => {}
        _ => bail!("history skipped range is not the exact authorized floor"),
    }
    if page.messages.len() > 200 {
        bail!("history page exceeds the requested bound");
    }
    let mut next = page.effective_after_seq;
    let mut ids = std::collections::HashSet::new();
    for message in &page.messages {
        if message.thread_id != page.thread_id || next.checked_add(1) != Some(message.seq) || !ids.insert(message.message_id) {
            bail!("history page has a real gap, duplicate or foreign message");
        }
        next = message.seq;
    }
    if page.next_after_seq != next {
        bail!("history next cursor is inconsistent");
    }
    Ok(())
}

impl CloudStore {
    /// Stable non-secret device identifier used for backend enrollment.
    pub fn mq_device_id(&self) -> Result<String> {
        self.db.transaction(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO cloud_mq_device(singleton,device_id,created_at) VALUES(1,?1,?2)",
                params![uuid::Uuid::new_v4().to_string(), chrono::Utc::now().to_rfc3339()],
            )?;
            Ok(conn.query_row("SELECT device_id FROM cloud_mq_device WHERE singleton=1", [], |r| r.get(0))?)
        })
    }

    /// Bind an explicitly selected, existing local session to one MQ thread
    /// as the server-derived enrollment principal. Refuses sessions owned by
    /// another account, cloud-executed or legacy remote-linked sessions, a
    /// thread already bound to another session and any silent policy change.
    pub fn connect_mq_participant(&self, lease: &ScopeLease, spec: &ParticipantSpec, enrollment: &EnrollmentBinding) -> Result<ParticipantRecord> {
        valid_id(&spec.thread_id)?;
        valid_id(&spec.local_session_id)?;
        valid_id(&enrollment.enrollment_id)?;
        valid_id(&enrollment.device_id)?;
        spec.policy.validate(spec.preset)?;
        if spec.peers.is_empty() || spec.peers.len() > MAX_PEERS {
            bail!("a connection names 1..=16 thread participants");
        }
        for peer in &spec.peers {
            valid_id(&peer.id)?;
            principal_from_peer(peer)?;
        }
        if enrollment.incarnation == 0 || enrollment.principal_id != format!("enrollment:{}", enrollment.enrollment_id) {
            bail!("enrollment principal must be server-derived");
        }
        validate_mq_origin(&enrollment.mq_endpoint)?;
        let peers_json = serde_json::to_string(&spec.peers)?;
        let policy_json = serde_json::to_string(&spec.policy)?;
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let org = scope_org(conn, lease)?;
            if enrollment.org_id != org || spec.peers.iter().any(|peer| peer.org_id != org) {
                bail!("MQ participants must belong to the verified organization");
            }
            let session: Option<(String, Option<String>)> = conn.query_row(
                "SELECT kind,remote_id FROM sessions WHERE id=?1", params![spec.local_session_id], |r| Ok((r.get(0)?, r.get(1)?)),
            ).optional()?;
            let Some((kind, remote_id)) = session else { bail!("selected local session does not exist") };
            if kind != "codex" || remote_id.is_some() {
                bail!("only a Local, non-legacy session can join an MQ thread");
            }
            let owner: Option<String> = conn.query_row("SELECT scope_id FROM cloud_owned_sessions WHERE local_session_id=?1", params![spec.local_session_id], |r| r.get(0)).optional()?;
            match owner {
                Some(owner) if owner != lease.scope_id => bail!("selected session is bound to another account"),
                Some(_) => {}
                None => { conn.execute("INSERT INTO cloud_owned_sessions VALUES(?1,?2)", params![spec.local_session_id, lease.scope_id])?; }
            }
            let other_thread: Option<String> = conn.query_row(
                "SELECT external_id FROM cloud_session_bindings WHERE scope_id=?1 AND local_session_id=?2 AND (adapter<>'mq' OR external_id<>?3) LIMIT 1",
                params![lease.scope_id, spec.local_session_id, spec.thread_id], |r| r.get(0),
            ).optional()?;
            if other_thread.is_some() {
                bail!("selected session already has a different cloud binding");
            }
            let bound: Option<String> = conn.query_row("SELECT local_session_id FROM cloud_session_bindings WHERE scope_id=?1 AND adapter='mq' AND external_id=?2", params![lease.scope_id, spec.thread_id], |r| r.get(0)).optional()?;
            match bound {
                Some(bound) if bound != spec.local_session_id => bail!("MQ thread is already bound to another session"),
                Some(_) => {}
                None => { conn.execute("INSERT INTO cloud_session_bindings VALUES(?1,'mq',?2,?3)", params![lease.scope_id, spec.thread_id, spec.local_session_id])?; }
            }
            let now = chrono::Utc::now().to_rfc3339();
            match participant_conn(conn, lease, &spec.thread_id)? {
                Some(existing) => {
                    if existing.local_session_id != spec.local_session_id
                        || existing.enrollment_id != enrollment.enrollment_id
                        || existing.device_id != enrollment.device_id
                        || existing.principal_id != enrollment.principal_id
                        || existing.mq_endpoint != enrollment.mq_endpoint
                        || existing.peers != spec.peers
                        || existing.preset != spec.preset
                        || existing.policy != spec.policy
                    {
                        bail!("participant identity, peers or policy differ; change them explicitly");
                    }
                    if enrollment.incarnation < existing.incarnation {
                        bail!("enrollment incarnation regressed");
                    }
                    conn.execute("UPDATE cloud_mq_participants SET incarnation=?1,updated_at=?2 WHERE scope_id=?3 AND thread_id=?4", params![i64::try_from(enrollment.incarnation)?, now, lease.scope_id, spec.thread_id])?;
                }
                None => {
                    conn.execute(
                        "INSERT INTO cloud_mq_participants(scope_id,thread_id,local_session_id,enrollment_id,device_id,incarnation,principal_id,org_id,mq_endpoint,peers_json,preset,policy_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'awaiting_grant',?13,?13)",
                        params![lease.scope_id, spec.thread_id, spec.local_session_id, enrollment.enrollment_id, enrollment.device_id, i64::try_from(enrollment.incarnation)?, enrollment.principal_id, org, enrollment.mq_endpoint, peers_json, spec.preset.as_str(), policy_json, now],
                    )?;
                }
            }
            scope_fence_conn(conn, lease)?;
            require_participant(conn, lease, &spec.thread_id)
        })
    }

    /// A new process re-enrolls and receives a higher incarnation. Only the
    /// current incarnation may accept deliveries afterwards.
    pub fn refresh_mq_incarnation(&self, lease: &ScopeLease, thread_id: &str, enrollment: &EnrollmentBinding) -> Result<ParticipantRecord> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let existing = require_participant(conn, lease, thread_id)?;
            if existing.enrollment_id != enrollment.enrollment_id
                || existing.device_id != enrollment.device_id
                || existing.principal_id != enrollment.principal_id
                || existing.mq_endpoint != enrollment.mq_endpoint
                || existing.org_id != enrollment.org_id
            {
                bail!("enrollment does not belong to this participant");
            }
            if enrollment.incarnation < existing.incarnation {
                bail!("enrollment incarnation regressed");
            }
            conn.execute("UPDATE cloud_mq_participants SET incarnation=?1,updated_at=?2 WHERE scope_id=?3 AND thread_id=?4", params![i64::try_from(enrollment.incarnation)?, chrono::Utc::now().to_rfc3339(), lease.scope_id, thread_id])?;
            require_participant(conn, lease, thread_id)
        })
    }

    /// Attach or refresh the server grant. A higher generation (a revoke,
    /// possibly followed by restore) fences every queued write and open
    /// delivery captured under the older generation.
    pub fn attach_mq_grant(&self, lease: &ScopeLease, thread_id: &str, grant: &GrantSnapshot) -> Result<ParticipantRecord> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let participant = require_participant(conn, lease, thread_id)?;
            if grant.thread_id != thread_id
                || grant.enrollment_id != participant.enrollment_id
                || grant.principal_id != participant.principal_id
                || grant.org_id != participant.org_id
            {
                bail!("grant does not belong to this participant");
            }
            if grant.incarnation != participant.incarnation {
                bail!("grant incarnation is not this process's enrollment incarnation");
            }
            if participant.grant_id.as_deref().is_some_and(|id| id != grant.grant_id) {
                bail!("grant identity changed; reconnect explicitly");
            }
            if participant.grant_generation.is_some_and(|stored| grant.generation < stored) {
                bail!("grant generation regressed");
            }
            if grant.operations.is_empty() || grant.operations.iter().any(|op| !matches!(op.as_str(), "read" | "publish")) {
                bail!("grant operations are invalid");
            }
            let generation = i64::try_from(grant.generation)?;
            // Revocation is the more specific reason, so it is applied first;
            // a restored grant with a newer generation fences the older rows.
            if grant.lifecycle == GrantLifecycle::Revoked {
                fence_pending_writes(conn, lease, thread_id, "1=1", None, "grant_revoked")?;
                fence_open_deliveries(conn, lease, thread_id, None, "grant_revoked")?;
            } else if participant.grant_generation.is_some_and(|stored| grant.generation > stored) {
                fence_pending_writes(conn, lease, thread_id, "o.grant_generation<?3", Some(generation), "grant_generation_fenced")?;
                fence_open_deliveries(conn, lease, thread_id, Some(generation), "grant_generation_fenced")?;
            }
            let (state, reason) = match grant.lifecycle {
                GrantLifecycle::Active => ("active", None),
                GrantLifecycle::Revoked => ("revoked", Some("grant_revoked")),
                GrantLifecycle::Expired => ("expired", Some("grant_expired")),
            };
            conn.execute(
                "UPDATE cloud_mq_participants SET grant_id=?1,grant_generation=?2,grant_operations=?3,history_after_seq=?4,grant_expires_ms=?5,state=?6,state_reason=?7,updated_at=?8 WHERE scope_id=?9 AND thread_id=?10",
                params![grant.grant_id, generation, grant.operations.join(","), i64::try_from(grant.history_after_seq)?, grant.expires_at_ms, state, reason, chrono::Utc::now().to_rfc3339(), lease.scope_id, thread_id],
            )?;
            require_participant(conn, lease, thread_id)
        })
    }

    /// Record a server-reported terminal authority state (revoked, fenced by
    /// another incarnation, expired). Revocation fences queued writes and
    /// open deliveries in the same transaction.
    pub fn set_mq_participant_state(&self, lease: &ScopeLease, thread_id: &str, state: &str, reason: &str) -> Result<ParticipantRecord> {
        if !matches!(state, "revoked" | "expired" | "fenced") {
            bail!("unsupported participant state transition");
        }
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            require_participant(conn, lease, thread_id)?;
            if state == "revoked" {
                fence_pending_writes(conn, lease, thread_id, "1=1", None, reason)?;
                fence_open_deliveries(conn, lease, thread_id, None, reason)?;
            }
            conn.execute("UPDATE cloud_mq_participants SET state=?1,state_reason=?2,updated_at=?3 WHERE scope_id=?4 AND thread_id=?5", params![state, reason, chrono::Utc::now().to_rfc3339(), lease.scope_id, thread_id])?;
            require_participant(conn, lease, thread_id)
        })
    }

    /// Every participant of the active account, for device sign-out.
    pub fn mq_participants(&self, lease: &ScopeLease) -> Result<Vec<ParticipantRecord>> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let mut statement = conn.prepare("SELECT thread_id FROM cloud_mq_participants WHERE scope_id=?1 ORDER BY thread_id")?;
            let threads = statement.query_map(params![lease.scope_id], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            threads.iter().map(|thread| require_participant(conn, lease, thread)).collect()
        })
    }

    pub fn mq_participant(&self, lease: &ScopeLease, thread_id: &str) -> Result<Option<ParticipantRecord>> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            participant_conn(conn, lease, thread_id)
        })
    }

    /// Durable cursor for the granted-history reader (0 before first page).
    pub fn mq_history_cursor(&self, lease: &ScopeLease, thread_id: &str) -> Result<(Option<Checkpoint>, u64)> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let participant = require_participant(conn, lease, thread_id)?;
            let current = checkpoint(conn, lease, &mq_stream(thread_id))?;
            let cursor = match &current {
                None => 0,
                Some(Checkpoint::Mq { subscription_id, sequence }) if *subscription_id == participant.subscription_id() => *sequence,
                _ => bail!("MQ checkpoint belongs to another subscription"),
            };
            Ok((current, cursor))
        })
    }

    /// Commit one granted-history page, its authorized gap and the cursor in
    /// one transaction. Own publications reconcile uncertain sends; they are
    /// never re-delivered to this session. Correlated answers are linked to
    /// the originating outbound message.
    pub fn commit_mq_history(&self, lease: &ScopeLease, thread_id: &str, expected: Option<&Checkpoint>, page: &MqHistoryPage) -> Result<MqHistoryCommit> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let participant = require_participant(conn, lease, thread_id)?;
            if participant.state != "active" {
                bail!("MQ participant is not active");
            }
            let stream = mq_stream(thread_id);
            let session = binding(conn, lease, &stream)?;
            let current = checkpoint(conn, lease, &stream)?;
            if current.as_ref() != expected {
                bail!("checkpoint changed; reload committed state");
            }
            let cursor = match &current {
                None => 0,
                Some(Checkpoint::Mq { subscription_id, sequence }) if *subscription_id == participant.subscription_id() => *sequence,
                _ => bail!("MQ checkpoint belongs to another subscription"),
            };
            let floor = participant.history_after_seq.context("grant history floor unknown")?;
            validate_history_page(page, thread_id, cursor, floor)?;
            if page.messages.iter().any(|message| message.sender.org_id != participant.org_id) {
                bail!("MQ message organization mismatch");
            }
            let mut report = MqHistoryCommit { cursor: page.next_after_seq, ..Default::default() };
            if let Some(skip) = &page.skipped {
                conn.execute("INSERT OR IGNORE INTO cloud_mq_history_gaps VALUES(?1,?2,?3,?4,?5,?6)", params![lease.scope_id, thread_id, i64::try_from(skip.after_seq)?, i64::try_from(skip.through_seq)?, skip.reason, chrono::Utc::now().to_rfc3339()])?;
                report.gap = Some((skip.after_seq, skip.through_seq));
            }
            let generation = participant.grant_generation.map(i64::try_from).transpose()?;
            let incarnation = i64::try_from(participant.incarnation)?;
            for message in &page.messages {
                let event = RemoteEvent {
                    id: message.message_id.0.to_string(),
                    kind: "mq.message".into(),
                    payload: serde_json::to_value(message)?,
                    sequence: Some(message.seq),
                    generation: None,
                };
                let Some((_app, journal_id)) = append_remote_event(conn, lease, &stream, &session, Some(page.effective_after_seq), &event)? else { continue };
                report.committed += 1;
                let own = message.sender.id == participant.principal_id && matches!(message.sender.kind, mq_core::PrincipalKind::Actor);
                if own {
                    if let Some(key) = &message.idempotency_key {
                        if let Some(command_id) = reconcile_own_publication(conn, lease, message, key)? {
                            report.own_reconciled.push(command_id);
                        }
                    }
                    continue;
                }
                let mut depth = crate::cloud::mailbox::policy::message_hop(&message.payload);
                if let Some(cause) = &message.causation_id {
                    let ours: Option<i64> = conn.query_row("SELECT causal_depth FROM cloud_mq_outbox WHERE scope_id=?1 AND thread_id=?2 AND mq_message_id=?3", params![lease.scope_id, thread_id, cause], |r| r.get(0)).optional()?;
                    if let Some(ours) = ours {
                        depth = depth.max(u32::try_from(ours)?.saturating_add(1));
                    }
                }
                let default_deadline = message.created_at.timestamp_millis() + i64::from(participant.policy.limits.deadline_secs) * 1000;
                let requested = message.payload.get("expires_at").and_then(Value::as_str).and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok()).map(|at| at.timestamp_millis());
                let deadline = requested.map_or(default_deadline, |at| at.min(default_deadline));
                conn.execute(
                    "INSERT INTO cloud_mq_pending_inputs(scope_id,external_id,remote_event_id,sequence,journal_event_id,stage,delivered_generation,delivered_incarnation,correlation_id,causal_depth,deadline_ms) VALUES(?1,?2,?3,?4,?5,'delivered',?6,?7,?8,?9,?10)",
                    params![lease.scope_id, thread_id, event.id, i64::try_from(message.seq)?, journal_id, generation, incarnation, message.correlation_id, depth, deadline],
                )?;
                if message.kind == MessageKind::Answer {
                    if let Some(correlation) = &message.correlation_id {
                        let answered: Vec<String> = {
                            let mut statement = conn.prepare("SELECT command_id FROM cloud_mq_outbox WHERE scope_id=?1 AND thread_id=?2 AND correlation_id=?3 AND disposition='message' AND answered_by_message_id IS NULL")?;
                            let rows = statement.query_map(params![lease.scope_id, thread_id, correlation], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
                            rows
                        };
                        for command_id in answered {
                            conn.execute("UPDATE cloud_mq_outbox SET answered_by_message_id=?1 WHERE scope_id=?2 AND command_id=?3", params![event.id, lease.scope_id, command_id])?;
                            report.answered_outbound.push(command_id);
                        }
                    }
                }
            }
            let next = Checkpoint::Mq { subscription_id: participant.subscription_id(), sequence: page.next_after_seq };
            conn.execute("INSERT INTO cloud_checkpoints VALUES(?1,?2,?3,?4) ON CONFLICT(scope_id,adapter,external_id) DO UPDATE SET checkpoint_json=excluded.checkpoint_json", params![lease.scope_id, stream.adapter.as_str(), stream.external_id, serde_json::to_string(&next)?])?;
            Ok(report)
        })
    }

    pub fn enqueue_mq_publish(&self, lease: &ScopeLease, thread_id: &str, draft: &OutboundDraft) -> Result<MqOutboxView> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let participant = require_participant(conn, lease, thread_id)?;
            enqueue_mq_conn(conn, lease, &participant, draft)
        })
    }

    /// Claim one queued write for its single send attempt. The uncertainty
    /// marker commits before any bytes leave. Account and grant-generation
    /// fences are checked here, so a write captured under another account or
    /// a revoked generation is fenced instead of flushed.
    pub fn begin_mq_send(&self, lease: &ScopeLease, thread_id: &str, command_id: &str) -> Result<SendAdmission> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let participant = require_participant(conn, lease, thread_id)?;
            let (state, body, link_generation, link_fence, local_id): (String, Vec<u8>, i64, i64, String) = conn.query_row(
                "SELECT c.delivery_state,c.body,o.grant_generation,o.scope_fence,c.local_command_id FROM cloud_command_outbox c JOIN cloud_mq_outbox o ON o.scope_id=c.scope_id AND o.command_id=c.command_id WHERE c.scope_id=?1 AND c.command_id=?2 AND o.thread_id=?3",
                params![lease.scope_id, command_id, thread_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            ).context("MQ outbox entry not found")?;
            if state != "pending" {
                bail!("MQ outbox entry is not queued");
            }
            let fence_reason = if link_fence != scope_fence_conn(conn, lease)? {
                Some("account_signed_out")
            } else if participant.state == "revoked" {
                Some("grant_revoked")
            } else if participant.grant_generation.map(i64::try_from).transpose()? != Some(link_generation) {
                Some("grant_generation_fenced")
            } else {
                None
            };
            if let Some(reason) = fence_reason {
                let n = conn.execute("UPDATE cloud_command_outbox SET delivery_state='refused',receipt_json=?1 WHERE scope_id=?2 AND command_id=?3 AND delivery_state='pending'", params![json!({"localFence":reason}).to_string(), lease.scope_id, command_id])?;
                if n == 1 {
                    conn.execute("UPDATE cloud_mq_outbox SET fenced_reason=?1 WHERE scope_id=?2 AND command_id=?3", params![reason, lease.scope_id, command_id])?;
                    conn.execute("UPDATE command_receipts SET status='rejected',response_json=?1,updated_at=?2 WHERE command_id=?3", params![json!({"deliveryState":"refused","localFence":reason}).to_string(), chrono::Utc::now().to_rfc3339(), local_id])?;
                }
                return Ok(SendAdmission::Fenced(reason.into()));
            }
            if participant.state != "active" {
                return Ok(SendAdmission::Deferred(format!("participant {}", participant.state)));
            }
            if !participant.can_publish() {
                return Ok(SendAdmission::Deferred("grant does not allow publish".into()));
            }
            if participant.grant_expires_ms.is_some_and(|until| until <= now_ms()) {
                return Ok(SendAdmission::Deferred("grant expired".into()));
            }
            let cursor = match checkpoint(conn, lease, &mq_stream(thread_id))? {
                Some(Checkpoint::Mq { sequence, .. }) => sequence,
                _ => 0,
            };
            let n = conn.execute("UPDATE cloud_command_outbox SET delivery_state='outcome_unknown' WHERE scope_id=?1 AND command_id=?2 AND delivery_state='pending'", params![lease.scope_id, command_id])?;
            if n != 1 {
                bail!("MQ outbox entry was claimed concurrently");
            }
            conn.execute("UPDATE cloud_mq_outbox SET sent_after_seq=?1 WHERE scope_id=?2 AND command_id=?3", params![i64::try_from(cursor)?, lease.scope_id, command_id])?;
            conn.execute("UPDATE command_receipts SET response_json=?1,updated_at=?2 WHERE command_id=?3", params![json!({"deliveryState":"outcome_unknown"}).to_string(), chrono::Utc::now().to_rfc3339(), local_id])?;
            let envelope: Value = serde_json::from_slice(&body)?;
            let publish: mq_core::PublishMessage = serde_json::from_value(envelope.get("publish").cloned().context("publish envelope")?)?;
            Ok(SendAdmission::Send(MqSendRequest { command_id: command_id.to_owned(), publish, grant_generation: link_generation as u64 }))
        })
    }

    /// The server accepted the exact request (publish response or history
    /// lookup). Only an uncertain or already-accepted entry may take this.
    pub fn record_mq_accepted(&self, lease: &ScopeLease, command_id: &str, message_id: &str, seq: u64, via: &str) -> Result<()> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            record_accepted_conn(conn, lease, command_id, message_id, seq, via)
        })
    }

    /// Definitive server refusal (4xx). Never applied to an uncertain loss.
    pub fn record_mq_rejected(&self, lease: &ScopeLease, command_id: &str, conflict: bool, detail: &Value) -> Result<()> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let (state, local_id): (String, String) = conn.query_row("SELECT delivery_state,local_command_id FROM cloud_command_outbox WHERE scope_id=?1 AND command_id=?2", params![lease.scope_id, command_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if state != "outcome_unknown" {
                bail!("only an in-flight MQ send can be rejected");
            }
            let next = if conflict { "conflict" } else { "refused" };
            conn.execute("UPDATE cloud_command_outbox SET delivery_state=?1,receipt_json=?2 WHERE scope_id=?3 AND command_id=?4", params![next, detail.to_string(), lease.scope_id, command_id])?;
            conn.execute("UPDATE command_receipts SET status='rejected',response_json=?1,updated_at=?2 WHERE command_id=?3", params![json!({"deliveryState":next,"receipt":detail}).to_string(), chrono::Utc::now().to_rfc3339(), local_id])?;
            Ok(())
        })
    }

    /// Record an authoritative lookup that did not find the publication. The
    /// outcome stays explicitly unknown; nothing is resent.
    pub fn record_mq_lookup(&self, lease: &ScopeLease, command_id: &str, lookup: &Value) -> Result<()> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let n = conn.execute("UPDATE cloud_mq_outbox SET lookup_json=?1 WHERE scope_id=?2 AND command_id=?3", params![lookup.to_string(), lease.scope_id, command_id])?;
            if n != 1 {
                bail!("MQ outbox entry not found");
            }
            Ok(())
        })
    }

    pub fn mq_outbox_entry(&self, lease: &ScopeLease, command_id: &str) -> Result<MqOutboxView> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            outbox_view_conn(conn, lease, command_id)
        })
    }

    pub fn mq_outbox(&self, lease: &ScopeLease, thread_id: &str, limit: usize) -> Result<Vec<MqOutboxView>> {
        if !(1..=500).contains(&limit) {
            bail!("invalid MQ outbox query");
        }
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let current = scope_fence_conn(conn, lease)?;
            let mut statement = conn.prepare(&format!("{OUTBOX_SELECT} WHERE o.scope_id=?1 AND o.thread_id=?2 ORDER BY o.created_at,o.command_id LIMIT ?3"))?;
            let rows = statement.query_map(params![lease.scope_id, thread_id, limit as i64], |row| outbox_row(conn, lease, row))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().map(|(view, row_fence)| finish_view(view, row_fence, current)).collect())
        })
    }

    /// Command ids in one raw delivery state, oldest first.
    pub fn mq_outbox_ids(&self, lease: &ScopeLease, thread_id: &str, delivery_state: &str, limit: usize) -> Result<Vec<String>> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let mut statement = conn.prepare("SELECT o.command_id FROM cloud_mq_outbox o JOIN cloud_command_outbox c ON c.scope_id=o.scope_id AND c.command_id=o.command_id WHERE o.scope_id=?1 AND o.thread_id=?2 AND c.delivery_state=?3 ORDER BY o.created_at,o.command_id LIMIT ?4")?;
            let rows = statement.query_map(params![lease.scope_id, thread_id, delivery_state, limit as i64], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(rows)
        })
    }

    pub fn mq_deliveries(&self, lease: &ScopeLease, thread_id: &str, stages: &[&str], limit: usize) -> Result<Vec<MqDeliveryView>> {
        if !(1..=500).contains(&limit) {
            bail!("invalid MQ delivery query");
        }
        let filter = if stages.is_empty() {
            "1=1".to_owned()
        } else {
            let quoted: Vec<String> = stages
                .iter()
                .filter(|stage| matches!(**stage, "delivered" | "observed" | "acting" | "answered" | "declined" | "expired" | "fenced"))
                .map(|stage| format!("'{stage}'"))
                .collect();
            format!("p.stage IN ({})", quoted.join(","))
        };
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            delivery_rows(conn, lease, thread_id, &filter, limit)
        })
    }

    pub fn mq_delivery(&self, lease: &ScopeLease, thread_id: &str, message_id: &str) -> Result<MqDeliveryView> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            delivery_conn(conn, lease, thread_id, message_id)
        })
    }

    /// Native acceptance at a safe turn boundary. Hands the persisted message
    /// to its idempotent `mq.input` command and marks it observed only while
    /// the caller still holds the exact session, incarnation and grant
    /// generation. A message delivered under an older generation is fenced.
    pub fn observe_mq_delivery(&self, lease: &ScopeLease, thread_id: &str, message_id: &str, delivery_fence: &DeliveryFence) -> Result<DeliveryAdmission> {
        valid_id(message_id)?;
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let participant = require_participant(conn, lease, thread_id)?;
            check_delivery_fence(&participant, delivery_fence)?;
            let view = delivery_conn(conn, lease, thread_id, message_id)?;
            match view.stage.as_str() {
                "delivered" => {}
                "observed" | "acting" => return Ok(DeliveryAdmission::Observed(view)),
                other => bail!("MQ delivery is already {other}"),
            }
            if view.delivered_generation != Some(delivery_fence.grant_generation) {
                conn.execute("UPDATE cloud_mq_pending_inputs SET stage='fenced',disposition_json=?1,settled_at=?2 WHERE scope_id=?3 AND external_id=?4 AND remote_event_id=?5", params![json!({"fence":"grant_generation_fenced"}).to_string(), chrono::Utc::now().to_rfc3339(), lease.scope_id, thread_id, message_id])?;
                return Ok(DeliveryAdmission::Fenced("grant_generation_fenced".into()));
            }
            accept_mq_input_conn(conn, lease, &mq_stream(thread_id), message_id)?;
            conn.execute("UPDATE cloud_mq_pending_inputs SET stage='observed',observed_at=?1 WHERE scope_id=?2 AND external_id=?3 AND remote_event_id=?4 AND stage='delivered'", params![chrono::Utc::now().to_rfc3339(), lease.scope_id, thread_id, message_id])?;
            append_event(conn, EventAppend {
                event_id: Some(format!("mq-observed:{}", digest(&serde_json::to_vec(&(&lease.scope_id, thread_id, message_id))?))),
                session_id: Some(participant.local_session_id.clone()),
                run_id: None,
                source: EventSource::Remote,
                kind: "mq.delivery.observed".into(),
                payload: json!({"threadId": thread_id, "messageId": message_id, "stage": "observed", "incarnation": participant.incarnation, "grantGeneration": delivery_fence.grant_generation}),
                remote_sequence: None,
                command_id: None,
                created_at: None,
            })?;
            Ok(DeliveryAdmission::Observed(delivery_conn(conn, lease, thread_id, message_id)?))
        })
    }

    /// Admit an automatic handler. Concurrency and the per-minute rate are
    /// counted from durable rows, so a restart cannot reset them.
    pub fn begin_mq_acting(&self, lease: &ScopeLease, thread_id: &str, message_id: &str, delivery_fence: &DeliveryFence, at_ms: i64) -> Result<ActingAdmission> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let participant = require_participant(conn, lease, thread_id)?;
            check_delivery_fence(&participant, delivery_fence)?;
            let limits = participant.policy.limits;
            let view = delivery_conn(conn, lease, thread_id, message_id)?;
            if view.stage != "observed" {
                bail!("only an observed delivery can start acting");
            }
            if view.deadline_ms.is_some_and(|deadline| deadline <= at_ms) {
                return Ok(ActingAdmission::Refused("expired"));
            }
            if view.causal_depth >= limits.max_causal_depth {
                return Ok(ActingAdmission::Refused("causal_depth_exceeded"));
            }
            let active: i64 = conn.query_row("SELECT COUNT(*) FROM cloud_mq_pending_inputs WHERE scope_id=?1 AND external_id=?2 AND stage='acting'", params![lease.scope_id, thread_id], |r| r.get(0))?;
            if active >= i64::from(limits.max_concurrent) {
                return Ok(ActingAdmission::Refused("handler_concurrency_exceeded"));
            }
            let recent: i64 = conn.query_row("SELECT COUNT(*) FROM cloud_mq_pending_inputs WHERE scope_id=?1 AND external_id=?2 AND acting_at_ms>?3", params![lease.scope_id, thread_id, at_ms - 60_000], |r| r.get(0))?;
            if recent >= i64::from(limits.max_per_minute) {
                return Ok(ActingAdmission::Refused("handler_rate_exceeded"));
            }
            conn.execute("UPDATE cloud_mq_pending_inputs SET stage='acting',acting_at_ms=?1 WHERE scope_id=?2 AND external_id=?3 AND remote_event_id=?4 AND stage='observed'", params![at_ms, lease.scope_id, thread_id, message_id])?;
            Ok(ActingAdmission::Admitted)
        })
    }

    /// Correlated disposition. The reply (if any) is queued in the same
    /// transaction as the stage change, so a crash cannot answer twice or
    /// lose the answer. `delivery_fence` is required for answers produced
    /// by this runtime; expiry of never-observed input needs only the lease.
    pub fn settle_mq_delivery(&self, lease: &ScopeLease, thread_id: &str, message_id: &str, delivery_fence: Option<&DeliveryFence>, settlement: DeliverySettlement, detail: &Value, reply: Option<&OutboundDraft>) -> Result<Option<MqOutboxView>> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let participant = require_participant(conn, lease, thread_id)?;
            if let Some(delivery_fence) = delivery_fence {
                check_delivery_fence(&participant, delivery_fence)?;
            }
            let view = delivery_conn(conn, lease, thread_id, message_id)?;
            let allowed = match settlement {
                DeliverySettlement::Answered => matches!(view.stage.as_str(), "observed" | "acting"),
                DeliverySettlement::Declined | DeliverySettlement::Expired => matches!(view.stage.as_str(), "delivered" | "observed" | "acting"),
            };
            if !allowed {
                bail!("MQ delivery cannot be settled from {}", view.stage);
            }
            let queued = match reply {
                Some(reply) => {
                    if reply.reply_to_message_id.as_deref() != Some(message_id) {
                        bail!("reply must name the delivery it answers");
                    }
                    Some(enqueue_mq_conn(conn, lease, &participant, reply)?)
                }
                None => None,
            };
            conn.execute(
                "UPDATE cloud_mq_pending_inputs SET stage=?1,settled_at=?2,disposition_json=?3,reply_command_id=?4 WHERE scope_id=?5 AND external_id=?6 AND remote_event_id=?7",
                params![settlement.stage(), chrono::Utc::now().to_rfc3339(), detail.to_string(), queued.as_ref().map(|entry| entry.command_id.clone()), lease.scope_id, thread_id, message_id],
            )?;
            let accepted: Option<String> = conn.query_row("SELECT accepted_command_id FROM cloud_mq_pending_inputs WHERE scope_id=?1 AND external_id=?2 AND remote_event_id=?3", params![lease.scope_id, thread_id, message_id], |r| r.get(0))?;
            if let Some(command) = accepted {
                let status = if settlement == DeliverySettlement::Answered { "completed" } else { "rejected" };
                conn.execute("UPDATE command_receipts SET status=?1,response_json=?2,updated_at=?3 WHERE command_id=?4", params![status, json!({"disposition": settlement.stage(), "detail": detail}).to_string(), chrono::Utc::now().to_rfc3339(), command])?;
            }
            Ok(queued)
        })
    }

    /// Open deliveries whose deadline has passed (sleep, restart, slow
    /// operator). They are expired, never executed late.
    pub fn overdue_mq_deliveries(&self, lease: &ScopeLease, thread_id: &str, at_ms: i64, limit: usize) -> Result<Vec<MqDeliveryView>> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let mut views = delivery_rows(conn, lease, thread_id, "p.stage IN ('delivered','observed','acting') AND p.deadline_ms IS NOT NULL", limit.clamp(1, 200))?;
            views.retain(|view| view.deadline_ms.is_some_and(|deadline| deadline <= at_ms));
            Ok(views)
        })
    }

    /// Remaining existing work authorization for the bound session: the
    /// conversation paid-compute budget, or zero when none exists or auto
    /// approval is disabled. A message can never create or raise it.
    pub fn mq_work_budget(&self, lease: &ScopeLease, session_id: &str) -> Result<u64> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let owned: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM cloud_owned_sessions WHERE scope_id=?1 AND local_session_id=?2)", params![lease.scope_id, session_id], |r| r.get(0))?;
            if !owned {
                bail!("session is not owned by the active cloud scope");
            }
            Ok(match crate::session::paid_compute_budget::snapshot(conn, session_id)? {
                Some(snapshot) if !snapshot.auto_disabled => snapshot.remaining_usd_micros,
                _ => 0,
            })
        })
    }

    /// Recorded authorized history gaps for this thread.
    pub fn mq_history_gaps(&self, lease: &ScopeLease, thread_id: &str) -> Result<Vec<(u64, u64, String)>> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let mut statement = conn.prepare("SELECT after_seq,through_seq,reason FROM cloud_mq_history_gaps WHERE scope_id=?1 AND thread_id=?2 ORDER BY after_seq")?;
            let rows = statement.query_map(params![lease.scope_id, thread_id], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64, r.get(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }
}

fn record_accepted_conn(conn: &Connection, lease: &ScopeLease, command_id: &str, message_id: &str, seq: u64, via: &str) -> Result<()> {
    let (state, local_id, previous): (String, String, Option<String>) = conn.query_row(
        "SELECT c.delivery_state,c.local_command_id,o.mq_message_id FROM cloud_command_outbox c JOIN cloud_mq_outbox o ON o.scope_id=c.scope_id AND o.command_id=c.command_id WHERE c.scope_id=?1 AND c.command_id=?2",
        params![lease.scope_id, command_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).context("MQ outbox entry not found")?;
    match state.as_str() {
        "outcome_unknown" => {}
        "received" if previous.as_deref() == Some(message_id) => return Ok(()),
        "received" => bail!("MQ publication identity drift"),
        other => bail!("MQ outbox entry cannot be accepted from {other}"),
    }
    let receipt = json!({"messageId": message_id, "seq": seq, "via": via});
    conn.execute("UPDATE cloud_command_outbox SET delivery_state='received',receipt_json=?1 WHERE scope_id=?2 AND command_id=?3", params![receipt.to_string(), lease.scope_id, command_id])?;
    conn.execute("UPDATE cloud_mq_outbox SET mq_message_id=?1,mq_seq=?2 WHERE scope_id=?3 AND command_id=?4", params![message_id, i64::try_from(seq)?, lease.scope_id, command_id])?;
    conn.execute("UPDATE command_receipts SET status='accepted',response_json=?1,updated_at=?2 WHERE command_id=?3", params![json!({"deliveryState":"received","receipt":receipt}).to_string(), chrono::Utc::now().to_rfc3339(), local_id])?;
    Ok(())
}

/// Our own publication observed in authoritative history. It settles an
/// uncertain send exactly when the stored request semantics match.
fn reconcile_own_publication(conn: &Connection, lease: &ScopeLease, message: &Message, key: &str) -> Result<Option<String>> {
    let row: Option<(String, String, Vec<u8>)> = conn.query_row(
        "SELECT c.command_id,c.delivery_state,c.body FROM cloud_command_outbox c JOIN cloud_mq_outbox o ON o.scope_id=c.scope_id AND o.command_id=c.command_id WHERE c.scope_id=?1 AND c.operation_id=?2 AND c.idempotency_key=?3",
        params![lease.scope_id, MQ_PUBLISH_OPERATION, key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).optional()?;
    let Some((command_id, state, body)) = row else { return Ok(None) };
    let envelope: Value = serde_json::from_slice(&body)?;
    let stored: mq_core::PublishMessage = serde_json::from_value(envelope.get("publish").cloned().context("publish envelope")?)?;
    let matches = stored.kind == message.kind
        && stored.body == message.body
        && stored.correlation_id == message.correlation_id
        && stored.causation_id == message.causation_id
        && stored.parent_message_id == message.parent_message_id;
    match state.as_str() {
        "outcome_unknown" | "pending" if matches => {
            if state == "pending" {
                conn.execute("UPDATE cloud_command_outbox SET delivery_state='outcome_unknown' WHERE scope_id=?1 AND command_id=?2", params![lease.scope_id, command_id])?;
            }
            record_accepted_conn(conn, lease, &command_id, &message.message_id.0.to_string(), message.seq, "history")?;
            Ok(Some(command_id))
        }
        "outcome_unknown" => {
            conn.execute("UPDATE cloud_command_outbox SET delivery_state='conflict',receipt_json=?1 WHERE scope_id=?2 AND command_id=?3", params![json!({"reason":"history_semantics_mismatch","messageId":message.message_id.0}).to_string(), lease.scope_id, command_id])?;
            Ok(Some(command_id))
        }
        _ => {
            conn.execute("UPDATE cloud_mq_outbox SET lookup_json=?1 WHERE scope_id=?2 AND command_id=?3 AND mq_message_id IS NULL", params![json!({"observedInHistory": message.message_id.0, "seq": message.seq, "localState": state}).to_string(), lease.scope_id, command_id])?;
            Ok(None)
        }
    }
}
