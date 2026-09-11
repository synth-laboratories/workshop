-- Additive migration candidate. Deliberately NOT in the desktop migration registry.
-- Registration requires the qualified cloud identity contract; never backfill identities.
CREATE TABLE cloud_scopes (
 id TEXT PRIMARY KEY,
 backend_origin TEXT NOT NULL,
 backend_id TEXT NOT NULL,
 account_id TEXT NOT NULL,
 org_id TEXT NOT NULL,
 profile_id TEXT NOT NULL,
 UNIQUE(backend_origin,backend_id,account_id,org_id,profile_id)
);
CREATE TABLE cloud_auth_state (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 epoch INTEGER NOT NULL CHECK(epoch>=0),
 active_scope_id TEXT REFERENCES cloud_scopes(id),
 valid_until_ms INTEGER
);
INSERT INTO cloud_auth_state VALUES(1,0,NULL,NULL);
CREATE TABLE cloud_owned_sessions (
 local_session_id TEXT PRIMARY KEY REFERENCES sessions(id),
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 UNIQUE(scope_id,local_session_id)
);
CREATE TABLE cloud_session_bindings (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 adapter TEXT NOT NULL CHECK(adapter IN ('intern_sync','intern_async','swarm','mq')),
 external_id TEXT NOT NULL,
 local_session_id TEXT NOT NULL REFERENCES sessions(id),
 PRIMARY KEY(scope_id,adapter,external_id),
 FOREIGN KEY(scope_id,local_session_id) REFERENCES cloud_owned_sessions(scope_id,local_session_id)
);
CREATE TABLE cloud_execution_bindings (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 adapter TEXT NOT NULL CHECK(adapter IN ('intern_sync','intern_async','swarm','mq')),
 external_run_id TEXT NOT NULL,
 local_session_id TEXT NOT NULL REFERENCES sessions(id),
 remote_state TEXT NOT NULL DEFAULT 'reconciling' CHECK(remote_state IN ('reconciling','running','paused','completed','failed','cancelled')),
 PRIMARY KEY(scope_id,adapter,external_run_id)
);
CREATE TABLE cloud_command_outbox (
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
CREATE TRIGGER immutable_cloud_command BEFORE UPDATE OF scope_id,command_id,local_command_id,adapter,external_id,operation_id,idempotency_key,body,body_sha256,expected_generation,auth_epoch ON cloud_command_outbox
BEGIN SELECT RAISE(ABORT,'command identity and body are immutable'); END;
CREATE TABLE cloud_checkpoints (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 adapter TEXT NOT NULL,
 external_id TEXT NOT NULL,
 checkpoint_json TEXT NOT NULL,
 PRIMARY KEY(scope_id,adapter,external_id),
 FOREIGN KEY(scope_id,adapter,external_id) REFERENCES cloud_session_bindings(scope_id,adapter,external_id)
);
CREATE TABLE cloud_event_bindings (
 scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
 adapter TEXT NOT NULL,
 external_id TEXT NOT NULL,
 remote_event_id TEXT NOT NULL,
 event_sha256 TEXT NOT NULL,
 journal_event_id TEXT NOT NULL UNIQUE,
 PRIMARY KEY(scope_id,adapter,external_id,remote_event_id),
 FOREIGN KEY(scope_id,adapter,external_id) REFERENCES cloud_session_bindings(scope_id,adapter,external_id)
);

-- Durable creation precedes remote binding. No retry after an uncertain create.
CREATE TABLE cloud_creation_intents (
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
CREATE TRIGGER immutable_cloud_creation BEFORE UPDATE OF scope_id,creation_id,adapter,operation_id,idempotency_key,local_session_id,auth_epoch,plan,plan_sha256 ON cloud_creation_intents
BEGIN SELECT RAISE(ABORT,'creation identity and first-send intent are immutable'); END;
