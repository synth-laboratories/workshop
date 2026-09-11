//! Creation and first-send durability. No live transport or automatic replay.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub struct FirstCommand {
    pub command_id: String,
    pub operation_id: String,
    pub idempotency_key: String,
    pub body: Vec<u8>,
    pub expected_generation: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CreationIntent {
    pub creation_id: String,
    pub adapter: Adapter,
    pub operation_id: String,
    pub idempotency_key: String,
    pub body: Vec<u8>,
    pub title: String,
    pub first: FirstCommand,
}

pub struct CreationReceipt {
    pub creation_id: String,
    pub adapter: Adapter,
    pub idempotency_key: String,
    pub external_id: String,
}

pub struct CreationRecord {
    pub local_session_id: String,
    pub state: String,
    pub external_id: Option<String>,
    pub intent: CreationIntent,
    auth_epoch: i64,
}
impl std::fmt::Debug for CreationRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreationRecord")
            .field("local_session_id", &self.local_session_id)
            .field("state", &self.state)
            .field("intent", &"<redacted>")
            .finish()
    }
}

impl CloudStore {
    /// Atomically preserve both original requests and allocate a fresh local
    /// conversation. Neither request may be changed after admission.
    pub fn stage_creation(
        &self,
        lease: &ScopeLease,
        intent: &CreationIntent,
    ) -> Result<CreationRecord> {
        for value in [
            &intent.creation_id,
            &intent.operation_id,
            &intent.idempotency_key,
            &intent.first.command_id,
            &intent.first.operation_id,
            &intent.first.idempotency_key,
        ] {
            valid_id(value)?;
        }
        if intent.body.len() > MAX_BODY
            || intent.first.body.len() > MAX_BODY
            || intent.title.len() > 4096
        {
            bail!("creation plan exceeds limit");
        }
        let create: Value = serde_json::from_slice(&intent.body)?;
        let first: Value = serde_json::from_slice(&intent.first.body)?;
        if create.get("idempotency_key").and_then(Value::as_str) != Some(&intent.idempotency_key) {
            bail!("creation key does not match persisted body");
        }
        if intent.first.expected_generation.is_some()
            && first.get("expected_generation").and_then(Value::as_u64)
                != intent.first.expected_generation
        {
            bail!("first-send generation does not match body");
        }
        intent
            .first
            .expected_generation
            .map(i64::try_from)
            .transpose()?;
        let mode = match intent.adapter {
            Adapter::InternSync => crate::domain::InternMode::Sync,
            Adapter::InternAsync => crate::domain::InternMode::Async,
            _ => bail!("creation requires an Intern adapter"),
        };
        let plan = serde_json::to_vec(intent)?;
        let hash = digest(&plan);
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            let existing: Option<(String, String, i64)> = conn.query_row(
                "SELECT creation_id,plan_sha256,auth_epoch FROM cloud_creation_intents WHERE scope_id=?1 AND operation_id=?2 AND idempotency_key=?3",
                params![lease.scope_id,intent.operation_id,intent.idempotency_key],
                |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            if let Some((id, old_hash, epoch)) = existing {
                if id != intent.creation_id || hash != old_hash || epoch != lease.epoch {
                    bail!("creation idempotency conflict or stale authority");
                }
                return load(conn, lease, &id);
            }
            let session = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            let target = crate::domain::RuntimeTarget::InternRuntime { mode, binding: None };
            conn.execute("INSERT INTO sessions(id,title,kind,target_json,runtime_target_kind,status,metadata_json,created_at,updated_at) VALUES(?1,?2,'intern',?3,'intern','ready','{}',?4,?4)",
                params![session,intent.title,target.to_json_value().to_string(),now])?;
            conn.execute("INSERT INTO cloud_owned_sessions VALUES(?1,?2)",params![session,lease.scope_id])?;
            conn.execute("INSERT INTO command_receipts(command_id,session_id,source,kind,status,request_json,created_at,updated_at) VALUES(?1,?2,'intern',?3,'accepted',?4,?5,?5)",
                params![receipt_id(lease,&intent.creation_id),session,intent.operation_id,std::str::from_utf8(&intent.body)?,now])?;
            conn.execute("INSERT INTO cloud_creation_intents VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending',NULL)",
                params![lease.scope_id,intent.creation_id,intent.adapter.as_str(),intent.operation_id,intent.idempotency_key,session,lease.epoch,plan,hash])?;
            load(conn,lease,&intent.creation_id)
        })
    }

    pub fn creation(&self, lease: &ScopeLease, id: &str) -> Result<CreationRecord> {
        self.db.transaction(|conn| {
            fence(conn, lease)?;
            load(conn, lease, id)
        })
    }

    /// Mark uncertain before the first network byte; never blindly retry create.
    pub fn begin_creation(&self, lease: &ScopeLease, id: &str) -> Result<CreationRecord> {
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            let n=conn.execute("UPDATE cloud_creation_intents SET delivery_state='outcome_unknown' WHERE scope_id=?1 AND creation_id=?2 AND auth_epoch=?3 AND delivery_state='pending'",params![lease.scope_id,id,lease.epoch])?;
            if n!=1 { bail!("creation is not pending under current authority"); }
            conn.execute("UPDATE command_receipts SET response_json=?1,updated_at=?2 WHERE command_id=?3",params![json!({"deliveryState":"outcome_unknown"}).to_string(),chrono::Utc::now().to_rfc3339(),receipt_id(lease,id)])?;
            load(conn,lease,id)
        })
    }

    /// One injected create attempt. Dropping/timing out the caller leaves the
    /// durable unknown outcome; a fresh key is never synthesized on retry.
    pub async fn dispatch_creation_once<F, Fut>(
        &self,
        lease: &ScopeLease,
        intent: &CreationIntent,
        send: F,
    ) -> Result<CreationRecord>
    where
        F: FnOnce(CreationRecord) -> Fut,
        Fut: std::future::Future<Output = Result<CreationReceipt>>,
    {
        let worker = self.clone();
        let worker_lease = lease.clone();
        let plan = intent.clone();
        let request = tokio::task::spawn_blocking(move || {
            worker.stage_creation(&worker_lease, &plan)?;
            worker.begin_creation(&worker_lease, &plan.creation_id)
        })
        .await
        .context("join creation admission")??;
        let receipt = send(request).await?;
        if receipt.creation_id != intent.creation_id
            || receipt.adapter != intent.adapter
            || receipt.idempotency_key != intent.idempotency_key
        {
            bail!("creation receipt identity drift");
        }
        let worker = self.clone();
        let worker_lease = lease.clone();
        let id = intent.creation_id.clone();
        tokio::task::spawn_blocking(move || {
            worker.bind_created(&worker_lease, &id, &receipt.external_id)
        })
        .await
        .context("join creation binding")?
    }

    /// An authoritative create result or lookup binds the original conversation
    /// and admits its original first command in ONE transaction. This never
    /// starts the command; normal outbox admission owns the first network send.
    pub fn bind_created(
        &self,
        lease: &ScopeLease,
        id: &str,
        external_id: &str,
    ) -> Result<CreationRecord> {
        valid_id(external_id)?;
        self.db.transaction(|conn| {
            fence(conn,lease)?;
            let record=load(conn,lease,id)?;
            if record.state=="bound" {
                if record.external_id.as_deref()!=Some(external_id) { bail!("creation result identity changed"); }
                return Ok(record);
            }
            if record.state!="outcome_unknown" { bail!("creation has not been dispatched"); }
            let plan=&record.intent;
            // Even under a freshly verified epoch, a prior create can only be
            // resolved by an authoritative observation, never automatically sent.
            conn.execute("INSERT INTO cloud_session_bindings VALUES(?1,?2,?3,?4)",params![lease.scope_id,plan.adapter.as_str(),external_id,record.local_session_id])?;
            conn.execute("UPDATE sessions SET remote_id=?1,updated_at=?2 WHERE id=?3",params![external_id,chrono::Utc::now().to_rfc3339(),record.local_session_id])?;
            enqueue_conn_with_epoch(conn,lease,&CommandIntent {
                command_id:plan.first.command_id.clone(),stream:Stream {adapter:plan.adapter,external_id:external_id.into()},
                operation_id:plan.first.operation_id.clone(),idempotency_key:plan.first.idempotency_key.clone(),
                body:plan.first.body.clone(),expected_generation:plan.first.expected_generation,
            }, record.auth_epoch)?;
            conn.execute("UPDATE cloud_creation_intents SET delivery_state='bound',external_id=?1 WHERE scope_id=?2 AND creation_id=?3",params![external_id,lease.scope_id,id])?;
            conn.execute("UPDATE command_receipts SET status='completed',response_json=?1,updated_at=?2 WHERE command_id=?3",params![json!({"deliveryState":"bound","externalId":external_id}).to_string(),chrono::Utc::now().to_rfc3339(),receipt_id(lease,id)])?;
            load(conn,lease,id)
        })
    }
}

fn receipt_id(lease: &ScopeLease, id: &str) -> String {
    format!(
        "cloud:create:{}",
        digest(format!("{}:{id}", lease.scope_id).as_bytes())
    )
}
fn load(conn: &Connection, lease: &ScopeLease, id: &str) -> Result<CreationRecord> {
    let (session,state,external,plan,auth_epoch):(String,String,Option<String>,Vec<u8>,i64)=conn.query_row(
        "SELECT local_session_id,delivery_state,external_id,plan,auth_epoch FROM cloud_creation_intents WHERE scope_id=?1 AND creation_id=?2",
        params![lease.scope_id,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    Ok(CreationRecord {
        local_session_id: session,
        state,
        external_id: external,
        intent: serde_json::from_slice(&plan)?,
        auth_epoch,
    })
}
