-- Proposed additive migration fixture. Not registered as a product migration.
-- Identity values MUST come from the qualified authenticated identity contract.
CREATE TABLE cloud_scopes (
    id TEXT PRIMARY KEY,
    backend_origin TEXT NOT NULL,
    account_id TEXT NOT NULL,
    org_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    auth_epoch INTEGER NOT NULL CHECK(auth_epoch >= 0),
    UNIQUE(backend_origin, account_id, org_id, profile_id)
);
CREATE TABLE cloud_session_bindings (
    scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
    runtime_kind TEXT NOT NULL,
    runtime_id TEXT NOT NULL,
    local_session_id TEXT NOT NULL REFERENCES sessions(id),
    PRIMARY KEY(scope_id, runtime_kind, runtime_id),
    UNIQUE(scope_id, local_session_id)
);
CREATE TABLE execution_bindings (
    scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
    adapter TEXT NOT NULL,
    external_run_id TEXT NOT NULL,
    local_session_id TEXT REFERENCES sessions(id),
    remote_state TEXT NOT NULL DEFAULT 'reconciling',
    PRIMARY KEY(scope_id, adapter, external_run_id)
);
CREATE TABLE external_checkpoints (
    scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
    adapter TEXT NOT NULL CHECK(adapter IN ('intern', 'swarm', 'mq')),
    stream_id TEXT NOT NULL,
    checkpoint_json TEXT NOT NULL,
    PRIMARY KEY(scope_id, adapter, stream_id)
);
CREATE TABLE cloud_command_outbox (
    scope_id TEXT NOT NULL REFERENCES cloud_scopes(id),
    command_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    body BLOB NOT NULL,
    body_sha256 TEXT NOT NULL,
    auth_epoch INTEGER NOT NULL,
    expected_generation INTEGER NOT NULL CHECK(expected_generation >= 0),
    delivery_state TEXT NOT NULL CHECK(delivery_state IN ('pending', 'outcome_unknown', 'received', 'delivered', 'applied', 'refused', 'conflict')),
    PRIMARY KEY(scope_id, command_id),
    UNIQUE(scope_id, operation_id, idempotency_key)
);
CREATE TRIGGER immutable_cloud_command BEFORE UPDATE OF scope_id, command_id, operation_id, idempotency_key, body, body_sha256, expected_generation, auth_epoch ON cloud_command_outbox
BEGIN SELECT RAISE(ABORT, 'command identity and body are immutable'); END;
