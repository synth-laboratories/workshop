//! Native scoped persistence, held behind the cloud qualification gate.
//!
//! There is deliberately no automatic schema registration or identity discovery.
//! The host must supply a verified identity after the cloud contract is qualified.
//! No renderer, credential hash, profile label alone or legacy row can supply it.
use crate::storage::{append_event, AppEvent, Database, EventAppend, EventSource};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub const MIGRATION_CANDIDATE: &str = include_str!("schema.sql");
const MAX_BODY: usize = 1024 * 1024;
const MAX_PAGE: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudScopeIdentity {
    pub backend_origin: String,
    pub backend_id: String,
    pub account_id: String,
    pub org_id: String,
    pub profile_id: String,
}

impl CloudScopeIdentity {
    fn key(&self) -> Result<String> {
        let url = reqwest::Url::parse(&self.backend_origin)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            bail!("cloud backend identity must be an origin");
        }
        for id in [
            &self.backend_id,
            &self.account_id,
            &self.org_id,
            &self.profile_id,
        ] {
            valid_id(id)?;
        }
        // Canonical origin, not credentials, participates in identity.
        Ok(digest(&serde_json::to_vec(&(
            url.origin().ascii_serialization(),
            &self.backend_id,
            &self.account_id,
            &self.org_id,
            &self.profile_id,
        ))?))
    }
}

/// Opaque local authority handle, invalidated by sign-out, identity switch or boot.
#[derive(Clone, Debug)]
pub struct ScopeLease {
    scope_id: String,
    epoch: i64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Adapter {
    InternSync,
    InternAsync,
    Swarm,
    Mq,
}
impl Adapter {
    fn as_str(self) -> &'static str {
        match self {
            Self::InternSync => "intern_sync",
            Self::InternAsync => "intern_async",
            Self::Swarm => "swarm",
            Self::Mq => "mq",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    pub adapter: Adapter,
    pub external_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "adapter", rename_all = "snake_case")]
pub enum Checkpoint {
    Intern {
        sequence: u64,
        generation: u64,
    },
    Swarm {
        event_id: Option<String>,
        state_version: Option<String>,
        transcript_cursor: Option<String>,
    },
    Mq {
        subscription_id: String,
        sequence: u64,
    },
}

#[derive(Clone, Debug)]
pub struct RemoteEvent {
    pub id: String,
    pub kind: String,
    pub payload: Value,
    pub sequence: Option<u64>,
    pub generation: Option<u64>,
}

#[derive(Clone)]
pub struct CommandIntent {
    pub command_id: String,
    pub stream: Stream,
    pub operation_id: String,
    pub idempotency_key: String,
    pub body: Vec<u8>,
    pub expected_generation: Option<u64>,
}

/// Exact persisted request. Do not create a fresh key after an uncertain send.
#[derive(Clone, PartialEq, Eq)]
pub struct PendingCommand {
    pub command_id: String,
    pub operation_id: String,
    pub idempotency_key: String,
    pub body: Vec<u8>,
    pub body_sha256: String,
    pub state: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptStage {
    Received,
    Delivered,
    Applied,
    Refused,
    Conflict,
}
impl ReceiptStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::Delivered => "delivered",
            Self::Applied => "applied",
            Self::Refused => "refused",
            Self::Conflict => "conflict",
        }
    }
}

impl std::fmt::Debug for PendingCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingCommand")
            .field("command_id", &self.command_id)
            .field("operation_id", &self.operation_id)
            .field("state", &self.state)
            .field("body", &"<redacted>")
            .finish()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteExecutionState {
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}
impl RemoteExecutionState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

pub struct DeliveryReceipt {
    pub command_id: String,
    pub stream: Stream,
    pub stage: ReceiptStage,
    pub detail: Value,
}

#[derive(Clone)]
pub struct CloudStore {
    db: Arc<Database>,
}
impl CloudStore {
    /// One CoreRuntime-owned instance. Reopening invalidates all prior workers;
    /// persisted remote objects remain intact until verified identity is supplied.
    pub fn open(db: Arc<Database>) -> Result<Self> {
        db.transaction(|conn| {
            invalidate(conn)?;
            Ok(())
        })?;
        Ok(Self { db })
    }

    /// Caller must have verified every identity component with the qualified
    /// authority. Currently only offline tests call this; no live caller exists.
    pub fn activate_verified(&self, identity: &CloudScopeIdentity) -> Result<ScopeLease> {
        let key = identity.key()?;
        self.db.transaction(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO cloud_scopes VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    key,
                    identity.backend_origin,
                    identity.backend_id,
                    identity.account_id,
                    identity.org_id,
                    identity.profile_id
                ],
            )?;
            let epoch = invalidate(conn)?;
            conn.execute(
                "UPDATE cloud_auth_state SET active_scope_id=?1 WHERE singleton=1",
                params![key],
            )?;
            Ok(ScopeLease {
                scope_id: key.clone(),
                epoch,
            })
        })
    }

    pub fn sign_out(&self) -> Result<()> {
        self.db.transaction(|conn| {
            invalidate(conn)?;
            Ok(())
        })
    }

    /// Create a fresh scoped conversation and binding atomically. No existing
    /// desktop ID can be supplied, preventing adoption of legacy/local rows.
    pub fn create_conversation(
        &self,
        lease: &ScopeLease,
        stream: &Stream,
        title: &str,
    ) -> Result<String> {
        valid_id(&stream.external_id)?;
        let mode = match stream.adapter {
            Adapter::InternSync => crate::domain::InternMode::Sync,
            Adapter::InternAsync => crate::domain::InternMode::Async,
            _ => bail!("conversation requires an Intern runtime"),
        };
        let target = crate::domain::RuntimeTarget::InternRuntime {
            mode,
            binding: None,
        };
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            let existing:Option<String>=conn.query_row("SELECT local_session_id FROM cloud_session_bindings WHERE scope_id=?1 AND adapter=?2 AND external_id=?3",params![lease.scope_id,stream.adapter.as_str(),stream.external_id],|r|r.get(0)).optional()?;
            if let Some(existing)=existing { return Ok(existing); }
            let id=uuid::Uuid::new_v4().to_string();
            let now=chrono::Utc::now().to_rfc3339();
            conn.execute("INSERT INTO sessions(id,title,kind,target_json,runtime_target_kind,remote_id,status,metadata_json,created_at,updated_at) VALUES(?1,?2,'intern',?3,'intern',?4,'ready','{}',?5,?5)",params![id,title,target.to_json_value().to_string(),stream.external_id,now])?;
            conn.execute("INSERT INTO cloud_owned_sessions VALUES(?1,?2)",params![id,lease.scope_id])?;
            conn.execute("INSERT INTO cloud_session_bindings VALUES(?1,?2,?3,?4)",params![lease.scope_id,stream.adapter.as_str(),stream.external_id,id])?;
            Ok(id)
        })
    }

    pub fn bind_session(
        &self,
        lease: &ScopeLease,
        stream: &Stream,
        local_session_id: &str,
    ) -> Result<()> {
        valid_id(&stream.external_id)?;
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            // Deliberate binding only. Never scan or adopt historical sessions.
            let existing: Option<String> = conn.query_row("SELECT local_session_id FROM cloud_session_bindings WHERE scope_id=?1 AND adapter=?2 AND external_id=?3",params![lease.scope_id,stream.adapter.as_str(),stream.external_id],|r|r.get(0)).optional()?;
            if let Some(id)=existing { if id!=local_session_id { bail!("external session already bound"); } return Ok(()); }
            conn.execute("INSERT INTO cloud_session_bindings VALUES(?1,?2,?3,?4)",params![lease.scope_id,stream.adapter.as_str(),stream.external_id,local_session_id])?;
            Ok(())
        })
    }

    pub fn bound_sessions(&self, lease: &ScopeLease) -> Result<Vec<String>> {
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            let mut stmt=conn.prepare("SELECT local_session_id FROM cloud_owned_sessions WHERE scope_id=?1 ORDER BY local_session_id")?;
            let rows=stmt.query_map(params![lease.scope_id],|r|r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(rows)
        })
    }

    pub fn enqueue(&self, lease: &ScopeLease, intent: &CommandIntent) -> Result<PendingCommand> {
        for id in [
            &intent.command_id,
            &intent.operation_id,
            &intent.idempotency_key,
        ] {
            valid_id(id)?;
        }
        if intent.body.len() > MAX_BODY {
            bail!("command body exceeds limit");
        }
        let body =
            serde_json::from_slice::<Value>(&intent.body).context("command body must be JSON")?;
        if intent.expected_generation.is_some()
            && body.get("expected_generation").and_then(Value::as_u64) != intent.expected_generation
        {
            bail!("stored generation does not match command body");
        }
        let body_digest = digest(&intent.body);
        let generation = intent.expected_generation.map(i64::try_from).transpose()?;
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            binding(conn,lease,&intent.stream)?;
            let existing:Option<(String,String,String,Option<i64>,i64,String)> = conn.query_row(
                "SELECT command_id,body_sha256,external_id,expected_generation,auth_epoch,adapter FROM cloud_command_outbox WHERE scope_id=?1 AND operation_id=?2 AND idempotency_key=?3",
                params![lease.scope_id,intent.operation_id,intent.idempotency_key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
            if let Some((id,hash,external,gen,epoch,adapter))=existing {
                if id!=intent.command_id || hash!=body_digest || external!=intent.stream.external_id || gen!=generation || epoch!=lease.epoch || adapter!=intent.stream.adapter.as_str() { bail!("idempotency conflict or stale command authority"); }
                return load_command(conn,lease,&id);
            }
            let session=binding(conn,lease,&intent.stream)?;
            let local_command_id=local_command_id(lease,&intent.command_id)?;
            let now=chrono::Utc::now().to_rfc3339();
            let source=match intent.stream.adapter {Adapter::InternSync|Adapter::InternAsync=>"intern",_=>"remote"};
            conn.execute("INSERT INTO command_receipts(command_id,session_id,source,kind,status,request_json,created_at,updated_at) VALUES(?1,?2,?3,?4,'accepted',?5,?6,?6)",params![local_command_id,session,source,intent.operation_id,std::str::from_utf8(&intent.body)?,now])?;
            conn.execute("INSERT INTO cloud_command_outbox(scope_id,command_id,local_command_id,adapter,external_id,operation_id,idempotency_key,body,body_sha256,auth_epoch,expected_generation,delivery_state) VALUES(?1,?2,?11,?3,?4,?5,?6,?7,?8,?9,?10,'pending')",
                params![lease.scope_id,intent.command_id,intent.stream.adapter.as_str(),intent.stream.external_id,intent.operation_id,intent.idempotency_key,intent.body,body_digest,lease.epoch,generation,local_command_id])?;
            load_command(conn,lease,&intent.command_id)
        })
    }

    /// Commit the uncertainty marker BEFORE handing bytes to the network.
    /// Concurrent workers cannot both claim a pending command. Unknown outcomes
    /// require an authoritative lookup; this method never blindly retries them.
    pub fn begin_send(&self, lease: &ScopeLease, command_id: &str) -> Result<PendingCommand> {
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            let n=conn.execute("UPDATE cloud_command_outbox SET delivery_state='outcome_unknown' WHERE scope_id=?1 AND command_id=?2 AND auth_epoch=?3 AND delivery_state='pending'",params![lease.scope_id,command_id,lease.epoch])?;
            if n!=1 { bail!("command is not pending under current authority"); }
            conn.execute("UPDATE command_receipts SET response_json=?1,updated_at=?2 WHERE command_id=?3",params![json!({"deliveryState":"outcome_unknown"}).to_string(),chrono::Utc::now().to_rfc3339(),local_command_id(lease,command_id)?])?;
            load_command(conn,lease,command_id)
        })
    }

    /// One dispatch attempt. The network adapter is injected and must use the
    /// exact persisted request. Cancellation/error leaves outcome_unknown.
    /// No transport is wired here until its live contract is qualified.
    pub async fn dispatch_once<F, Fut>(
        &self,
        lease: &ScopeLease,
        intent: &CommandIntent,
        send: F,
    ) -> Result<PendingCommand>
    where
        F: FnOnce(PendingCommand) -> Fut,
        Fut: std::future::Future<Output = Result<DeliveryReceipt>>,
    {
        let worker = self.clone();
        let worker_lease = lease.clone();
        let worker_intent = intent.clone();
        let request = tokio::task::spawn_blocking(move || {
            worker.enqueue(&worker_lease, &worker_intent)?;
            worker.begin_send(&worker_lease, &worker_intent.command_id)
        })
        .await
        .context("join cloud outbox admission")??;
        let receipt = send(request).await?;
        if receipt.command_id != intent.command_id || receipt.stream != intent.stream {
            bail!("receipt identity drift");
        }
        let worker = self.clone();
        let worker_lease = lease.clone();
        let command_id = intent.command_id.clone();
        tokio::task::spawn_blocking(move || {
            worker.record_receipt(&worker_lease, &command_id, receipt.stage, &receipt.detail)?;
            worker.command(&worker_lease, &command_id)
        })
        .await
        .context("join cloud receipt persistence")?
    }

    pub fn command(&self, lease: &ScopeLease, id: &str) -> Result<PendingCommand> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            load_command(conn, lease, id)
        })
    }

    pub fn record_receipt(
        &self,
        lease: &ScopeLease,
        id: &str,
        stage: ReceiptStage,
        receipt: &Value,
    ) -> Result<()> {
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            let current=load_command(conn,lease,id)?;
            let next=stage.as_str();
            let allowed= match current.state.as_str() {
                "outcome_unknown" => true,
                "received" => matches!(stage,ReceiptStage::Received|ReceiptStage::Delivered|ReceiptStage::Applied|ReceiptStage::Refused|ReceiptStage::Conflict),
                "delivered" => matches!(stage,ReceiptStage::Delivered|ReceiptStage::Applied|ReceiptStage::Refused|ReceiptStage::Conflict),
                terminal=>terminal==next,
            };
            if !allowed { bail!("receipt stage regression or command not sent"); }
            if matches!(current.state.as_str(),"applied"|"refused"|"conflict") {
                let previous:String=conn.query_row("SELECT receipt_json FROM cloud_command_outbox WHERE scope_id=?1 AND command_id=?2",params![lease.scope_id,id],|r|r.get(0))?;
                if serde_json::from_str::<Value>(&previous)?!=*receipt { bail!("terminal receipt content changed"); }
            }
            let body=serde_json::to_vec(receipt)?;
            if body.len()>MAX_BODY { bail!("receipt exceeds limit"); }
            conn.execute("UPDATE cloud_command_outbox SET delivery_state=?1,receipt_json=?2 WHERE scope_id=?3 AND command_id=?4",params![next,String::from_utf8(body)?,lease.scope_id,id])?;
            let local_status=match stage {ReceiptStage::Received|ReceiptStage::Delivered=>"accepted",ReceiptStage::Applied=>"completed",ReceiptStage::Refused|ReceiptStage::Conflict=>"rejected"};
            conn.execute("UPDATE command_receipts SET status=?1,response_json=?2,updated_at=?3 WHERE command_id=?4",params![local_status,json!({"deliveryState":next,"receipt":receipt}).to_string(),chrono::Utc::now().to_rfc3339(),local_command_id(lease,id)?])?;
            // Receipt delivery/application does not change remote run completion.
            Ok(())
        })
    }

    pub fn bind_execution(&self, lease: &ScopeLease, stream: &Stream, run_id: &str) -> Result<()> {
        valid_id(run_id)?;
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            let session=binding(conn,lease,stream)?;
            let previous:Option<String>=conn.query_row("SELECT local_session_id FROM cloud_execution_bindings WHERE scope_id=?1 AND adapter=?2 AND external_run_id=?3",params![lease.scope_id,stream.adapter.as_str(),run_id],|r|r.get(0)).optional()?;
            if let Some(previous)=previous { if previous!=session { bail!("execution owner conflict"); } return Ok(()); }
            conn.execute("INSERT INTO cloud_execution_bindings(scope_id,adapter,external_run_id,local_session_id) VALUES(?1,?2,?3,?4)",params![lease.scope_id,stream.adapter.as_str(),run_id,session])?;
            Ok(())
        })
    }

    pub fn execution_owner(
        &self,
        lease: &ScopeLease,
        adapter: Adapter,
        run_id: &str,
    ) -> Result<Option<String>> {
        self.db.transaction(|conn| { fence(conn,lease)?; Ok(conn.query_row("SELECT local_session_id FROM cloud_execution_bindings WHERE scope_id=?1 AND adapter=?2 AND external_run_id=?3",params![lease.scope_id,adapter.as_str(),run_id],|r|r.get(0)).optional()?) })
    }

    /// Only a scoped authoritative observation may change remote state.
    /// Local UI mount/unmount and command delivery never call this method.
    pub fn observe_execution(
        &self,
        lease: &ScopeLease,
        adapter: Adapter,
        run_id: &str,
        state: RemoteExecutionState,
    ) -> Result<()> {
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            let previous:String=conn.query_row("SELECT remote_state FROM cloud_execution_bindings WHERE scope_id=?1 AND adapter=?2 AND external_run_id=?3",params![lease.scope_id,adapter.as_str(),run_id],|r|r.get(0))?;
            if matches!(previous.as_str(),"completed"|"failed"|"cancelled") && previous!=state.as_str() { bail!("terminal remote execution cannot regress"); }
            conn.execute("UPDATE cloud_execution_bindings SET remote_state=?1 WHERE scope_id=?2 AND adapter=?3 AND external_run_id=?4",params![state.as_str(),lease.scope_id,adapter.as_str(),run_id])?;
            Ok(())
        })
    }

    pub fn event_payloads(
        &self,
        lease: &ScopeLease,
        stream: &Stream,
        limit: u16,
    ) -> Result<Vec<Value>> {
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            binding(conn,lease,stream)?;
            let mut stmt=conn.prepare("SELECT e.payload_json FROM cloud_event_bindings b JOIN events e ON e.event_id=b.journal_event_id WHERE b.scope_id=?1 AND b.adapter=?2 AND b.external_id=?3 ORDER BY e.sequence LIMIT ?4")?;
            let rows=stmt.query_map(params![lease.scope_id,stream.adapter.as_str(),stream.external_id,limit.clamp(1,500)],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().map(|s|serde_json::from_str(&s).map_err(Into::into)).collect()
        })
    }

    pub fn checkpoint(&self, lease: &ScopeLease, stream: &Stream) -> Result<Option<Checkpoint>> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            binding(conn, lease, stream)?;
            checkpoint(conn, lease, stream)
        })
    }

    /// CAS the checkpoint and append scoped journal events in one transaction.
    /// Opaque swarm cursors are compared for equality, never arithmetically.
    pub fn commit_page(
        &self,
        lease: &ScopeLease,
        stream: &Stream,
        expected: Option<&Checkpoint>,
        next: &Checkpoint,
        events: &[RemoteEvent],
    ) -> Result<Vec<AppEvent>> {
        if events.len() > MAX_PAGE {
            bail!("event page exceeds limit");
        }
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            let session=binding(conn,lease,stream)?;
            let current=checkpoint(conn,lease,stream)?;
            if current.as_ref()!=expected { bail!("checkpoint changed; reload committed state"); }
            validate_page(stream,expected,next,events)?;
            let mut committed=Vec::new();
            for event in events {
                valid_id(&event.id)?;
                let bytes=serde_json::to_vec(&(&event.kind,&event.payload,event.sequence,event.generation))?;
                if bytes.len()>MAX_BODY { bail!("event exceeds limit"); }
                let hash=digest(&bytes);
                let old:Option<String>=conn.query_row("SELECT event_sha256 FROM cloud_event_bindings WHERE scope_id=?1 AND adapter=?2 AND external_id=?3 AND remote_event_id=?4",params![lease.scope_id,stream.adapter.as_str(),stream.external_id,event.id],|r|r.get(0)).optional()?;
                if let Some(old)=old { if old!=hash { bail!("remote event identity reused with different content"); } continue; }
                let prior_sequence = match expected {
                    Some(Checkpoint::Intern { sequence,.. }) | Some(Checkpoint::Mq { sequence,.. }) => Some(*sequence),
                    _ => None,
                };
                if matches!(stream.adapter,Adapter::InternSync|Adapter::InternAsync|Adapter::Mq)
                    && event.sequence.is_some_and(|s| s == 0 || prior_sequence.is_some_and(|p|s<=p)) {
                    bail!("unrecognized event at an already committed sequence");
                }
                let journal_id=format!("cloud:{}",digest(&serde_json::to_vec(&(&lease.scope_id,stream.adapter.as_str(),&stream.external_id,&event.id))?));
                let app=append_event(conn,EventAppend {
                    event_id:Some(journal_id.clone()),session_id:Some(session.clone()),run_id:None,
                    source:match stream.adapter { Adapter::InternSync|Adapter::InternAsync=>EventSource::Intern,_=>EventSource::Remote },kind:event.kind.clone(),
                    payload:json!({"cloudScopeId":lease.scope_id,"adapter":stream.adapter.as_str(),"externalId":stream.external_id,"data":event.payload}),
                    remote_sequence:None,command_id:None,created_at:None,
                })?;
                conn.execute("INSERT INTO cloud_event_bindings VALUES(?1,?2,?3,?4,?5,?6)",params![lease.scope_id,stream.adapter.as_str(),stream.external_id,event.id,hash,journal_id])?;
                committed.push(app);
            }
            conn.execute("INSERT INTO cloud_checkpoints VALUES(?1,?2,?3,?4) ON CONFLICT(scope_id,adapter,external_id) DO UPDATE SET checkpoint_json=excluded.checkpoint_json",params![lease.scope_id,stream.adapter.as_str(),stream.external_id,serde_json::to_string(next)?])?;
            Ok(committed)
        })
    }
}

fn invalidate(conn: &Connection) -> Result<i64> {
    let epoch: i64 = conn.query_row(
        "SELECT epoch FROM cloud_auth_state WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    let next = epoch.checked_add(1).context("auth epoch exhausted")?;
    conn.execute("UPDATE cloud_execution_bindings SET remote_state='reconciling' WHERE remote_state IN ('running','paused')",[])?;
    conn.execute(
        "UPDATE cloud_auth_state SET epoch=?1,active_scope_id=NULL WHERE singleton=1",
        params![next],
    )?;
    Ok(next)
}
fn fence(conn: &Connection, lease: &ScopeLease) -> Result<()> {
    let current: (i64, Option<String>) = conn.query_row(
        "SELECT epoch,active_scope_id FROM cloud_auth_state WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if current != (lease.epoch, Some(lease.scope_id.clone())) {
        bail!("stale cloud authentication epoch");
    }
    Ok(())
}
fn binding(conn: &Connection, lease: &ScopeLease, stream: &Stream) -> Result<String> {
    conn.query_row("SELECT local_session_id FROM cloud_session_bindings WHERE scope_id=?1 AND adapter=?2 AND external_id=?3",params![lease.scope_id,stream.adapter.as_str(),stream.external_id],|r|r.get(0)).context("cloud stream is unbound")
}
fn checkpoint(
    conn: &Connection,
    lease: &ScopeLease,
    stream: &Stream,
) -> Result<Option<Checkpoint>> {
    let json:Option<String>=conn.query_row("SELECT checkpoint_json FROM cloud_checkpoints WHERE scope_id=?1 AND adapter=?2 AND external_id=?3",params![lease.scope_id,stream.adapter.as_str(),stream.external_id],|r|r.get(0)).optional()?;
    json.map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
fn load_command(conn: &Connection, lease: &ScopeLease, id: &str) -> Result<PendingCommand> {
    conn.query_row("SELECT command_id,operation_id,idempotency_key,body,body_sha256,delivery_state FROM cloud_command_outbox WHERE scope_id=?1 AND command_id=?2",params![lease.scope_id,id],|r|Ok(PendingCommand{command_id:r.get(0)?,operation_id:r.get(1)?,idempotency_key:r.get(2)?,body:r.get(3)?,body_sha256:r.get(4)?,state:r.get(5)?})).context("cloud command not found")
}
fn local_command_id(lease: &ScopeLease, id: &str) -> Result<String> {
    Ok(format!(
        "cloud:{}",
        digest(&serde_json::to_vec(&(&lease.scope_id, id))?)
    ))
}
fn valid_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 512 || id.chars().any(char::is_control) {
        bail!("invalid cloud identity");
    }
    Ok(())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_page(
    stream: &Stream,
    previous: Option<&Checkpoint>,
    next: &Checkpoint,
    events: &[RemoteEvent],
) -> Result<()> {
    match (stream.adapter, previous, next) {
        (
            Adapter::InternSync | Adapter::InternAsync,
            prior,
            Checkpoint::Intern {
                sequence,
                generation,
            },
        ) => {
            let (mut seq, mut gen) = match prior {
                None => (0, 0),
                Some(Checkpoint::Intern {
                    sequence,
                    generation,
                }) => (*sequence, *generation),
                _ => bail!("checkpoint adapter mismatch"),
            };
            let initial_sequence = seq;
            for e in events {
                let s = e.sequence.context("Intern event needs sequence")?;
                if s == 0 || (s <= seq && s > initial_sequence) {
                    bail!("duplicate or zero Intern sequence");
                }
                let g = e.generation.context("Intern event needs generation")?;
                if s <= seq {
                    continue;
                }
                if seq.checked_add(1) != Some(s) || g < gen {
                    bail!("Intern sequence gap or generation regression");
                }
                seq = s;
                gen = g;
            }
            if seq != *sequence || gen != *generation {
                bail!("Intern checkpoint ahead of committed events");
            }
        }
        (Adapter::Swarm, None | Some(Checkpoint::Swarm { .. }), Checkpoint::Swarm { .. }) => {
            if events.is_empty() && previous != Some(next) {
                bail!("heartbeat cannot advance durable swarm checkpoint");
            }
        }
        (
            Adapter::Mq,
            prior,
            Checkpoint::Mq {
                subscription_id,
                sequence,
            },
        ) => {
            valid_id(subscription_id)?;
            let mut seq = match prior {
                None => 0,
                Some(Checkpoint::Mq {
                    subscription_id: old,
                    sequence,
                }) if old == subscription_id => *sequence,
                _ => bail!("MQ subscription drift"),
            };
            let initial_sequence = seq;
            for e in events {
                let s = e.sequence.context("MQ durable message needs sequence")?;
                if s == 0 || (s <= seq && s > initial_sequence) {
                    bail!("duplicate or zero MQ sequence");
                }
                if s <= seq {
                    continue;
                }
                if seq.checked_add(1) != Some(s) {
                    bail!("MQ sequence gap");
                }
                seq = s;
            }
            if seq != *sequence {
                bail!("MQ wake is not a durable acknowledgement");
            }
        }
        _ => bail!("checkpoint adapter mismatch"),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
