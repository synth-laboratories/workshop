-- Scoped cloud storage, registered as desktop schema migration 69
-- (storage/migrations.rs). Every statement is idempotent: a prerelease lane
-- whose registry collided on version 69 heals these tables through
-- heal_missing_tables without replaying any data movement. Nothing here
-- backfills identities, adopts legacy sessions or reads credentials.
CREATE TABLE IF NOT EXISTS cloud_scopes (
 id TEXT PRIMARY KEY,
 backend_origin TEXT NOT NULL,
 backend_id TEXT NOT NULL,
 account_id TEXT NOT NULL,
 org_id TEXT NOT NULL,
 profile_id TEXT NOT NULL,
 UNIQUE(backend_origin,backend_id,account_id,org_id,profile_id)
);
CREATE TABLE IF NOT EXISTS cloud_auth_state (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 epoch INTEGER NOT NULL CHECK(epoch>=0),
 active_scope_id TEXT REFERENCES cloud_scopes(id),
 valid_until_ms INTEGER,
 -- The most recently activated scope, retained across expiry so an explicit
 -- sign-out after an idle expiry still fences that account's queued writes.
 last_scope_id TEXT REFERENCES cloud_scopes(id)
);
INSERT OR IGNORE INTO cloud_auth_state(singleton,epoch) VALUES(1,0);
CREATE TABLE IF NOT EXISTS cloud_owned_sessions (
 local_session_id TEXT PRIMARY KEY REFERENCES sessions(id),
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 UNIQUE(scope_id,local_session_id)
);
CREATE TABLE IF NOT EXISTS cloud_session_bindings (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 adapter TEXT NOT NULL CHECK(adapter IN ('intern_sync','intern_async','swarm','mq')),
 external_id TEXT NOT NULL,
 local_session_id TEXT NOT NULL REFERENCES sessions(id),
 PRIMARY KEY(scope_id,adapter,external_id),
 FOREIGN KEY(scope_id,local_session_id) REFERENCES cloud_owned_sessions(scope_id,local_session_id)
);
CREATE TABLE IF NOT EXISTS cloud_execution_bindings (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 adapter TEXT NOT NULL CHECK(adapter IN ('intern_sync','intern_async','swarm','mq')),
 external_run_id TEXT NOT NULL,
 local_session_id TEXT NOT NULL REFERENCES sessions(id),
 remote_state TEXT NOT NULL DEFAULT 'reconciling' CHECK(remote_state IN ('reconciling','running','paused','completed','failed','cancelled')),
 PRIMARY KEY(scope_id,adapter,external_run_id)
);
CREATE TABLE IF NOT EXISTS cloud_command_outbox (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 command_id TEXT NOT NULL,
 local_command_id TEXT NOT NULL UNIQUE REFERENCES command_receipts(command_id),
 adapter TEXT NOT NULL,
 external_id TEXT NOT NULL,
 operation_id TEXT NOT NULL,
 idempotency_key TEXT NOT NULL,
 body BLOB NOT NULL,
 body_sha256 TEXT NOT NULL,
 auth_epoch INTEGER NOT NULL CHECK(auth_epoch>=0),
 expected_generation INTEGER CHECK(expected_generation>=0),
 delivery_state TEXT NOT NULL CHECK(delivery_state IN ('pending','outcome_unknown','received','delivered','applied','refused','conflict')),
 receipt_json TEXT,
 PRIMARY KEY(scope_id,command_id),
 UNIQUE(scope_id,operation_id,idempotency_key),
 FOREIGN KEY(scope_id,adapter,external_id) REFERENCES cloud_session_bindings(scope_id,adapter,external_id)
);
CREATE TRIGGER IF NOT EXISTS immutable_cloud_command BEFORE UPDATE OF scope_id,command_id,local_command_id,adapter,external_id,operation_id,idempotency_key,body,body_sha256,expected_generation,auth_epoch ON cloud_command_outbox
BEGIN SELECT RAISE(ABORT,'command identity and body are immutable'); END;
CREATE TABLE IF NOT EXISTS cloud_checkpoints (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 adapter TEXT NOT NULL,
 external_id TEXT NOT NULL,
 checkpoint_json TEXT NOT NULL,
 PRIMARY KEY(scope_id,adapter,external_id),
 FOREIGN KEY(scope_id,adapter,external_id) REFERENCES cloud_session_bindings(scope_id,adapter,external_id)
);
CREATE TABLE IF NOT EXISTS cloud_event_bindings (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 adapter TEXT NOT NULL,
 external_id TEXT NOT NULL,
 remote_event_id TEXT NOT NULL,
 event_sha256 TEXT NOT NULL,
 journal_event_id TEXT NOT NULL UNIQUE,
 PRIMARY KEY(scope_id,adapter,external_id,remote_event_id),
 FOREIGN KEY(scope_id,adapter,external_id) REFERENCES cloud_session_bindings(scope_id,adapter,external_id)
);

-- Acceptance and local-turn consumption are separate durable facts.
-- `stage` records the receipt ladder for one inbound message:
-- delivered (durable inbox) -> observed (consumed at a safe turn boundary)
-- -> acting (an admitted handler started) -> answered | declined | expired.
-- `fenced` means grant/incarnation/session authority changed before
-- consumption; a fenced input is never executed.
CREATE TABLE IF NOT EXISTS cloud_mq_pending_inputs (
 scope_id TEXT NOT NULL,
 adapter TEXT NOT NULL DEFAULT 'mq' CHECK(adapter='mq'),
 external_id TEXT NOT NULL,
 remote_event_id TEXT NOT NULL,
 sequence INTEGER NOT NULL CHECK(sequence>0),
 journal_event_id TEXT NOT NULL UNIQUE,
 accepted_command_id TEXT UNIQUE REFERENCES command_receipts(command_id),
 stage TEXT NOT NULL DEFAULT 'delivered' CHECK(stage IN ('delivered','observed','acting','answered','declined','expired','fenced')),
 delivered_generation INTEGER CHECK(delivered_generation>=0),
 delivered_incarnation INTEGER CHECK(delivered_incarnation>0),
 correlation_id TEXT,
 causal_depth INTEGER NOT NULL DEFAULT 0 CHECK(causal_depth>=0),
 deadline_ms INTEGER,
 observed_at TEXT,
 acting_at_ms INTEGER,
 settled_at TEXT,
 disposition_json TEXT,
 reply_command_id TEXT,
 PRIMARY KEY(scope_id,external_id,remote_event_id),
 UNIQUE(scope_id,external_id,sequence),
 FOREIGN KEY(scope_id,adapter,external_id,remote_event_id)
 REFERENCES cloud_event_bindings(scope_id,adapter,external_id,remote_event_id)
);
CREATE INDEX IF NOT EXISTS cloud_mq_pending_inputs_stage ON cloud_mq_pending_inputs(scope_id,external_id,stage,sequence);

-- Durable creation precedes remote binding. No retry after an uncertain create.
CREATE TABLE IF NOT EXISTS cloud_creation_intents (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 creation_id TEXT NOT NULL,
 adapter TEXT NOT NULL CHECK(adapter IN ('intern_sync','intern_async')),
 operation_id TEXT NOT NULL,
 idempotency_key TEXT NOT NULL,
 local_session_id TEXT NOT NULL,
 auth_epoch INTEGER NOT NULL CHECK(auth_epoch>=0),
 plan BLOB NOT NULL,
 plan_sha256 TEXT NOT NULL,
 delivery_state TEXT NOT NULL CHECK(delivery_state IN ('pending','outcome_unknown','bound')),
 external_id TEXT,
 PRIMARY KEY(scope_id,creation_id),
 UNIQUE(scope_id,operation_id,idempotency_key),
 FOREIGN KEY(scope_id,local_session_id) REFERENCES cloud_owned_sessions(scope_id,local_session_id)
);
CREATE TRIGGER IF NOT EXISTS immutable_cloud_creation BEFORE UPDATE OF scope_id,creation_id,adapter,operation_id,idempotency_key,local_session_id,auth_epoch,plan,plan_sha256 ON cloud_creation_intents
BEGIN SELECT RAISE(ABORT,'creation identity and first-send intent are immutable'); END;

-- One stable, non-secret device identifier per installation. It names the
-- device in backend enrollment; it is not an authorization credential.
CREATE TABLE IF NOT EXISTS cloud_mq_device (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 device_id TEXT NOT NULL,
 created_at TEXT NOT NULL
);

-- Explicit sign-out and account switch advance a scope's write fence. Queued
-- MQ writes captured under an older fence value can never flush; they remain
-- visible as fenced. Identity-observation expiry does not advance it, so an
-- offline outbox for the same account and grant generation survives sleep.
CREATE TABLE IF NOT EXISTS cloud_mq_scope_fences (
 scope_id TEXT PRIMARY KEY REFERENCES cloud_scopes(id),
 fence INTEGER NOT NULL DEFAULT 0 CHECK(fence>=0)
);

-- An explicitly selected local session bound to one MQ thread as the
-- server-derived enrollment principal. Never created by scanning or adopting
-- existing rows. Incarnation and grant generation are server-owned values.
CREATE TABLE IF NOT EXISTS cloud_mq_participants (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 adapter TEXT NOT NULL DEFAULT 'mq' CHECK(adapter='mq'),
 thread_id TEXT NOT NULL,
 local_session_id TEXT NOT NULL,
 enrollment_id TEXT NOT NULL,
 device_id TEXT NOT NULL,
 incarnation INTEGER NOT NULL CHECK(incarnation>0),
 principal_id TEXT NOT NULL,
 org_id TEXT NOT NULL,
 mq_endpoint TEXT NOT NULL,
 peers_json TEXT NOT NULL,
 preset TEXT NOT NULL CHECK(preset IN ('observe','collaborate','respond')),
 policy_json TEXT NOT NULL,
 grant_id TEXT,
 grant_generation INTEGER CHECK(grant_generation>=0),
 grant_operations TEXT,
 history_after_seq INTEGER CHECK(history_after_seq>=0),
 grant_expires_ms INTEGER,
 state TEXT NOT NULL CHECK(state IN ('awaiting_grant','active','revoked','expired','fenced')),
 state_reason TEXT,
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL,
 PRIMARY KEY(scope_id,thread_id),
 FOREIGN KEY(scope_id,local_session_id) REFERENCES cloud_owned_sessions(scope_id,local_session_id),
 FOREIGN KEY(scope_id,adapter,thread_id) REFERENCES cloud_session_bindings(scope_id,adapter,external_id)
);

-- Outbound MQ publications. The exact request body/key lives immutably in
-- cloud_command_outbox; this row carries the correlation lineage, the
-- authority it was captured under and the reconciled server identity.
CREATE TABLE IF NOT EXISTS cloud_mq_outbox (
 scope_id TEXT NOT NULL,
 command_id TEXT NOT NULL,
 thread_id TEXT NOT NULL,
 kind TEXT NOT NULL,
 disposition TEXT NOT NULL CHECK(disposition IN ('message','answer','decline','expiry')),
 correlation_id TEXT,
 causation_id TEXT,
 parent_message_id TEXT,
 reply_to_message_id TEXT,
 causal_depth INTEGER NOT NULL DEFAULT 0 CHECK(causal_depth>=0),
 grant_generation INTEGER NOT NULL CHECK(grant_generation>=0),
 incarnation INTEGER NOT NULL CHECK(incarnation>0),
 scope_fence INTEGER NOT NULL CHECK(scope_fence>=0),
 sent_after_seq INTEGER,
 fenced_reason TEXT,
 mq_message_id TEXT,
 mq_seq INTEGER,
 answered_by_message_id TEXT,
 lookup_json TEXT,
 created_at TEXT NOT NULL,
 PRIMARY KEY(scope_id,command_id),
 FOREIGN KEY(scope_id,command_id) REFERENCES cloud_command_outbox(scope_id,command_id)
);
CREATE INDEX IF NOT EXISTS cloud_mq_outbox_thread ON cloud_mq_outbox(scope_id,thread_id,created_at);
CREATE INDEX IF NOT EXISTS cloud_mq_outbox_correlation ON cloud_mq_outbox(scope_id,thread_id,correlation_id);
CREATE TRIGGER IF NOT EXISTS immutable_cloud_mq_outbox BEFORE UPDATE OF scope_id,command_id,thread_id,kind,disposition,correlation_id,causation_id,parent_message_id,reply_to_message_id,causal_depth,grant_generation,incarnation,scope_fence ON cloud_mq_outbox
BEGIN SELECT RAISE(ABORT,'MQ outbox lineage and authority are immutable'); END;

-- Authorized history gaps: the grant's history floor hid (after_seq,
-- through_seq]. Recorded so a cursor jump is never silent.
CREATE TABLE IF NOT EXISTS cloud_mq_history_gaps (
 scope_id TEXT NOT NULL,
 thread_id TEXT NOT NULL,
 after_seq INTEGER NOT NULL CHECK(after_seq>=0),
 through_seq INTEGER NOT NULL,
 reason TEXT NOT NULL,
 recorded_at TEXT NOT NULL,
 PRIMARY KEY(scope_id,thread_id,after_seq),
 CHECK(through_seq>after_seq)
);
