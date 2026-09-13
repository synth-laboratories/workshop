//! Renderer IPC over the native mailbox (WP7), behind the qualification gate.
//!
//! Every command first checks the host scope: while the store is not
//! installed (`QualificationRequired`) it refuses before reading any config,
//! credential or network. The views carry only local, redacted state: no
//! credential, token or grant secret is ever returned to the renderer.
use crate::cloud::scoped_runtime::{Availability, MailboxDeps, OperatorReply, ScopeView};
use crate::cloud::storage::{MqDeliveryView, MqOutboxView, ParticipantRecord};
use crate::core_runtime::CoreRuntime;
use anyhow::{bail, Result};
use serde::Serialize;

const MAX_BODY_CHARS: usize = 4_000;

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MailboxConnectionView {
    pub thread_id: String,
    pub local_session_id: String,
    pub preset: String,
    /// awaiting_grant | active | revoked | expired | fenced
    pub state: String,
    pub state_reason: Option<String>,
    pub grant_operations: Vec<String>,
    pub grant_expires_at: Option<String>,
    pub peers: Vec<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MailboxInboxRowView {
    pub message_id: String,
    pub sequence: f64,
    pub kind: String,
    pub sender: String,
    pub body: String,
    pub correlation_id: Option<String>,
    /// delivered | observed | acting | answered | declined | expired | fenced
    pub stage: String,
    pub deadline_at: Option<String>,
    /// An observed request that waits for an operator answer or decline.
    pub awaiting_operator: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MailboxOutboxRowView {
    pub command_id: String,
    pub kind: String,
    pub disposition: String,
    /// queued | unknown | accepted | answered | refused | conflict | fenced
    pub status: String,
    /// The send may or may not have been accepted. It is never resent.
    pub unknown_outcome: bool,
    pub fenced_reason: Option<String>,
    pub correlation_id: Option<String>,
    pub reply_to_message_id: Option<String>,
    pub mq_message_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MailboxGapView {
    pub after_seq: f64,
    pub through_seq: f64,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MailboxStatusView {
    pub generation: u32,
    pub thread_id: String,
    pub connection: Option<MailboxConnectionView>,
    pub inbox: Vec<MailboxInboxRowView>,
    pub outbox: Vec<MailboxOutboxRowView>,
    pub unknown_outcomes: u32,
    pub gaps: Vec<MailboxGapView>,
}

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSignOutView {
    pub revoked_enrollments: u32,
    pub unconfirmed_enrollments: u32,
    /// Why the server-side revocation could not run (the local sign-out
    /// happens regardless).
    pub revocation_error: Option<String>,
    pub view: ScopeView,
}

fn connection_view(record: &ParticipantRecord) -> MailboxConnectionView {
    MailboxConnectionView {
        thread_id: record.thread_id.clone(),
        local_session_id: record.local_session_id.clone(),
        preset: record.preset.as_str().into(),
        state: record.state.clone(),
        state_reason: record.state_reason.clone(),
        grant_operations: record.grant_operations.clone(),
        grant_expires_at: record.grant_expires_ms.and_then(chrono::DateTime::from_timestamp_millis).map(|at| at.to_rfc3339()),
        peers: record.peers.iter().map(|peer| format!("{}:{}", peer.kind, peer.id)).collect(),
    }
}

fn inbox_view(row: &MqDeliveryView) -> MailboxInboxRowView {
    let message = row.message.as_ref();
    let kind = message.and_then(|m| serde_json::to_value(m.kind).ok()).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
    let request = message.is_some_and(|m| matches!(crate::cloud::mailbox::policy::classify(m), crate::cloud::mailbox::policy::InboundIntent::WorkRequest));
    MailboxInboxRowView {
        message_id: row.message_id.clone(),
        sequence: row.sequence as f64,
        kind,
        sender: message.map(|m| format!("{}:{}", serde_json::to_value(m.sender.kind).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default(), m.sender.id)).unwrap_or_default(),
        body: message.map(|m| m.body.chars().take(MAX_BODY_CHARS).collect()).unwrap_or_default(),
        correlation_id: row.correlation_id.clone(),
        stage: row.stage.clone(),
        deadline_at: row.deadline_ms.and_then(chrono::DateTime::from_timestamp_millis).map(|at| at.to_rfc3339()),
        awaiting_operator: request && row.stage == "observed",
    }
}

fn outbox_view(row: &MqOutboxView) -> MailboxOutboxRowView {
    MailboxOutboxRowView {
        command_id: row.command_id.clone(),
        kind: row.kind.clone(),
        disposition: row.disposition.clone(),
        status: row.status.clone(),
        unknown_outcome: row.status == "unknown",
        fenced_reason: row.fenced_reason.clone(),
        correlation_id: row.correlation_id.clone(),
        reply_to_message_id: row.reply_to_message_id.clone(),
        mq_message_id: row.mq_message_id.clone(),
        created_at: row.created_at.clone(),
    }
}

/// The qualification gate: refuse before any config, credential or network.
pub async fn require_qualified(core: &CoreRuntime) -> Result<()> {
    if core.scoped_cloud().view().await?.availability == Availability::QualificationRequired {
        bail!("cloud profile qualification is required");
    }
    Ok(())
}

pub async fn connections(core: &CoreRuntime, deps: &MailboxDeps) -> Result<Vec<MailboxConnectionView>> {
    let records = core.scoped_cloud().mailbox_participants_with(deps).await?;
    Ok(records.iter().map(connection_view).collect())
}

pub async fn status(core: &CoreRuntime, deps: &MailboxDeps, thread_id: &str) -> Result<MailboxStatusView> {
    let status = core.scoped_cloud().mailbox_status_with(deps, thread_id).await?;
    let outbox: Vec<MailboxOutboxRowView> = status.outbox.iter().map(outbox_view).collect();
    Ok(MailboxStatusView {
        generation: status.generation,
        thread_id: thread_id.to_owned(),
        connection: status.participant.as_ref().map(connection_view),
        inbox: status.deliveries.iter().map(inbox_view).collect(),
        unknown_outcomes: outbox.iter().filter(|row| row.unknown_outcome).count() as u32,
        outbox,
        gaps: status.gaps.iter().map(|(after, through, reason)| MailboxGapView { after_seq: *after as f64, through_seq: *through as f64, reason: reason.clone() }).collect(),
    })
}

pub async fn reply(core: &CoreRuntime, deps: &MailboxDeps, thread_id: &str, message_id: &str, reply: OperatorReply) -> Result<MailboxOutboxRowView> {
    let text = match &reply {
        OperatorReply::Answer(text) | OperatorReply::Decline(text) => text,
    };
    if text.trim().is_empty() || text.len() > 64 * 1024 {
        bail!("reply text must be 1..=65536 bytes");
    }
    let entry = core.scoped_cloud().answer_mq_with(deps, thread_id, message_id, reply).await?;
    Ok(outbox_view(&entry))
}

/// Device sign-out when the backend is configured, then the local sign-out.
pub async fn sign_out(core: &CoreRuntime, deps: Option<&MailboxDeps>) -> Result<MailboxSignOutView> {
    let (revoked, unconfirmed, error) = match deps {
        Some(deps) => match core.scoped_cloud().sign_out_device_with(deps).await {
            Ok(outcome) => (outcome.revoked.len(), outcome.unconfirmed.len(), None),
            Err(error) => (0, 0, Some(format!("{error:#}"))),
        },
        None => (0, 0, Some("cloud backend configuration unavailable".into())),
    };
    let view = core.scoped_cloud().invalidate().await?;
    Ok(MailboxSignOutView { revoked_enrollments: revoked as u32, unconfirmed_enrollments: unconfirmed as u32, revocation_error: error, view })
}

// Tauri command bodies: gate first, then the configured production deps.

pub async fn connections_command(core: &CoreRuntime) -> Result<Vec<MailboxConnectionView>> {
    require_qualified(core).await?;
    connections(core, &super::host::configured_deps(core)?).await
}

pub async fn status_command(core: &CoreRuntime, thread_id: &str) -> Result<MailboxStatusView> {
    require_qualified(core).await?;
    status(core, &super::host::configured_deps(core)?, thread_id).await
}

pub async fn reply_command(core: &CoreRuntime, thread_id: &str, message_id: &str, reply_kind: OperatorReply) -> Result<MailboxOutboxRowView> {
    require_qualified(core).await?;
    reply(core, &super::host::configured_deps(core)?, thread_id, message_id, reply_kind).await
}

pub async fn sign_out_command(core: &CoreRuntime) -> Result<MailboxSignOutView> {
    require_qualified(core).await?;
    let deps = super::host::configured_deps(core).ok();
    sign_out(core, deps.as_ref()).await
}
