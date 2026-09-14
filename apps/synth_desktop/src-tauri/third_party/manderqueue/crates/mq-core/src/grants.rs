//! Device/session enrollment and thread access grants.
//!
//! See docs/WORKSHOP_GRANT_CONTRACT.md. Every rule here is shared by the memory
//! and Postgres stores so both enforce identical authority from one locked
//! snapshot of storage.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::types::{Cap, Participant, Principal, PrincipalKind, Role, Thread, ThreadId, Message};

/// Reserved principal id prefix for server-derived enrollment participants.
/// Credentials for these principals must be asymmetric grant credentials.
pub const ENROLLMENT_PRINCIPAL_PREFIX: &str = "enrollment:";
pub const GRANT_MIN_TTL_SECONDS: i64 = 60;
pub const GRANT_MAX_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
pub const HISTORY_MAX_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantOperation {
    Read,
    Publish,
}

impl GrantOperation {
    pub fn cap(self) -> Cap {
        match self {
            GrantOperation::Read => Cap::Read,
            GrantOperation::Publish => Cap::Publish,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            GrantOperation::Read => "read",
            GrantOperation::Publish => "publish",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(GrantOperation::Read),
            "publish" => Some(GrantOperation::Publish),
            _ => None,
        }
    }
}

pub fn is_enrollment_principal(principal: &Principal) -> bool {
    principal.id.starts_with(ENROLLMENT_PRINCIPAL_PREFIX)
}

pub fn enrollment_principal(org_id: &str, enrollment_id: Uuid) -> Principal {
    Principal {
        kind: PrincipalKind::Actor,
        id: format!("{ENROLLMENT_PRINCIPAL_PREFIX}{enrollment_id}"),
        org_id: org_id.to_string(),
    }
}

/// Wire request: enroll (or re-enroll, advancing the incarnation) a device session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollDevice {
    pub device_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enrollment {
    pub enrollment_id: Uuid,
    pub org_id: String,
    pub owner: Principal,
    pub device_id: String,
    pub session_id: String,
    pub label: Option<String>,
    /// Server-derived participant identity; never caller-chosen.
    pub principal: Principal,
    /// Advances on every enroll call; only the current value is valid.
    pub incarnation: u64,
    /// Device sign-out: once set, every grant and incarnation is refused and
    /// the (owner, device, session) key cannot be re-enrolled.
    #[serde(default)]
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Enrollment {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantStatus {
    Active,
    Revoked,
}

impl GrantStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            GrantStatus::Active => "active",
            GrantStatus::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(GrantStatus::Active),
            "revoked" => Some(GrantStatus::Revoked),
            _ => None,
        }
    }
}

/// Computed at read time from status and expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantState {
    Active,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub grant_id: Uuid,
    pub org_id: String,
    pub thread_id: ThreadId,
    pub enrollment_id: Uuid,
    pub principal: Principal,
    pub operations: Vec<GrantOperation>,
    /// Exclusive history lower bound: only `seq > history_after_seq` is visible.
    pub history_after_seq: u64,
    pub expires_at: DateTime<Utc>,
    /// Current enrollment incarnation (live view, not a stored copy).
    pub incarnation: u64,
    /// Server-owned; revoke increments it, restore/renew never change it.
    pub generation: u64,
    pub status: GrantStatus,
    pub state: GrantState,
    pub granted_by: Principal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Grant {
    /// Attach the live incarnation and computed state.
    pub fn view(mut self, enrollment: &Enrollment, now: DateTime<Utc>) -> Self {
        self.incarnation = enrollment.incarnation;
        self.state = if self.status == GrantStatus::Revoked || enrollment.is_revoked() {
            GrantState::Revoked
        } else if self.expires_at <= now {
            GrantState::Expired
        } else {
            GrantState::Active
        };
        self
    }

    pub fn allows(&self, operation: GrantOperation) -> bool {
        self.operations.contains(&operation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateGrant {
    pub thread_id: ThreadId,
    pub enrollment_id: Uuid,
    pub operations: Vec<GrantOperation>,
    pub ttl_seconds: i64,
    /// Defaults to the current thread head (future messages only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_after_seq: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewGrant {
    pub ttl_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantIssuanceRequest {
    pub enrollment_id: Uuid,
    pub incarnation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantIssuance {
    pub grant: Grant,
    /// Credentials must not outlive this instant.
    pub not_after: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantFilter {
    #[serde(default)]
    pub enrollment_id: Option<Uuid>,
    #[serde(default)]
    pub thread_id: Option<ThreadId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantMutation {
    Revoke,
    Restore,
    Renew { expires_at: DateTime<Utc> },
}

/// Verified grant credential claims plus the evaluation instant. Trusted
/// ingress authority only; never deserialized from a request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantFence {
    pub grant_id: Uuid,
    pub thread_id: ThreadId,
    pub enrollment_id: Uuid,
    pub operations: Vec<GrantOperation>,
    pub generation: u64,
    pub incarnation: u64,
    /// Set by [`crate::Fabric`] from its clock.
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistorySkip {
    pub after_seq: u64,
    pub through_seq: u64,
    pub reason: String,
}

/// Granted-history page. See contract §8.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPage {
    pub thread_id: ThreadId,
    pub requested_after_seq: u64,
    pub history_after_seq: u64,
    pub effective_after_seq: u64,
    pub skipped: Option<HistorySkip>,
    pub messages: Vec<Message>,
    pub next_after_seq: u64,
    pub has_more: bool,
}

impl HistoryPage {
    /// `messages` must be the contiguous rows after `effective_after_seq`,
    /// fetched with `limit + 1` so `has_more` is exact.
    pub fn build(
        thread: &Thread,
        requested_after_seq: u64,
        history_after_seq: u64,
        mut messages: Vec<Message>,
        limit: usize,
    ) -> Self {
        let effective_after_seq = requested_after_seq.max(history_after_seq);
        let has_more = messages.len() > limit;
        messages.truncate(limit);
        let next_after_seq = messages.last().map(|m| m.seq).unwrap_or(effective_after_seq);
        let skipped = (requested_after_seq < history_after_seq).then(|| HistorySkip {
            after_seq: requested_after_seq,
            through_seq: history_after_seq,
            reason: "before_grant_history".into(),
        });
        Self {
            thread_id: thread.thread_id,
            requested_after_seq,
            history_after_seq,
            effective_after_seq,
            skipped,
            messages,
            next_after_seq,
            has_more,
        }
    }
}

fn valid_device_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

pub fn validate_enroll(owner: &Principal, req: &EnrollDevice) -> Result<()> {
    if owner.kind != PrincipalKind::Human || is_enrollment_principal(owner) {
        return Err(Error::Forbidden("enrollment_owner_must_be_human"));
    }
    if !valid_device_token(&req.device_id)
        || !valid_device_token(&req.session_id)
        || req.label.as_ref().is_some_and(|l| l.chars().count() > 200)
    {
        return Err(Error::Invalid("invalid_device_identity"));
    }
    Ok(())
}

pub fn validate_operations(operations: &[GrantOperation]) -> Result<()> {
    if operations.is_empty()
        || operations.len() > 2
        || (operations.len() == 2 && operations[0] == operations[1])
    {
        return Err(Error::Invalid("invalid_operations"));
    }
    Ok(())
}

pub fn validate_ttl(ttl_seconds: i64) -> Result<()> {
    if !(GRANT_MIN_TTL_SECONDS..=GRANT_MAX_TTL_SECONDS).contains(&ttl_seconds) {
        return Err(Error::Invalid("invalid_ttl"));
    }
    Ok(())
}

fn caller_can_invite(actor: &Principal, members: &[Participant]) -> bool {
    members
        .iter()
        .any(|p| p.principal == *actor && p.role != Role::Revoked && p.caps.contains(&Cap::Invite))
}

/// Validate a grant creation against one locked snapshot. Returns the
/// participant row to insert when the grantee is not yet a member.
pub fn check_grant_create(
    actor: &Principal,
    thread: &Thread,
    members: &[Participant],
    enrollment: &Enrollment,
    req: &CreateGrant,
    head_seq: u64,
) -> Result<(u64, Option<Participant>)> {
    if thread.org_id != actor.org_id {
        return Err(Error::NotFound("thread"));
    }
    if enrollment.org_id != actor.org_id || enrollment.owner != *actor {
        return Err(Error::NotFound("enrollment"));
    }
    if enrollment.is_revoked() {
        return Err(Error::Forbidden("enrollment_revoked"));
    }
    validate_operations(&req.operations)?;
    validate_ttl(req.ttl_seconds)?;
    if !caller_can_invite(actor, members) {
        return Err(Error::Forbidden("invite_required"));
    }
    let floor = req.history_after_seq.unwrap_or(head_seq);
    if floor > head_seq {
        return Err(Error::Invalid("invalid_history_bound"));
    }
    match members.iter().find(|p| p.principal == enrollment.principal) {
        Some(existing) if existing.role == Role::Revoked => {
            Err(Error::Forbidden("grant_membership_required"))
        }
        Some(_) => Ok((floor, None)),
        None => {
            let role = if req.operations.contains(&GrantOperation::Publish) {
                Role::Member
            } else {
                Role::Observer
            };
            Ok((floor, Some(Participant::new(enrollment.principal.clone(), role))))
        }
    }
}

/// Enrollment owner, or a current `invite` holder on the grant's thread.
pub fn can_view_grant(actor: &Principal, enrollment: &Enrollment, members: &[Participant]) -> bool {
    actor.org_id == enrollment.org_id
        && (enrollment.owner == *actor || caller_can_invite(actor, members))
}

fn grantee_member_ok(grant: &Grant, members: &[Participant], cap: Option<Cap>) -> bool {
    members.iter().any(|p| {
        p.principal == grant.principal
            && p.role != Role::Revoked
            && cap.is_none_or(|c| p.caps.contains(&c))
    })
}

/// Apply a mutation to a locked grant. `Ok(None)` means a no-op (idempotent revoke/restore).
pub fn check_grant_mutation(
    actor: &Principal,
    grant: &Grant,
    enrollment: &Enrollment,
    members: &[Participant],
    mutation: GrantMutation,
    now: DateTime<Utc>,
) -> Result<Option<Grant>> {
    if !can_view_grant(actor, enrollment, members) {
        return Err(Error::NotFound("grant"));
    }
    let mut next = grant.clone();
    match mutation {
        GrantMutation::Revoke => {
            if grant.status == GrantStatus::Revoked {
                return Ok(None);
            }
            next.status = GrantStatus::Revoked;
            next.generation = grant
                .generation
                .checked_add(1)
                .filter(|g| *g <= i64::MAX as u64)
                .ok_or(Error::Invalid("grant_generation_exhausted"))?;
        }
        GrantMutation::Restore => {
            if !caller_can_invite(actor, members) {
                return Err(Error::Forbidden("invite_required"));
            }
            if enrollment.is_revoked() {
                return Err(Error::Forbidden("enrollment_revoked"));
            }
            if grant.status == GrantStatus::Active {
                return Ok(None);
            }
            if grant.expires_at <= now {
                return Err(Error::Forbidden("grant_expired"));
            }
            if !grantee_member_ok(grant, members, None) {
                return Err(Error::Forbidden("grant_membership_required"));
            }
            next.status = GrantStatus::Active;
        }
        GrantMutation::Renew { expires_at } => {
            if !caller_can_invite(actor, members) {
                return Err(Error::Forbidden("invite_required"));
            }
            if enrollment.is_revoked() {
                return Err(Error::Forbidden("enrollment_revoked"));
            }
            if grant.status == GrantStatus::Revoked {
                return Err(Error::Forbidden("grant_revoked"));
            }
            next.expires_at = expires_at;
        }
    }
    next.updated_at = now;
    Ok(Some(next))
}

fn check_live(grant: &Grant, members: &[Participant], now: DateTime<Utc>, cap: Option<Cap>) -> Result<()> {
    if grant.status == GrantStatus::Revoked {
        return Err(Error::Forbidden("grant_revoked"));
    }
    if grant.expires_at <= now {
        return Err(Error::Forbidden("grant_expired"));
    }
    if !grantee_member_ok(grant, members, cap) {
        return Err(Error::Forbidden("grant_membership_required"));
    }
    Ok(())
}

/// Authorize credential issuance from live storage (backend asks as the owner).
pub fn check_grant_issuance(
    actor: &Principal,
    grant: &Grant,
    enrollment: &Enrollment,
    members: &[Participant],
    req: &GrantIssuanceRequest,
    now: DateTime<Utc>,
) -> Result<()> {
    if enrollment.owner != *actor || req.enrollment_id != grant.enrollment_id {
        return Err(Error::NotFound("grant"));
    }
    if enrollment.is_revoked() {
        return Err(Error::Forbidden("enrollment_revoked"));
    }
    if req.incarnation != enrollment.incarnation {
        return Err(Error::Forbidden("grant_incarnation_fenced"));
    }
    check_live(grant, members, now, None)
}

/// Enforce one grant credential at the operation boundary.
pub fn check_grant_access(
    actor: &Principal,
    thread_id: ThreadId,
    grant: &Grant,
    enrollment: &Enrollment,
    members: &[Participant],
    fence: &GrantFence,
    operation: GrantOperation,
) -> Result<()> {
    if grant.grant_id != fence.grant_id
        || grant.thread_id != thread_id
        || fence.thread_id != thread_id
        || grant.principal != *actor
        || grant.enrollment_id != fence.enrollment_id
        || enrollment.enrollment_id != grant.enrollment_id
    {
        return Err(Error::Forbidden("grant_operation_denied"));
    }
    if enrollment.is_revoked() {
        return Err(Error::Forbidden("enrollment_revoked"));
    }
    if grant.status == GrantStatus::Revoked {
        return Err(Error::Forbidden("grant_revoked"));
    }
    if grant.generation != fence.generation {
        return Err(Error::Forbidden("grant_generation_stale"));
    }
    if enrollment.incarnation != fence.incarnation {
        return Err(Error::Forbidden("grant_incarnation_fenced"));
    }
    if !fence.operations.contains(&operation) || !grant.allows(operation) {
        return Err(Error::Forbidden("grant_operation_denied"));
    }
    check_live(grant, members, fence.at, Some(operation.cap()))
}

/// Queued delivery to an enrollment principal needs a live read grant that
/// covers the message sequence.
pub fn delivery_allowed(
    grant: Option<&Grant>,
    enrollment: Option<&Enrollment>,
    members: &[Participant],
    message_seq: u64,
    now: DateTime<Utc>,
) -> bool {
    let (Some(grant), Some(enrollment)) = (grant, enrollment) else { return false };
    enrollment.enrollment_id == grant.enrollment_id
        && !enrollment.is_revoked()
        && grant.allows(GrantOperation::Read)
        && message_seq > grant.history_after_seq
        && check_live(grant, members, now, Some(Cap::Read)).is_ok()
}

/// Revoke every active grant of a signed-out enrollment. Validates all
/// generation increments first so a failure leaves nothing half-applied.
pub fn revoke_enrollment_grants<'a>(
    grants: impl IntoIterator<Item = &'a mut Grant>,
    now: DateTime<Utc>,
) -> Result<()> {
    let mut active: Vec<&mut Grant> = grants.into_iter().filter(|g| g.status == GrantStatus::Active).collect();
    if active.iter().any(|g| g.generation >= i64::MAX as u64) {
        return Err(Error::Invalid("grant_generation_exhausted"));
    }
    for grant in active.iter_mut() {
        grant.status = GrantStatus::Revoked;
        grant.generation += 1;
        grant.updated_at = now;
    }
    Ok(())
}

/// The only principal allowed to ask for delivery verification: the backend
/// delivery bridge, minted by the backend issuer with an asymmetric signature.
pub const DELIVERY_VERIFIER_ID: &str = "mq-delivery-bridge";

/// Bridge request: is this envelope's grant still live for this delivery?
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryCheckRequest {
    pub generation: u64,
    pub incarnation: u64,
    pub recipient: Principal,
    pub message_seq: u64,
}

/// Echo of the verified triple; the bridge requires an exact match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryCheck {
    pub grant_id: Uuid,
    pub generation: u64,
    pub incarnation: u64,
}

/// Same rules as an operation-boundary read, plus the history floor for the
/// delivered message. Refusal codes match the contract §7.
pub fn check_delivery_authority(
    grant: &Grant,
    enrollment: &Enrollment,
    members: &[Participant],
    req: &DeliveryCheckRequest,
    now: DateTime<Utc>,
) -> Result<DeliveryCheck> {
    let fence = GrantFence {
        grant_id: grant.grant_id,
        thread_id: grant.thread_id,
        enrollment_id: grant.enrollment_id,
        operations: vec![GrantOperation::Read],
        generation: req.generation,
        incarnation: req.incarnation,
        at: now,
    };
    check_grant_access(&req.recipient, grant.thread_id, grant, enrollment, members, &fence, GrantOperation::Read)?;
    if req.message_seq <= grant.history_after_seq {
        return Err(Error::Forbidden("grant_operation_denied"));
    }
    Ok(DeliveryCheck { grant_id: grant.grant_id, generation: grant.generation, incarnation: enrollment.incarnation })
}

/// Outcome of the pre-dispatch delivery check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryGrant {
    /// Recipient is not an enrollment principal; membership rules apply.
    NotGoverned,
    Allowed(Grant),
    Denied,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn human(id: &str) -> Principal {
        Principal { kind: PrincipalKind::Human, id: id.into(), org_id: "org".into() }
    }

    #[test]
    fn operations_and_ttl_are_bounded() {
        assert!(validate_operations(&[]).is_err());
        assert!(validate_operations(&[GrantOperation::Read, GrantOperation::Read]).is_err());
        assert!(validate_operations(&[GrantOperation::Read, GrantOperation::Publish]).is_ok());
        assert!(validate_ttl(59).is_err());
        assert!(validate_ttl(GRANT_MAX_TTL_SECONDS + 1).is_err());
        assert!(validate_ttl(60).is_ok());
    }

    #[test]
    fn enrollment_requires_human_and_bounded_identity() {
        let ok = EnrollDevice { device_id: "dev-1".into(), session_id: "s:1".into(), label: None };
        assert!(validate_enroll(&human("u"), &ok).is_ok());
        let actor = Principal { kind: PrincipalKind::Actor, ..human("u") };
        assert_eq!(validate_enroll(&actor, &ok), Err(Error::Forbidden("enrollment_owner_must_be_human")));
        for bad in ["", "has space", "slash/", &"x".repeat(129)] {
            let req = EnrollDevice { device_id: bad.into(), ..ok.clone() };
            assert_eq!(validate_enroll(&human("u"), &req), Err(Error::Invalid("invalid_device_identity")));
        }
    }

    #[test]
    fn history_page_reports_explicit_skip() {
        let thread = Thread {
            thread_id: ThreadId::new(), org_id: "org".into(),
            scope: crate::ScopeBinding { kind: crate::ScopeKind::Org, id: "org".into() },
            title: None, idempotency_key: None, created_at: Utc::now(),
        };
        let page = HistoryPage::build(&thread, 2, 5, Vec::new(), 10);
        assert_eq!(page.effective_after_seq, 5);
        assert_eq!(page.next_after_seq, 5);
        assert_eq!(page.skipped.as_ref().map(|s| (s.after_seq, s.through_seq)), Some((2, 5)));
        let page = HistoryPage::build(&thread, 7, 5, Vec::new(), 10);
        assert!(page.skipped.is_none());
        assert_eq!(page.effective_after_seq, 7);
    }
}
