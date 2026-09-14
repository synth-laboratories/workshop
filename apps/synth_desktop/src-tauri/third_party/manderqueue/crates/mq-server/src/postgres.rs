//! Postgres [`mq_core::Store`] adapter.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mq_core::{
    publish_fingerprint, BufferedPublish, Cap, CreateThread, DeliveryJob, DeliveryJobId,
    DeliveryStatus, Error, Message, MessageId, MessageKind, Participant, Principal, PrincipalKind,
    PublishMessage, Result, Role, ScopeBinding, ScopeKind, Store, Thread, ThreadId,
};
use mq_core::grants::{
    check_grant_access, check_grant_create, check_grant_issuance, check_grant_mutation,
    delivery_allowed, enrollment_principal, is_enrollment_principal, HISTORY_MAX_LIMIT,
};
use mq_core::{
    CreateGrant, DeliveryGrant, EnrollDevice, Enrollment, Grant, GrantFence, GrantFilter,
    GrantIssuanceRequest, GrantMutation, GrantOperation, GrantState, GrantStatus,
};
use serde_json::Value as JsonValue;
use sqlx::{postgres::PgPoolOptions, FromRow, PgConnection, PgPool};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

/// Postgres write-path metrics for load tests.
#[derive(Debug, Default)]
pub struct PgWriteMetrics {
    /// Begun/committed write transactions on the hot path.
    pub write_txns: AtomicU64,
    pub message_rows: AtomicU64,
    pub job_rows: AtomicU64,
}

impl PgWriteMetrics {
    pub fn snapshot(&self) -> PgWriteSnapshot {
        PgWriteSnapshot {
            write_txns: self.write_txns.load(Ordering::Relaxed),
            message_rows: self.message_rows.load(Ordering::Relaxed),
            job_rows: self.job_rows.load(Ordering::Relaxed),
        }
    }

    pub fn reset(&self) {
        self.write_txns.store(0, Ordering::Relaxed);
        self.message_rows.store(0, Ordering::Relaxed);
        self.job_rows.store(0, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PgWriteSnapshot {
    pub write_txns: u64,
    pub message_rows: u64,
    pub job_rows: u64,
}

#[derive(Clone)]
pub struct PostgresStore {
    pool: PgPool,
    metrics: Option<Arc<PgWriteMetrics>>,
}

impl PostgresStore {
    pub async fn connect(database_url: &str) -> std::result::Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self {
            pool,
            metrics: None,
        })
    }

    pub fn with_metrics(mut self, metrics: Arc<PgWriteMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub fn metrics(&self) -> Option<Arc<PgWriteMetrics>> {
        self.metrics.clone()
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn note_txn(&self) {
        if let Some(m) = &self.metrics {
            m.write_txns.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn note_messages(&self, n: u64) {
        if let Some(m) = &self.metrics {
            m.message_rows.fetch_add(n, Ordering::Relaxed);
        }
    }

    fn note_jobs(&self, n: u64) {
        if let Some(m) = &self.metrics {
            m.job_rows.fetch_add(n, Ordering::Relaxed);
        }
    }
}

fn kind_str(k: PrincipalKind) -> &'static str {
    match k {
        PrincipalKind::Human => "human",
        PrincipalKind::InternSync => "intern_sync",
        PrincipalKind::InternAsync => "intern_async",
        PrincipalKind::Actor => "actor",
        PrincipalKind::System => "system",
    }
}

fn parse_kind(s: &str) -> Result<PrincipalKind> {
    match s {
        "human" => Ok(PrincipalKind::Human),
        "intern_sync" => Ok(PrincipalKind::InternSync),
        "intern_async" => Ok(PrincipalKind::InternAsync),
        "actor" => Ok(PrincipalKind::Actor),
        "system" => Ok(PrincipalKind::System),
        _ => Err(Error::Invalid("principal_kind")),
    }
}

fn scope_str(k: ScopeKind) -> &'static str {
    match k {
        ScopeKind::Org => "org",
        ScopeKind::Factory => "factory",
        ScopeKind::Effort => "effort",
        ScopeKind::Project => "project",
        ScopeKind::SyncSession => "sync_session",
        ScopeKind::AsyncRuntime => "async_runtime",
    }
}

fn parse_scope(s: &str) -> Result<ScopeKind> {
    match s {
        "org" => Ok(ScopeKind::Org),
        "factory" => Ok(ScopeKind::Factory),
        "effort" => Ok(ScopeKind::Effort),
        "project" => Ok(ScopeKind::Project),
        "sync_session" => Ok(ScopeKind::SyncSession),
        "async_runtime" => Ok(ScopeKind::AsyncRuntime),
        "run" => Err(Error::Invalid("scope_run_forbidden")),
        _ => Err(Error::Invalid("scope_kind")),
    }
}

fn cap_strs(caps: &[Cap]) -> Vec<String> {
    caps.iter()
        .map(|c| {
            match c {
                Cap::Read => "read",
                Cap::Publish => "publish",
                Cap::Invite => "invite",
                Cap::Close => "close",
            }
            .to_string()
        })
        .collect()
}

fn parse_caps(v: &[String]) -> Vec<Cap> {
    v.iter()
        .filter_map(|s| match s.as_str() {
            "read" => Some(Cap::Read),
            "publish" => Some(Cap::Publish),
            "invite" => Some(Cap::Invite),
            "close" => Some(Cap::Close),
            _ => None,
        })
        .collect()
}

fn msg_kind_str(k: MessageKind) -> &'static str {
    match k {
        MessageKind::Ask => "ask",
        MessageKind::Answer => "answer",
        MessageKind::Steer => "steer",
        MessageKind::Notice => "notice",
        MessageKind::ActorRuntime => "actor_runtime",
        MessageKind::Blocker => "blocker",
        MessageKind::HandoffPing => "handoff_ping",
    }
}

fn parse_msg_kind(s: &str) -> Result<MessageKind> {
    match s {
        "ask" => Ok(MessageKind::Ask),
        "answer" => Ok(MessageKind::Answer),
        "steer" => Ok(MessageKind::Steer),
        "notice" => Ok(MessageKind::Notice),
        "actor_runtime" => Ok(MessageKind::ActorRuntime),
        "blocker" => Ok(MessageKind::Blocker),
        "handoff_ping" => Ok(MessageKind::HandoffPing),
        _ => Err(Error::Invalid("message_kind")),
    }
}

fn delivery_str(s: DeliveryStatus) -> &'static str {
    match s {
        DeliveryStatus::Pending => "pending",
        DeliveryStatus::Dispatched => "dispatched",
        DeliveryStatus::AwaitingPull => "awaiting_pull",
        DeliveryStatus::NotRoutable => "not_routable",
        DeliveryStatus::Delivered => "delivered",
        DeliveryStatus::DeadLetter => "dead_letter",
    }
}

fn parse_delivery(s: &str) -> Result<DeliveryStatus> {
    match s {
        "pending" => Ok(DeliveryStatus::Pending),
        "dispatched" => Ok(DeliveryStatus::Dispatched),
        "awaiting_pull" => Ok(DeliveryStatus::AwaitingPull),
        "not_routable" => Ok(DeliveryStatus::NotRoutable),
        "delivered" => Ok(DeliveryStatus::Delivered),
        "dead_letter" => Ok(DeliveryStatus::DeadLetter),
        _ => Err(Error::Invalid("delivery_status")),
    }
}

#[derive(FromRow)]
struct ThreadRow {
    thread_id: Uuid,
    org_id: String,
    scope_kind: String,
    scope_id: String,
    title: Option<String>,
    idempotency_key: Option<String>,
    created_at: DateTime<Utc>,
}

impl ThreadRow {
    fn into_thread(self) -> Result<Thread> {
        Ok(Thread {
            thread_id: ThreadId(self.thread_id),
            org_id: self.org_id,
            scope: ScopeBinding {
                kind: parse_scope(&self.scope_kind)?,
                id: self.scope_id,
            },
            title: self.title,
            idempotency_key: self.idempotency_key,
            created_at: self.created_at,
        })
    }
}

#[derive(FromRow)]
struct ParticipantRow {
    principal_kind: String,
    principal_id: String,
    org_id: String,
    role: String,
    caps: Vec<String>,
    grant_generation: i64,
}

impl ParticipantRow {
    fn into_participant(self) -> Result<Participant> {
        let role = parse_role(&self.role)?;
        Ok(Participant {
            principal: Principal {
                kind: parse_kind(&self.principal_kind)?,
                id: self.principal_id,
                org_id: self.org_id,
            },
            role,
            grant_generation: u64::try_from(self.grant_generation).map_err(|_| Error::Invalid("grant_generation"))?,
            caps: {
                let stored = parse_caps(&self.caps);
                if stored.is_empty() {
                    role.caps()
                } else {
                    stored
                }
            },
        })
    }
}

#[derive(FromRow)]
struct MessageRow {
    message_id: Uuid,
    thread_id: Uuid,
    seq: i64,
    kind: String,
    body: String,
    payload: JsonValue,
    sender_kind: String,
    sender_id: String,
    sender_org_id: String,
    idempotency_key: Option<String>,
    correlation_id: Option<String>,
    parent_message_id: Option<Uuid>,
    causation_id: Option<String>,
    created_at: DateTime<Utc>,
}

impl MessageRow {
    fn into_message(self) -> Result<Message> {
        Ok(Message {
            message_id: MessageId(self.message_id),
            thread_id: ThreadId(self.thread_id),
            seq: self.seq as u64,
            kind: parse_msg_kind(&self.kind)?,
            body: self.body,
            payload: self.payload,
            sender: Principal {
                kind: parse_kind(&self.sender_kind)?,
                id: self.sender_id,
                org_id: self.sender_org_id,
            },
            idempotency_key: self.idempotency_key,
            correlation_id: self.correlation_id,
            parent_message_id: self.parent_message_id.map(MessageId),
            causation_id: self.causation_id,
            created_at: self.created_at,
        })
    }
}

#[derive(FromRow)]
struct JobRow {
    job_id: Uuid,
    message_id: Uuid,
    thread_id: Uuid,
    recipient_kind: String,
    recipient_id: String,
    recipient_org_id: String,
    status: String,
    attempts: i32,
    lease_until: Option<DateTime<Utc>>,
    next_attempt_at: Option<DateTime<Utc>>,
}

impl JobRow {
    fn into_job(self) -> Result<DeliveryJob> {
        Ok(DeliveryJob {
            job_id: DeliveryJobId(self.job_id),
            message_id: MessageId(self.message_id),
            thread_id: ThreadId(self.thread_id),
            recipient: Principal {
                kind: parse_kind(&self.recipient_kind)?,
                id: self.recipient_id,
                org_id: self.recipient_org_id,
            },
            status: parse_delivery(&self.status)?,
            attempts: self.attempts as u32,
            lease_until: self.lease_until,
            next_attempt_at: self.next_attempt_at,
        })
    }
}

fn map_db(err: sqlx::Error) -> Error {
    Error::Invalid(match &err {
        sqlx::Error::Database(d) if d.constraint().is_some() => "db_constraint",
        _ => "db_error",
    })
}

const THREAD_COLUMNS: &str =
    "thread_id, org_id, scope_kind, scope_id, title, idempotency_key, created_at";
const PARTICIPANT_COLUMNS: &str =
    "principal_kind, principal_id, org_id, role, caps, grant_generation";
const ENROLLMENT_COLUMNS: &str = "enrollment_id, org_id, owner_kind, owner_id, device_id, session_id, label, incarnation, revoked_at, created_at, updated_at";
const GRANT_COLUMNS: &str = "grant_id, org_id, thread_id, enrollment_id, principal_kind, principal_id, operations, history_after_seq, expires_at, generation, status, granted_by_kind, granted_by_id, created_at, updated_at";

#[derive(FromRow)]
struct EnrollmentRow {
    enrollment_id: Uuid,
    org_id: String,
    owner_kind: String,
    owner_id: String,
    device_id: String,
    session_id: String,
    label: Option<String>,
    incarnation: i64,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl EnrollmentRow {
    fn into_enrollment(self) -> Result<Enrollment> {
        Ok(Enrollment {
            principal: enrollment_principal(&self.org_id, self.enrollment_id),
            owner: Principal {
                kind: parse_kind(&self.owner_kind)?,
                id: self.owner_id,
                org_id: self.org_id.clone(),
            },
            enrollment_id: self.enrollment_id,
            org_id: self.org_id,
            device_id: self.device_id,
            session_id: self.session_id,
            label: self.label,
            incarnation: u64::try_from(self.incarnation).map_err(|_| Error::Invalid("incarnation"))?,
            revoked_at: self.revoked_at,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

#[derive(FromRow)]
struct GrantRow {
    grant_id: Uuid,
    org_id: String,
    thread_id: Uuid,
    enrollment_id: Uuid,
    principal_kind: String,
    principal_id: String,
    operations: Vec<String>,
    history_after_seq: i64,
    expires_at: DateTime<Utc>,
    generation: i64,
    status: String,
    granted_by_kind: String,
    granted_by_id: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl GrantRow {
    /// Incarnation and state are filled by [`Grant::view`].
    fn into_grant(self) -> Result<Grant> {
        Ok(Grant {
            grant_id: self.grant_id,
            thread_id: ThreadId(self.thread_id),
            enrollment_id: self.enrollment_id,
            principal: Principal {
                kind: parse_kind(&self.principal_kind)?,
                id: self.principal_id,
                org_id: self.org_id.clone(),
            },
            operations: self
                .operations
                .iter()
                .map(|op| GrantOperation::parse(op).ok_or(Error::Invalid("grant_operation")))
                .collect::<Result<Vec<_>>>()?,
            history_after_seq: u64::try_from(self.history_after_seq).map_err(|_| Error::Invalid("history_after_seq"))?,
            expires_at: self.expires_at,
            incarnation: 0,
            generation: u64::try_from(self.generation).map_err(|_| Error::Invalid("grant_generation"))?,
            status: GrantStatus::parse(&self.status).ok_or(Error::Invalid("grant_status"))?,
            state: GrantState::Active,
            granted_by: Principal {
                kind: parse_kind(&self.granted_by_kind)?,
                id: self.granted_by_id,
                org_id: self.org_id.clone(),
            },
            org_id: self.org_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

async fn load_enrollment(conn: &mut PgConnection, enrollment_id: Uuid, lock: &str) -> Result<Option<Enrollment>> {
    sqlx::query_as::<_, EnrollmentRow>(&format!(
        "SELECT {ENROLLMENT_COLUMNS} FROM mq_enrollments WHERE enrollment_id=$1 {lock}"
    ))
    .bind(enrollment_id)
    .fetch_optional(conn)
    .await
    .map_err(map_db)?
    .map(EnrollmentRow::into_enrollment)
    .transpose()
}

async fn load_grant(conn: &mut PgConnection, grant_id: Uuid, lock: &str) -> Result<Option<Grant>> {
    sqlx::query_as::<_, GrantRow>(&format!("SELECT {GRANT_COLUMNS} FROM mq_grants WHERE grant_id=$1 {lock}"))
        .bind(grant_id)
        .fetch_optional(conn)
        .await
        .map_err(map_db)?
        .map(GrantRow::into_grant)
        .transpose()
}

/// Members of a thread, or only `only` when given.
async fn load_members(
    conn: &mut PgConnection, thread_id: Uuid, only: Option<&Principal>, lock: &str,
) -> Result<Vec<Participant>> {
    let rows = match only {
        Some(p) => sqlx::query_as::<_, ParticipantRow>(&format!(
            "SELECT {PARTICIPANT_COLUMNS} FROM mq_participants WHERE thread_id=$1 AND principal_kind=$2 AND principal_id=$3 AND org_id=$4 {lock}"))
            .bind(thread_id).bind(kind_str(p.kind)).bind(&p.id).bind(&p.org_id)
            .fetch_all(conn).await,
        None => sqlx::query_as::<_, ParticipantRow>(&format!(
            "SELECT {PARTICIPANT_COLUMNS} FROM mq_participants WHERE thread_id=$1 {lock}"))
            .bind(thread_id).fetch_all(conn).await,
    }
    .map_err(map_db)?;
    rows.into_iter().map(ParticipantRow::into_participant).collect()
}

/// Grant plus enrollment for an operation check, both share-locked.
async fn load_grant_parts(conn: &mut PgConnection, grant_id: Uuid) -> Result<(Grant, Enrollment)> {
    let grant = load_grant(conn, grant_id, "FOR SHARE")
        .await?
        .ok_or(Error::Forbidden("grant_operation_denied"))?;
    let enrollment = load_enrollment(conn, grant.enrollment_id, "FOR SHARE")
        .await?
        .ok_or(Error::Forbidden("grant_operation_denied"))?;
    Ok((grant, enrollment))
}

fn role_str(r: Role) -> &'static str {
    r.as_str()
}

fn parse_role(s: &str) -> Result<Role> {
    Role::parse(s).ok_or(Error::Invalid("role"))
}

#[async_trait]
impl Store for PostgresStore {
    async fn insert_thread(&self, _creator: &Principal, req: CreateThread) -> Result<Thread> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let thread_id = Uuid::new_v4();
        let row = match sqlx::query_as::<_, ThreadRow>(
            r#"
            INSERT INTO mq_threads (thread_id, org_id, scope_kind, scope_id, title, idempotency_key)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING thread_id, org_id, scope_kind, scope_id, title, idempotency_key, created_at
            "#,
        )
        .bind(thread_id)
        .bind(&req.org_id)
        .bind(scope_str(req.scope.kind))
        .bind(&req.scope.id)
        .bind(&req.title)
        .bind(&req.idempotency_key)
        .fetch_one(&mut *tx)
        .await
        {
            Ok(row) => row,
            Err(sqlx::Error::Database(d))
                if d.constraint() == Some("uq_mq_threads_org_idem")
                    || d.message().contains("uq_mq_threads_org_idem") =>
            {
                // Concurrent ensure: re-fetch winner.
                let key = req
                    .idempotency_key
                    .as_deref()
                    .ok_or(Error::Conflict("thread_idempotency"))?;
                drop(tx);
                return self
                    .find_thread_by_idempotency(&req.org_id, key)
                    .await?
                    .ok_or(Error::Conflict("thread_idempotency"));
            }
            Err(e) => return Err(map_db(e)),
        };

        for p in &req.participants {
            let p = p.clone().normalize();
            sqlx::query(
                r#"
                INSERT INTO mq_participants
                  (thread_id, principal_kind, principal_id, org_id, role, caps)
                VALUES ($1, $2, $3, $4, $5, $6)
                "#,
            )
            .bind(thread_id)
            .bind(kind_str(p.principal.kind))
            .bind(&p.principal.id)
            .bind(&p.principal.org_id)
            .bind(role_str(p.role))
            .bind(cap_strs(&p.caps))
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        }

        tx.commit().await.map_err(map_db)?;
        row.into_thread()
    }

    async fn find_thread_by_idempotency(
        &self,
        org_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<Thread>> {
        let row = sqlx::query_as::<_, ThreadRow>(
            r#"
            SELECT thread_id, org_id, scope_kind, scope_id, title, idempotency_key, created_at
            FROM mq_threads
            WHERE org_id = $1 AND idempotency_key = $2
            "#,
        )
        .bind(org_id)
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(|r| r.into_thread()).transpose()
    }

    async fn get_thread(&self, thread_id: ThreadId) -> Result<Option<Thread>> {
        let row = sqlx::query_as::<_, ThreadRow>(
            r#"
            SELECT thread_id, org_id, scope_kind, scope_id, title, idempotency_key, created_at
            FROM mq_threads WHERE thread_id = $1
            "#,
        )
        .bind(thread_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(|r| r.into_thread()).transpose()
    }

    async fn read_scoped(
        &self, actor: &Principal, thread_id: ThreadId, generation: u64,
        after_seq: u64, limit: usize,
    ) -> Result<(Thread, Vec<Message>)> {
        let after_seq = i64::try_from(after_seq).map_err(|_| Error::Invalid("sequence_out_of_range"))?;
        let generation = i64::try_from(generation).map_err(|_| Error::Forbidden("stale_grant_generation"))?;
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        // Same lock order as membership mutation/publish. Revocation cannot
        // commit between the authority check and this bounded snapshot read.
        let row = sqlx::query_as::<_, ThreadRow>(
            "SELECT thread_id, org_id, scope_kind, scope_id, title, idempotency_key, created_at FROM mq_threads WHERE thread_id=$1 FOR SHARE")
            .bind(thread_id.0).fetch_optional(&mut *tx).await.map_err(map_db)?
            .ok_or(Error::NotFound("thread"))?;
        let thread = row.into_thread()?;
        if thread.org_id != actor.org_id {
            return Err(Error::NotFound("thread"));
        }
        let authorized: Option<bool> = sqlx::query_scalar(
            "SELECT grant_generation=$5 AND 'read'=ANY(caps) AND role<>'revoked' FROM mq_participants WHERE thread_id=$1 AND principal_kind=$2 AND principal_id=$3 AND org_id=$4 FOR SHARE")
            .bind(thread_id.0).bind(kind_str(actor.kind)).bind(&actor.id).bind(&actor.org_id)
            .bind(generation).fetch_optional(&mut *tx).await.map_err(map_db)?;
        if authorized != Some(true) {
            return Err(Error::Forbidden("stale_grant_generation"));
        }
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT message_id, thread_id, seq, kind, body, payload, sender_kind, sender_id, sender_org_id, idempotency_key, correlation_id, parent_message_id, causation_id, created_at FROM mq_messages WHERE thread_id=$1 AND seq>$2 ORDER BY seq ASC LIMIT $3")
            .bind(thread_id.0).bind(after_seq).bind(limit.min(HISTORY_MAX_LIMIT + 1) as i64)
            .fetch_all(&mut *tx).await.map_err(map_db)?;
        let messages = rows.into_iter().map(|row| row.into_message()).collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(map_db)?;
        Ok((thread, messages))
    }

    async fn enroll(&self, owner: &Principal, req: EnrollDevice, now: DateTime<Utc>) -> Result<Enrollment> {
        // One statement: a concurrent re-enroll serializes on the unique key.
        // A revoked (signed-out) enrollment is never advanced or reused.
        sqlx::query_as::<_, EnrollmentRow>(&format!(
            "INSERT INTO mq_enrollments (enrollment_id, org_id, owner_kind, owner_id, device_id, session_id, label, incarnation, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,1,$8,$8)
             ON CONFLICT ON CONSTRAINT uq_mq_enrollments_owner_device_session DO UPDATE SET
               incarnation = mq_enrollments.incarnation + 1,
               label = COALESCE(EXCLUDED.label, mq_enrollments.label),
               updated_at = EXCLUDED.updated_at
             WHERE mq_enrollments.revoked_at IS NULL
             RETURNING {ENROLLMENT_COLUMNS}"))
            .bind(Uuid::new_v4()).bind(&owner.org_id).bind(kind_str(owner.kind)).bind(&owner.id)
            .bind(&req.device_id).bind(&req.session_id).bind(&req.label).bind(now)
            .fetch_optional(&self.pool).await.map_err(map_db)?
            .ok_or(Error::Forbidden("enrollment_revoked"))?
            .into_enrollment()
    }

    async fn revoke_enrollment(&self, owner: &Principal, enrollment_id: Uuid, now: DateTime<Utc>) -> Result<Enrollment> {
        // Lock order matches every other grant path: threads (sorted), then the
        // enrollment, then grant rows. A grant created concurrently on another
        // thread waits on the enrollment lock and then sees the revocation.
        let threads: Vec<Uuid> = sqlx::query_scalar("SELECT DISTINCT thread_id FROM mq_grants WHERE enrollment_id=$1")
            .bind(enrollment_id).fetch_all(&self.pool).await.map_err(map_db)?;
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        if !threads.is_empty() {
            sqlx::query("SELECT thread_id FROM mq_threads WHERE thread_id = ANY($1) ORDER BY thread_id FOR UPDATE")
                .bind(&threads).fetch_all(&mut *tx).await.map_err(map_db)?;
        }
        let enrollment = load_enrollment(&mut tx, enrollment_id, "FOR UPDATE").await?
            .ok_or(Error::NotFound("enrollment"))?;
        if enrollment.owner != *owner {
            return Err(Error::NotFound("enrollment"));
        }
        if enrollment.is_revoked() {
            tx.commit().await.map_err(map_db)?;
            return Ok(enrollment);
        }
        sqlx::query("UPDATE mq_grants SET status='revoked', generation=generation+1, updated_at=$2 WHERE enrollment_id=$1 AND status='active'")
            .bind(enrollment_id).bind(now).execute(&mut *tx).await.map_err(map_db)?;
        let principal = &enrollment.principal;
        sqlx::query("UPDATE mq_delivery_jobs SET status='dead_letter',lease_until=NULL,next_attempt_at=NULL,updated_at=now() WHERE recipient_kind=$1 AND recipient_id=$2 AND recipient_org_id=$3 AND status='pending'")
            .bind(kind_str(principal.kind)).bind(&principal.id).bind(&principal.org_id)
            .execute(&mut *tx).await.map_err(map_db)?;
        let revoked = sqlx::query_as::<_, EnrollmentRow>(&format!(
            "UPDATE mq_enrollments SET revoked_at=$2, updated_at=$2 WHERE enrollment_id=$1 RETURNING {ENROLLMENT_COLUMNS}"))
            .bind(enrollment_id).bind(now).fetch_one(&mut *tx).await.map_err(map_db)?
            .into_enrollment()?;
        tx.commit().await.map_err(map_db)?;
        Ok(revoked)
    }

    async fn get_enrollment(&self, enrollment_id: Uuid) -> Result<Option<Enrollment>> {
        let mut conn = self.pool.acquire().await.map_err(map_db)?;
        load_enrollment(&mut conn, enrollment_id, "").await
    }

    async fn list_enrollments(&self, owner: &Principal) -> Result<Vec<Enrollment>> {
        sqlx::query_as::<_, EnrollmentRow>(&format!(
            "SELECT {ENROLLMENT_COLUMNS} FROM mq_enrollments WHERE org_id=$1 AND owner_kind=$2 AND owner_id=$3 ORDER BY created_at, enrollment_id"))
            .bind(&owner.org_id).bind(kind_str(owner.kind)).bind(&owner.id)
            .fetch_all(&self.pool).await.map_err(map_db)?
            .into_iter().map(EnrollmentRow::into_enrollment).collect()
    }

    async fn create_grant(&self, actor: &Principal, req: CreateGrant, now: DateTime<Utc>) -> Result<Grant> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        // Same lock order as membership mutation and publish: thread first.
        let thread = sqlx::query_as::<_, ThreadRow>(&format!(
            "SELECT {THREAD_COLUMNS} FROM mq_threads WHERE thread_id=$1 FOR UPDATE"))
            .bind(req.thread_id.0).fetch_optional(&mut *tx).await.map_err(map_db)?
            .ok_or(Error::NotFound("thread"))?.into_thread()?;
        if thread.org_id != actor.org_id {
            return Err(Error::NotFound("thread"));
        }
        let enrollment = load_enrollment(&mut tx, req.enrollment_id, "FOR SHARE").await?
            .ok_or(Error::NotFound("enrollment"))?;
        let members = load_members(&mut tx, req.thread_id.0, None, "FOR UPDATE").await?;
        let head: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM mq_messages WHERE thread_id=$1")
            .bind(req.thread_id.0).fetch_one(&mut *tx).await.map_err(map_db)?;
        let head = u64::try_from(head).map_err(|_| Error::Invalid("sequence_out_of_range"))?;
        let (floor, add) = check_grant_create(actor, &thread, &members, &enrollment, &req, head)?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mq_grants WHERE thread_id=$1 AND enrollment_id=$2)")
            .bind(req.thread_id.0).bind(req.enrollment_id).fetch_one(&mut *tx).await.map_err(map_db)?;
        if exists {
            return Err(Error::Conflict("grant_exists"));
        }
        if let Some(p) = add {
            sqlx::query("INSERT INTO mq_participants(thread_id,principal_kind,principal_id,org_id,role,caps) VALUES($1,$2,$3,$4,$5,$6)")
                .bind(req.thread_id.0).bind(kind_str(p.principal.kind)).bind(&p.principal.id)
                .bind(&p.principal.org_id).bind(role_str(p.role)).bind(cap_strs(&p.caps))
                .execute(&mut *tx).await.map_err(map_db)?;
        }
        let operations: Vec<String> = req.operations.iter().map(|op| op.as_str().to_string()).collect();
        let grant = sqlx::query_as::<_, GrantRow>(&format!(
            "INSERT INTO mq_grants ({GRANT_COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,0,'active',$10,$11,$12,$12)
             RETURNING {GRANT_COLUMNS}"))
            .bind(Uuid::new_v4()).bind(&thread.org_id).bind(req.thread_id.0).bind(enrollment.enrollment_id)
            .bind(kind_str(enrollment.principal.kind)).bind(&enrollment.principal.id).bind(&operations)
            .bind(i64::try_from(floor).map_err(|_| Error::Invalid("invalid_history_bound"))?)
            .bind(now + chrono::Duration::seconds(req.ttl_seconds))
            .bind(kind_str(actor.kind)).bind(&actor.id).bind(now)
            .fetch_one(&mut *tx).await.map_err(map_db)?
            .into_grant()?;
        tx.commit().await.map_err(map_db)?;
        Ok(grant.view(&enrollment, now))
    }

    async fn get_grant(&self, grant_id: Uuid, now: DateTime<Utc>) -> Result<Option<(Grant, Enrollment)>> {
        let mut conn = self.pool.acquire().await.map_err(map_db)?;
        let Some(grant) = load_grant(&mut conn, grant_id, "").await? else { return Ok(None) };
        let enrollment = load_enrollment(&mut conn, grant.enrollment_id, "").await?
            .ok_or(Error::NotFound("enrollment"))?;
        Ok(Some((grant.view(&enrollment, now), enrollment)))
    }

    async fn list_grants(&self, org_id: &str, filter: &GrantFilter, now: DateTime<Utc>) -> Result<Vec<(Grant, Enrollment)>> {
        let mut conn = self.pool.acquire().await.map_err(map_db)?;
        let grants = sqlx::query_as::<_, GrantRow>(&format!(
            "SELECT {GRANT_COLUMNS} FROM mq_grants WHERE org_id=$1
               AND ($2::uuid IS NULL OR enrollment_id=$2) AND ($3::uuid IS NULL OR thread_id=$3)
             ORDER BY created_at, grant_id"))
            .bind(org_id).bind(filter.enrollment_id).bind(filter.thread_id.map(|t| t.0))
            .fetch_all(&mut *conn).await.map_err(map_db)?;
        let mut out = Vec::with_capacity(grants.len());
        for row in grants {
            let grant = row.into_grant()?;
            let enrollment = load_enrollment(&mut conn, grant.enrollment_id, "").await?
                .ok_or(Error::NotFound("enrollment"))?;
            out.push((grant.view(&enrollment, now), enrollment));
        }
        Ok(out)
    }

    async fn mutate_grant(&self, actor: &Principal, grant_id: Uuid, mutation: GrantMutation, now: DateTime<Utc>) -> Result<Grant> {
        // thread_id is immutable; read it first so the thread lock is taken first.
        let thread_id: Uuid = sqlx::query_scalar("SELECT thread_id FROM mq_grants WHERE grant_id=$1")
            .bind(grant_id).fetch_optional(&self.pool).await.map_err(map_db)?
            .ok_or(Error::NotFound("grant"))?;
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        sqlx::query("SELECT 1 FROM mq_threads WHERE thread_id=$1 FOR UPDATE")
            .bind(thread_id).execute(&mut *tx).await.map_err(map_db)?;
        let grant = load_grant(&mut tx, grant_id, "FOR UPDATE").await?.ok_or(Error::NotFound("grant"))?;
        if grant.org_id != actor.org_id {
            return Err(Error::NotFound("grant"));
        }
        let enrollment = load_enrollment(&mut tx, grant.enrollment_id, "FOR SHARE").await?
            .ok_or(Error::NotFound("grant"))?;
        let members = load_members(&mut tx, thread_id, None, "FOR SHARE").await?;
        let current = grant.view(&enrollment, now);
        let Some(next) = check_grant_mutation(actor, &current, &enrollment, &members, mutation, now)? else {
            tx.commit().await.map_err(map_db)?;
            return Ok(current);
        };
        sqlx::query("UPDATE mq_grants SET status=$2, generation=$3, expires_at=$4, updated_at=$5 WHERE grant_id=$1")
            .bind(grant_id).bind(next.status.as_str())
            .bind(i64::try_from(next.generation).map_err(|_| Error::Invalid("grant_generation_exhausted"))?)
            .bind(next.expires_at).bind(now)
            .execute(&mut *tx).await.map_err(map_db)?;
        if matches!(mutation, GrantMutation::Revoke) {
            sqlx::query("UPDATE mq_delivery_jobs SET status='dead_letter',lease_until=NULL,next_attempt_at=NULL,updated_at=now() WHERE thread_id=$1 AND recipient_kind=$2 AND recipient_id=$3 AND recipient_org_id=$4 AND status='pending'")
                .bind(thread_id).bind(kind_str(next.principal.kind)).bind(&next.principal.id).bind(&next.principal.org_id)
                .execute(&mut *tx).await.map_err(map_db)?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(next.view(&enrollment, now))
    }

    async fn grant_issuance(&self, actor: &Principal, grant_id: Uuid, req: GrantIssuanceRequest, now: DateTime<Utc>) -> Result<Grant> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let grant = load_grant(&mut tx, grant_id, "FOR SHARE").await?.ok_or(Error::NotFound("grant"))?;
        if grant.org_id != actor.org_id {
            return Err(Error::NotFound("grant"));
        }
        let enrollment = load_enrollment(&mut tx, grant.enrollment_id, "FOR SHARE").await?
            .ok_or(Error::NotFound("grant"))?;
        let members = load_members(&mut tx, grant.thread_id.0, Some(&grant.principal), "FOR SHARE").await?;
        let current = grant.view(&enrollment, now);
        check_grant_issuance(actor, &current, &enrollment, &members, &req, now)?;
        tx.commit().await.map_err(map_db)?;
        Ok(current)
    }

    async fn read_granted(
        &self, actor: &Principal, thread_id: ThreadId, fence: &GrantFence,
        after_seq: u64, limit: usize,
    ) -> Result<(Thread, Grant, Vec<Message>)> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        // Same lock order as membership/grant mutation. Revocation cannot
        // commit between this authority check and the bounded snapshot read.
        let thread = sqlx::query_as::<_, ThreadRow>(&format!(
            "SELECT {THREAD_COLUMNS} FROM mq_threads WHERE thread_id=$1 FOR SHARE"))
            .bind(thread_id.0).fetch_optional(&mut *tx).await.map_err(map_db)?
            .ok_or(Error::NotFound("thread"))?.into_thread()?;
        if thread.org_id != actor.org_id {
            return Err(Error::NotFound("thread"));
        }
        let (grant, enrollment) = load_grant_parts(&mut tx, fence.grant_id).await?;
        let members = load_members(&mut tx, thread_id.0, Some(actor), "FOR SHARE").await?;
        check_grant_access(actor, thread_id, &grant, &enrollment, &members, fence, GrantOperation::Read)?;
        let effective = i64::try_from(after_seq.max(grant.history_after_seq))
            .map_err(|_| Error::Invalid("sequence_out_of_range"))?;
        let limit = limit.min(HISTORY_MAX_LIMIT + 1);
        let messages = if limit == 0 {
            Vec::new()
        } else {
            sqlx::query_as::<_, MessageRow>(
                "SELECT message_id, thread_id, seq, kind, body, payload, sender_kind, sender_id, sender_org_id, idempotency_key, correlation_id, parent_message_id, causation_id, created_at FROM mq_messages WHERE thread_id=$1 AND seq>$2 ORDER BY seq ASC LIMIT $3")
                .bind(thread_id.0).bind(effective).bind(limit as i64)
                .fetch_all(&mut *tx).await.map_err(map_db)?
                .into_iter().map(MessageRow::into_message).collect::<Result<Vec<_>>>()?
        };
        tx.commit().await.map_err(map_db)?;
        Ok((thread, grant.view(&enrollment, fence.at), messages))
    }

    async fn delivery_grant(
        &self, thread_id: ThreadId, recipient: &Principal, message_seq: u64, now: DateTime<Utc>,
    ) -> Result<DeliveryGrant> {
        if !is_enrollment_principal(recipient) {
            return Ok(DeliveryGrant::NotGoverned);
        }
        let mut conn = self.pool.acquire().await.map_err(map_db)?;
        let grant = sqlx::query_as::<_, GrantRow>(&format!(
            "SELECT {GRANT_COLUMNS} FROM mq_grants WHERE thread_id=$1 AND principal_kind=$2 AND principal_id=$3 AND org_id=$4"))
            .bind(thread_id.0).bind(kind_str(recipient.kind)).bind(&recipient.id).bind(&recipient.org_id)
            .fetch_optional(&mut *conn).await.map_err(map_db)?
            .map(GrantRow::into_grant).transpose()?;
        let members = load_members(&mut conn, thread_id.0, Some(recipient), "").await?;
        let enrollment = match &grant {
            Some(grant) => load_enrollment(&mut conn, grant.enrollment_id, "").await?,
            None => None,
        };
        match (grant, enrollment) {
            (Some(grant), Some(enrollment))
                if delivery_allowed(Some(&grant), Some(&enrollment), &members, message_seq, now) =>
            {
                Ok(DeliveryGrant::Allowed(grant.view(&enrollment, now)))
            }
            _ => Ok(DeliveryGrant::Denied),
        }
    }

    async fn list_threads(
        &self,
        org_id: &str,
        scope: Option<&ScopeBinding>,
    ) -> Result<Vec<Thread>> {
        let rows = if let Some(scope) = scope {
            sqlx::query_as::<_, ThreadRow>(
                r#"
                SELECT thread_id, org_id, scope_kind, scope_id, title, idempotency_key, created_at
                FROM mq_threads
                WHERE org_id = $1 AND scope_kind = $2 AND scope_id = $3
                ORDER BY created_at DESC
                "#,
            )
            .bind(org_id)
            .bind(scope_str(scope.kind))
            .bind(&scope.id)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, ThreadRow>(
                r#"
                SELECT thread_id, org_id, scope_kind, scope_id, title, idempotency_key, created_at
                FROM mq_threads WHERE org_id = $1
                ORDER BY created_at DESC
                "#,
            )
            .bind(org_id)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(map_db)?;
        rows.into_iter().map(|r| r.into_thread()).collect()
    }

    async fn has_cap(&self, thread_id: ThreadId, principal: &Principal, cap: Cap) -> Result<bool> {
        let caps: Option<Vec<String>> = sqlx::query_scalar(
            r#"
            SELECT caps FROM mq_participants
            WHERE thread_id = $1 AND principal_kind = $2 AND principal_id = $3 AND org_id = $4
            "#,
        )
        .bind(thread_id.0)
        .bind(kind_str(principal.kind))
        .bind(&principal.id)
        .bind(&principal.org_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(caps.map(|c| parse_caps(&c).contains(&cap)).unwrap_or(false))
    }

    async fn add_participant(&self, thread_id: ThreadId, participant: Participant) -> Result<()> {
        let participant = participant.normalize();
        let res = sqlx::query(
            r#"
            INSERT INTO mq_participants
              (thread_id, principal_kind, principal_id, org_id, role, caps)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(thread_id.0)
        .bind(kind_str(participant.principal.kind))
        .bind(&participant.principal.id)
        .bind(&participant.principal.org_id)
        .bind(role_str(participant.role))
        .bind(cap_strs(&participant.caps))
        .execute(&self.pool)
        .await;
        match res {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(d)) if d.constraint().is_some() => {
                Err(Error::Conflict("participant_exists"))
            }
            Err(e) => Err(map_db(e)),
        }
    }

    async fn set_participant_role(
        &self,
        thread_id: ThreadId,
        principal: &Principal,
        role: Role,
    ) -> Result<()> {
        let caps = role.caps();
        let res = sqlx::query(
            r#"
            UPDATE mq_participants
            SET role = $5, caps = $6,
                grant_generation = grant_generation + CASE WHEN $5='revoked' AND role<>'revoked' THEN 1 ELSE 0 END
            WHERE thread_id = $1 AND principal_kind = $2 AND principal_id = $3 AND org_id = $4
            "#,
        )
        .bind(thread_id.0)
        .bind(kind_str(principal.kind))
        .bind(&principal.id)
        .bind(&principal.org_id)
        .bind(role_str(role))
        .bind(cap_strs(&caps))
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        if res.rows_affected() == 0 {
            return Err(Error::NotFound("participant"));
        }
        Ok(())
    }

    async fn mutate_participant(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
        target: Participant,
        create: bool,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let org: String =
            sqlx::query_scalar("SELECT org_id FROM mq_threads WHERE thread_id=$1 FOR UPDATE")
                .bind(thread_id.0)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_db)?
                .ok_or(Error::NotFound("thread"))?;
        let rows = sqlx::query_as::<_, ParticipantRow>("SELECT principal_kind, principal_id, org_id, role, caps, grant_generation FROM mq_participants WHERE thread_id=$1 FOR UPDATE")
            .bind(thread_id.0).fetch_all(&mut *tx).await.map_err(map_db)?;
        let members = rows
            .into_iter()
            .map(|r| r.into_participant())
            .collect::<Result<Vec<_>>>()?;
        let target = target.normalize();
        if mq_core::validate_participant_change(&org, actor, &members, &target, create)? {
            sqlx::query("INSERT INTO mq_participants(thread_id,principal_kind,principal_id,org_id,role,caps) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(thread_id,principal_kind,principal_id,org_id) DO UPDATE SET role=EXCLUDED.role,caps=EXCLUDED.caps,grant_generation=mq_participants.grant_generation + CASE WHEN EXCLUDED.role='revoked' AND mq_participants.role<>'revoked' THEN 1 ELSE 0 END")
                .bind(thread_id.0).bind(kind_str(target.principal.kind)).bind(&target.principal.id)
                .bind(&target.principal.org_id).bind(role_str(target.role)).bind(cap_strs(&target.caps))
                .execute(&mut *tx).await.map_err(map_db)?;
        }
        if target.role == Role::Revoked {
            sqlx::query("UPDATE mq_delivery_jobs SET status='dead_letter',lease_until=NULL,next_attempt_at=NULL,updated_at=now() WHERE thread_id=$1 AND recipient_kind=$2 AND recipient_id=$3 AND recipient_org_id=$4 AND status='pending'")
                .bind(thread_id.0).bind(kind_str(target.principal.kind)).bind(&target.principal.id).bind(&target.principal.org_id)
                .execute(&mut *tx).await.map_err(map_db)?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn append_message(
        &self,
        thread_id: ThreadId,
        sender: &Principal,
        req: PublishMessage,
    ) -> Result<(Message, bool)> {
        self.append_with_delivery(thread_id, sender, req, &[]).await
    }

    async fn append_with_delivery(
        &self,
        thread_id: ThreadId,
        sender: &Principal,
        req: PublishMessage,
        recipients: &[Principal],
    ) -> Result<(Message, bool)> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;

        let thread_org: String =
            sqlx::query_scalar("SELECT org_id FROM mq_threads WHERE thread_id = $1 FOR UPDATE")
                .bind(thread_id.0)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_db)?
                .ok_or(Error::NotFound("thread"))?;

        if thread_org != sender.org_id {
            return Err(Error::Forbidden("org_workspace_mismatch"));
        }
        if let Some(fence) = req.grant_fence.as_ref() {
            // Under the thread lock, before idempotent replay or insert.
            let (grant, enrollment) = load_grant_parts(&mut tx, fence.grant_id).await?;
            let members = load_members(&mut tx, thread_id.0, Some(sender), "FOR SHARE").await?;
            check_grant_access(sender, thread_id, &grant, &enrollment, &members, fence, GrantOperation::Publish)?;
        }
        let publisher: Option<bool> = sqlx::query_scalar("SELECT 'publish'=ANY(caps) FROM mq_participants WHERE thread_id=$1 AND principal_kind=$2 AND principal_id=$3 AND org_id=$4 FOR SHARE")
            .bind(thread_id.0).bind(kind_str(sender.kind)).bind(&sender.id).bind(&sender.org_id)
            .fetch_optional(&mut *tx).await.map_err(map_db)?;
        if publisher != Some(true) {
            return Err(Error::Forbidden("publish_membership_required"));
        }
        if let Some(expected) = req.expected_grant_generation {
            let generation: i64 = sqlx::query_scalar("SELECT grant_generation FROM mq_participants WHERE thread_id=$1 AND principal_kind=$2 AND principal_id=$3 AND org_id=$4 FOR SHARE")
                .bind(thread_id.0).bind(kind_str(sender.kind)).bind(&sender.id).bind(&sender.org_id)
                .fetch_one(&mut *tx).await.map_err(map_db)?;
            if u64::try_from(generation).ok() != Some(expected) {
                return Err(Error::Forbidden("stale_grant_generation"));
            }
        }
        let fingerprint = publish_fingerprint(&req);
        if let Some(key) = req.idempotency_key.as_ref() {
            let existing = sqlx::query_as::<_, MessageRow>(
                r#"
                SELECT message_id, thread_id, seq, kind, body, payload,
                       sender_kind, sender_id, sender_org_id,
                       idempotency_key, correlation_id, parent_message_id, causation_id, created_at
                FROM mq_messages
                WHERE org_id = $1 AND idempotency_key = $2 AND thread_id = $3 AND sender_kind = $4 AND sender_id = $5
                "#,
            )
            .bind(&thread_org)
            .bind(key)
            .bind(thread_id.0).bind(kind_str(sender.kind)).bind(&sender.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_db)?;
            if let Some(row) = existing {
                let msg = row.into_message()?;
                if msg.thread_id != thread_id {
                    return Err(Error::Conflict("idempotency_key_reuse"));
                }
                let (stored_fingerprint, stored_recipients): (Option<String>, JsonValue) = sqlx::query_as(
                    "SELECT request_fingerprint, delivery_recipients FROM mq_messages WHERE message_id = $1")
                    .bind(msg.message_id.0).fetch_one(&mut *tx).await.map_err(map_db)?;
                let stored_fingerprint =
                    stored_fingerprint.ok_or(Error::Conflict("legacy_publish_unverifiable"))?;
                if stored_fingerprint != fingerprint {
                    return Err(Error::Conflict("idempotency_payload_mismatch"));
                }
                let frozen: Vec<Principal> = serde_json::from_value(stored_recipients)
                    .map_err(|_| Error::Invalid("stored_delivery_recipients"))?;
                insert_delivery_intent(&mut tx, &msg, &frozen).await?;
                tx.commit().await.map_err(map_db)?;
                self.note_txn();
                return Ok((msg, false));
            }
        }

        let next_seq: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM mq_messages WHERE thread_id = $1",
        )
        .bind(thread_id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_db)?;

        let payload = if req.payload.is_null() {
            serde_json::json!({})
        } else {
            req.payload.clone()
        };

        let row = sqlx::query_as::<_, MessageRow>(
            r#"
            INSERT INTO mq_messages (
              message_id, thread_id, org_id, seq, kind, body, payload,
              sender_kind, sender_id, sender_org_id, idempotency_key, correlation_id,
              parent_message_id, causation_id
            ) VALUES (
              $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14
            )
            RETURNING message_id, thread_id, seq, kind, body, payload,
                      sender_kind, sender_id, sender_org_id,
                      idempotency_key, correlation_id, parent_message_id, causation_id, created_at
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(thread_id.0)
        .bind(&thread_org)
        .bind(next_seq)
        .bind(msg_kind_str(req.kind))
        .bind(&req.body)
        .bind(&payload)
        .bind(kind_str(sender.kind))
        .bind(&sender.id)
        .bind(&sender.org_id)
        .bind(&req.idempotency_key)
        .bind(&req.correlation_id)
        .bind(req.parent_message_id.map(|m| m.0))
        .bind(&req.causation_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_db)?;

        let message = row.into_message()?;
        sqlx::query("UPDATE mq_messages SET request_fingerprint=$2, delivery_recipients=$3 WHERE message_id=$1")
            .bind(message.message_id.0).bind(fingerprint)
            .bind(serde_json::to_value(recipients).map_err(|_| Error::Invalid("delivery_recipients"))?)
            .execute(&mut *tx).await.map_err(map_db)?;
        insert_delivery_intent(&mut tx, &message, recipients).await?;
        tx.commit().await.map_err(map_db)?;
        self.note_txn();
        self.note_messages(1);
        self.note_jobs(recipients.len() as u64);
        Ok((message, true))
    }

    async fn read_messages(
        &self,
        thread_id: ThreadId,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let rows = sqlx::query_as::<_, MessageRow>(
            r#"
            SELECT message_id, thread_id, seq, kind, body, payload,
                   sender_kind, sender_id, sender_org_id,
                   idempotency_key, correlation_id, parent_message_id, causation_id, created_at
            FROM mq_messages
            WHERE thread_id = $1 AND seq > $2
            ORDER BY seq ASC
            LIMIT $3
            "#,
        )
        .bind(thread_id.0)
        .bind(after_seq as i64)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(|r| r.into_message()).collect()
    }

    async fn list_participants(&self, thread_id: ThreadId) -> Result<Vec<Participant>> {
        let rows = sqlx::query_as::<_, ParticipantRow>(
            r#"
            SELECT principal_kind, principal_id, org_id, role, caps, grant_generation
            FROM mq_participants WHERE thread_id = $1
            "#,
        )
        .bind(thread_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(|r| r.into_participant()).collect()
    }

    async fn get_message(&self, message_id: MessageId) -> Result<Option<Message>> {
        let row = sqlx::query_as::<_, MessageRow>(
            r#"
            SELECT message_id, thread_id, seq, kind, body, payload,
                   sender_kind, sender_id, sender_org_id,
                   idempotency_key, correlation_id, parent_message_id, causation_id, created_at
            FROM mq_messages WHERE message_id = $1
            "#,
        )
        .bind(message_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(|r| r.into_message()).transpose()
    }

    async fn enqueue_delivery_jobs(
        &self,
        message: &Message,
        recipients: &[Principal],
    ) -> Result<Vec<DeliveryJob>> {
        let mut out = Vec::new();
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        for recipient in recipients {
            let row = sqlx::query_as::<_, JobRow>(
                r#"
                INSERT INTO mq_delivery_jobs (
                  job_id, message_id, thread_id,
                  recipient_kind, recipient_id, recipient_org_id, status, attempts
                ) VALUES ($1,$2,$3,$4,$5,$6,'pending',0)
                ON CONFLICT (message_id, recipient_kind, recipient_id, recipient_org_id)
                DO UPDATE SET status = mq_delivery_jobs.status
                RETURNING job_id, message_id, thread_id,
                          recipient_kind, recipient_id, recipient_org_id, status, attempts, lease_until, next_attempt_at
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(message.message_id.0)
            .bind(message.thread_id.0)
            .bind(kind_str(recipient.kind))
            .bind(&recipient.id)
            .bind(&recipient.org_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_db)?;
            out.push(row.into_job()?);
        }
        tx.commit().await.map_err(map_db)?;
        self.note_txn();
        self.note_jobs(out.len() as u64);
        Ok(out)
    }

    async fn claim_delivery_jobs(&self, limit: usize) -> Result<Vec<DeliveryJob>> {
        let rows = sqlx::query_as::<_, JobRow>(
            r#"
            UPDATE mq_delivery_jobs
            SET attempts = attempts + 1, updated_at = now(), lease_until = now() + interval '30 seconds'
            WHERE job_id IN (
              SELECT job_id FROM mq_delivery_jobs
              WHERE status = 'pending' AND (lease_until IS NULL OR lease_until <= now())
                AND (next_attempt_at IS NULL OR next_attempt_at <= now())
              ORDER BY created_at ASC
              FOR UPDATE SKIP LOCKED
              LIMIT $1
            )
            RETURNING job_id, message_id, thread_id,
                      recipient_kind, recipient_id, recipient_org_id, status, attempts, lease_until, next_attempt_at
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(|r| r.into_job()).collect()
    }

    async fn settle_delivery_job(
        &self,
        job_id: DeliveryJobId,
        expected_attempt: u32,
        status: DeliveryStatus,
    ) -> Result<()> {
        let res = sqlx::query(
            r#"
            UPDATE mq_delivery_jobs
            SET status = $2, updated_at = now(), lease_until = NULL,
                next_attempt_at = CASE WHEN $2 = 'pending' THEN now() + $4 * interval '1 second' ELSE NULL END
            WHERE job_id = $1 AND attempts = $3 AND status = 'pending' AND lease_until > now()
            "#,
        )
        .bind(job_id.0)
        .bind(delivery_str(status))
        .bind(expected_attempt as i32)
        .bind(mq_core::delivery_backoff_seconds(expected_attempt) as f64)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        if res.rows_affected() == 0 {
            return Err(Error::Conflict("stale_delivery_claim"));
        }
        Ok(())
    }

    async fn flush_write_batch(&self, batch: &[BufferedPublish]) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let mut job_count = 0u64;
        for item in batch {
            sqlx::query(
                r#"
                INSERT INTO mq_messages (
                  message_id, thread_id, org_id, seq, kind, body, payload,
                  sender_kind, sender_id, sender_org_id, idempotency_key, correlation_id,
                  parent_message_id, causation_id
                ) VALUES (
                  $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14
                )
                ON CONFLICT (message_id) DO NOTHING
                "#,
            )
            .bind(item.message.message_id.0)
            .bind(item.message.thread_id.0)
            .bind(&item.org_id)
            .bind(item.message.seq as i64)
            .bind(msg_kind_str(item.message.kind))
            .bind(&item.message.body)
            .bind(&item.message.payload)
            .bind(kind_str(item.message.sender.kind))
            .bind(&item.message.sender.id)
            .bind(&item.message.sender.org_id)
            .bind(&item.message.idempotency_key)
            .bind(&item.message.correlation_id)
            .bind(item.message.parent_message_id.map(|m| m.0))
            .bind(&item.message.causation_id)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;

            for recipient in &item.recipients {
                sqlx::query(
                    r#"
                    INSERT INTO mq_delivery_jobs (
                      job_id, message_id, thread_id,
                      recipient_kind, recipient_id, recipient_org_id, status, attempts, lease_until, next_attempt_at
                    ) VALUES ($1,$2,$3,$4,$5,$6,'pending',0,NULL,NULL)
                    ON CONFLICT (message_id, recipient_kind, recipient_id, recipient_org_id)
                    DO NOTHING
                    "#,
                )
                .bind(Uuid::new_v4())
                .bind(item.message.message_id.0)
                .bind(item.message.thread_id.0)
                .bind(kind_str(recipient.kind))
                .bind(&recipient.id)
                .bind(&recipient.org_id)
                .execute(&mut *tx)
                .await
                .map_err(map_db)?;
                job_count += 1;
            }
        }
        tx.commit().await.map_err(map_db)?;
        self.note_txn();
        self.note_messages(batch.len() as u64);
        self.note_jobs(job_count);
        Ok(())
    }
}

/// Insert missing jobs without resetting settled work. Runs inside message acceptance transaction.
async fn insert_delivery_intent(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    message: &Message,
    recipients: &[Principal],
) -> Result<()> {
    for recipient in recipients {
        if recipient.org_id != message.sender.org_id {
            return Err(Error::Forbidden("org_workspace_mismatch"));
        }
        let readable: Option<bool> = sqlx::query_scalar("SELECT 'read'=ANY(caps) FROM mq_participants WHERE thread_id=$1 AND principal_kind=$2 AND principal_id=$3 AND org_id=$4 FOR SHARE")
            .bind(message.thread_id.0).bind(kind_str(recipient.kind)).bind(&recipient.id).bind(&recipient.org_id)
            .fetch_optional(&mut **tx).await.map_err(map_db)?;
        if readable != Some(true) {
            return Err(Error::Invalid("recipient_not_a_member"));
        }
        sqlx::query("INSERT INTO mq_delivery_jobs (job_id,message_id,thread_id,recipient_kind,recipient_id,recipient_org_id,status,attempts) VALUES ($1,$2,$3,$4,$5,$6,'pending',0) ON CONFLICT (message_id,recipient_kind,recipient_id,recipient_org_id) DO NOTHING")
            .bind(Uuid::new_v4()).bind(message.message_id.0).bind(message.thread_id.0)
            .bind(kind_str(recipient.kind)).bind(&recipient.id).bind(&recipient.org_id)
            .execute(&mut **tx).await.map_err(map_db)?;
    }
    Ok(())
}
