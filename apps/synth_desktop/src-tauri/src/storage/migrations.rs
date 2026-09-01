use anyhow::{Context, Result};
use rusqlite::Connection;

/// Every embedded migration, in order. Version N applies only after 1..=N-1.
const MIGRATIONS: &[&str] = &[
    MIGRATION_1,
    MIGRATION_2,
    MIGRATION_3,
    MIGRATION_4,
    MIGRATION_5,
    MIGRATION_6,
    MIGRATION_7,
    MIGRATION_8,
    MIGRATION_9,
    MIGRATION_10,
    MIGRATION_11,
    MIGRATION_12,
    MIGRATION_13,
    MIGRATION_14,
    MIGRATION_15,
    MIGRATION_16,
    MIGRATION_17,
    MIGRATION_18,
    MIGRATION_19,
    MIGRATION_20,
    MIGRATION_21,
    MIGRATION_22,
    MIGRATION_23,
    MIGRATION_24,
    MIGRATION_25,
    MIGRATION_26,
    MIGRATION_27,
    MIGRATION_28,
    MIGRATION_29,
    MIGRATION_30,
    MIGRATION_31,
    MIGRATION_32,
    MIGRATION_33,
    MIGRATION_34,
    MIGRATION_35,
    MIGRATION_36,
    MIGRATION_37,
    MIGRATION_38,
    MIGRATION_39,
    MIGRATION_40,
    MIGRATION_41,
    MIGRATION_42,
    MIGRATION_43,
    MIGRATION_44,
    MIGRATION_45,
    MIGRATION_46,
    MIGRATION_47,
    MIGRATION_48,
    MIGRATION_49,
    MIGRATION_50,
    MIGRATION_51,
    MIGRATION_52,
    MIGRATION_53,
    MIGRATION_54,
    MIGRATION_55,
    MIGRATION_56,
    MIGRATION_57,
    MIGRATION_58,
    MIGRATION_59,
    MIGRATION_60,
    MIGRATION_61,
    MIGRATION_62,
];

/// Apply every migration the database has not reached yet.
pub fn apply_migrations(conn: &Connection) -> Result<i64> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;
    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for (index, migration) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if current < version {
            apply_one(conn, version, migration)?;
        }
    }

    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if version >= 12 {
        fold_rollback_usage_rows(conn)?;
    }
    heal_missing_tables(conn)?;
    heal_missing_columns(conn)?;
    heal_experiment_graph_shape(conn)?;
    // Builds briefly shipped these migrations under versions 64-66. Such a
    // database already has a maximum version above this lineage's contiguous
    // registry, so the ordinary loop cannot replay the data backfill. The
    // statement is idempotent and keeps those installations readable.
    conn.execute_batch(MIGRATION_60)?;
    Ok(version)
}

/// Tables this build cannot run without, keyed to the DDL that creates them.
///
/// A version number is a promise about *this* lineage. Several v0.5 lanes were
/// developed in parallel and each numbered its own migration 23, so an install
/// that reached version 23 on another lane's DDL would skip this lane's table
/// forever — `apply_migrations` only runs versions above the recorded maximum.
/// The DDL is `CREATE TABLE IF NOT EXISTS`, so re-running it costs nothing and
/// closes that hole regardless of which lane merged first.
///
/// This is intentionally CREATE-only. Migration 50 also contains ALTER TABLE
/// statements; replaying those during repair would fail as soon as one kernel
/// column already existed.
const OPTIMIZER_KERNEL_CREATE_ONLY: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_run_drafts (
    id TEXT PRIMARY KEY,
    algorithm TEXT NOT NULL,
    spec_json TEXT NOT NULL,
    spec_digest TEXT NOT NULL,
    admission_state TEXT NOT NULL CHECK (admission_state IN (
        'draft','validating','awaiting_approval','approved','not_required','rejected','expired','consumed'
    )),
    authorization_ref TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimizer_run_specs (
    optimizer_run_id TEXT PRIMARY KEY REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    spec_json TEXT NOT NULL,
    spec_digest TEXT NOT NULL,
    authorization_json TEXT NOT NULL DEFAULT '{}',
    admitted_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimizer_algorithm_projections (
    optimizer_run_id TEXT PRIMARY KEY REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    algorithm TEXT NOT NULL,
    reducer_version TEXT NOT NULL,
    as_of_sequence INTEGER NOT NULL,
    projection_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimizer_work_items (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    work_item_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    lifecycle TEXT NOT NULL,
    terminal TEXT,
    external_ref TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, work_item_id)
);
CREATE INDEX IF NOT EXISTS optimizer_work_items_run
ON optimizer_work_items(optimizer_run_id, lifecycle);

CREATE TABLE IF NOT EXISTS optimizer_usage_ledger (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    lane TEXT NOT NULL,
    cost_usd REAL,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, sequence, lane)
);

CREATE TABLE IF NOT EXISTS optimizer_evidence_refs (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    ref_id TEXT NOT NULL,
    digest TEXT,
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, kind, ref_id)
);

CREATE TABLE IF NOT EXISTS optimizer_evidence_amendments (
    amendment_id TEXT PRIMARY KEY,
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    terminal_sequence INTEGER NOT NULL,
    evidence_json TEXT NOT NULL,
    recorded_at TEXT NOT NULL
);
"#;

const REQUIRED_TABLES: &[(&str, &str)] = &[
    ("optimizer_terminal_manifests", MIGRATION_23),
    ("secret_refs", MIGRATION_25),
    ("credential_locators", CREDENTIAL_LOCATORS_TABLE_DDL),
    ("product_telemetry_events", MIGRATION_26),
    ("local_lora_checkpoints", MIGRATION_28),
    ("hosted_lora_overlays", MIGRATION_29),
    ("optimizer_run_ownership", MIGRATION_33),
    ("optimizer_run_media", MIGRATION_41),
    ("optimizer_frames", MIGRATION_42),
    ("optimizer_frame_usage", MIGRATION_42),
    ("optimizer_run_drafts", OPTIMIZER_KERNEL_CREATE_ONLY),
    ("optimizer_run_specs", OPTIMIZER_KERNEL_CREATE_ONLY),
    (
        "optimizer_algorithm_projections",
        OPTIMIZER_KERNEL_CREATE_ONLY,
    ),
    ("optimizer_work_items", OPTIMIZER_KERNEL_CREATE_ONLY),
    ("optimizer_usage_ledger", OPTIMIZER_KERNEL_CREATE_ONLY),
    ("optimizer_evidence_refs", OPTIMIZER_KERNEL_CREATE_ONLY),
    (
        "optimizer_evidence_amendments",
        OPTIMIZER_KERNEL_CREATE_ONLY,
    ),
    ("experiment_lineage", MIGRATION_44),
    ("experiment_session_cursor", MIGRATION_44),
    ("optimizer_cancellation_requests", MIGRATION_55),
    ("optimizer_effective_contracts", MIGRATION_56),
    ("optimizer_run_artifacts", MIGRATION_56),
    ("optimizer_projection_outbox", PROJECTION_OUTBOX_CREATE_ONLY),
    (
        "paid_compute_conversation_budgets",
        PAID_COMPUTE_BUDGET_CREATE_ONLY,
    ),
    ("paid_compute_reservations", PAID_COMPUTE_BUDGET_CREATE_ONLY),
    ("annotation_reservations", MIGRATION_62),
    ("annotation_broker_secrets", MIGRATION_62),
];

const PROJECTION_OUTBOX_CREATE_ONLY: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_projection_outbox (
    run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    projection_revision INTEGER NOT NULL,
    consumer TEXT NOT NULL,
    delivery_state TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (run_id, projection_revision, consumer)
);
CREATE INDEX IF NOT EXISTS optimizer_projection_outbox_pending
ON optimizer_projection_outbox(delivery_state, updated_at);
"#;

const PAID_COMPUTE_BUDGET_CREATE_ONLY: &str = r#"
CREATE TABLE IF NOT EXISTS paid_compute_conversation_budgets (
    session_id TEXT PRIMARY KEY,
    conversation_cap_usd_micros INTEGER NOT NULL,
    max_request_usd_micros INTEGER NOT NULL,
    providers_json TEXT NOT NULL,
    settled_spend_usd_micros INTEGER NOT NULL DEFAULT 0,
    auto_disabled INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS paid_compute_reservations (
    approval_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    reserved_usd_micros INTEGER NOT NULL,
    preparation_digest TEXT,
    status TEXT NOT NULL CHECK (status IN ('reserved', 'settled', 'released')),
    settled_usd_micros INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS paid_compute_reservations_session
ON paid_compute_reservations(session_id, status);
"#;

fn heal_missing_tables(conn: &Connection) -> Result<()> {
    for (table, ddl) in REQUIRED_TABLES {
        let present: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )?;
        if !present {
            conn.execute_batch(ddl)
                .with_context(|| format!("heal missing table {table}"))?;
        }
    }
    Ok(())
}

/// Repair columns consumed by prerelease migration-number collisions.
///
/// SQLite has no `ADD COLUMN IF NOT EXISTS`, so this repair must inspect the
/// authoritative table shape before applying the one missing alteration. A
/// missing cursor column is not optional: experiment attachment otherwise
/// fails later with an unrelated lookup error.
fn heal_missing_columns(conn: &Connection) -> Result<()> {
    let active_experiment_id_present: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM pragma_table_info('sessions')
            WHERE name='active_experiment_id'
        )",
        [],
        |row| row.get(0),
    )?;
    if !active_experiment_id_present {
        conn.execute_batch(
            "ALTER TABLE sessions ADD COLUMN active_experiment_id TEXT \
             REFERENCES experiment_groups(id);",
        )
        .context("heal missing sessions.active_experiment_id")?;
    }
    for (table, column) in [
        ("containers", "current_failure_id"),
        ("optimizer_runs", "terminal_failure_id"),
        ("visuals", "current_failure_id"),
        ("runs", "terminal_failure_id"),
        ("experiment_groups", "current_failure_id"),
        ("experiment_groups", "request_id"),
    ] {
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name='{column}')"
        );
        let present: bool = conn.query_row(&sql, [], |row| row.get(0))?;
        if !present {
            let ddl = format!("ALTER TABLE {table} ADD COLUMN {column} TEXT");
            conn.execute_batch(&ddl)
                .with_context(|| format!("heal missing {table}.{column}"))?;
        }
    }
    for (column, ddl) in [
        (
            "locator_id",
            "ALTER TABLE secret_refs ADD COLUMN locator_id TEXT REFERENCES credential_locators(id)",
        ),
        (
            "preferred",
            "ALTER TABLE secret_refs ADD COLUMN preferred INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "source_state",
            "ALTER TABLE secret_refs ADD COLUMN source_state TEXT",
        ),
    ] {
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('secret_refs') WHERE name='{column}')"
        );
        let present: bool = conn.query_row(&sql, [], |row| row.get(0))?;
        if !present {
            conn.execute_batch(ddl)
                .with_context(|| format!("heal missing secret_refs.{column}"))?;
        }
    }
    for (table, column, sql_type) in [
        ("optimizer_runs", "lifecycle", "TEXT"),
        ("optimizer_runs", "phase", "TEXT"),
        ("optimizer_runs", "condition", "TEXT"),
        ("optimizer_runs", "placement", "TEXT"),
        ("optimizer_runs", "aggregate_sequence", "INTEGER"),
        ("optimizer_runs", "projection_revision", "INTEGER"),
        ("optimizer_events", "producer_id", "TEXT"),
        ("optimizer_events", "producer_sequence", "INTEGER"),
        ("optimizer_events", "payload_digest", "TEXT"),
        ("optimizer_events", "aggregate_sequence", "INTEGER"),
        ("optimizer_events", "committed_at", "TEXT"),
    ] {
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name='{column}')"
        );
        let present: bool = conn.query_row(&sql, [], |row| row.get(0))?;
        if !present {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {sql_type}"
            ))
            .with_context(|| format!("heal missing {table}.{column}"))?;
        }
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS secret_refs_locator ON secret_refs(locator_id);
         CREATE INDEX IF NOT EXISTS secret_refs_preferred
         ON secret_refs(provider, preferred) WHERE preferred = 1;
         CREATE UNIQUE INDEX IF NOT EXISTS experiment_groups_request
         ON experiment_groups(request_id) WHERE request_id IS NOT NULL;",
    )
    .context("heal credential source indexes")?;
    Ok(())
}

/// Repair the graph projection only from canonical optimizer-run membership.
/// Request ids and legacy campaign ids are no longer execution members.
fn heal_experiment_graph_shape(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        INSERT OR IGNORE INTO experiment_nodes(
            id, experiment_id, kind, title, status, config_json, created_at, updated_at
        )
        SELECT
            group_id || ':' || member_kind || ':' || member_id,
            group_id,
            member_kind,
            title,
            'running',
            '{"memberKind":"' || replace(member_kind, '"', '\"') ||
                '","memberId":"' || replace(member_id, '"', '\"') || '"}',
            attached_at,
            attached_at
        FROM experiment_group_members
        WHERE member_kind = 'optimizer_run';
        "#,
    )
    .context("backfill experiment member graph projection")?;
    Ok(())
}

/// Migration 11 keeps an empty legacy table as a rollback write buffer. A v10
/// binary can therefore launch against a v11 database and keep recording local
/// dev charges. The next current-binary launch folds those rows into the authoritative
/// ledger and clears the buffer atomically, so neither binary double-counts.
fn fold_rollback_usage_rows(conn: &Connection) -> Result<()> {
    let batch = format!("BEGIN IMMEDIATE;\n{FOLD_LEGACY_USAGE_ROWS}\nCOMMIT;");
    if let Err(error) = conn.execute_batch(&batch) {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(anyhow::Error::from(error).context("fold rollback usage rows"));
    }
    Ok(())
}

/// One migration and its version stamp commit as a single transaction: a
/// crash leaves the database entirely before or entirely after a version,
/// never between. Half-applying a data-moving migration (6 renames a table,
/// 8 copies one) while the stamp stays behind would make every relaunch
/// replay the move into already-moved data and wedge the app at open.
fn apply_one(conn: &Connection, version: i64, sql: &str) -> Result<()> {
    let batch = format!(
        "BEGIN IMMEDIATE;\n{sql}\nINSERT INTO schema_migrations(version, applied_at) VALUES ({version}, datetime('now'));\nCOMMIT;"
    );
    if let Err(error) = conn.execute_batch(&batch) {
        // A failed batch can leave its transaction open on the shared
        // connection; roll it back so the caller's error path still works.
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(anyhow::Error::from(error).context(format!("apply schema migration {version}")));
    }
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )
    .context("read schema version")
}

const MIGRATION_1: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    vcs TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    target_json TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    remote_id TEXT,
    codex_thread_id TEXT,
    status TEXT NOT NULL,
    state_generation INTEGER,
    latest_cursor INTEGER NOT NULL DEFAULT 0,
    active_run_id TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    mode TEXT NOT NULL,
    status TEXT NOT NULL,
    latest_cursor INTEGER NOT NULL DEFAULT 0,
    checkpoint_json TEXT,
    outcome_json TEXT,
    model TEXT,
    adapter TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT
);

CREATE TABLE IF NOT EXISTS events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    session_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    session_sequence INTEGER,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    remote_sequence INTEGER,
    command_id TEXT,
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS events_session_sequence
ON events(session_id, session_sequence)
WHERE session_id IS NOT NULL AND session_sequence IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS events_remote_dedupe
ON events(session_id, source, remote_sequence)
WHERE remote_sequence IS NOT NULL;

CREATE INDEX IF NOT EXISTS events_global_sequence ON events(sequence);
CREATE INDEX IF NOT EXISTS sessions_updated_at ON sessions(updated_at DESC);

CREATE TABLE IF NOT EXISTS source_cursors (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    cursor INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (session_id, source)
);

CREATE TABLE IF NOT EXISTS approvals (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    request_method TEXT NOT NULL,
    status TEXT NOT NULL,
    request_json TEXT NOT NULL,
    decision TEXT,
    created_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE TABLE IF NOT EXISTS containers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    location TEXT NOT NULL,
    status TEXT NOT NULL,
    base_url TEXT,
    pool_id TEXT,
    task_family TEXT,
    last_rollout_id TEXT,
    health_json TEXT NOT NULL DEFAULT '{}',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS traces (
    id TEXT PRIMARY KEY,
    digest TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    source TEXT NOT NULL,
    container_id TEXT REFERENCES containers(id) ON DELETE SET NULL,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    reward REAL,
    metrics_json TEXT NOT NULL DEFAULT '[]',
    path TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS visuals (
    id TEXT PRIMARY KEY,
    current_revision INTEGER NOT NULL DEFAULT 1,
    title TEXT NOT NULL,
    template_id TEXT NOT NULL,
    status TEXT NOT NULL,
    renderer_kind TEXT NOT NULL,
    bindings_json TEXT NOT NULL DEFAULT '{}',
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    message_id TEXT,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    trace_id TEXT REFERENCES traces(id) ON DELETE SET NULL,
    parent_visual_id TEXT REFERENCES visuals(id) ON DELETE SET NULL,
    source_agent_id TEXT,
    source_model TEXT,
    content_digest TEXT,
    preview_digest TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS visual_revisions (
    visual_id TEXT NOT NULL REFERENCES visuals(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    template_id TEXT NOT NULL,
    renderer_kind TEXT NOT NULL,
    content_digest TEXT,
    bindings_digest TEXT,
    bindings_json TEXT,
    preview_digest TEXT,
    author_agent_id TEXT,
    parent_revision INTEGER,
    created_at TEXT NOT NULL,
    PRIMARY KEY (visual_id, revision)
);

CREATE TABLE IF NOT EXISTS visual_relationships (
    visual_id TEXT NOT NULL REFERENCES visuals(id) ON DELETE CASCADE,
    related_kind TEXT NOT NULL,
    related_id TEXT NOT NULL,
    PRIMARY KEY (visual_id, related_kind, related_id)
);

CREATE TABLE IF NOT EXISTS usage_ledger (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runtime_settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

const MIGRATION_2: &str = r#"
ALTER TABLE runs ADD COLUMN updated_at TEXT;
UPDATE runs
SET updated_at = COALESCE(completed_at, started_at, created_at)
WHERE updated_at IS NULL;

CREATE INDEX IF NOT EXISTS runs_session_created
ON runs(session_id, created_at DESC);

CREATE TABLE IF NOT EXISTS command_receipts (
    command_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    request_json TEXT NOT NULL DEFAULT '{}',
    response_json TEXT,
    remote_cursor INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS command_receipts_session_created
ON command_receipts(session_id, created_at DESC);
"#;

const MIGRATION_3: &str = r#"
CREATE TABLE IF NOT EXISTS trace_imports (
    input_digest TEXT PRIMARY KEY,
    stored_path TEXT,
    source_kind TEXT NOT NULL,
    source_uri TEXT,
    compatibility_level TEXT NOT NULL,
    validation_status TEXT NOT NULL,
    detected_schema TEXT,
    detected_bundle_digest TEXT,
    byte_size INTEGER NOT NULL DEFAULT 0,
    imported_at TEXT NOT NULL,
    error_json TEXT NOT NULL DEFAULT '[]',
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS trace_bundles (
    bundle_digest TEXT PRIMARY KEY,
    archive_digest TEXT NOT NULL UNIQUE,
    archive_path TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    compatibility_level TEXT NOT NULL,
    validation_status TEXT NOT NULL,
    self_contained INTEGER NOT NULL,
    source_kind TEXT NOT NULL,
    source_uri TEXT,
    manifest_generation INTEGER,
    object_count INTEGER NOT NULL DEFAULT 0,
    byte_size INTEGER NOT NULL DEFAULT 0,
    imported_at TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS trace_bundle_members (
    bundle_digest TEXT NOT NULL REFERENCES trace_bundles(bundle_digest) ON DELETE CASCADE,
    trace_row_id TEXT NOT NULL REFERENCES traces(id) ON DELETE CASCADE,
    trace_digest TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    capture_id TEXT,
    binding_digest TEXT,
    sealed_path TEXT,
    PRIMARY KEY (bundle_digest, trace_digest)
);

CREATE INDEX IF NOT EXISTS trace_bundle_members_trace_digest
ON trace_bundle_members(trace_digest);

CREATE TABLE IF NOT EXISTS trace_assets (
    bundle_digest TEXT NOT NULL REFERENCES trace_bundles(bundle_digest) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    kind TEXT NOT NULL,
    role TEXT,
    bytes_digest TEXT NOT NULL,
    semantic_digest TEXT,
    media_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    availability TEXT NOT NULL,
    PRIMARY KEY (bundle_digest, relative_path)
);

CREATE TABLE IF NOT EXISTS trace_index (
    trace_digest TEXT PRIMARY KEY,
    projector_version TEXT NOT NULL,
    trace_kind TEXT,
    producer TEXT,
    model TEXT,
    provider TEXT,
    harness TEXT,
    benchmark TEXT,
    task_id TEXT,
    seed INTEGER,
    terminal_reason TEXT,
    lifecycle_status TEXT,
    capture_status TEXT,
    reward REAL,
    cost_usd REAL,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    span_count INTEGER NOT NULL DEFAULT 0,
    event_count INTEGER NOT NULL DEFAULT 0,
    tool_call_count INTEGER NOT NULL DEFAULT 0,
    error_count INTEGER NOT NULL DEFAULT 0,
    started_at TEXT,
    ended_at TEXT,
    duration_ms INTEGER,
    has_media INTEGER NOT NULL DEFAULT 0,
    has_evidence INTEGER NOT NULL DEFAULT 0,
    search_text TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS trace_index_model ON trace_index(model);
CREATE INDEX IF NOT EXISTS trace_index_benchmark_task ON trace_index(benchmark, task_id);
CREATE INDEX IF NOT EXISTS trace_index_reward ON trace_index(reward);

CREATE TABLE IF NOT EXISTS trace_tags (
    trace_digest TEXT NOT NULL,
    namespace TEXT NOT NULL,
    value TEXT NOT NULL,
    source_digest TEXT,
    PRIMARY KEY (trace_digest, namespace, value)
);

CREATE TABLE IF NOT EXISTS trace_projection_cache (
    trace_digest TEXT NOT NULL,
    projection_kind TEXT NOT NULL,
    projection_schema TEXT NOT NULL,
    projector_version TEXT NOT NULL,
    source_digest TEXT NOT NULL,
    payload_digest TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (trace_digest, projection_kind, projector_version)
);

CREATE TABLE IF NOT EXISTS trace_annotations (
    id TEXT PRIMARY KEY,
    trace_digest TEXT NOT NULL,
    selector_json TEXT NOT NULL,
    kind TEXT NOT NULL,
    body_json TEXT NOT NULL,
    author_json TEXT NOT NULL,
    supersedes_id TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS trace_annotations_trace_created
ON trace_annotations(trace_digest, created_at);
"#;

const MIGRATION_4: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_runs (
    id TEXT PRIMARY KEY,
    algorithm_id TEXT NOT NULL,
    algorithm_version TEXT,
    status TEXT NOT NULL,
    source TEXT NOT NULL,
    objective TEXT,
    project_ref TEXT,
    session_ref TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    cursor_seq INTEGER NOT NULL DEFAULT 0,
    capabilities_json TEXT NOT NULL DEFAULT '{}',
    bindings_json TEXT NOT NULL DEFAULT '[]',
    input_refs_json TEXT NOT NULL DEFAULT '[]',
    output_refs_json TEXT NOT NULL DEFAULT '[]',
    visual_refs_json TEXT NOT NULL DEFAULT '[]',
    summary_json TEXT NOT NULL DEFAULT '{}',
    usage_json TEXT NOT NULL DEFAULT '{}',
    error_json TEXT,
    payload_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS optimizer_runs_status ON optimizer_runs(status);
CREATE INDEX IF NOT EXISTS optimizer_runs_algorithm ON optimizer_runs(algorithm_id);
CREATE INDEX IF NOT EXISTS optimizer_runs_updated ON optimizer_runs(updated_at);

CREATE TABLE IF NOT EXISTS optimizer_event_cursors (
    optimizer_run_id TEXT PRIMARY KEY REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    cursor_seq INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimizer_events (
    event_id TEXT PRIMARY KEY,
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    sequence_number INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    algorithm_id TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (optimizer_run_id, sequence_number)
);

CREATE INDEX IF NOT EXISTS optimizer_events_run_seq
ON optimizer_events(optimizer_run_id, sequence_number);

CREATE TABLE IF NOT EXISTS optimizer_relationships (
    from_kind TEXT NOT NULL,
    from_id TEXT NOT NULL,
    edge TEXT NOT NULL,
    to_kind TEXT NOT NULL,
    to_id TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    PRIMARY KEY (from_kind, from_id, edge, to_kind, to_id)
);

CREATE INDEX IF NOT EXISTS optimizer_relationships_to
ON optimizer_relationships(to_kind, to_id);

CREATE TABLE IF NOT EXISTS optimizer_cached_slices (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    slice_id TEXT NOT NULL,
    cursor_seq INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, slice_id)
);
"#;

const MIGRATION_5: &str = r#"
CREATE TABLE IF NOT EXISTS conversation_workspace_scopes (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    workspace TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    bound_revision INTEGER NOT NULL DEFAULT 0,
    binding_status TEXT NOT NULL DEFAULT 'pending',
    binding_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspace_attachments (
    session_id TEXT NOT NULL REFERENCES conversation_workspace_scopes(session_id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    access TEXT NOT NULL CHECK(access IN ('read_only', 'read_write')),
    source TEXT NOT NULL CHECK(source IN ('user_picker', 'agent_request', 'migrated_default')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (session_id, path)
);

CREATE TABLE IF NOT EXISTS workspace_grant_requests (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    access TEXT NOT NULL CHECK(access IN ('read_only', 'read_write')),
    reason TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'approved', 'denied')),
    created_at TEXT NOT NULL,
    resolved_at TEXT
);
CREATE INDEX IF NOT EXISTS workspace_grants_session_status
ON workspace_grant_requests(session_id, status, created_at);
"#;

const MIGRATION_6: &str = r#"
ALTER TABLE workspace_attachments RENAME TO workspace_attachments_v5;
CREATE TABLE workspace_attachments (
    session_id TEXT NOT NULL REFERENCES conversation_workspace_scopes(session_id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    access TEXT NOT NULL CHECK(access IN ('read_only', 'read_write')),
    source TEXT NOT NULL CHECK(source IN ('user_picker', 'recent_folder', 'agent_request', 'migrated_default')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (session_id, path)
);
INSERT INTO workspace_attachments(session_id,path,access,source,created_at)
SELECT session_id,path,access,source,created_at FROM workspace_attachments_v5;
DROP TABLE workspace_attachments_v5;
"#;

const MIGRATION_7: &str = r#"
CREATE TABLE IF NOT EXISTS model_performance_samples (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_revision TEXT,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    request_id TEXT NOT NULL,
    measurement_kind TEXT NOT NULL CHECK(measurement_kind IN ('decode', 'observed_stream', 'end_to_end', 'provider_reported')),
    status TEXT NOT NULL CHECK(status IN ('completed', 'failed', 'interrupted')),
    started_at_ms INTEGER NOT NULL,
    first_output_at_ms INTEGER,
    last_output_at_ms INTEGER,
    completed_at_ms INTEGER NOT NULL,
    input_tokens INTEGER,
    cached_input_tokens INTEGER,
    reasoning_tokens INTEGER,
    output_tokens INTEGER,
    ttft_ms REAL,
    observed_output_tps REAL,
    end_to_end_output_tps REAL,
    source TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(provider, request_id)
);

CREATE INDEX IF NOT EXISTS model_performance_model_created
ON model_performance_samples(provider, model_id, measurement_kind, created_at DESC);

CREATE INDEX IF NOT EXISTS model_performance_completed
ON model_performance_samples(status, completed_at_ms DESC);
"#;

/// One authoritative accounting record per provider request. Supersedes
/// `model_performance_samples` (all rows are imported, then the old table is
/// dropped) and takes over as the source for both throughput summaries and
/// the usage dashboard. The legacy `usage_ledger` table is left in place
/// until migration 11 folds its rows into this ledger and drops it.
///
/// `total_tokens` is a generated column so `input + output` can never drift
/// from its parts, and it stays NULL — not zero — while neither side has been
/// reported. Unlike the performance table this one has no retention cap:
/// billing history must never be silently trimmed.
const MIGRATION_8: &str = r#"
CREATE TABLE IF NOT EXISTS usage_records (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_revision TEXT,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    request_id TEXT NOT NULL,
    measurement_kind TEXT NOT NULL CHECK(measurement_kind IN ('decode', 'observed_stream', 'end_to_end', 'provider_reported')),
    status TEXT NOT NULL CHECK(status IN ('completed', 'failed', 'interrupted')),
    started_at_ms INTEGER NOT NULL,
    first_output_at_ms INTEGER,
    last_output_at_ms INTEGER,
    completed_at_ms INTEGER NOT NULL,
    input_tokens INTEGER,
    cached_input_tokens INTEGER,
    cache_write_tokens INTEGER,
    reasoning_tokens INTEGER,
    output_tokens INTEGER,
    total_tokens INTEGER GENERATED ALWAYS AS (
        CASE WHEN input_tokens IS NULL AND output_tokens IS NULL THEN NULL
             ELSE COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) END
    ) STORED,
    ttft_ms REAL,
    observed_output_tps REAL,
    end_to_end_output_tps REAL,
    billed_cost_usd REAL,
    estimated_cost_usd REAL,
    cost_source TEXT NOT NULL DEFAULT 'none' CHECK(cost_source IN ('provider_reported', 'synth_cloud', 'tariff_estimate', 'none')),
    source TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(provider, request_id)
);

-- OR IGNORE: recovery for databases wedged by a pre-transactional build that
-- crashed after this copy committed but before the version stamp did —
-- replaying the copy must skip rows that already arrived, not fail the open.
INSERT OR IGNORE INTO usage_records (
    id, provider, model_id, model_revision, session_id, run_id, request_id,
    measurement_kind, status, started_at_ms, first_output_at_ms, last_output_at_ms,
    completed_at_ms, input_tokens, cached_input_tokens, reasoning_tokens,
    output_tokens, ttft_ms, observed_output_tps, end_to_end_output_tps,
    source, created_at
)
SELECT
    id, provider, model_id, model_revision, session_id, run_id, request_id,
    measurement_kind, status, started_at_ms, first_output_at_ms, last_output_at_ms,
    completed_at_ms, input_tokens, cached_input_tokens, reasoning_tokens,
    output_tokens, ttft_ms, observed_output_tps, end_to_end_output_tps,
    source, created_at
FROM model_performance_samples;

DROP TABLE model_performance_samples;

CREATE INDEX IF NOT EXISTS usage_records_model_created
ON usage_records(provider, model_id, measurement_kind, created_at DESC);

CREATE INDEX IF NOT EXISTS usage_records_completed
ON usage_records(status, completed_at_ms DESC);

CREATE INDEX IF NOT EXISTS usage_records_window
ON usage_records(completed_at_ms DESC);
"#;

/// First-class SessionKind column. Authority moves off `target_json.kind`
/// string checks (`metadata_bags_are_not_authority`). Backfill treats
/// `"intern"` as Intern and everything else (including historical `"local"`
/// Codex/Laguna bags) as Codex.
const MIGRATION_9: &str = r#"
ALTER TABLE sessions ADD COLUMN kind TEXT NOT NULL DEFAULT 'codex';

UPDATE sessions
SET kind = 'intern'
WHERE json_extract(target_json, '$.kind') = 'intern';

CREATE INDEX IF NOT EXISTS sessions_kind ON sessions(kind);
"#;

/// Typed RuntimeTarget index + normalize legacy remote+synth-cloud bags to cloud.
const MIGRATION_10: &str = r#"
ALTER TABLE sessions ADD COLUMN runtime_target_kind TEXT NOT NULL DEFAULT 'local';

UPDATE sessions SET runtime_target_kind = CASE
    WHEN json_extract(target_json, '$.kind') = 'intern' THEN 'intern'
    WHEN json_extract(target_json, '$.kind') = 'cloud' THEN 'cloud'
    WHEN json_extract(target_json, '$.kind') = 'remote'
         AND json_extract(target_json, '$.provider') = 'synth-cloud' THEN 'cloud'
    WHEN json_extract(target_json, '$.kind') = 'remote' THEN 'remote'
    WHEN json_extract(target_json, '$.kind') = 'local' THEN 'local'
    WHEN json_extract(target_json, '$.kind') = 'codex' THEN 'local'
    ELSE 'local'
END;

-- Canonical CloudRuntime wire: kind=cloud (drop provider=synth-cloud).
UPDATE sessions
SET target_json = json_object(
    'kind', 'cloud',
    'model', COALESCE(json_extract(target_json, '$.model'), ''),
    'adapter', json_extract(target_json, '$.adapter')
)
WHERE runtime_target_kind = 'cloud'
  AND (
    json_extract(target_json, '$.kind') = 'remote'
    OR json_extract(target_json, '$.provider') = 'synth-cloud'
  );

CREATE INDEX IF NOT EXISTS sessions_runtime_target_kind
ON sessions(runtime_target_kind);
"#;

/// Shared copy used by migration 11 and by every later open. Rows are deleted
/// only after their stable legacy request id exists in `usage_records`; an
/// unexpected primary-key collision therefore retains the source row rather
/// than silently losing accounting data.
const FOLD_LEGACY_USAGE_ROWS: &str = r#"
INSERT OR IGNORE INTO usage_records (
    id, provider, model_id, session_id, run_id, request_id,
    measurement_kind, status, started_at_ms, completed_at_ms,
    input_tokens, output_tokens,
    billed_cost_usd, estimated_cost_usd, cost_source,
    source, created_at
)
SELECT
    id,
    provider,
    model,
    CASE
        WHEN session_id IS NOT NULL
             AND EXISTS (SELECT 1 FROM sessions s WHERE s.id = usage_ledger.session_id)
            THEN session_id
        ELSE NULL
    END,
    CASE
        WHEN run_id IS NOT NULL
             AND EXISTS (SELECT 1 FROM runs r WHERE r.id = usage_ledger.run_id)
            THEN run_id
        ELSE NULL
    END,
    'legacy-ledger:' || id,
    'provider_reported',
    'completed',
    COALESCE(CAST(strftime('%s', substr(created_at, 1, 19)) AS INTEGER), 0) * 1000,
    COALESCE(CAST(strftime('%s', substr(created_at, 1, 19)) AS INTEGER), 0) * 1000,
    prompt_tokens,
    completion_tokens,
    cost_usd,
    NULL,
    CASE WHEN cost_usd IS NOT NULL THEN 'provider_reported' ELSE 'none' END,
    'legacy_usage_ledger',
    created_at
FROM usage_ledger;

-- A primary-key collision with an unrelated usage record must not discard the
-- rollback row. Leave it buffered unless its legacy request identity landed.
DELETE FROM usage_ledger
WHERE EXISTS (
    SELECT 1 FROM usage_records
    WHERE request_id = 'legacy-ledger:' || usage_ledger.id
      AND provider = usage_ledger.provider
      AND source = 'legacy_usage_ledger'
);
"#;

/// Fold the legacy ledger into `usage_records`, then retain its empty schema as
/// a rollback write buffer. The previous production binary reads and writes
/// this table; dropping it makes rollback fail with `no such table`.
const MIGRATION_11: &str = FOLD_LEGACY_USAGE_ROWS;

/// Additive repair for databases already stamped at the original migration 11,
/// which dropped `usage_ledger`. Fresh upgrades retain the migration-1 table;
/// already-upgraded QA databases recreate the exact legacy schema here.
const MIGRATION_12: &str = r#"
CREATE TABLE IF NOT EXISTS usage_ledger (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL,
    created_at TEXT NOT NULL
);
"#;

const MIGRATION_13: &str = r#"
CREATE TABLE IF NOT EXISTS visual_renditions (
    visual_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    format TEXT NOT NULL,
    theme TEXT NOT NULL,
    size_class TEXT NOT NULL,
    content_digest TEXT NOT NULL,
    media_type TEXT NOT NULL,
    renderer_version TEXT NOT NULL,
    width_px INTEGER,
    height_px INTEGER,
    created_at TEXT NOT NULL,
    PRIMARY KEY (visual_id, revision, format, theme, size_class),
    FOREIGN KEY (visual_id, revision)
        REFERENCES visual_revisions(visual_id, revision)
);
"#;

const MIGRATION_14: &str = r#"
CREATE TABLE IF NOT EXISTS visual_annotations (
    id TEXT PRIMARY KEY,
    visual_id TEXT NOT NULL REFERENCES visuals(id) ON DELETE CASCADE,
    visual_revision INTEGER NOT NULL,
    source_digest TEXT,
    selector_json TEXT NOT NULL,
    kind TEXT NOT NULL,
    body TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    author_id TEXT NOT NULL,
    supersedes_id TEXT REFERENCES visual_annotations(id),
    tombstoned INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (visual_id, visual_revision)
        REFERENCES visual_revisions(visual_id, revision)
);

CREATE INDEX IF NOT EXISTS visual_annotations_visual_revision
ON visual_annotations(visual_id, visual_revision, created_at);

CREATE TABLE IF NOT EXISTS visual_seals (
    receipt_digest TEXT PRIMARY KEY,
    visual_id TEXT NOT NULL REFERENCES visuals(id) ON DELETE CASCADE,
    visual_revision INTEGER NOT NULL,
    artifact_id TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    compiler_name TEXT NOT NULL,
    compiler_version TEXT NOT NULL,
    runtime_digest TEXT NOT NULL,
    index_digest TEXT NOT NULL,
    data_digest TEXT NOT NULL,
    receipt_size_bytes INTEGER NOT NULL,
    total_size_bytes INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (visual_id, visual_revision)
        REFERENCES visual_revisions(visual_id, revision)
);

CREATE INDEX IF NOT EXISTS visual_seals_visual_revision
ON visual_seals(visual_id, visual_revision, created_at);

CREATE TABLE IF NOT EXISTS visual_uploads (
    receipt_digest TEXT PRIMARY KEY REFERENCES visual_seals(receipt_digest) ON DELETE CASCADE,
    collection_id TEXT,
    publication_id TEXT,
    publication_revision INTEGER,
    prepare_expires_at TEXT,
    completed_members_json TEXT NOT NULL DEFAULT '[]',
    state TEXT NOT NULL,
    committed_url TEXT,
    error TEXT,
    updated_at TEXT NOT NULL,
    CHECK (state IN ('prepared','uploading','finalizing','committed','failed')),
    CHECK ((state = 'committed' AND committed_url IS NOT NULL) OR (state != 'committed' AND committed_url IS NULL))
);
"#;

const MIGRATION_15: &str = r#"
CREATE TABLE IF NOT EXISTS reports (
    id TEXT PRIMARY KEY,
    project_ref TEXT,
    current_revision INTEGER NOT NULL DEFAULT 1,
    title TEXT NOT NULL,
    summary TEXT,
    authors_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL CHECK(status IN ('draft','sealed')),
    created_by TEXT NOT NULL DEFAULT 'user',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS report_revisions (
    report_id TEXT NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    title TEXT NOT NULL,
    summary TEXT,
    authors_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('draft','sealed')),
    content_digest TEXT,
    compiler_name TEXT,
    compiler_version TEXT,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (report_id, revision)
);

CREATE TABLE IF NOT EXISTS report_revision_blocks (
    report_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    position INTEGER NOT NULL,
    block_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    anchor TEXT NOT NULL,
    title TEXT,
    payload_json TEXT NOT NULL,
    source_revision TEXT,
    source_digest TEXT,
    access_state TEXT NOT NULL DEFAULT 'accessible',
    integrity_state TEXT NOT NULL DEFAULT 'unknown',
    PRIMARY KEY (report_id, revision, position),
    UNIQUE (report_id, revision, block_id),
    UNIQUE (report_id, revision, anchor),
    FOREIGN KEY (report_id, revision) REFERENCES report_revisions(report_id, revision)
);

CREATE TABLE IF NOT EXISTS report_sources (
    report_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    source_id TEXT NOT NULL,
    resource_kind TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    resource_revision TEXT,
    resource_digest TEXT,
    relation TEXT NOT NULL,
    access_state TEXT NOT NULL,
    integrity_state TEXT NOT NULL,
    PRIMARY KEY (report_id, revision, source_id),
    FOREIGN KEY (report_id, revision) REFERENCES report_revisions(report_id, revision)
);

CREATE TABLE IF NOT EXISTS report_claims (
    report_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    claim_id TEXT NOT NULL,
    statement TEXT NOT NULL,
    status TEXT NOT NULL,
    evidence_json TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (report_id, revision, claim_id),
    FOREIGN KEY (report_id, revision) REFERENCES report_revisions(report_id, revision)
);

CREATE TABLE IF NOT EXISTS report_limitations (
    report_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    limitation_id TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (report_id, revision, limitation_id),
    FOREIGN KEY (report_id, revision) REFERENCES report_revisions(report_id, revision)
);

CREATE TABLE IF NOT EXISTS experiment_records (
    experiment_id TEXT PRIMARY KEY,
    report_id TEXT REFERENCES reports(id) ON DELETE CASCADE,
    revision INTEGER,
    title TEXT NOT NULL,
    hypothesis TEXT,
    status TEXT NOT NULL,
    protocol_digest TEXT,
    arms_json TEXT NOT NULL DEFAULT '[]',
    runs_json TEXT NOT NULL DEFAULT '[]',
    results_json TEXT NOT NULL DEFAULT '[]',
    evaluator_refs_json TEXT NOT NULL DEFAULT '[]',
    trace_collection_refs_json TEXT NOT NULL DEFAULT '[]',
    claim_refs_json TEXT NOT NULL DEFAULT '[]',
    research_log_refs_json TEXT NOT NULL DEFAULT '[]',
    limitations_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    created_by TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS experiment_records_report
ON experiment_records(report_id, created_at);

CREATE TABLE IF NOT EXISTS research_log_entries (
    entry_id TEXT PRIMARY KEY,
    report_id TEXT REFERENCES reports(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    occurred_at TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    author TEXT NOT NULL,
    actor_kind TEXT NOT NULL CHECK(actor_kind IN ('human','agent')),
    entry_kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    tags_json TEXT NOT NULL DEFAULT '[]',
    links_json TEXT NOT NULL DEFAULT '[]',
    claim_effect TEXT,
    supersedes_entry_id TEXT REFERENCES research_log_entries(entry_id),
    source_digest TEXT,
    UNIQUE (report_id, sequence)
);

CREATE INDEX IF NOT EXISTS research_log_entries_report
ON research_log_entries(report_id, sequence);

CREATE TABLE IF NOT EXISTS report_seals (
    receipt_digest TEXT PRIMARY KEY,
    report_id TEXT NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    report_revision INTEGER NOT NULL,
    schema_version TEXT NOT NULL,
    compiler_name TEXT NOT NULL,
    compiler_version TEXT NOT NULL,
    runtime_digest TEXT NOT NULL,
    index_digest TEXT NOT NULL,
    data_digest TEXT NOT NULL,
    receipt_size_bytes INTEGER NOT NULL,
    total_size_bytes INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (report_id, report_revision)
        REFERENCES report_revisions(report_id, revision)
);

CREATE INDEX IF NOT EXISTS report_seals_report_revision
ON report_seals(report_id, report_revision, created_at);
"#;

const MIGRATION_16: &str = r#"
CREATE TABLE IF NOT EXISTS report_uploads (
    receipt_digest TEXT PRIMARY KEY REFERENCES report_seals(receipt_digest) ON DELETE CASCADE,
    collection_id TEXT,
    publication_id TEXT,
    publication_revision INTEGER,
    state TEXT NOT NULL,
    committed_url TEXT,
    error TEXT,
    updated_at TEXT NOT NULL,
    CHECK (state IN ('prepared','uploading','finalizing','committed','failed')),
    CHECK ((state = 'committed' AND committed_url IS NOT NULL) OR (state != 'committed' AND committed_url IS NULL))
);

CREATE TABLE IF NOT EXISTS report_review_comments (
    comment_id TEXT PRIMARY KEY,
    report_id TEXT NOT NULL,
    report_revision INTEGER NOT NULL,
    receipt_digest TEXT,
    publication_id TEXT,
    anchor TEXT,
    body TEXT NOT NULL,
    author_id TEXT NOT NULL DEFAULT 'user',
    created_at TEXT NOT NULL,
    FOREIGN KEY (report_id, report_revision)
        REFERENCES report_revisions(report_id, revision)
);

CREATE INDEX IF NOT EXISTS report_review_comments_revision
ON report_review_comments(report_id, report_revision, created_at);
"#;

const MIGRATION_17: &str = r#"
ALTER TABLE reports ADD COLUMN archived_at TEXT;

CREATE TABLE IF NOT EXISTS report_visibility_requests (
    request_id TEXT PRIMARY KEY,
    report_id TEXT NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    report_revision INTEGER NOT NULL,
    receipt_digest TEXT NOT NULL REFERENCES report_seals(receipt_digest) ON DELETE RESTRICT,
    target TEXT NOT NULL CHECK(target IN ('private','public','unpublished')),
    slug TEXT,
    reason TEXT,
    requested_by TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending','approved','denied','executed','failed','expired')),
    decision_by TEXT,
    error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    CHECK ((target = 'public' AND slug IS NOT NULL) OR target != 'public'),
    FOREIGN KEY (report_id, report_revision)
        REFERENCES report_revisions(report_id, revision)
);

CREATE INDEX IF NOT EXISTS report_visibility_requests_report
ON report_visibility_requests(report_id, created_at DESC);
"#;

/// Immutable query snapshots.
///
/// A visual must never bind to a live query string: it would silently return
/// different rows on every render and the app could not state what the user is
/// looking at. A snapshot freezes the normalized query, the matching record
/// ids, the facets needed to render, and when it was taken. Refreshing mints a
/// new snapshot; nothing here is ever updated in place, which is why there is
/// no `updated_at` and why the table carries no UPDATE path.
const MIGRATION_18: &str = r#"
CREATE TABLE IF NOT EXISTS query_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    domain TEXT NOT NULL CHECK(domain IN ('traces','optimizer_runs','containers','usage','plugins')),
    query_schema_version TEXT NOT NULL,
    query_ast TEXT NOT NULL,
    result_ids TEXT NOT NULL,
    result_count INTEGER NOT NULL,
    facets TEXT NOT NULL DEFAULT '{}',
    result_digest TEXT NOT NULL,
    queried_at TEXT NOT NULL,
    truncated INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS query_snapshots_domain_time
ON query_snapshots(domain, queried_at DESC);
"#;

/// Diagnostics (`synth.diagnostic-event.v1`) live in the journal as one more
/// event kind — there is no second authoritative event model. What they do
/// need is a way to be *found*: a typed diagnostic query filters on the
/// envelope's indexed labels and its correlation identities, and without these
/// partial expression indexes every such lookup degrades into a full scan of
/// every journal payload the app has ever written.
///
/// Every index here is partial on the diagnostic kind, so an installation that
/// never emits a diagnostic pays nothing for them.
const MIGRATION_19: &str = r#"
CREATE INDEX IF NOT EXISTS events_diagnostics_sequence
ON events(sequence DESC) WHERE kind = 'diagnostic.event';

CREATE INDEX IF NOT EXISTS events_diagnostics_component
ON events(json_extract(payload_json, '$.component'), sequence DESC)
WHERE kind = 'diagnostic.event';

CREATE INDEX IF NOT EXISTS events_diagnostics_severity
ON events(json_extract(payload_json, '$.severity'), sequence DESC)
WHERE kind = 'diagnostic.event';

CREATE INDEX IF NOT EXISTS events_diagnostics_code
ON events(json_extract(payload_json, '$.code'), sequence DESC)
WHERE kind = 'diagnostic.event';

CREATE INDEX IF NOT EXISTS events_diagnostics_visual
ON events(json_extract(payload_json, '$.visual_id'), sequence DESC)
WHERE kind = 'diagnostic.event';

CREATE INDEX IF NOT EXISTS events_diagnostics_rollout
ON events(json_extract(payload_json, '$.rollout_id'), sequence DESC)
WHERE kind = 'diagnostic.event';

CREATE INDEX IF NOT EXISTS events_diagnostics_stream
ON events(json_extract(payload_json, '$.stream_id'), sequence DESC)
WHERE kind = 'diagnostic.event';

CREATE INDEX IF NOT EXISTS events_diagnostics_container
ON events(json_extract(payload_json, '$.container_id'), sequence DESC)
WHERE kind = 'diagnostic.event';

CREATE INDEX IF NOT EXISTS events_diagnostics_optimizer_run
ON events(json_extract(payload_json, '$.optimizer_run_id'), sequence DESC)
WHERE kind = 'diagnostic.event';

CREATE INDEX IF NOT EXISTS events_diagnostics_trace
ON events(json_extract(payload_json, '$.trace_id'), sequence DESC)
WHERE kind = 'diagnostic.event';
"#;

/// Crash-recovery ownership.
///
/// `runs` stays an immutable historical record, so live ownership of the one
/// active turn gets its own row instead of more mutable columns there. A
/// `running` run is only live while a row here names the current boot epoch
/// and its lease has not expired; everything else is history that must be
/// reconciled before any client can read it as present tense.
///
/// `action_receipts` is what makes automatic restart decidable. Replaying a
/// turn is safe only while nothing consequential left the process; a receipt
/// records that it did, which external object it produced, and whether the
/// outcome ever settled.
const MIGRATION_20: &str = r#"
CREATE TABLE IF NOT EXISTS turn_ownership (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    owner_instance_id TEXT NOT NULL,
    owner_attachment_id TEXT,
    claimed_at TEXT NOT NULL,
    heartbeat_at TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    recovery_attempt INTEGER NOT NULL DEFAULT 0,
    last_checkpoint_json TEXT
);

CREATE INDEX IF NOT EXISTS turn_ownership_owner
ON turn_ownership(owner_instance_id);

CREATE INDEX IF NOT EXISTS turn_ownership_lease
ON turn_ownership(lease_expires_at);

CREATE TABLE IF NOT EXISTS action_receipts (
    tool_call_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    run_id TEXT,
    idempotency_key TEXT NOT NULL,
    operation TEXT NOT NULL,
    external_object_id TEXT,
    request_digest TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    settled_at TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS action_receipts_idempotency
ON action_receipts(idempotency_key);

CREATE INDEX IF NOT EXISTS action_receipts_session
ON action_receipts(session_id, started_at DESC);

CREATE INDEX IF NOT EXISTS action_receipts_run
ON action_receipts(run_id);
"#;

/// Observed generation TPS, measured per output-text segment.
///
/// Two things happen here, and they belong in one transaction.
///
/// First, the new ledger. A measurement keeps its own raw evidence —
/// `samples_json` holds the `(monotonic microseconds, cumulative exact tokens,
/// sequence number)` triples the rate was regressed from — so any displayed
/// value can be recomputed offline from the row that produced it. Storing only
/// the derived scalar would make the number unauditable, which is how the
/// previous estimate survived as long as it did. The `CHECK` makes the honest
/// state representable and the dishonest one impossible: a row carries either a
/// rate or a machine-readable reason it has none, never both and never neither.
///
/// Second, the legacy quarantine. Every `observed_stream` figure already in
/// `usage_records` came from the turn-wide, 2-second-gap estimate: turn-level
/// output tokens over a denominator that excluded tool time and any gap longer
/// than two seconds. Those numbers are not segment measurements and must not be
/// reinterpreted as if they were, so their `measurement_kind` is relabelled and
/// their throughput column is cleared — which is what removes them from the
/// dashboard's percentiles. Nothing is destroyed: the original value moves to
/// `legacy_observed_output_tps`, where it stays readable as what it always was,
/// an estimate.
const MIGRATION_21: &str = r#"
CREATE TABLE IF NOT EXISTS generation_speed_measurements (
    measurement_id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    measurement_kind TEXT NOT NULL,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    response_id TEXT,
    item_id TEXT NOT NULL,
    output_index INTEGER NOT NULL,
    content_index INTEGER NOT NULL,
    phase TEXT NOT NULL CHECK(phase IN ('commentary','final_answer','other')),
    status TEXT NOT NULL CHECK(status IN ('completed','partial','unavailable')),
    tps REAL,
    exact_tokens_after_first_sample INTEGER NOT NULL,
    duration_ms REAL NOT NULL,
    sample_count INTEGER NOT NULL,
    token_count_source TEXT NOT NULL
        CHECK(token_count_source IN (
            'provider_item_usage','provider_response_visible_usage','provider_response_output_usage','exact_tokenizer','unavailable'
        )),
    tokenizer_id TEXT,
    clock_source TEXT NOT NULL
        CHECK(clock_source IN ('provider_event_timestamp','workshop_monotonic_receive')),
    unavailable_reason TEXT,
    quality_flags TEXT NOT NULL DEFAULT '[]',
    samples_json TEXT NOT NULL DEFAULT '[]',
    provider TEXT,
    model_id TEXT,
    created_at TEXT NOT NULL,
    CHECK ((tps IS NULL) = (unavailable_reason IS NOT NULL))
);

CREATE INDEX IF NOT EXISTS generation_speed_measurements_turn
ON generation_speed_measurements(session_id, turn_id, created_at);

-- The ledger's `measurement_kind` vocabulary is a CHECK constraint, and SQLite
-- cannot alter one in place, so the table is rebuilt. The rebuild is also what
-- performs the relabelling, in the same transaction, so no build ever observes
-- a ledger where the two disagree.
CREATE TABLE usage_records_v21 (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_revision TEXT,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    request_id TEXT NOT NULL,
    measurement_kind TEXT NOT NULL CHECK(measurement_kind IN (
        'decode', 'observed_stream_segment', 'legacy_observed_stream_estimate',
        'end_to_end', 'provider_reported'
    )),
    status TEXT NOT NULL CHECK(status IN ('completed', 'failed', 'interrupted')),
    started_at_ms INTEGER NOT NULL,
    first_output_at_ms INTEGER,
    last_output_at_ms INTEGER,
    completed_at_ms INTEGER NOT NULL,
    input_tokens INTEGER,
    cached_input_tokens INTEGER,
    cache_write_tokens INTEGER,
    reasoning_tokens INTEGER,
    output_tokens INTEGER,
    total_tokens INTEGER GENERATED ALWAYS AS (
        CASE WHEN input_tokens IS NULL AND output_tokens IS NULL THEN NULL
             ELSE COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) END
    ) STORED,
    ttft_ms REAL,
    observed_output_tps REAL,
    legacy_observed_output_tps REAL,
    end_to_end_output_tps REAL,
    billed_cost_usd REAL,
    estimated_cost_usd REAL,
    cost_source TEXT NOT NULL DEFAULT 'none' CHECK(cost_source IN ('provider_reported', 'synth_cloud', 'tariff_estimate', 'none')),
    source TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(provider, request_id)
);

INSERT INTO usage_records_v21 (
    id, provider, model_id, model_revision, session_id, run_id, request_id,
    measurement_kind, status, started_at_ms, first_output_at_ms, last_output_at_ms,
    completed_at_ms, input_tokens, cached_input_tokens, cache_write_tokens,
    reasoning_tokens, output_tokens, ttft_ms, observed_output_tps,
    legacy_observed_output_tps, end_to_end_output_tps, billed_cost_usd,
    estimated_cost_usd, cost_source, source, created_at
)
SELECT
    id, provider, model_id, model_revision, session_id, run_id, request_id,
    CASE WHEN measurement_kind = 'observed_stream'
         THEN 'legacy_observed_stream_estimate' ELSE measurement_kind END,
    status, started_at_ms, first_output_at_ms, last_output_at_ms,
    completed_at_ms, input_tokens, cached_input_tokens, cache_write_tokens,
    reasoning_tokens, output_tokens, ttft_ms,
    CASE WHEN measurement_kind = 'observed_stream' THEN NULL ELSE observed_output_tps END,
    CASE WHEN measurement_kind = 'observed_stream' THEN observed_output_tps ELSE NULL END,
    end_to_end_output_tps, billed_cost_usd, estimated_cost_usd, cost_source,
    source, created_at
FROM usage_records;

DROP TABLE usage_records;

ALTER TABLE usage_records_v21 RENAME TO usage_records;

CREATE INDEX IF NOT EXISTS usage_records_model_created
ON usage_records(provider, model_id, measurement_kind, created_at DESC);

CREATE INDEX IF NOT EXISTS usage_records_completed
ON usage_records(status, completed_at_ms DESC);

CREATE INDEX IF NOT EXISTS usage_records_window
ON usage_records(completed_at_ms DESC);
"#;

/// Evaluation campaigns: the plan a set of rollouts belongs to.
///
/// Five chats were asked for one ten-rollout evaluation each and each produced a
/// single rollout, because "evaluation" was a word in a prompt rather than a
/// contract with a count. A campaign records how many terminal rollouts it owes
/// and which seeds are its own, before any of them run.
///
/// A seed is unique *within* a campaign here; overlap between campaigns that are
/// still open is rejected when the plan is created, since a seed reused by a
/// later, unrelated experiment is legitimate and a permanent uniqueness
/// constraint would forbid it.
const MIGRATION_22: &str = r#"
CREATE TABLE IF NOT EXISTS eval_campaigns (
    id TEXT PRIMARY KEY,
    session_id TEXT,
    container_id TEXT NOT NULL,
    title TEXT NOT NULL,
    expected_rollouts INTEGER NOT NULL CHECK (expected_rollouts > 0),
    max_concurrency INTEGER NOT NULL CHECK (max_concurrency > 0),
    policy_ref_json TEXT NOT NULL,
    plan_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('planned','running','complete','partial','failed')),
    created_at TEXT NOT NULL,
    started_at TEXT,
    settled_at TEXT
);

CREATE INDEX IF NOT EXISTS eval_campaigns_session ON eval_campaigns(session_id, created_at DESC);

CREATE TABLE IF NOT EXISTS eval_campaign_rollouts (
    campaign_id TEXT NOT NULL REFERENCES eval_campaigns(id) ON DELETE CASCADE,
    rollout_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    seed INTEGER NOT NULL,
    task_instance_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('planned','started','terminal','failed','missing')),
    terminal_json TEXT,
    started_at TEXT,
    settled_at TEXT,
    PRIMARY KEY (campaign_id, rollout_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS eval_campaign_rollouts_ordinal
ON eval_campaign_rollouts(campaign_id, ordinal);

CREATE UNIQUE INDEX IF NOT EXISTS eval_campaign_rollout_identity
ON eval_campaign_rollouts(rollout_id);

CREATE UNIQUE INDEX IF NOT EXISTS eval_campaign_seed_identity
ON eval_campaign_rollouts(campaign_id, seed);
"#;

/// The write-once terminal manifest. One row per run, sealed in the same
/// transaction as the terminal event, and never rewritten by a later poll —
/// which is why `terminal_cursor` lives in its own column rather than only in
/// the payload: a settled run's cursor must be queryable without parsing JSON.
const MIGRATION_23: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_terminal_manifests (
    optimizer_run_id TEXT PRIMARY KEY REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    algorithm_id TEXT NOT NULL,
    terminal_status TEXT NOT NULL,
    terminal_cursor INTEGER NOT NULL,
    sealed_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS optimizer_terminal_manifests_status
ON optimizer_terminal_manifests(terminal_status, sealed_at DESC);
"#;

/// Session-scoped experiment grouping for v0.5 campaign/eval DAGs.
/// One group per chat; members are evaluation campaigns and optimizer runs.
const MIGRATION_24: &str = r#"
CREATE TABLE IF NOT EXISTS experiment_groups (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS experiment_groups_session
ON experiment_groups(session_id);

CREATE TABLE IF NOT EXISTS experiment_group_members (
    group_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    member_kind TEXT NOT NULL CHECK (member_kind IN ('eval_campaign','optimizer_run')),
    member_id TEXT NOT NULL,
    title TEXT NOT NULL,
    attached_at TEXT NOT NULL,
    PRIMARY KEY (group_id, member_kind, member_id)
);

CREATE INDEX IF NOT EXISTS experiment_group_members_kind
ON experiment_group_members(member_kind, member_id);
"#;

/// Local secrets vault metadata. Values live in the OS credential store;
/// these tables hold only aliases, opaque refs, fingerprints, and audit.
const MIGRATION_25: &str = r#"
CREATE TABLE IF NOT EXISTS secret_refs (
    id TEXT PRIMARY KEY,
    alias TEXT NOT NULL,
    provider TEXT NOT NULL,
    scope TEXT NOT NULL,
    backend TEXT NOT NULL,
    backend_ref TEXT NOT NULL UNIQUE,
    fingerprint TEXT NOT NULL,
    display_suffix TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('valid','invalid','untested','locked')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_validated_at TEXT
);

CREATE INDEX IF NOT EXISTS secret_refs_provider ON secret_refs(provider, alias);

CREATE TABLE IF NOT EXISTS secret_recipe_grants (
    secret_id TEXT NOT NULL REFERENCES secret_refs(id) ON DELETE CASCADE,
    recipe_id TEXT NOT NULL,
    granted_at TEXT NOT NULL,
    PRIMARY KEY (secret_id, recipe_id)
);

CREATE TABLE IF NOT EXISTS secret_audit (
    event_id TEXT PRIMARY KEY,
    at TEXT NOT NULL,
    actor_kind TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    action TEXT NOT NULL,
    secret_id TEXT,
    provider TEXT,
    operation TEXT,
    model TEXT,
    decision TEXT NOT NULL,
    capability_id TEXT,
    usage_json TEXT,
    detail TEXT
);

CREATE INDEX IF NOT EXISTS secret_audit_at ON secret_audit(at DESC);
CREATE INDEX IF NOT EXISTS secret_audit_secret ON secret_audit(secret_id, at DESC);

CREATE TABLE IF NOT EXISTS secret_capabilities (
    id TEXT PRIMARY KEY,
    handle TEXT NOT NULL UNIQUE,
    secret_id TEXT NOT NULL REFERENCES secret_refs(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    recipe_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    operations_json TEXT NOT NULL,
    models_json TEXT NOT NULL,
    reasoning_efforts_json TEXT NOT NULL,
    max_calls INTEGER NOT NULL,
    max_input_tokens INTEGER NOT NULL,
    max_output_tokens INTEGER NOT NULL,
    max_cost_usd_micros INTEGER NOT NULL,
    used_calls INTEGER NOT NULL DEFAULT 0,
    used_input_tokens INTEGER NOT NULL DEFAULT 0,
    used_output_tokens INTEGER NOT NULL DEFAULT 0,
    used_cost_usd_micros INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK (status IN ('granted','active','exhausted','expired','revoked')),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    revoked_at TEXT
);

CREATE INDEX IF NOT EXISTS secret_capabilities_run ON secret_capabilities(run_id, status);
"#;

const MIGRATION_26: &str = r#"
CREATE TABLE IF NOT EXISTS product_telemetry_events (
    event_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    at TEXT NOT NULL,
    sensitivity TEXT NOT NULL CHECK (sensitivity IN ('optional','essential')),
    properties_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS product_telemetry_events_at ON product_telemetry_events(at DESC);
CREATE INDEX IF NOT EXISTS product_telemetry_events_name ON product_telemetry_events(name, at DESC);
"#;

/// Reports v0.6 makes evidence mode and claim rationale explicit while keeping
/// every v1 row readable. Legacy claims receive deliberately conservative
/// metadata instead of silently acquiring high confidence.
const MIGRATION_27: &str = r#"
ALTER TABLE report_revision_blocks ADD COLUMN reference_mode TEXT NOT NULL DEFAULT 'live'
    CHECK(reference_mode IN ('live','pinned'));
ALTER TABLE report_sources ADD COLUMN reference_mode TEXT NOT NULL DEFAULT 'live'
    CHECK(reference_mode IN ('live','pinned'));
ALTER TABLE report_claims ADD COLUMN confidence TEXT NOT NULL DEFAULT 'low'
    CHECK(confidence IN ('low','medium','high','overwhelming'));
ALTER TABLE report_claims ADD COLUMN why TEXT NOT NULL DEFAULT
    'Migrated legacy claim; rationale was not recorded.';

UPDATE report_revision_blocks
SET reference_mode = 'pinned'
WHERE source_revision IS NOT NULL AND source_digest IS NOT NULL
  AND source_revision NOT IN ('working','live');
UPDATE report_sources
SET reference_mode = 'pinned'
WHERE resource_revision IS NOT NULL AND resource_digest IS NOT NULL;
UPDATE report_revision_blocks SET access_state = 'available' WHERE access_state = 'accessible';
UPDATE report_revision_blocks SET integrity_state = 'unresolved' WHERE integrity_state = 'unknown';
UPDATE report_sources SET access_state = 'available' WHERE access_state = 'accessible';
UPDATE report_sources SET integrity_state = 'unresolved' WHERE integrity_state = 'unknown';
"#;

/// One status vocabulary for `optimizer_runs`, enforced in the database.
///
/// The column carried fifteen spellings for nine states, and four predicates in
/// Rust each drew the terminal line somewhere different. `OptimizerRunStatus`
/// is now the one authority; this migration folds the legacy spellings into it
/// and installs a trigger pair so a new one cannot be written.
///
/// A trigger, not a `CHECK`: adding a column constraint to an existing SQLite
/// table means rebuilding it, and `optimizer_runs` has four children with
/// `ON DELETE CASCADE` while `foreign_keys` is ON and migrations run inside a
/// transaction (so `PRAGMA foreign_keys=OFF` is a no-op). The safe rebuild is
/// unavailable here; `RAISE(ABORT)` gives the same refusal without risking the
/// event, cursor, slice, and manifest rows.
///
/// Anything still outside the vocabulary after the alias pass is a word no
/// build in this lineage emits; it is recorded as `failed` rather than left to
/// abort the next write.
const MIGRATION_28: &str = r#"
UPDATE optimizer_runs SET status = 'completed' WHERE status = 'succeeded';
UPDATE optimizer_runs SET status = 'cancelled' WHERE status = 'canceled';
UPDATE optimizer_runs SET status = 'failed'
    WHERE status IN ('done','stopped','aborted','error');
UPDATE optimizer_runs SET status = 'queued' WHERE status = 'created';
UPDATE optimizer_runs SET status = 'failed'
    WHERE status NOT IN ('queued','validating','provisioning','starting','waiting_for_viewer','running','paused','cancelling','env_unreachable','degraded','completed','failed','failed_evidence','cancelled','interrupted','infrastructure_lost','cap_reached');

CREATE TRIGGER IF NOT EXISTS optimizer_runs_status_domain_insert
BEFORE INSERT ON optimizer_runs
FOR EACH ROW WHEN NEW.status NOT IN ('queued','validating','provisioning','starting','waiting_for_viewer','running','paused','cancelling','env_unreachable','degraded','completed','failed','failed_evidence','cancelled','interrupted','infrastructure_lost','cap_reached')
BEGIN
    SELECT RAISE(ABORT, 'optimizer_runs.status outside OptimizerRunStatus');
END;

CREATE TRIGGER IF NOT EXISTS optimizer_runs_status_domain_update
BEFORE UPDATE OF status ON optimizer_runs
FOR EACH ROW WHEN NEW.status NOT IN ('queued','validating','provisioning','starting','waiting_for_viewer','running','paused','cancelling','env_unreachable','degraded','completed','failed','failed_evidence','cancelled','interrupted','infrastructure_lost','cap_reached')
BEGIN
    SELECT RAISE(ABORT, 'optimizer_runs.status outside OptimizerRunStatus');
END;
"#;

const MIGRATION_29: &str = r#"
CREATE TABLE IF NOT EXISTS local_lora_checkpoints (
    checkpoint_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    base_model TEXT NOT NULL,
    optimizer_algorithm TEXT,
    checkpoint_kind TEXT NOT NULL,
    step INTEGER,
    lora_rank INTEGER,
    status TEXT NOT NULL,
    adapter_path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size_bytes INTEGER,
    run_id TEXT,
    source_checkpoint_id TEXT,
    tags_json TEXT NOT NULL DEFAULT '[]',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT
);
"#;

const MIGRATION_30: &str = r#"
CREATE TABLE IF NOT EXISTS hosted_lora_overlays (
    checkpoint_id TEXT PRIMARY KEY,
    name TEXT,
    description TEXT,
    tags_json TEXT,
    updated_at TEXT NOT NULL
);
"#;

const MIGRATION_31: &str = "SELECT 1;";
const MIGRATION_32: &str = "SELECT 1;";

/// Optimizer run ownership: one live claim per campaign, held by one boot.
/// Shaped like `turn_ownership`, keyed by run id. A `running` row is only live
/// while a claim here names the current instance and its lease has not expired.
const MIGRATION_33: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_run_ownership (
    run_id TEXT PRIMARY KEY,
    owner_instance_id TEXT NOT NULL,
    boot_epoch TEXT NOT NULL,
    pid INTEGER,
    process_start_identity TEXT,
    heartbeat_at TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS optimizer_run_ownership_owner
ON optimizer_run_ownership(owner_instance_id);

CREATE INDEX IF NOT EXISTS optimizer_run_ownership_lease
ON optimizer_run_ownership(lease_expires_at);
"#;

/// v0.8 local-first experiment projection. The v0.5 group remains the stable
/// experiment identity; nodes and edges are explicit facts, never inferred by
/// the renderer from titles or timestamps.
const MIGRATION_34: &str = r#"
ALTER TABLE experiment_groups ADD COLUMN updated_at TEXT;
ALTER TABLE experiment_groups ADD COLUMN status TEXT NOT NULL DEFAULT 'draft';
ALTER TABLE experiment_groups ADD COLUMN task TEXT;
ALTER TABLE experiment_groups ADD COLUMN model TEXT;
ALTER TABLE experiment_groups ADD COLUMN best_result_json TEXT;

UPDATE experiment_groups SET updated_at = created_at WHERE updated_at IS NULL;

CREATE TABLE IF NOT EXISTS experiment_nodes (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('baseline','variant','run','result')),
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    metrics_json TEXT,
    cost_usd REAL,
    artifact_refs_json TEXT NOT NULL DEFAULT '[]',
    trace_refs_json TEXT NOT NULL DEFAULT '[]',
    provenance_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(experiment_id, id)
);

CREATE TABLE IF NOT EXISTS experiment_edges (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    source_node_id TEXT NOT NULL REFERENCES experiment_nodes(id) ON DELETE CASCADE,
    target_node_id TEXT NOT NULL REFERENCES experiment_nodes(id) ON DELETE CASCADE,
    relation TEXT NOT NULL CHECK (relation IN ('forked_from','rerun_of','warm_started_from','produced','evaluated','compared_with','promoted_to','reproduced_on','rolled_back_to')),
    created_at TEXT NOT NULL,
    UNIQUE(experiment_id, source_node_id, target_node_id, relation)
);

CREATE INDEX IF NOT EXISTS experiment_groups_updated ON experiment_groups(updated_at DESC);
CREATE INDEX IF NOT EXISTS experiment_nodes_experiment ON experiment_nodes(experiment_id, created_at, id);
CREATE INDEX IF NOT EXISTS experiment_edges_experiment ON experiment_edges(experiment_id, created_at, id);
"#;

/// Typed, idempotent evidence references on experiment nodes. Bodies remain in
/// their authoritative CAS/container/visual registry and may be materialized
/// just in time; the experiment stores identity, expected digest, and locator.
const MIGRATION_35: &str = r#"
ALTER TABLE experiment_nodes ADD COLUMN evidence_refs_json TEXT NOT NULL DEFAULT '[]';
"#;

const MIGRATION_36: &str = r#"
ALTER TABLE experiment_group_members RENAME TO experiment_group_members_v35;
CREATE TABLE experiment_group_members (
    group_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    member_kind TEXT NOT NULL CHECK (member_kind IN ('eval_campaign','optimizer_run','direct_evaluation')),
    member_id TEXT NOT NULL,
    title TEXT NOT NULL,
    attached_at TEXT NOT NULL,
    PRIMARY KEY (group_id, member_kind, member_id)
);
INSERT INTO experiment_group_members SELECT * FROM experiment_group_members_v35;
DROP TABLE experiment_group_members_v35;
CREATE INDEX experiment_group_members_kind ON experiment_group_members(member_kind, member_id);
"#;

/// Member nodes use the member kind (`eval_campaign`, `optimizer_run`,
/// `direct_evaluation`). Historical `baseline`/`variant`/`result`/`run` rows
/// stay readable. `follow_up` is a stored experiment-to-experiment relation,
/// not a canvas layout fact.
const MIGRATION_37: &str = r#"
CREATE TABLE experiment_nodes_next (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN (
        'baseline','variant','run','result',
        'eval_campaign','optimizer_run','direct_evaluation'
    )),
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    metrics_json TEXT,
    cost_usd REAL,
    artifact_refs_json TEXT NOT NULL DEFAULT '[]',
    trace_refs_json TEXT NOT NULL DEFAULT '[]',
    provenance_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    evidence_refs_json TEXT NOT NULL DEFAULT '[]',
    UNIQUE(experiment_id, id)
);
INSERT INTO experiment_nodes_next(
    id, experiment_id, kind, title, status, config_json, metrics_json, cost_usd,
    artifact_refs_json, trace_refs_json, provenance_json, created_at, updated_at, evidence_refs_json
)
SELECT
    id, experiment_id, kind, title, status, config_json, metrics_json, cost_usd,
    artifact_refs_json, trace_refs_json, provenance_json, created_at, updated_at, evidence_refs_json
FROM experiment_nodes;

CREATE TABLE experiment_edges_next (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL,
    source_node_id TEXT NOT NULL,
    target_node_id TEXT NOT NULL,
    relation TEXT NOT NULL,
    created_at TEXT NOT NULL
);
INSERT INTO experiment_edges_next SELECT * FROM experiment_edges;

DROP TABLE experiment_edges;
DROP TABLE experiment_nodes;
ALTER TABLE experiment_nodes_next RENAME TO experiment_nodes;

CREATE TABLE experiment_edges (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    source_node_id TEXT NOT NULL REFERENCES experiment_nodes(id) ON DELETE CASCADE,
    target_node_id TEXT NOT NULL REFERENCES experiment_nodes(id) ON DELETE CASCADE,
    relation TEXT NOT NULL CHECK (relation IN (
        'forked_from','rerun_of','warm_started_from','produced','evaluated',
        'compared_with','promoted_to','reproduced_on','rolled_back_to','follow_up'
    )),
    created_at TEXT NOT NULL,
    UNIQUE(experiment_id, source_node_id, target_node_id, relation)
);
INSERT INTO experiment_edges SELECT * FROM experiment_edges_next;
DROP TABLE experiment_edges_next;

CREATE INDEX IF NOT EXISTS experiment_nodes_experiment ON experiment_nodes(experiment_id, created_at, id);
CREATE INDEX IF NOT EXISTS experiment_edges_experiment ON experiment_edges(experiment_id, created_at, id);
"#;

/// A session may own many experiments. `follow_up` is stored as an
/// experiment-to-experiment fact, not a member-node edge. Attach targets the
/// session's active experiment (cursor, then oldest).
const MIGRATION_38: &str = r#"
DROP INDEX IF EXISTS experiment_groups_session;
CREATE INDEX IF NOT EXISTS experiment_groups_session
ON experiment_groups(session_id, created_at, id);

CREATE TABLE IF NOT EXISTS experiment_lineage (
    id TEXT PRIMARY KEY,
    source_experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    target_experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    relation TEXT NOT NULL CHECK (relation IN ('follow_up','forked_from','rerun_of')),
    created_at TEXT NOT NULL,
    UNIQUE(source_experiment_id, target_experiment_id, relation)
);

CREATE INDEX IF NOT EXISTS experiment_lineage_source
ON experiment_lineage(source_experiment_id, created_at, id);
CREATE INDEX IF NOT EXISTS experiment_lineage_target
ON experiment_lineage(target_experiment_id);

CREATE TABLE IF NOT EXISTS experiment_session_cursor (
    session_id TEXT PRIMARY KEY,
    active_experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE
);

ALTER TABLE sessions ADD COLUMN active_experiment_id TEXT REFERENCES experiment_groups(id);
"#;

/// Durable Candidate rows hanging off an `optimizer_run` member. Not a fourth
/// member kind and not an `experiment_edges` relation. Producer identity is
/// unique per run; parentage stays JSON from the producer.
const MIGRATION_39: &str = r#"
CREATE TABLE IF NOT EXISTS experiment_candidates (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    optimizer_run_id TEXT NOT NULL,
    producer_candidate_id TEXT NOT NULL,
    kind TEXT,
    protocol_id TEXT,
    status TEXT,
    parent_ids_json TEXT NOT NULL DEFAULT '[]',
    metrics_json TEXT,
    content_digest TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(optimizer_run_id, producer_candidate_id)
);

CREATE INDEX IF NOT EXISTS experiment_candidates_experiment
ON experiment_candidates(experiment_id, optimizer_run_id, created_at, id);
CREATE INDEX IF NOT EXISTS experiment_candidates_run
ON experiment_candidates(optimizer_run_id, created_at, id);
"#;

/// Candidate compare/promote stay on the candidate row, not `experiment_edges`.
/// `experiment_records.experiment_group_id` is a pointer at ExperimentGroup.
const MIGRATION_40: &str = r#"
-- Some prerelease databases recorded version 39 from a parallel migration
-- lineage that did not create this table. Re-declare the prerequisite here:
-- a version stamp is not proof that the expected schema exists.
CREATE TABLE IF NOT EXISTS experiment_candidates (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    optimizer_run_id TEXT NOT NULL,
    producer_candidate_id TEXT NOT NULL,
    kind TEXT,
    protocol_id TEXT,
    status TEXT,
    parent_ids_json TEXT NOT NULL DEFAULT '[]',
    metrics_json TEXT,
    content_digest TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(optimizer_run_id, producer_candidate_id)
);

ALTER TABLE experiment_candidates ADD COLUMN compared_with_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE experiment_candidates ADD COLUMN promoted_to TEXT;

ALTER TABLE experiment_records ADD COLUMN experiment_group_id TEXT REFERENCES experiment_groups(id);
"#;

/// The media a run is allowed to hand to its visual.
///
/// A visual asks the host for a frame by digest. Deciding whether it may have
/// it by scanning the run's whole event log is both slow on a 500-step episode
/// and fragile — one relay shape change and the check silently stops matching,
/// which is the worst possible failure for an authorization gate. This is the
/// authoritative index instead: the relay writes a row when it stores the
/// bytes, and the bridge answers from exactly these rows.
///
/// `cas_digest` is Workshop's own SHA-256 of the stored object. `producer_digest`
/// is whatever the container called it — sixteen hex characters in the field —
/// and is provenance only. They are separate columns because conflating them
/// is how a truncated label becomes a content address.
const MIGRATION_41: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_run_media (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    cas_digest TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'eval_frames',
    media_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    width INTEGER,
    height INTEGER,
    rollout_id TEXT,
    trial_id TEXT,
    step INTEGER,
    producer_digest TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, cas_digest)
);

CREATE INDEX IF NOT EXISTS optimizer_run_media_rollout
ON optimizer_run_media(optimizer_run_id, rollout_id, step);
"#;

/// Native optimizer frames are durable media, not event-state payload. The
/// event log retains a small immutable reference while the PNG bytes live once
/// in the content-addressed store. A run/seed sequence index supports both the
/// live "latest per seed" cursor and bounded, lazy drill-down history.
const MIGRATION_42: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_frames (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    seed INTEGER NOT NULL,
    frame_sequence INTEGER NOT NULL,
    event_id TEXT NOT NULL,
    content_digest TEXT NOT NULL,
    content_type TEXT NOT NULL CHECK (content_type = 'image/png'),
    size_bytes INTEGER NOT NULL CHECK (size_bytes > 0),
    occurred_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, seed, frame_sequence),
    UNIQUE (optimizer_run_id, event_id)
);

CREATE INDEX IF NOT EXISTS optimizer_frames_run_sequence
ON optimizer_frames(optimizer_run_id, frame_sequence);

CREATE INDEX IF NOT EXISTS optimizer_frames_run_seed_sequence
ON optimizer_frames(optimizer_run_id, seed, frame_sequence DESC);

CREATE TABLE IF NOT EXISTS optimizer_frame_usage (
    optimizer_run_id TEXT PRIMARY KEY REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    retained_frames INTEGER NOT NULL DEFAULT 0,
    retained_bytes INTEGER NOT NULL DEFAULT 0,
    rejected_frames INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);
"#;

/// Inline-first evaluation admission.
///
/// A run keeps the whole canonical execution specification, not a projection of
/// it, so reopening a finished run reconstructs exactly what executed and can
/// serve as the basis of a new one. `execution_spec_digest` is stored beside the
/// JSON rather than recomputed on read, which is what makes a specification
/// rewritten in place detectable instead of becoming the new truth.
///
/// Every per-rollout evidence column is nullable on purpose. A reward that was
/// never observed must read back as missing, not as `0.0`; rendering absent
/// telemetry as a number is the failure this whole lane is built against.
const MIGRATION_43: &str = r#"
CREATE TABLE IF NOT EXISTS evaluation_runs (
    optimizer_run_id TEXT PRIMARY KEY REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    recipe_source_kind TEXT NOT NULL CHECK (recipe_source_kind IN ('inline','catalog')),
    catalog_recipe_id TEXT,
    execution_spec_json TEXT NOT NULL,
    execution_spec_digest TEXT NOT NULL,
    container_declaration_digest TEXT NOT NULL,
    policy_revision TEXT NOT NULL,
    policy_configuration_digest TEXT NOT NULL,
    approval_receipt_id TEXT NOT NULL,
    run_state TEXT,
    credential_revocation_confirmed INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    -- A catalog run names its recipe; an inline run must not carry one, so a
    -- run cannot be relabelled as catalog-derived after the fact.
    CHECK ((recipe_source_kind = 'catalog') = (catalog_recipe_id IS NOT NULL))
);

CREATE INDEX IF NOT EXISTS evaluation_runs_spec_digest
ON evaluation_runs(execution_spec_digest);

CREATE INDEX IF NOT EXISTS evaluation_runs_receipt
ON evaluation_runs(approval_receipt_id);

CREATE TABLE IF NOT EXISTS evaluation_rollouts (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    rollout_index INTEGER NOT NULL,
    rollout_state TEXT CHECK (rollout_state IN (
        'planned','queued','starting','running','completed','failed','cancelled','degraded')),
    rollout_id TEXT,
    reward REAL,
    trace_ref TEXT,
    cost_micros INTEGER,
    total_tokens INTEGER,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, rollout_index)
);

CREATE TABLE IF NOT EXISTS evaluation_run_drafts (
    optimizer_run_id TEXT PRIMARY KEY,
    recipe_source_kind TEXT NOT NULL CHECK (recipe_source_kind IN ('inline','catalog')),
    catalog_recipe_id TEXT,
    execution_spec_json TEXT NOT NULL,
    execution_spec_digest TEXT NOT NULL,
    created_at TEXT NOT NULL
);
"#;

/// Repair the experiment cursor authority on lineages where another
/// prerelease consumed migration 38 without installing its experiment DDL.
/// The companion `sessions.active_experiment_id` column is shape-healed in
/// Rust because SQLite cannot express `ADD COLUMN IF NOT EXISTS`.
const MIGRATION_44: &str = r#"
CREATE INDEX IF NOT EXISTS experiment_groups_session
ON experiment_groups(session_id, created_at, id);

CREATE TABLE IF NOT EXISTS experiment_lineage (
    id TEXT PRIMARY KEY,
    source_experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    target_experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    relation TEXT NOT NULL CHECK (relation IN ('follow_up','forked_from','rerun_of')),
    created_at TEXT NOT NULL,
    UNIQUE(source_experiment_id, target_experiment_id, relation)
);

CREATE INDEX IF NOT EXISTS experiment_lineage_source
ON experiment_lineage(source_experiment_id, created_at, id);
CREATE INDEX IF NOT EXISTS experiment_lineage_target
ON experiment_lineage(target_experiment_id);

CREATE TABLE IF NOT EXISTS experiment_session_cursor (
    session_id TEXT PRIMARY KEY,
    active_experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE
);
"#;

/// Version marker for the shape-aware experiment graph repair performed by
/// `heal_experiment_graph_shape`. The repair itself cannot be pure SQL because
/// it must first distinguish the old and current CHECK constraints.
const MIGRATION_45: &str = "SELECT 1;";

/// Response usage arrives after the final-answer lifecycle event. Generation
/// speed v1 can bind exact response output minus exact reasoning output to that
/// answer's observed delivery interval, but SQLite cannot alter the v21 token
/// source CHECK in place. Rebuild the evidence table without changing any row
/// identity or measurement data.
const MIGRATION_46: &str = r#"
CREATE TABLE generation_speed_measurements_v46 (
    measurement_id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    measurement_kind TEXT NOT NULL,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    response_id TEXT,
    item_id TEXT NOT NULL,
    output_index INTEGER NOT NULL,
    content_index INTEGER NOT NULL,
    phase TEXT NOT NULL CHECK(phase IN ('commentary','final_answer','other')),
    status TEXT NOT NULL CHECK(status IN ('completed','partial','unavailable')),
    tps REAL,
    exact_tokens_after_first_sample INTEGER NOT NULL,
    duration_ms REAL NOT NULL,
    sample_count INTEGER NOT NULL,
    token_count_source TEXT NOT NULL CHECK(token_count_source IN (
        'provider_item_usage','provider_response_visible_usage','provider_response_output_usage','exact_tokenizer','unavailable'
    )),
    tokenizer_id TEXT,
    clock_source TEXT NOT NULL
        CHECK(clock_source IN ('provider_event_timestamp','workshop_monotonic_receive')),
    unavailable_reason TEXT,
    quality_flags TEXT NOT NULL DEFAULT '[]',
    samples_json TEXT NOT NULL DEFAULT '[]',
    provider TEXT,
    model_id TEXT,
    created_at TEXT NOT NULL,
    CHECK ((tps IS NULL) = (unavailable_reason IS NOT NULL))
);

INSERT INTO generation_speed_measurements_v46 (
    measurement_id,schema_version,measurement_kind,session_id,turn_id,response_id,item_id,
    output_index,content_index,phase,status,tps,exact_tokens_after_first_sample,duration_ms,
    sample_count,token_count_source,tokenizer_id,clock_source,unavailable_reason,
    quality_flags,samples_json,provider,model_id,created_at
)
SELECT
    measurement_id,schema_version,measurement_kind,session_id,turn_id,response_id,item_id,
    output_index,content_index,phase,status,tps,exact_tokens_after_first_sample,duration_ms,
    sample_count,token_count_source,tokenizer_id,clock_source,unavailable_reason,
    quality_flags,samples_json,provider,model_id,created_at
FROM generation_speed_measurements;

DROP TABLE generation_speed_measurements;
ALTER TABLE generation_speed_measurements_v46 RENAME TO generation_speed_measurements;
CREATE INDEX generation_speed_measurements_turn
ON generation_speed_measurements(session_id, turn_id, created_at);
"#;

/// Full response output includes reasoning and must be paired with the full
/// model-output interval. Keep the earlier visible-only source readable for
/// audit history while admitting the corrected source for new rows.
const MIGRATION_47: &str = r#"
CREATE TABLE generation_speed_measurements_v47 (
    measurement_id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    measurement_kind TEXT NOT NULL,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    response_id TEXT,
    item_id TEXT NOT NULL,
    output_index INTEGER NOT NULL,
    content_index INTEGER NOT NULL,
    phase TEXT NOT NULL CHECK(phase IN ('commentary','final_answer','other')),
    status TEXT NOT NULL CHECK(status IN ('completed','partial','unavailable')),
    tps REAL,
    exact_tokens_after_first_sample INTEGER NOT NULL,
    duration_ms REAL NOT NULL,
    sample_count INTEGER NOT NULL,
    token_count_source TEXT NOT NULL CHECK(token_count_source IN (
        'provider_item_usage','provider_response_visible_usage',
        'provider_response_output_usage','exact_tokenizer','unavailable'
    )),
    tokenizer_id TEXT,
    clock_source TEXT NOT NULL
        CHECK(clock_source IN ('provider_event_timestamp','workshop_monotonic_receive')),
    unavailable_reason TEXT,
    quality_flags TEXT NOT NULL DEFAULT '[]',
    samples_json TEXT NOT NULL DEFAULT '[]',
    provider TEXT,
    model_id TEXT,
    created_at TEXT NOT NULL,
    CHECK ((tps IS NULL) = (unavailable_reason IS NOT NULL))
);

INSERT INTO generation_speed_measurements_v47 (
    measurement_id,schema_version,measurement_kind,session_id,turn_id,response_id,item_id,
    output_index,content_index,phase,status,tps,exact_tokens_after_first_sample,duration_ms,
    sample_count,token_count_source,tokenizer_id,clock_source,unavailable_reason,
    quality_flags,samples_json,provider,model_id,created_at
)
SELECT
    measurement_id,schema_version,measurement_kind,session_id,turn_id,response_id,item_id,
    output_index,content_index,phase,status,tps,exact_tokens_after_first_sample,duration_ms,
    sample_count,token_count_source,tokenizer_id,clock_source,unavailable_reason,
    quality_flags,samples_json,provider,model_id,created_at
FROM generation_speed_measurements;

DROP TABLE generation_speed_measurements;
ALTER TABLE generation_speed_measurements_v47 RENAME TO generation_speed_measurements;
CREATE INDEX generation_speed_measurements_turn
ON generation_speed_measurements(session_id, turn_id, created_at);
"#;

/// Repair prerelease databases that recorded the experiment-lineage migration
/// version without replacing the original one-experiment-per-session index.
/// `CREATE INDEX IF NOT EXISTS` cannot change an existing UNIQUE index, so the
/// migration must explicitly remove the legacy index before recreating it.
const MIGRATION_48: &str = r#"
DROP INDEX IF EXISTS experiment_groups_session;
CREATE INDEX experiment_groups_session
ON experiment_groups(session_id, created_at, id);
"#;

/// Failure ledger, structured logs, recovery plans, and canonical failure FKs.
/// Historical error prose is copied into `historical_failure_unclassified`
/// rows by `migrate_historical_failures` after this DDL lands.
const MIGRATION_49: &str = r#"
CREATE TABLE IF NOT EXISTS operation_records (
    operation_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    phase TEXT NOT NULL,
    parent_operation_id TEXT,
    session_id TEXT,
    turn_id TEXT,
    tool_call_id TEXT,
    container_id TEXT,
    evaluation_id TEXT,
    rollout_id TEXT,
    visual_id TEXT,
    approval_id TEXT,
    context_json TEXT NOT NULL DEFAULT '{}',
    started_at TEXT NOT NULL,
    completed_at TEXT
);
CREATE INDEX IF NOT EXISTS operation_records_session ON operation_records(session_id, started_at);
CREATE INDEX IF NOT EXISTS operation_records_container ON operation_records(container_id, started_at);

CREATE TABLE IF NOT EXISTS failure_occurrences (
    failure_id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    code TEXT NOT NULL,
    domain TEXT NOT NULL,
    category TEXT NOT NULL,
    disposition TEXT NOT NULL CHECK(disposition IN (
        'approval_required','repair_required','retryable','terminal','cancelled','programmer_error'
    )),
    lifecycle_state TEXT NOT NULL CHECK(lifecycle_state IN (
        'open','awaiting_approval','repairing','retry_scheduled','retrying','resolved','terminalized','superseded'
    )),
    operation_kind TEXT NOT NULL,
    operation_phase TEXT NOT NULL,
    operation_id TEXT,
    session_id TEXT,
    turn_id TEXT,
    container_id TEXT,
    evaluation_id TEXT,
    rollout_id TEXT,
    visual_id TEXT,
    kind_json TEXT NOT NULL,
    facts_json TEXT NOT NULL DEFAULT '{}',
    cause_json TEXT NOT NULL DEFAULT 'null',
    raised_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS failure_occurrences_lifecycle ON failure_occurrences(lifecycle_state, raised_at);
CREATE INDEX IF NOT EXISTS failure_occurrences_code ON failure_occurrences(code, raised_at);
CREATE INDEX IF NOT EXISTS failure_occurrences_container ON failure_occurrences(container_id, raised_at);
CREATE INDEX IF NOT EXISTS failure_occurrences_session ON failure_occurrences(session_id, raised_at);
CREATE INDEX IF NOT EXISTS failure_occurrences_evaluation ON failure_occurrences(evaluation_id, raised_at);

CREATE TABLE IF NOT EXISTS failure_transitions (
    failure_id TEXT NOT NULL REFERENCES failure_occurrences(failure_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    from_state TEXT,
    to_state TEXT NOT NULL,
    reason TEXT NOT NULL,
    actor TEXT NOT NULL,
    at TEXT NOT NULL,
    PRIMARY KEY (failure_id, sequence)
);

CREATE TABLE IF NOT EXISTS failure_relationships (
    from_failure_id TEXT NOT NULL REFERENCES failure_occurrences(failure_id) ON DELETE CASCADE,
    to_failure_id TEXT NOT NULL REFERENCES failure_occurrences(failure_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN (
        'caused_by','consequence_of','supersedes','repair_of','retry_of'
    )),
    PRIMARY KEY (from_failure_id, to_failure_id, kind)
);

CREATE TABLE IF NOT EXISTS log_records (
    log_id TEXT PRIMARY KEY,
    level TEXT NOT NULL CHECK(level IN ('debug','info','warn','error')),
    component TEXT NOT NULL,
    event TEXT NOT NULL,
    message TEXT NOT NULL,
    operation_id TEXT,
    failure_id TEXT,
    fields_json TEXT NOT NULL DEFAULT '{}',
    at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS log_records_operation ON log_records(operation_id, at);
CREATE INDEX IF NOT EXISTS log_records_failure ON log_records(failure_id, at);
CREATE INDEX IF NOT EXISTS log_records_component ON log_records(component, at);

CREATE TABLE IF NOT EXISTS recovery_plans (
    recovery_id TEXT PRIMARY KEY,
    failure_id TEXT NOT NULL REFERENCES failure_occurrences(failure_id) ON DELETE CASCADE,
    action_json TEXT NOT NULL,
    approval_requirement_json TEXT NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    bounds_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS recovery_receipts (
    recovery_id TEXT NOT NULL,
    failure_id TEXT NOT NULL,
    status TEXT NOT NULL,
    approval_id TEXT,
    detail_json TEXT NOT NULL DEFAULT '{}',
    completed_at TEXT NOT NULL,
    PRIMARY KEY (recovery_id, completed_at)
);

ALTER TABLE containers ADD COLUMN current_failure_id TEXT REFERENCES failure_occurrences(failure_id);
ALTER TABLE optimizer_runs ADD COLUMN terminal_failure_id TEXT REFERENCES failure_occurrences(failure_id);
ALTER TABLE evaluation_runs ADD COLUMN terminal_failure_id TEXT REFERENCES failure_occurrences(failure_id);
ALTER TABLE visuals ADD COLUMN current_failure_id TEXT REFERENCES failure_occurrences(failure_id);
ALTER TABLE runs ADD COLUMN terminal_failure_id TEXT REFERENCES failure_occurrences(failure_id);
ALTER TABLE experiment_groups ADD COLUMN current_failure_id TEXT REFERENCES failure_occurrences(failure_id);

CREATE TABLE evaluation_rollouts_v49 (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    rollout_index INTEGER NOT NULL,
    rollout_state TEXT CHECK (rollout_state IN (
        'not_started','planned','queued','starting','running','completed','failed','cancelled','degraded')),
    rollout_id TEXT,
    reward REAL,
    trace_ref TEXT,
    cost_micros INTEGER,
    total_tokens INTEGER,
    updated_at TEXT NOT NULL,
    terminal_failure_id TEXT REFERENCES failure_occurrences(failure_id),
    PRIMARY KEY (optimizer_run_id, rollout_index)
);
INSERT INTO evaluation_rollouts_v49(
    optimizer_run_id, rollout_index, rollout_state, rollout_id, reward, trace_ref,
    cost_micros, total_tokens, updated_at, terminal_failure_id
)
SELECT optimizer_run_id, rollout_index, rollout_state, rollout_id, reward, trace_ref,
       cost_micros, total_tokens, updated_at, NULL
FROM evaluation_rollouts;
DROP TABLE evaluation_rollouts;
ALTER TABLE evaluation_rollouts_v49 RENAME TO evaluation_rollouts;
"#;

/// Optimizers run kernel: admission drafts, sealed specs, producer/aggregate
/// sequences, algorithm projections, work items, usage, and evidence amendments.
/// Legacy evaluation_runs / eval_campaigns remain until the cutover migration
/// copies them into optimizer_run(kind=eval) and drops the old tables.
const MIGRATION_50: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_run_drafts (
    id TEXT PRIMARY KEY,
    algorithm TEXT NOT NULL,
    spec_json TEXT NOT NULL,
    spec_digest TEXT NOT NULL,
    admission_state TEXT NOT NULL CHECK (admission_state IN (
        'draft','validating','awaiting_approval','approved','not_required','rejected','expired','consumed'
    )),
    authorization_ref TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimizer_run_specs (
    optimizer_run_id TEXT PRIMARY KEY REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    spec_json TEXT NOT NULL,
    spec_digest TEXT NOT NULL,
    authorization_json TEXT NOT NULL DEFAULT '{}',
    admitted_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimizer_algorithm_projections (
    optimizer_run_id TEXT PRIMARY KEY REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    algorithm TEXT NOT NULL,
    reducer_version TEXT NOT NULL,
    as_of_sequence INTEGER NOT NULL,
    projection_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimizer_work_items (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    work_item_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    lifecycle TEXT NOT NULL,
    terminal TEXT,
    external_ref TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, work_item_id)
);
CREATE INDEX IF NOT EXISTS optimizer_work_items_run
ON optimizer_work_items(optimizer_run_id, lifecycle);

CREATE TABLE IF NOT EXISTS optimizer_usage_ledger (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    lane TEXT NOT NULL,
    cost_usd REAL,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, sequence, lane)
);

CREATE TABLE IF NOT EXISTS optimizer_evidence_refs (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    ref_id TEXT NOT NULL,
    digest TEXT,
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, kind, ref_id)
);

CREATE TABLE IF NOT EXISTS optimizer_evidence_amendments (
    amendment_id TEXT PRIMARY KEY,
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    terminal_sequence INTEGER NOT NULL,
    evidence_json TEXT NOT NULL,
    recorded_at TEXT NOT NULL
);

ALTER TABLE optimizer_runs ADD COLUMN lifecycle TEXT;
ALTER TABLE optimizer_runs ADD COLUMN phase TEXT;
ALTER TABLE optimizer_runs ADD COLUMN condition TEXT;
ALTER TABLE optimizer_runs ADD COLUMN placement TEXT;
ALTER TABLE optimizer_runs ADD COLUMN aggregate_sequence INTEGER;
ALTER TABLE optimizer_runs ADD COLUMN projection_revision INTEGER;

ALTER TABLE optimizer_events ADD COLUMN producer_id TEXT;
ALTER TABLE optimizer_events ADD COLUMN producer_sequence INTEGER;
ALTER TABLE optimizer_events ADD COLUMN payload_digest TEXT;
ALTER TABLE optimizer_events ADD COLUMN aggregate_sequence INTEGER;
ALTER TABLE optimizer_events ADD COLUMN committed_at TEXT;
"#;

/// Credential locator registry. Paths identify source licenses; credential
/// bytes continue to live only in the process-local EnvSourceStore.
const CREDENTIAL_LOCATORS_TABLE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS credential_locators (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN (
        'workspace_env_file','instance_env_file','process_environment','external_env_file'
    )),
    workspace_root_ref TEXT,
    workspace_canonical TEXT,
    relative_path TEXT,
    external_canonical TEXT,
    format TEXT NOT NULL DEFAULT 'dotenv',
    provider TEXT NOT NULL,
    variable TEXT NOT NULL,
    label TEXT NOT NULL,
    state TEXT NOT NULL,
    upsert_key TEXT NOT NULL UNIQUE,
    last_seen_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS credential_locators_pair
ON credential_locators(provider, variable, updated_at);
"#;

const MIGRATION_51: &str = r#"
CREATE TABLE IF NOT EXISTS credential_locators (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN (
        'workspace_env_file','instance_env_file','process_environment','external_env_file'
    )),
    workspace_root_ref TEXT,
    workspace_canonical TEXT,
    relative_path TEXT,
    external_canonical TEXT,
    format TEXT NOT NULL DEFAULT 'dotenv',
    provider TEXT NOT NULL,
    variable TEXT NOT NULL,
    label TEXT NOT NULL,
    state TEXT NOT NULL,
    upsert_key TEXT NOT NULL UNIQUE,
    last_seen_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS credential_locators_pair
ON credential_locators(provider, variable, updated_at);

ALTER TABLE secret_refs ADD COLUMN locator_id TEXT REFERENCES credential_locators(id);
ALTER TABLE secret_refs ADD COLUMN preferred INTEGER NOT NULL DEFAULT 0;
ALTER TABLE secret_refs ADD COLUMN source_state TEXT;
"#;

/// Convert historical evaluation executions to canonical eval optimizer runs
/// and make experiment request idempotency independent of execution members.
/// The legacy campaign tables remain read-only until the final UI/drop cut.
const MIGRATION_52: &str = r#"
ALTER TABLE experiment_groups ADD COLUMN request_id TEXT;

UPDATE experiment_groups
SET request_id = (
    SELECT member_id
    FROM experiment_group_members member
    WHERE member.group_id = experiment_groups.id
      AND member.member_kind = 'direct_evaluation'
    ORDER BY attached_at, member_id
    LIMIT 1
)
WHERE request_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS experiment_groups_request
ON experiment_groups(request_id) WHERE request_id IS NOT NULL;

-- Campaign and optimizer-run identities share the same namespace after this
-- cutover.  Refuse an existing collision instead of attaching the campaign's
-- evidence and experiment membership to an unrelated optimizer execution.
CREATE TEMP TABLE migration_52_identity_guard (
    collision INTEGER NOT NULL CHECK (collision = 0)
);
INSERT INTO migration_52_identity_guard(collision)
SELECT 1
FROM eval_campaigns campaign
JOIN optimizer_runs run ON run.id = campaign.id;
DROP TABLE migration_52_identity_guard;

INSERT OR IGNORE INTO optimizer_runs(
    id, algorithm_id, algorithm_version, status, source, objective,
    project_ref, session_ref, created_at, started_at, finished_at,
    cursor_seq, capabilities_json, bindings_json, input_refs_json,
    output_refs_json, visual_refs_json, summary_json, usage_json,
    error_json, payload_json, updated_at,
    lifecycle, phase, condition, placement, aggregate_sequence, projection_revision
)
SELECT
    campaign.id,
    'eval',
    'legacy-campaign-migration.v1',
    CASE campaign.status
        WHEN 'planned' THEN 'queued'
        WHEN 'running' THEN 'running'
        WHEN 'complete' THEN 'completed'
        WHEN 'partial' THEN 'degraded'
        ELSE 'failed'
    END,
    'legacy_campaign_migration',
    campaign.title,
    NULL,
    campaign.session_id,
    campaign.created_at,
    campaign.started_at,
    campaign.settled_at,
    campaign.expected_rollouts + 2 +
        CASE WHEN campaign.status IN ('complete','partial','failed') THEN 1 ELSE 0 END,
    '{"cancel":false,"pause":false,"resume":false,"streamEvents":true,"stateSlices":true,"candidates":false,"checkpoints":false,"checkpointEvaluations":false,"inferenceEndpoint":false,"localSlotBinding":false}',
    json_array(),
    json_array(),
    json_array(),
    json_array(),
    json_object(
        'migratedFrom', 'eval_campaign',
        'legacyCampaignId', campaign.id,
        'containerId', campaign.container_id,
        'expectedRollouts', campaign.expected_rollouts,
        'maxConcurrency', campaign.max_concurrency,
        'policyRef', json(campaign.policy_ref_json),
        'plan', json(campaign.plan_json)
    ),
    '{"costUsd":null,"promptTokens":0,"completionTokens":0,"rollouts":0,"wallTimeMs":0,"extra":{}}',
    NULL,
    json_object(
        'schemaVersion', 'optimizer_run.v1',
        'id', campaign.id,
        'algorithmId', 'eval',
        'algorithmVersion', 'legacy-campaign-migration.v1',
        'status', CASE campaign.status
            WHEN 'planned' THEN 'queued'
            WHEN 'running' THEN 'running'
            WHEN 'complete' THEN 'completed'
            WHEN 'partial' THEN 'degraded'
            ELSE 'failed'
        END,
        'source', 'legacy_campaign_migration',
        'objective', campaign.title,
        'projectRef', NULL,
        'sessionRef', campaign.session_id,
        'createdAt', campaign.created_at,
        'startedAt', campaign.started_at,
        'finishedAt', campaign.settled_at,
        'cursorSeq', campaign.expected_rollouts + 2 +
            CASE WHEN campaign.status IN ('complete','partial','failed') THEN 1 ELSE 0 END,
        'capabilities', json('{"cancel":false,"pause":false,"resume":false,"streamEvents":true,"stateSlices":true,"candidates":false,"checkpoints":false,"checkpointEvaluations":false,"inferenceEndpoint":false,"localSlotBinding":false}'),
        'executionBindings', json_array(),
        'inputRefs', json_array(),
        'outputRefs', json_array(),
        'visualRefs', json_array(),
        'summary', json_object(
            'migratedFrom', 'eval_campaign',
            'legacyCampaignId', campaign.id,
            'containerId', campaign.container_id,
            'expectedRollouts', campaign.expected_rollouts,
            'maxConcurrency', campaign.max_concurrency,
            'policyRef', json(campaign.policy_ref_json),
            'plan', json(campaign.plan_json)
        ),
        'usage', json('{"costUsd":null,"promptTokens":0,"completionTokens":0,"rollouts":0,"wallTimeMs":0,"extra":{}}'),
        'error', NULL
    ),
    COALESCE(campaign.settled_at, campaign.started_at, campaign.created_at),
    CASE WHEN campaign.status = 'planned' THEN 'queued'
         WHEN campaign.status = 'running' THEN 'running' ELSE 'terminal' END,
    NULL,
    'healthy',
    'direct_container_evaluation',
    campaign.expected_rollouts + 2 +
        CASE WHEN campaign.status IN ('complete','partial','failed') THEN 1 ELSE 0 END,
    campaign.expected_rollouts + 2 +
        CASE WHEN campaign.status IN ('complete','partial','failed') THEN 1 ELSE 0 END
FROM eval_campaigns campaign;

INSERT OR IGNORE INTO optimizer_run_drafts(
    id, algorithm, spec_json, spec_digest, admission_state,
    authorization_ref, created_at, updated_at
)
SELECT
    'migration52:' || campaign.id,
    'eval',
    json_object(
        'schemaVersion', 'legacy_eval_campaign_spec.v1',
        'campaignId', campaign.id,
        'containerId', campaign.container_id,
        'expectedRollouts', campaign.expected_rollouts,
        'maxConcurrency', campaign.max_concurrency,
        'policyRef', json(campaign.policy_ref_json),
        'plan', json(campaign.plan_json)
    ),
    'legacy-campaign:' || campaign.id,
    'consumed',
    'workshop:admission-not-required:legacy-campaign-migration',
    campaign.created_at,
    COALESCE(campaign.settled_at, campaign.started_at, campaign.created_at)
FROM eval_campaigns campaign
JOIN optimizer_runs run ON run.id = campaign.id
WHERE run.source = 'legacy_campaign_migration';

INSERT OR IGNORE INTO optimizer_run_specs(
    optimizer_run_id, spec_json, spec_digest, authorization_json, admitted_at
)
SELECT
    campaign.id,
    json_object(
        'schemaVersion', 'legacy_eval_campaign_spec.v1',
        'campaignId', campaign.id,
        'containerId', campaign.container_id,
        'expectedRollouts', campaign.expected_rollouts,
        'maxConcurrency', campaign.max_concurrency,
        'policyRef', json(campaign.policy_ref_json),
        'plan', json(campaign.plan_json)
    ),
    'legacy-campaign:' || campaign.id,
    '{"state":"not_required","reason":"legacy_campaign_migration"}',
    campaign.created_at
FROM eval_campaigns campaign
JOIN optimizer_runs run ON run.id = campaign.id
WHERE run.source = 'legacy_campaign_migration';

INSERT OR IGNORE INTO optimizer_algorithm_projections(
    optimizer_run_id, algorithm, reducer_version, as_of_sequence, projection_json, updated_at
)
SELECT
    campaign.id,
    'eval',
    'eval.projection.v1',
    campaign.expected_rollouts + 2 +
        CASE WHEN campaign.status IN ('complete','partial','failed') THEN 1 ELSE 0 END,
    json_object(
        'algorithm', 'eval',
        'candidates', json_array(),
        'seeds', json(COALESCE((
            SELECT json_group_array(seed) FROM (
                SELECT seed FROM eval_campaign_rollouts
                WHERE campaign_id = campaign.id ORDER BY ordinal
            )
        ), '[]')),
        'scenarios', json_array(),
        'workItems', json(COALESCE((
            SELECT json_group_array(json_object(
                'workItemId', rollout_id,
                'kind', 'eval_trial',
                'lifecycle', CASE status
                    WHEN 'planned' THEN 'planned'
                    WHEN 'started' THEN 'running'
                    ELSE 'terminal'
                END,
                'terminal', CASE status
                    WHEN 'terminal' THEN 'completed'
                    WHEN 'failed' THEN 'failed'
                    WHEN 'missing' THEN 'failed'
                    ELSE NULL
                END,
                'externalRef', task_instance_id
            )) FROM (
                SELECT * FROM eval_campaign_rollouts
                WHERE campaign_id = campaign.id ORDER BY ordinal
            )
        ), '[]')),
        'phase', NULL,
        'usage', json('{"costUsd":null,"promptTokens":null,"completionTokens":null,"steps":null}'),
        'meanReward', (
            SELECT AVG(CASE WHEN json_valid(terminal_json)
                THEN json_extract(terminal_json, '$.reward') END)
            FROM eval_campaign_rollouts WHERE campaign_id = campaign.id
        ),
        'scoredTrials', (
            SELECT COUNT(*) FROM eval_campaign_rollouts
            WHERE campaign_id = campaign.id
              AND json_valid(terminal_json)
              AND json_extract(terminal_json, '$.reward') IS NOT NULL
        ),
        'promotionApplicable', json('false'),
        'traces', (
            SELECT COUNT(*) FROM eval_campaign_rollouts
            WHERE campaign_id = campaign.id AND terminal_json IS NOT NULL
        )
    ),
    COALESCE(campaign.settled_at, campaign.started_at, campaign.created_at)
FROM eval_campaigns campaign
JOIN optimizer_runs run ON run.id = campaign.id
WHERE run.source = 'legacy_campaign_migration';

INSERT OR IGNORE INTO optimizer_work_items(
    work_item_id, optimizer_run_id, kind, lifecycle, terminal,
    external_ref, created_at, updated_at
)
SELECT
    rollout.rollout_id,
    rollout.campaign_id,
    'eval_trial',
    CASE rollout.status
        WHEN 'planned' THEN 'planned'
        WHEN 'started' THEN 'running'
        ELSE 'terminal'
    END,
    CASE rollout.status
        WHEN 'terminal' THEN 'completed'
        WHEN 'failed' THEN 'failed'
        WHEN 'missing' THEN 'failed'
        ELSE NULL
    END,
    rollout.task_instance_id,
    campaign.created_at,
    COALESCE(rollout.settled_at, rollout.started_at, campaign.created_at)
FROM eval_campaign_rollouts rollout
JOIN eval_campaigns campaign ON campaign.id = rollout.campaign_id
JOIN optimizer_runs run ON run.id = campaign.id
WHERE run.source = 'legacy_campaign_migration';

INSERT OR IGNORE INTO optimizer_events(
    event_id, optimizer_run_id, sequence_number, event_type, algorithm_id,
    occurred_at, payload_json, producer_id, producer_sequence,
    payload_digest, aggregate_sequence, committed_at
)
SELECT
    'migration52:' || campaign.id || ':plan',
    campaign.id,
    1,
    'eval.run.planned',
    'eval',
    campaign.created_at,
    json_object(
        'schemaVersion', 'optimizer_event.v1',
        'eventId', 'migration52:' || campaign.id || ':plan',
        'type', 'eval.run.planned',
        'sequenceNumber', 1,
        'occurredAt', campaign.created_at,
        'optimizerRunId', campaign.id,
        'algorithmId', 'eval',
        'level', 'info',
        'item', NULL,
        'delta', json_object(),
        'snapshot', json_object(
            'plannedTrials', campaign.expected_rollouts,
            'workItemIds', json(COALESCE((
                SELECT json_group_array(rollout_id) FROM (
                    SELECT rollout_id FROM eval_campaign_rollouts
                    WHERE campaign_id = campaign.id ORDER BY ordinal
                )
            ), '[]'))
        ),
        'usageDelta', NULL,
        'artifactRefs', json_array(),
        'error', NULL,
        'raw', json_object('source', 'migration_52')
    ),
    'migration-52', 1, 'migration-52-unavailable', 1, campaign.created_at
FROM eval_campaigns campaign
JOIN optimizer_runs run ON run.id = campaign.id
WHERE run.source = 'legacy_campaign_migration';

INSERT OR IGNORE INTO optimizer_events(
    event_id, optimizer_run_id, sequence_number, event_type, algorithm_id,
    occurred_at, payload_json, producer_id, producer_sequence,
    payload_digest, aggregate_sequence, committed_at
)
SELECT
    'migration52:' || campaign.id || ':lifecycle',
    campaign.id,
    2,
    CASE WHEN campaign.status = 'planned'
         THEN 'migration.eval.run.planned' ELSE 'optimizer.run.started' END,
    'eval',
    COALESCE(campaign.started_at, campaign.created_at),
    json_object(
        'schemaVersion', 'optimizer_event.v1',
        'eventId', 'migration52:' || campaign.id || ':lifecycle',
        'type', CASE WHEN campaign.status = 'planned'
             THEN 'migration.eval.run.planned' ELSE 'optimizer.run.started' END,
        'sequenceNumber', 2,
        'occurredAt', COALESCE(campaign.started_at, campaign.created_at),
        'optimizerRunId', campaign.id,
        'algorithmId', 'eval',
        'level', 'info',
        'item', NULL,
        'delta', json_object('status', campaign.status),
        'snapshot', NULL,
        'usageDelta', NULL,
        'artifactRefs', json_array(),
        'error', NULL,
        'raw', json_object('source', 'migration_52')
    ),
    'migration-52', 2, 'migration-52-unavailable', 2,
    COALESCE(campaign.started_at, campaign.created_at)
FROM eval_campaigns campaign
JOIN optimizer_runs run ON run.id = campaign.id
WHERE run.source = 'legacy_campaign_migration';

INSERT OR IGNORE INTO optimizer_events(
    event_id, optimizer_run_id, sequence_number, event_type, algorithm_id,
    occurred_at, payload_json, producer_id, producer_sequence,
    payload_digest, aggregate_sequence, committed_at
)
SELECT
    'migration52:' || rollout.campaign_id || ':rollout:' || rollout.ordinal,
    rollout.campaign_id,
    rollout.ordinal + 2,
    CASE rollout.status
        WHEN 'started' THEN 'eval.trial.started'
        WHEN 'terminal' THEN 'eval.trial.terminal'
        WHEN 'failed' THEN 'eval.trial.terminal'
        WHEN 'missing' THEN 'eval.trial.terminal'
        ELSE 'migration.eval.trial.planned'
    END,
    'eval',
    COALESCE(rollout.settled_at, rollout.started_at, campaign.created_at),
    json_object(
        'schemaVersion', 'optimizer_event.v1',
        'eventId', 'migration52:' || rollout.campaign_id || ':rollout:' || rollout.ordinal,
        'type', CASE rollout.status
            WHEN 'started' THEN 'eval.trial.started'
            WHEN 'terminal' THEN 'eval.trial.terminal'
            WHEN 'failed' THEN 'eval.trial.terminal'
            WHEN 'missing' THEN 'eval.trial.terminal'
            ELSE 'migration.eval.trial.planned'
        END,
        'sequenceNumber', rollout.ordinal + 2,
        'occurredAt', COALESCE(rollout.settled_at, rollout.started_at, campaign.created_at),
        'optimizerRunId', rollout.campaign_id,
        'algorithmId', 'eval',
        'level', CASE WHEN rollout.status IN ('failed','missing') THEN 'warn' ELSE 'info' END,
        'item', json_object(
            'kind', 'trial',
            'id', rollout.rollout_id,
            'status', rollout.status,
            'valid', json(CASE WHEN rollout.status = 'terminal' THEN 'true' ELSE 'false' END),
            'seed', rollout.seed,
            'taskInstanceId', rollout.task_instance_id,
            'reward', CASE WHEN json_valid(rollout.terminal_json)
                THEN json_extract(rollout.terminal_json, '$.reward') END,
            'raw', CASE WHEN json_valid(rollout.terminal_json)
                THEN json(rollout.terminal_json) ELSE NULL END
        ),
        'delta', json_object(),
        'snapshot', NULL,
        'usageDelta', NULL,
        'artifactRefs', CASE
            WHEN rollout.terminal_json IS NOT NULL THEN json_array(json_object(
                'kind', 'legacy_eval_terminal',
                'id', rollout.rollout_id
            ))
            ELSE json_array()
        END,
        'error', NULL,
        'raw', json_object('source', 'migration_52')
    ),
    'migration-52', rollout.ordinal + 2, 'migration-52-unavailable', rollout.ordinal + 2,
    COALESCE(rollout.settled_at, rollout.started_at, campaign.created_at)
FROM eval_campaign_rollouts rollout
JOIN eval_campaigns campaign ON campaign.id = rollout.campaign_id
JOIN optimizer_runs run ON run.id = campaign.id
WHERE run.source = 'legacy_campaign_migration';

INSERT OR IGNORE INTO optimizer_events(
    event_id, optimizer_run_id, sequence_number, event_type, algorithm_id,
    occurred_at, payload_json, producer_id, producer_sequence,
    payload_digest, aggregate_sequence, committed_at
)
SELECT
    'migration52:' || campaign.id || ':terminal',
    campaign.id,
    campaign.expected_rollouts + 3,
    CASE campaign.status WHEN 'complete' THEN 'optimizer.run.completed'
         WHEN 'partial' THEN 'optimizer.run.degraded' ELSE 'optimizer.run.failed' END,
    'eval',
    COALESCE(campaign.settled_at, campaign.started_at, campaign.created_at),
    json_object(
        'schemaVersion', 'optimizer_event.v1',
        'eventId', 'migration52:' || campaign.id || ':terminal',
        'type', CASE campaign.status WHEN 'complete' THEN 'optimizer.run.completed'
             WHEN 'partial' THEN 'optimizer.run.degraded' ELSE 'optimizer.run.failed' END,
        'sequenceNumber', campaign.expected_rollouts + 3,
        'occurredAt', COALESCE(campaign.settled_at, campaign.started_at, campaign.created_at),
        'optimizerRunId', campaign.id,
        'algorithmId', 'eval',
        'level', CASE WHEN campaign.status = 'complete' THEN 'info' ELSE 'warn' END,
        'item', NULL,
        'delta', json_object('legacyCampaignStatus', campaign.status),
        'snapshot', NULL,
        'usageDelta', NULL,
        'artifactRefs', json_array(),
        'error', NULL,
        'raw', json_object('source', 'migration_52')
    ),
    'migration-52', campaign.expected_rollouts + 3, 'migration-52-unavailable',
    campaign.expected_rollouts + 3,
    COALESCE(campaign.settled_at, campaign.started_at, campaign.created_at)
FROM eval_campaigns campaign
JOIN optimizer_runs run ON run.id = campaign.id
WHERE run.source = 'legacy_campaign_migration'
  AND campaign.status IN ('complete','partial','failed');

-- Migration 52 creates a complete durable kernel history for terminal legacy
-- campaigns. Seal the matching cursor as well: terminal kernel loads replay
-- from this write-once boundary and must never infer it from the mutable run
-- row alone.
INSERT OR IGNORE INTO optimizer_terminal_manifests(
    optimizer_run_id, schema_version, algorithm_id, terminal_status,
    terminal_cursor, sealed_at, payload_json
)
SELECT
    campaign.id,
    'optimizer_terminal_manifest.v1',
    'eval',
    CASE campaign.status WHEN 'complete' THEN 'completed'
         WHEN 'partial' THEN 'degraded' ELSE 'failed' END,
    campaign.expected_rollouts + 3,
    COALESCE(campaign.settled_at, campaign.started_at, campaign.created_at),
    json_object(
        'schemaVersion', 'optimizer_terminal_manifest.v1',
        'optimizerRunId', campaign.id,
        'algorithmId', 'eval',
        'terminalStatus', CASE campaign.status WHEN 'complete' THEN 'completed'
             WHEN 'partial' THEN 'degraded' ELSE 'failed' END,
        'terminalCursor', campaign.expected_rollouts + 3,
        'work', json_object(
            'planned', campaign.expected_rollouts,
            'succeeded', (
                SELECT COUNT(*) FROM eval_campaign_rollouts
                WHERE campaign_id = campaign.id AND status = 'terminal'
            ),
            'failed', (
                SELECT COUNT(*) FROM eval_campaign_rollouts
                WHERE campaign_id = campaign.id AND status IN ('failed','missing')
            ),
            'cancelled', 0,
            'skipped', 0,
            'unit', 'trials'
        )
    )
FROM eval_campaigns campaign
JOIN optimizer_runs run ON run.id = campaign.id
WHERE run.source = 'legacy_campaign_migration'
  AND campaign.status IN ('complete','partial','failed');

INSERT OR REPLACE INTO optimizer_event_cursors(optimizer_run_id, cursor_seq, updated_at)
SELECT
    campaign.id,
    campaign.expected_rollouts + 2 +
        CASE WHEN campaign.status IN ('complete','partial','failed') THEN 1 ELSE 0 END,
    COALESCE(campaign.settled_at, campaign.started_at, campaign.created_at)
FROM eval_campaigns campaign
JOIN optimizer_runs run ON run.id = campaign.id
WHERE run.source = 'legacy_campaign_migration';

CREATE TABLE experiment_group_members_next (
    group_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    member_kind TEXT NOT NULL CHECK (member_kind = 'optimizer_run'),
    member_id TEXT NOT NULL,
    title TEXT NOT NULL,
    attached_at TEXT NOT NULL,
    PRIMARY KEY (group_id, member_kind, member_id)
);
INSERT OR IGNORE INTO experiment_group_members_next(
    group_id, member_kind, member_id, title, attached_at
)
SELECT
    member.group_id,
    'optimizer_run',
    member.member_id,
    member.title,
    member.attached_at
FROM experiment_group_members member
JOIN optimizer_runs run ON run.id = member.member_id
WHERE member.member_kind IN ('optimizer_run','eval_campaign');
DROP TABLE experiment_group_members;
ALTER TABLE experiment_group_members_next RENAME TO experiment_group_members;
CREATE INDEX experiment_group_members_kind
ON experiment_group_members(member_kind, member_id);

CREATE TABLE experiment_nodes_next_v52 (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('baseline','variant','run','result','optimizer_run')),
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    metrics_json TEXT,
    cost_usd REAL,
    artifact_refs_json TEXT NOT NULL DEFAULT '[]',
    trace_refs_json TEXT NOT NULL DEFAULT '[]',
    provenance_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    evidence_refs_json TEXT NOT NULL DEFAULT '[]',
    UNIQUE(experiment_id, id)
);
INSERT INTO experiment_nodes_next_v52(
    id, experiment_id, kind, title, status, config_json, metrics_json, cost_usd,
    artifact_refs_json, trace_refs_json, provenance_json, created_at, updated_at,
    evidence_refs_json
)
SELECT
    node.id,
    node.experiment_id,
    CASE WHEN node.kind = 'eval_campaign' THEN 'optimizer_run' ELSE node.kind END,
    node.title,
    node.status,
    CASE WHEN node.kind = 'eval_campaign' AND json_valid(node.config_json)
         THEN json_set(node.config_json, '$.memberKind', 'optimizer_run')
         ELSE node.config_json END,
    node.metrics_json,
    node.cost_usd,
    node.artifact_refs_json,
    node.trace_refs_json,
    node.provenance_json,
    node.created_at,
    node.updated_at,
    node.evidence_refs_json
FROM experiment_nodes node
WHERE node.kind != 'direct_evaluation';

CREATE TABLE experiment_edges_next_v52 AS
SELECT edge.* FROM experiment_edges edge
JOIN experiment_nodes_next_v52 source ON source.id = edge.source_node_id
JOIN experiment_nodes_next_v52 target ON target.id = edge.target_node_id;

DROP TABLE experiment_edges;
DROP TABLE experiment_nodes;
ALTER TABLE experiment_nodes_next_v52 RENAME TO experiment_nodes;

CREATE TABLE experiment_edges (
    id TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiment_groups(id) ON DELETE CASCADE,
    source_node_id TEXT NOT NULL REFERENCES experiment_nodes(id) ON DELETE CASCADE,
    target_node_id TEXT NOT NULL REFERENCES experiment_nodes(id) ON DELETE CASCADE,
    relation TEXT NOT NULL CHECK (relation IN (
        'forked_from','rerun_of','warm_started_from','produced','evaluated',
        'compared_with','promoted_to','reproduced_on','rolled_back_to','follow_up'
    )),
    created_at TEXT NOT NULL,
    UNIQUE(experiment_id, source_node_id, target_node_id, relation)
);
INSERT INTO experiment_edges SELECT * FROM experiment_edges_next_v52;
DROP TABLE experiment_edges_next_v52;

CREATE INDEX experiment_nodes_experiment
ON experiment_nodes(experiment_id, created_at, id);
CREATE INDEX experiment_edges_experiment
ON experiment_edges(experiment_id, created_at, id);
"#;

/// Final optimizer-run authority cutover. Migration 52 copied every legacy
/// execution, work item, event, and experiment membership into the kernel.
/// These tables are therefore rollback-only duplicates and must not survive as
/// a second writable aggregate.
const MIGRATION_53: &str = r#"
DROP TABLE eval_campaign_rollouts;
DROP TABLE eval_campaigns;
DROP TABLE evaluation_rollouts;
DROP TABLE evaluation_run_drafts;
DROP TABLE evaluation_runs;
"#;

/// Work-item identity is local to an optimizer run. Algorithm producers use
/// stable logical identities such as `eval:trial:0`; a global primary key made
/// the second run of the same algorithm collide with the first during plan
/// projection. Preserve those producer identities and scope uniqueness to the
/// aggregate that owns them.
const MIGRATION_54: &str = r#"
ALTER TABLE optimizer_work_items RENAME TO optimizer_work_items_v53;

CREATE TABLE optimizer_work_items (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    work_item_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    lifecycle TEXT NOT NULL,
    terminal TEXT,
    external_ref TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, work_item_id)
);

INSERT INTO optimizer_work_items(
    optimizer_run_id, work_item_id, kind, lifecycle, terminal,
    external_ref, created_at, updated_at
)
SELECT optimizer_run_id, work_item_id, kind, lifecycle, terminal,
       external_ref, created_at, updated_at
FROM optimizer_work_items_v53;

DROP TABLE optimizer_work_items_v53;

CREATE INDEX optimizer_work_items_run
ON optimizer_work_items(optimizer_run_id, lifecycle);
"#;

/// F2: cancellation as a durable command + receipt. A request row is written
/// when the typed cancellation is issued; the sealing transaction backfills
/// `settled_sequence`, turning the request into a receipt.
const MIGRATION_55: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_cancellation_requests (
    request_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    cause TEXT NOT NULL,
    requested_by TEXT NOT NULL,
    requested_at TEXT NOT NULL,
    scope TEXT NOT NULL,
    reason_code TEXT,
    observed_at TEXT,
    settled_sequence INTEGER
);

CREATE INDEX IF NOT EXISTS optimizer_cancellation_requests_run
ON optimizer_cancellation_requests(run_id, settled_sequence);
"#;

/// F4: the admitted producer/consumer contract and the artifacts it declares.
/// Both are run-owned facts: the effective contract is immutable for one run,
/// while artifact identity is scoped to the event that first declared it.
const MIGRATION_56: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_effective_contracts (
    optimizer_run_id TEXT PRIMARY KEY REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    schema_version TEXT NOT NULL,
    contract_json TEXT NOT NULL,
    negotiated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimizer_run_artifacts (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    artifact_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    work_item_id TEXT,
    rollout_id TEXT,
    kind TEXT NOT NULL,
    locator TEXT NOT NULL,
    digest TEXT,
    media_type TEXT,
    byte_size INTEGER,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    declared_at TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, artifact_id)
);
CREATE INDEX IF NOT EXISTS optimizer_run_artifacts_sequence
ON optimizer_run_artifacts(optimizer_run_id, sequence, artifact_id);
CREATE INDEX IF NOT EXISTS optimizer_run_artifacts_work_item
ON optimizer_run_artifacts(optimizer_run_id, work_item_id, sequence);
CREATE INDEX IF NOT EXISTS optimizer_run_artifacts_rollout
ON optimizer_run_artifacts(optimizer_run_id, rollout_id, sequence);
"#;

/// F5 delivery and cost truth. A projection commit also commits one durable
/// wake-up per bound surface. Cost completeness is stored independently from
/// the integer so a reported $0.00 cannot be confused with an absent charge.
const MIGRATION_57: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_projection_outbox (
    run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    projection_revision INTEGER NOT NULL,
    consumer TEXT NOT NULL,
    delivery_state TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (run_id, projection_revision, consumer)
);
CREATE INDEX IF NOT EXISTS optimizer_projection_outbox_pending
ON optimizer_projection_outbox(delivery_state, updated_at);

ALTER TABLE secret_capabilities ADD COLUMN used_cost_known INTEGER NOT NULL DEFAULT 0;
UPDATE secret_capabilities
SET used_cost_known = CASE WHEN used_calls = 0 THEN 1 ELSE 0 END;
"#;

/// Phase-5 Stage 0: make the optimizer event log queryable without decoding
/// every envelope. `occurred_at` remains the producer clock for compatibility;
/// `ingested_at` is the host witness used for rates and resume diagnostics.
/// Legacy rows use their producer timestamp as an explicitly named fallback,
/// while every new append records the host clock at the write boundary.
/// `payload_cas_digest` reserves the future replay-CAS cutover; it stays NULL
/// until every historical replay reader can hydrate through ContentStore.
const MIGRATION_58: &str = r#"
ALTER TABLE optimizer_events ADD COLUMN rollout_id TEXT;
ALTER TABLE optimizer_events ADD COLUMN kind TEXT;
ALTER TABLE optimizer_events ADD COLUMN step INTEGER;
ALTER TABLE optimizer_events ADD COLUMN span_id TEXT;
ALTER TABLE optimizer_events ADD COLUMN producer_occurred_at TEXT;
ALTER TABLE optimizer_events ADD COLUMN ingested_at TEXT;
ALTER TABLE optimizer_events ADD COLUMN ingest_witness TEXT;
ALTER TABLE optimizer_events ADD COLUMN producer_digest TEXT;
ALTER TABLE optimizer_events ADD COLUMN payload_cas_digest TEXT;

UPDATE optimizer_events
SET kind = COALESCE(
        json_extract(payload_json, '$.delta.container_event.kind'),
        json_extract(payload_json, '$.delta.containerEvent.kind'),
        json_extract(payload_json, '$.raw.container_event.kind'),
        json_extract(payload_json, '$.raw.containerEvent.kind'),
        event_type
    ),
    rollout_id = COALESCE(
        json_extract(payload_json, '$.delta.container_event.rollout_id'),
        json_extract(payload_json, '$.delta.container_event.rolloutId'),
        json_extract(payload_json, '$.delta.containerEvent.rollout_id'),
        json_extract(payload_json, '$.delta.containerEvent.rolloutId'),
        json_extract(payload_json, '$.raw.container_event.rollout_id'),
        json_extract(payload_json, '$.raw.containerEvent.rolloutId'),
        json_extract(payload_json, '$.raw.rollout_id')
    ),
    step = CAST(COALESCE(
        json_extract(payload_json, '$.delta.container_event.payload.step'),
        json_extract(payload_json, '$.delta.containerEvent.payload.step'),
        json_extract(payload_json, '$.raw.container_event.payload.step'),
        json_extract(payload_json, '$.raw.containerEvent.payload.step'),
        json_extract(payload_json, '$.raw.container_event.step'),
        json_extract(payload_json, '$.raw.step')
    ) AS INTEGER),
    span_id = COALESCE(
        json_extract(payload_json, '$.delta.container_event.payload.span_id'),
        json_extract(payload_json, '$.delta.container_event.payload.spanId'),
        json_extract(payload_json, '$.delta.containerEvent.payload.span_id'),
        json_extract(payload_json, '$.delta.containerEvent.payload.spanId'),
        json_extract(payload_json, '$.raw.container_event.payload.span_id'),
        json_extract(payload_json, '$.raw.container_event.payload.spanId'),
        json_extract(payload_json, '$.raw.container_event.span_id'),
        json_extract(payload_json, '$.raw.span_id')
    ),
    producer_occurred_at = COALESCE(
        json_extract(payload_json, '$.delta.container_event.occurred_at'),
        json_extract(payload_json, '$.delta.containerEvent.occurredAt'),
        json_extract(payload_json, '$.raw.container_event.occurred_at'),
        json_extract(payload_json, '$.raw.containerEvent.occurredAt'),
        occurred_at
    ),
    ingested_at = occurred_at,
    ingest_witness = 'legacy_producer_clock',
    producer_digest = COALESCE(
        json_extract(payload_json, '$.delta.container_event.digest'),
        json_extract(payload_json, '$.delta.containerEvent.digest'),
        json_extract(payload_json, '$.raw.container_event.digest'),
        json_extract(payload_json, '$.raw.containerEvent.digest'),
        json_extract(payload_json, '$.raw.digest')
    )
WHERE kind IS NULL;

CREATE INDEX IF NOT EXISTS optimizer_events_run_kind
ON optimizer_events(optimizer_run_id, kind, sequence_number);
CREATE INDEX IF NOT EXISTS optimizer_events_rollout_step
ON optimizer_events(rollout_id, step, sequence_number);
CREATE INDEX IF NOT EXISTS optimizer_events_ingested
ON optimizer_events(optimizer_run_id, ingested_at, sequence_number);
"#;

/// Conversation-scoped paid-compute auto-approval projection. Sealed at
/// session start; reservations and settled spend survive Workshop restart.
const MIGRATION_59: &str = PAID_COMPUTE_BUDGET_CREATE_ONLY;

#[cfg(test)]
mod tests {
    /// Derived, not pinned: adding a migration should not mean editing
    /// every test that asserts the database reached the newest version.
    const LATEST_VERSION: i64 = super::MIGRATIONS.len() as i64;

    use super::*;

    /// Every `MIGRATION_N` constant is registered exactly once, and the registry
    /// is contiguous from 1.
    ///
    /// This reads its own source because the hazard is textual: two branches
    /// that each add a migration produce two identical `MIGRATION_N,` registry
    /// lines, and a merge collapses them into one — dropping a migration whose
    /// constant is still defined and still compiles. `MIGRATIONS.len()` cannot
    /// notice, because it shrinks with the registry.
    #[test]
    fn every_defined_migration_is_registered_exactly_once() {
        let source = include_str!("migrations.rs");
        let mut defined: Vec<usize> = source
            .lines()
            .filter_map(|line| line.strip_prefix("const MIGRATION_"))
            .filter_map(|rest| rest.split(':').next())
            .filter_map(|digits| digits.parse().ok())
            .collect();
        let mut registered: Vec<usize> = source
            .lines()
            .map(str::trim)
            .filter_map(|line| line.strip_prefix("MIGRATION_"))
            .filter_map(|rest| rest.strip_suffix(','))
            .filter_map(|digits| digits.parse().ok())
            .collect();
        defined.sort_unstable();
        registered.sort_unstable();
        assert!(!defined.is_empty(), "no migrations were parsed from source");
        assert_eq!(
            defined, registered,
            "every defined migration must appear in the registry exactly once"
        );
        assert_eq!(
            registered,
            (1..=registered.len()).collect::<Vec<_>>(),
            "the registry must be contiguous from 1 with no gaps or duplicates"
        );
        assert_eq!(MIGRATIONS.len(), registered.len());
    }

    #[test]
    fn migration_38_drops_session_uniqueness_and_adds_lineage() {
        let conn = seed_at_version(37);
        conn.execute(
            "INSERT INTO experiment_groups(id, session_id, title, created_at, updated_at)
             VALUES('exp_a', 'session_shared', 'First', '2026-08-26T00:00:00Z', '2026-08-26T00:00:00Z')",
            [],
        )
        .unwrap();
        let duplicate = conn.execute(
            "INSERT INTO experiment_groups(id, session_id, title, created_at, updated_at)
             VALUES('exp_b', 'session_shared', 'Second', '2026-08-26T00:01:00Z', '2026-08-26T00:01:00Z')",
            [],
        );
        assert!(
            duplicate.is_err(),
            "v37 must still refuse two experiments in one session"
        );
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        conn.execute(
            "INSERT INTO experiment_groups(id, session_id, title, created_at, updated_at)
             VALUES('exp_b', 'session_shared', 'Second', '2026-08-26T00:01:00Z', '2026-08-26T00:01:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO experiment_lineage(id, source_experiment_id, target_experiment_id, relation, created_at)
             VALUES('lin:exp_a:follow_up:exp_b', 'exp_a', 'exp_b', 'follow_up', '2026-08-26T00:01:00Z')",
            [],
        )
        .unwrap();
        let duplicate_follow_up = conn.execute(
            "INSERT INTO experiment_lineage(id, source_experiment_id, target_experiment_id, relation, created_at)
             VALUES('lin:dup', 'exp_a', 'exp_b', 'follow_up', '2026-08-26T00:02:00Z')",
            [],
        );
        assert!(
            duplicate_follow_up.is_err(),
            "follow_up must be unique per parent/child pair"
        );
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM experiment_groups WHERE session_id='session_shared'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        let present: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('sessions') WHERE name='active_experiment_id')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(present);
    }

    #[test]
    fn migration_39_adds_experiment_candidates() {
        let conn = seed_at_version(38);
        conn.execute(
            "INSERT INTO experiment_groups(id, session_id, title, created_at, updated_at)
             VALUES('exp_a', 'session_cand', 'GEPA', '2026-08-26T00:00:00Z', '2026-08-26T00:00:00Z')",
            [],
        )
        .unwrap();
        let present: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='experiment_candidates')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!present, "v38 must not already have experiment_candidates");
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        conn.execute(
            "INSERT INTO experiment_candidates(
                id, experiment_id, optimizer_run_id, producer_candidate_id,
                parent_ids_json, created_at, updated_at
             ) VALUES('can:opt_1:gepa_seed', 'exp_a', 'opt_1', 'gepa_seed', '[]',
                      '2026-08-26T00:01:00Z', '2026-08-26T00:01:00Z')",
            [],
        )
        .unwrap();
        let duplicate = conn.execute(
            "INSERT INTO experiment_candidates(
                id, experiment_id, optimizer_run_id, producer_candidate_id,
                parent_ids_json, created_at, updated_at
             ) VALUES('can:opt_1:gepa_seed_dup', 'exp_a', 'opt_1', 'gepa_seed', '[]',
                      '2026-08-26T00:02:00Z', '2026-08-26T00:02:00Z')",
            [],
        );
        assert!(
            duplicate.is_err(),
            "producer identity must be unique per optimizer_run"
        );
        conn.execute(
            "INSERT INTO experiment_candidates(
                id, experiment_id, optimizer_run_id, producer_candidate_id,
                parent_ids_json, created_at, updated_at
             ) VALUES('can:opt_1:gepa_child', 'exp_a', 'opt_1', 'gepa_child', '[]',
                      '2026-08-26T00:03:00Z', '2026-08-26T00:03:00Z')",
            [],
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM experiment_candidates WHERE optimizer_run_id='opt_1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn migration_40_adds_candidate_compare_promote_and_report_group_pointer() {
        let conn = seed_at_version(39);
        conn.execute(
            "INSERT INTO experiment_groups(id, session_id, title, created_at, updated_at)
             VALUES('exp_a', 'session_rel', 'GEPA', '2026-08-26T00:00:00Z', '2026-08-26T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO experiment_candidates(
                id, experiment_id, optimizer_run_id, producer_candidate_id,
                parent_ids_json, created_at, updated_at
             ) VALUES('can:opt_1:gepa_seed', 'exp_a', 'opt_1', 'gepa_seed', '[]',
                      '2026-08-26T00:01:00Z', '2026-08-26T00:01:00Z')",
            [],
        )
        .unwrap();
        let compared: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('experiment_candidates') WHERE name='compared_with_json')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!compared, "v39 must not already have compared_with_json");
        let group_col: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('experiment_records') WHERE name='experiment_group_id')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            !group_col,
            "v39 must not already have experiment_records.experiment_group_id"
        );
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        let compared_default: String = conn
            .query_row(
                "SELECT compared_with_json FROM experiment_candidates WHERE id='can:opt_1:gepa_seed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(compared_default, "[]");
        conn.execute(
            "UPDATE experiment_candidates SET compared_with_json='[\"can:opt_1:other\"]', promoted_to='can:opt_1:other'
             WHERE id='can:opt_1:gepa_seed'",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO experiment_records(
                experiment_id, title, status, created_at, created_by, experiment_group_id
             ) VALUES('rec_a', 'Pointer', 'planned', '2026-08-26T00:02:00Z', 'user', 'exp_a')",
            [],
        )
        .unwrap();
        let pointed: String = conn
            .query_row(
                "SELECT experiment_group_id FROM experiment_records WHERE experiment_id='rec_a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pointed, "exp_a");
    }

    /// A database that already recorded this version number under a different
    /// lane's DDL still ends up with the tables this build requires.
    #[test]
    fn a_version_collision_from_another_lane_still_heals_required_tables() {
        let conn = seed_at_version(MIGRATIONS.len() - 1);
        // Another lane's migration 23 landed here: the version is consumed, but
        // this lane's table was never created.
        conn.execute_batch("CREATE TABLE IF NOT EXISTS experiment_groups (id TEXT PRIMARY KEY);")
            .unwrap();
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, datetime('now'))",
            [MIGRATIONS.len() as i64],
        )
        .unwrap();
        assert_eq!(apply_migrations(&conn).unwrap(), MIGRATIONS.len() as i64);
        for (table, _) in REQUIRED_TABLES {
            let present: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                present,
                "{table} must exist even when its version was consumed elsewhere"
            );
        }
        let active_experiment_id_present: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM pragma_table_info('sessions')
                    WHERE name='active_experiment_id'
                )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            active_experiment_id_present,
            "sessions.active_experiment_id must survive a consumed migration version"
        );
    }

    /// A database stamped `version` with migrations 1..=version applied, as a
    /// real installation of that era would have shipped it.
    /// Legacy status spellings fold, and the column stops accepting new ones.
    ///
    /// The four terminal predicates that used to disagree are gone from Rust;
    /// this is what stops a fifth spelling from arriving through the database.
    #[test]
    fn migration_28_folds_legacy_statuses_and_closes_the_column() {
        let conn = seed_at_version(27);
        let insert = "INSERT INTO optimizer_runs(
                id, algorithm_id, status, source, created_at, payload_json, updated_at)
             VALUES (?1, 'gepa', ?2, 'local', 'now', '{}', 'now')";
        for (id, status) in [
            ("run-succeeded", "succeeded"),
            ("run-canceled", "canceled"),
            ("run-done", "done"),
            ("run-error", "error"),
            ("run-created", "created"),
            ("run-nonsense", "who_knows"),
            ("run-running", "running"),
        ] {
            conn.execute(insert, rusqlite::params![id, status]).unwrap();
        }

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);

        let status_of = |id: &str| -> String {
            conn.query_row(
                "SELECT status FROM optimizer_runs WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(status_of("run-succeeded"), "completed");
        assert_eq!(status_of("run-canceled"), "cancelled");
        assert_eq!(status_of("run-done"), "failed");
        assert_eq!(status_of("run-error"), "failed");
        assert_eq!(status_of("run-created"), "queued");
        assert_eq!(status_of("run-running"), "running");
        // A word from no known producer is recorded as failed rather than left
        // to abort the next write.
        assert_eq!(status_of("run-nonsense"), "failed");

        let inserted = conn.execute(insert, rusqlite::params!["run-new", "succeeded"]);
        assert!(
            inserted.is_err(),
            "the column must refuse a status outside OptimizerRunStatus"
        );
        let updated = conn.execute(
            "UPDATE optimizer_runs SET status = 'terminated' WHERE id = 'run-running'",
            [],
        );
        assert!(updated.is_err(), "an update must be refused the same way");
        assert_eq!(status_of("run-running"), "running");

        // Every canonical spelling is still writable.
        for status in [
            "queued",
            "validating",
            "provisioning",
            "starting",
            "waiting_for_viewer",
            "running",
            "paused",
            "cancelling",
            "env_unreachable",
            "degraded",
            "completed",
            "failed",
            "failed_evidence",
            "cancelled",
            "interrupted",
            "infrastructure_lost",
            "cap_reached",
        ] {
            conn.execute(
                "UPDATE optimizer_runs SET status = ?1 WHERE id = 'run-running'",
                [status],
            )
            .unwrap_or_else(|error| panic!("{status} must be writable: {error}"));
        }
    }

    fn seed_at_version(version: usize) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
        )
        .unwrap();
        for (index, migration) in MIGRATIONS.iter().take(version).enumerate() {
            conn.execute_batch(migration).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, datetime('now'))",
                [index as i64 + 1],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn upgrades_v1_database_without_losing_runs() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
        )
        .unwrap();
        conn.execute_batch(MIGRATION_1).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions(id, title, target_json, status, created_at, updated_at)
             VALUES ('session-1', 'Session', '{}', 'ready', 'before', 'before')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO runs(id, session_id, mode, status, created_at)
             VALUES ('run-1', 'session-1', 'test', 'completed', 'before')",
            [],
        )
        .unwrap();

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        let updated_at: String = conn
            .query_row(
                "SELECT updated_at FROM runs WHERE id = 'run-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(updated_at, "before");
        let receipts: i64 = conn
            .query_row("SELECT COUNT(*) FROM command_receipts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(receipts, 0);
        let trace_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('trace_imports','trace_bundles','trace_bundle_members','trace_assets','trace_index','trace_tags','trace_projection_cache','trace_annotations')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(trace_tables, 8);
        let accounting_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_records'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accounting_tables, 1);
        // The performance table was folded into usage_records by migration 8;
        // Migration 11 folds the legacy rows; migration 12 guarantees the
        // empty rollback staging schema for already-upgraded QA databases.
        let performance_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'model_performance_samples'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(performance_tables, 0);
        let ledger_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_ledger'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            ledger_tables, 1,
            "rollback staging schema remains available"
        );
    }

    /// A v7 database imports performance samples into `usage_records`, then
    /// migration 11 folds legacy `usage_ledger` rows into the same ledger.
    #[test]
    fn migration_8_and_11_fold_ledger_and_import_performance_samples() {
        let conn = seed_at_version(7);
        conn.execute(
            "INSERT INTO usage_ledger(id,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at)
             VALUES('ledger-1','openrouter','luna',10,20,30,0.5,'2026-08-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO model_performance_samples(id,provider,model_id,request_id,measurement_kind,status,started_at_ms,completed_at_ms,input_tokens,cached_input_tokens,reasoning_tokens,output_tokens,ttft_ms,observed_output_tps,source,created_at)
             VALUES('perf-1','openrouter','openai/gpt-5.6-luna','req-1','observed_stream','completed',1000,2000,100,40,5,50,120.0,25.0,'codex_app_server','2026-08-01T00:00:01Z')",
            [],
        )
        .unwrap();

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);

        let ledger_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_ledger'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledger_tables, 1);
        let staged: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_ledger", [], |row| row.get(0))
            .unwrap();
        assert_eq!(staged, 0, "folded rows are cleared from rollback staging");
        let (billed, cost_source, source): (f64, String, String) = conn
            .query_row(
                "SELECT billed_cost_usd, cost_source, source
                 FROM usage_records WHERE request_id='legacy-ledger:ledger-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert!((billed - 0.5).abs() < 1e-9);
        assert_eq!(cost_source, "provider_reported");
        assert_eq!(source, "legacy_usage_ledger");
        let (input, cached, total, cost_source): (i64, i64, i64, String) = conn
            .query_row(
                "SELECT input_tokens, cached_input_tokens, total_tokens, cost_source
                 FROM usage_records WHERE provider='openrouter' AND request_id='req-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!((input, cached, total), (100, 40, 150));
        assert_eq!(cost_source, "none");
    }

    /// Candidate builds already applied the original migration 11, which
    /// folded rows and dropped the table. Migration 12 repairs that database
    /// for rollback. This query is copied from the pre-Wave-7 account summary:
    /// the old binary sums both ledgers, so compatibility must not mirror rows
    /// into both places or historical usage would double-count.
    #[test]
    fn migration_12_repairs_original_v11_and_folds_rollback_writes_once() {
        let conn = seed_at_version(10);
        conn.execute(
            "INSERT INTO usage_ledger(id,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at)
             VALUES('before-v11','openrouter','luna',10,20,30,0.5,'2026-08-01T00:00:00Z')",
            [],
        )
        .unwrap();

        // Exact original candidate migration-11 shape: fold, drop, stamp.
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE;\n{FOLD_LEGACY_USAGE_ROWS}\nDROP TABLE usage_ledger;\nINSERT INTO schema_migrations(version, applied_at) VALUES (11, datetime('now'));\nCOMMIT;"
        ))
        .unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 11);

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        let old_binary_sum = |conn: &Connection| -> f64 {
            conn.query_row(
                "SELECT
                    (SELECT COALESCE(SUM(COALESCE(billed_cost_usd, estimated_cost_usd)), 0)
                     FROM usage_records
                     WHERE COALESCE(billed_cost_usd, estimated_cost_usd) IS NOT NULL)
                    +
                    (SELECT COALESCE(SUM(cost_usd), 0)
                     FROM usage_ledger
                     WHERE cost_usd IS NOT NULL)",
                [],
                |row| row.get(0),
            )
            .unwrap()
        };
        let old_inventory_count = |conn: &Connection| -> i64 {
            conn.query_row(
                "SELECT (SELECT COUNT(*) FROM usage_records) + (SELECT COUNT(*) FROM usage_ledger)",
                [],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert!((old_binary_sum(&conn) - 0.5).abs() < 1e-9);
        assert_eq!(old_inventory_count(&conn), 1);

        // A rolled-back v10 binary can write the legacy schema without error.
        conn.execute(
            "INSERT INTO usage_ledger(id,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at)
             VALUES('during-rollback','synth','laguna-s',4,6,10,0.25,'2026-08-02T00:00:00Z')",
            [],
        )
        .unwrap();
        assert!((old_binary_sum(&conn) - 0.75).abs() < 1e-9);
        assert_eq!(old_inventory_count(&conn), 2);

        // Relaunching v12 folds and clears staging. The union remains exactly
        // the same, proving neither data loss nor a transient double charge.
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        assert!((old_binary_sum(&conn) - 0.75).abs() < 1e-9);
        assert_eq!(old_inventory_count(&conn), 2);
        let old_inventory_ids: Vec<String> = {
            let mut statement = conn
                .prepare(
                    "SELECT id FROM (
                        SELECT id, created_at FROM usage_records
                        UNION ALL
                        SELECT id, created_at FROM usage_ledger
                     ) ORDER BY created_at DESC, id",
                )
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(
            old_inventory_ids,
            vec!["during-rollback".to_string(), "before-v11".to_string()]
        );
        let (staged, folded): (i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM usage_ledger),
                    (SELECT COUNT(*) FROM usage_records WHERE request_id='legacy-ledger:during-rollback')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((staged, folded), (0, 1));
    }

    /// A realistic v7 history — a large sample backlog and samples with
    /// unreported token counts — comes across whole: every row imported (no
    /// retention trim), unreported counters stay NULL (no silent zeroes), and
    /// generated totals equal input + output where both exist.
    #[test]
    fn migration_8_imports_every_sample_and_never_zeroes_unreported_tokens() {
        let conn = seed_at_version(7);
        conn.execute(
            "INSERT INTO usage_ledger(id,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at)
             VALUES('ledger-1','openrouter','luna',10,20,30,0.5,'2026-08-01T00:00:00Z')",
            [],
        )
        .unwrap();
        // A sample whose provider never reported token counts at all.
        conn.execute(
            "INSERT INTO model_performance_samples(id,provider,model_id,request_id,measurement_kind,status,started_at_ms,completed_at_ms,source,created_at)
             VALUES('perf-null','openrouter','openai/gpt-5.6-luna','req-null','end_to_end','failed',1000,1500,'codex_app_server','2026-08-01T00:00:01Z')",
            [],
        )
        .unwrap();
        for index in 0..1_200 {
            conn.execute(
                "INSERT INTO model_performance_samples(id,provider,model_id,request_id,measurement_kind,status,started_at_ms,completed_at_ms,input_tokens,output_tokens,source,created_at)
                 VALUES(?1,'openrouter','openai/gpt-5.6-luna',?2,'observed_stream','completed',1000,2000,100,50,'codex_app_server','2026-08-01T00:00:02Z')",
                rusqlite::params![format!("perf-{index}"), format!("req-{index}")],
            )
            .unwrap();
        }

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);

        let imported: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            imported, 1_202,
            "history must never be silently trimmed (perf samples + folded ledger)"
        );
        let (null_total, billed, estimated, cost_source): (
            Option<i64>,
            Option<f64>,
            Option<f64>,
            String,
        ) = conn
            .query_row(
                "SELECT total_tokens, billed_cost_usd, estimated_cost_usd, cost_source
                 FROM usage_records WHERE request_id='req-null'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(null_total, None, "unreported tokens must not become zero");
        assert_eq!((billed, estimated), (None, None));
        assert_eq!(cost_source, "none");
        let (totals_ok, ledger_tables, folded): (i64, i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM usage_records WHERE request_id LIKE 'req-%' AND request_id != 'req-null' AND total_tokens = 150),
                        (SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='usage_ledger'),
                        (SELECT COUNT(*) FROM usage_records WHERE request_id='legacy-ledger:ledger-1')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(totals_ok, 1_200);
        assert_eq!(ledger_tables, 1, "rollback staging schema remains");
        assert_eq!(folded, 1, "legacy ledger rows land in usage_records");
    }

    /// Recovery from the pre-transactional failure mode: a crash landed after
    /// migration 8's data copy but before its version stamp, so the app
    /// relaunches at version 7 with `usage_records` already populated.
    /// Replaying the migration must recover — not fail `Database::open`
    /// forever on the copy's primary key.
    #[test]
    fn a_partially_applied_migration_8_recovers_instead_of_wedging_open() {
        let conn = seed_at_version(7);
        conn.execute(
            "INSERT INTO model_performance_samples(id,provider,model_id,request_id,measurement_kind,status,started_at_ms,completed_at_ms,input_tokens,output_tokens,source,created_at)
             VALUES('perf-1','openrouter','openai/gpt-5.6-luna','req-1','observed_stream','completed',1000,2000,100,50,'codex_app_server','2026-08-01T00:00:01Z')",
            [],
        )
        .unwrap();
        // Everything up to (but excluding) the DROP: the create and the copy
        // committed, the version stamp did not.
        let partial = MIGRATION_8
            .split("DROP TABLE")
            .next()
            .expect("migration 8 drops the samples table");
        conn.execute_batch(partial).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 7);

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        let (copies, leftovers): (i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM usage_records WHERE request_id='req-1'),
                        (SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='model_performance_samples')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(copies, 1, "the replayed copy must not duplicate rows");
        assert_eq!(leftovers, 0);
    }

    /// A migration that fails mid-flight must leave no half-applied state
    /// behind: its statements and its version stamp commit atomically.
    #[test]
    fn session_kind_backfills_from_target_json() {
        let conn = seed_at_version(8);
        conn.execute(
            r#"INSERT INTO sessions(id, title, target_json, status, created_at, updated_at)
               VALUES
                 ('codex-1', 'Codex', '{"kind":"codex"}', 'ready', 'now', 'now'),
                 ('local-1', 'Local', '{"kind":"local"}', 'ready', 'now', 'now'),
                 ('intern-1', 'Intern', '{"kind":"intern","mode":"sync"}', 'ready', 'now', 'now')"#,
            [],
        )
        .unwrap();
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        let kinds: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT id, kind FROM sessions ORDER BY id")
                .unwrap();
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(
            kinds,
            vec![
                ("codex-1".into(), "codex".into()),
                ("intern-1".into(), "intern".into()),
                ("local-1".into(), "codex".into()),
            ]
        );
    }

    #[test]
    fn a_failing_migration_leaves_neither_data_nor_version_behind() {
        let conn = seed_at_version(7);
        let poison = "CREATE TABLE wp4_atomicity_probe (id TEXT PRIMARY KEY);\nTHIS IS NOT SQL;";
        let error = apply_one(&conn, 8, poison).expect_err("the batch must fail");
        assert!(error.to_string().contains("apply schema migration 8"));
        let (probe, version): (i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM sqlite_master WHERE name='wp4_atomicity_probe'),
                        (SELECT COALESCE(MAX(version),0) FROM schema_migrations)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(probe, 0, "the failed migration's table must roll back");
        assert_eq!(version, 7);
        // The connection stays usable and the real migration still applies.
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
    }

    #[test]
    fn migration_10_indexes_runtime_target_kind_and_normalizes_cloud() {
        let conn = seed_at_version(9);
        conn.execute(
            "INSERT INTO sessions(id, title, target_json, status, created_at, updated_at)
             VALUES ('cloud-1', 'Cloud', ?1, 'ready', 'now', 'now')",
            [r#"{"kind":"remote","provider":"synth-cloud","model":"openrouter/poolside/laguna-s-2.1","adapter":null}"#],
        )
        .unwrap();
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        let (kind, target): (String, String) = conn
            .query_row(
                "SELECT runtime_target_kind, target_json FROM sessions WHERE id='cloud-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "cloud");
        assert!(target.contains(r#""kind":"cloud""#) || target.contains(r#""kind": "cloud""#));
        let indexed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='sessions_runtime_target_kind'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 1);
    }

    #[test]
    fn migration_14_never_allows_a_partial_upload_permalink() {
        let conn = seed_at_version(13);
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        conn.execute(
            "INSERT INTO visuals(id,current_revision,title,template_id,status,renderer_kind,bindings_json,metadata_json,created_at,updated_at)
             VALUES ('vis-1',1,'Visual','template.v1','saved','template','{}','{}','now','now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO visual_revisions(visual_id,revision,template_id,renderer_kind,bindings_json,created_at)
             VALUES ('vis-1',1,'template.v1','template','{}','now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO visual_seals(receipt_digest,visual_id,visual_revision,artifact_id,schema_version,compiler_name,compiler_version,runtime_digest,index_digest,data_digest,receipt_size_bytes,total_size_bytes,created_at)
             VALUES (?1,'vis-1',1,'visual:vis-1','synth.artifact-bundle.v1','workshop','0.3',?2,?3,?4,10,30,'now')",
            ["a".repeat(64), "b".repeat(64), "c".repeat(64), "d".repeat(64)],
        )
        .unwrap();
        let partial_with_url = conn.execute(
            "INSERT INTO visual_uploads(receipt_digest,state,committed_url,updated_at)
             VALUES (?1,'uploading','https://should-not-exist','now')",
            ["a".repeat(64)],
        );
        assert!(partial_with_url.is_err());
        let committed_without_url = conn.execute(
            "INSERT INTO visual_uploads(receipt_digest,state,updated_at)
             VALUES (?1,'committed','now')",
            ["a".repeat(64)],
        );
        assert!(committed_without_url.is_err());
        conn.execute(
            "INSERT INTO visual_uploads(receipt_digest,state,committed_url,updated_at)
             VALUES (?1,'committed','https://private.example/p','now')",
            ["a".repeat(64)],
        )
        .unwrap();
    }

    #[test]
    fn migration_21_quarantines_the_old_estimate_and_guards_the_new_measurement() {
        let conn = seed_at_version(20);
        conn.execute(
            "INSERT INTO usage_records(id,provider,model_id,request_id,measurement_kind,status,
                started_at_ms,completed_at_ms,input_tokens,output_tokens,observed_output_tps,
                end_to_end_output_tps,cost_source,source,created_at)
             VALUES ('u-1','synth-cloud','laguna-s-2.1','req-1','observed_stream','completed',
                1000,2000,100,200,643.0,12.5,'none','codex_app_server','now')",
            [],
        )
        .unwrap();
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);

        // The 643 tok/s figure is still readable, but it is no longer a
        // measurement and no longer feeds a percentile.
        let (kind, observed, legacy, e2e): (String, Option<f64>, Option<f64>, Option<f64>) = conn
            .query_row(
                "SELECT measurement_kind, observed_output_tps, legacy_observed_output_tps,
                        end_to_end_output_tps FROM usage_records WHERE id='u-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(kind, "legacy_observed_stream_estimate");
        assert_eq!(observed, None);
        assert_eq!(legacy, Some(643.0));
        assert_eq!(e2e, Some(12.5), "unrelated columns survive the rebuild");

        // The old vocabulary is gone; the new one is accepted.
        assert!(conn
            .execute(
                "UPDATE usage_records SET measurement_kind='observed_stream' WHERE id='u-1'",
                []
            )
            .is_err());
        conn.execute(
            "UPDATE usage_records SET measurement_kind='observed_stream_segment' WHERE id='u-1'",
            [],
        )
        .unwrap();

        // A measurement row carries either a rate or a reason it has none.
        let insert = |id: &str, tps: &str, reason: &str| {
            conn.execute(
                &format!(
                    "INSERT INTO generation_speed_measurements(measurement_id,schema_version,
                        measurement_kind,session_id,turn_id,item_id,output_index,content_index,
                        phase,status,tps,exact_tokens_after_first_sample,duration_ms,sample_count,
                        token_count_source,clock_source,unavailable_reason,created_at)
                     VALUES ('{id}','synth.generation-speed.v1','observed_stream_segment',
                        's','t','msg_1',0,0,'final_answer','completed',{tps},60,1200.0,4,
                        'provider_item_usage','workshop_monotonic_receive',{reason},'now')"
                ),
                [],
            )
        };
        assert!(insert("m-rate", "50.0", "NULL").is_ok());
        assert!(insert("m-reason", "NULL", "'insufficient_samples'").is_ok());
        assert!(
            insert("m-both", "50.0", "'insufficient_samples'").is_err(),
            "a row must not carry both a rate and a reason it has none"
        );
        assert!(
            insert("m-neither", "NULL", "NULL").is_err(),
            "a row without a rate must say why"
        );
    }

    #[test]
    fn migration_47_preserves_measurements_and_accepts_full_response_output_usage() {
        let conn = seed_at_version(45);
        conn.execute(
            "INSERT INTO generation_speed_measurements(
                measurement_id,schema_version,measurement_kind,session_id,turn_id,item_id,
                output_index,content_index,phase,status,tps,exact_tokens_after_first_sample,
                duration_ms,sample_count,token_count_source,clock_source,created_at)
             VALUES ('before','synth.generation-speed.v1','observed_stream_segment','s','t',
                'msg-before',0,0,'final_answer','completed',50.0,60,1200.0,4,
                'provider_item_usage','workshop_monotonic_receive','now')",
            [],
        )
        .unwrap();

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        let preserved: f64 = conn
            .query_row(
                "SELECT tps FROM generation_speed_measurements WHERE measurement_id='before'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(preserved, 50.0);

        conn.execute(
            "INSERT INTO generation_speed_measurements(
                measurement_id,schema_version,measurement_kind,session_id,turn_id,item_id,
                output_index,content_index,phase,status,tps,exact_tokens_after_first_sample,
                duration_ms,sample_count,token_count_source,clock_source,created_at)
             VALUES ('after','synth.generation-speed.v1','observed_stream_segment','s','t',
                'msg-after',0,0,'final_answer','completed',56.5,318,5620.0,313,
                'provider_response_output_usage','workshop_monotonic_receive','now')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn migration_48_repairs_legacy_unique_experiment_session_index() {
        let conn = seed_at_version(47);
        conn.execute_batch(
            "DROP INDEX IF EXISTS experiment_groups_session;
             CREATE UNIQUE INDEX experiment_groups_session ON experiment_groups(session_id);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO experiment_groups(id, session_id, title, created_at, updated_at)
             VALUES('exp_a', 'session_shared', 'First', '2026-08-27T00:00:00Z', '2026-08-27T00:00:00Z')",
            [],
        )
        .unwrap();

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        conn.execute(
            "INSERT INTO experiment_groups(id, session_id, title, created_at, updated_at)
             VALUES('exp_b', 'session_shared', 'Second', '2026-08-27T00:01:00Z', '2026-08-27T00:01:00Z')",
            [],
        )
        .expect("migration 48 must permit multiple experiments in one session");
    }

    #[test]
    fn migration_50_installs_the_run_kernel_tables() {
        let conn = seed_at_version(49);
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        for table in [
            "optimizer_run_drafts",
            "optimizer_run_specs",
            "optimizer_algorithm_projections",
            "optimizer_work_items",
            "optimizer_usage_ledger",
            "optimizer_evidence_refs",
            "optimizer_evidence_amendments",
        ] {
            let present: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(present, "{table} must exist after migration 50");
        }
        let lifecycle_present: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('optimizer_runs') WHERE name='lifecycle')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(lifecycle_present);
    }

    #[test]
    fn migration_51_installs_locator_registry_and_source_license_columns() {
        let conn = seed_at_version(50);
        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        let table_present: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='credential_locators')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(table_present);
        for column in ["locator_id", "preferred", "source_state"] {
            let sql = format!(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('secret_refs') WHERE name='{column}')"
            );
            let present: bool = conn.query_row(&sql, [], |row| row.get(0)).unwrap();
            assert!(
                present,
                "secret_refs.{column} must exist after migration 51"
            );
        }
    }

    #[test]
    fn migration_52_moves_campaign_execution_and_request_identity_to_canonical_owners() {
        let conn = seed_at_version(51);
        conn.execute(
            "INSERT INTO eval_campaigns(
                id, session_id, container_id, title, expected_rollouts, max_concurrency,
                policy_ref_json, plan_json, status, created_at, started_at, settled_at
             ) VALUES(
                'campaign_1', 'session_1', 'container_1', 'Legacy eval', 2, 1,
                '{}', '{}', 'complete', '2026-08-27T00:00:00Z',
                '2026-08-27T00:01:00Z', '2026-08-27T00:02:00Z'
             )",
            [],
        )
        .unwrap();
        for (ordinal, reward) in [(1, 0.5), (2, 1.0)] {
            conn.execute(
                "INSERT INTO eval_campaign_rollouts(
                    campaign_id, rollout_id, ordinal, seed, task_instance_id,
                    status, terminal_json, started_at, settled_at
                 ) VALUES(
                    'campaign_1', ?1, ?2, ?2, ?3, 'terminal', ?4,
                    '2026-08-27T00:01:00Z', '2026-08-27T00:02:00Z'
                 )",
                rusqlite::params![
                    format!("campaign_1_r{ordinal:02}"),
                    ordinal,
                    format!("seed:{ordinal}"),
                    serde_json::json!({"reward": reward}).to_string()
                ],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO experiment_groups(
                id, session_id, title, created_at, updated_at
             ) VALUES('exp_1','session_1','Legacy','2026-08-27T00:00:00Z','2026-08-27T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO experiment_group_members(
                group_id, member_kind, member_id, title, attached_at
             ) VALUES('exp_1','direct_evaluation','request_1','Request','2026-08-27T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO experiment_group_members(
                group_id, member_kind, member_id, title, attached_at
             ) VALUES('exp_1','eval_campaign','campaign_1','Legacy eval','2026-08-27T00:00:00Z')",
            [],
        )
        .unwrap();

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        let state = crate::optimizers::kernel::persist::load_state(&conn, "campaign_1")
            .unwrap()
            .expect("migrated campaign must have a durable kernel projection");
        assert_eq!(
            state.algorithm,
            crate::optimizers::kernel::AlgorithmKind::Eval
        );
        assert!(state.lifecycle.is_terminal());
        assert_eq!(state.work_summary().planned, Some(2));
        assert_eq!(state.work_summary().succeeded, Some(2));
        assert_eq!(state.spec_digest, "legacy-campaign:campaign_1");

        let request_id: String = conn
            .query_row(
                "SELECT request_id FROM experiment_groups WHERE id='exp_1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(request_id, "request_1");
        let members: Vec<(String, String)> = conn
            .prepare(
                "SELECT member_kind, member_id FROM experiment_group_members
                 WHERE group_id='exp_1' ORDER BY member_id",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(members, vec![("optimizer_run".into(), "campaign_1".into())]);
        for retired in [
            "eval_campaigns",
            "eval_campaign_rollouts",
            "evaluation_runs",
            "evaluation_rollouts",
            "evaluation_run_drafts",
        ] {
            let sql = format!(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='{retired}')"
            );
            let present: bool = conn.query_row(&sql, [], |row| row.get(0)).unwrap();
            assert!(
                !present,
                "{retired} must be removed after the one-way cutover"
            );
        }
    }

    #[test]
    fn migration_52_refuses_campaign_optimizer_identity_collisions() {
        let conn = seed_at_version(51);
        conn.execute(
            "INSERT INTO optimizer_runs(
                id, algorithm_id, status, source, created_at, payload_json, updated_at
             ) VALUES('shared_id','gepa','queued','local','now','{}','now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO eval_campaigns(
                id, session_id, container_id, title, expected_rollouts, max_concurrency,
                policy_ref_json, plan_json, status, created_at
             ) VALUES('shared_id','session_1','container_1','Legacy eval',1,1,
                '{}','{}','planned','now')",
            [],
        )
        .unwrap();

        let error = format!("{:#}", apply_migrations(&conn).unwrap_err());
        assert!(
            error.contains("migration_52_identity_guard")
                || error.contains("CHECK constraint failed"),
            "unexpected collision failure: {error}"
        );
        let algorithm: String = conn
            .query_row(
                "SELECT algorithm_id FROM optimizer_runs WHERE id='shared_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(algorithm, "gepa", "the colliding run must remain untouched");
    }

    #[test]
    fn migration_54_scopes_work_item_identity_to_its_optimizer_run() {
        let conn = seed_at_version(53);
        let insert_run = "INSERT INTO optimizer_runs(
                id, algorithm_id, status, source, created_at, payload_json, updated_at
             ) VALUES (?1, 'eval', 'queued', 'local', 'now', '{}', 'now')";
        conn.execute(insert_run, ["run-a"]).unwrap();
        conn.execute(insert_run, ["run-b"]).unwrap();
        conn.execute(
            "INSERT INTO optimizer_work_items(
                work_item_id, optimizer_run_id, kind, lifecycle, created_at, updated_at
             ) VALUES('eval:trial:0', 'run-a', 'eval_trial', 'planned', 'now', 'now')",
            [],
        )
        .unwrap();

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        conn.execute(
            "INSERT INTO optimizer_work_items(
                optimizer_run_id, work_item_id, kind, lifecycle, created_at, updated_at
             ) VALUES('run-b', 'eval:trial:0', 'eval_trial', 'planned', 'now', 'now')",
            [],
        )
        .expect("two runs may use the same algorithm-local work-item identity");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM optimizer_work_items WHERE work_item_id='eval:trial:0'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        let duplicate_in_one_run = conn.execute(
            "INSERT INTO optimizer_work_items(
                optimizer_run_id, work_item_id, kind, lifecycle, created_at, updated_at
             ) VALUES('run-b', 'eval:trial:0', 'eval_trial', 'planned', 'now', 'now')",
            [],
        );
        assert!(
            duplicate_in_one_run.is_err(),
            "one run must not contain duplicate work-item identities"
        );
    }

    #[test]
    fn migration_60_backfills_specs_for_pre_admission_local_runs() {
        // Without a spec row `persist_kernel_projection` refuses to rebuild,
        // and without a projection `run_view_v2` has nothing to return — so a
        // pre-admission run replays its whole journal on every open only to
        // fail identically. The backfill is what makes those runs readable at
        // all; it is not a performance nicety.
        let conn = seed_at_version(63);
        for (id, algorithm) in [("gepa-legacy", "gepa"), ("eval-legacy", "eval")] {
            conn.execute(
                "INSERT INTO optimizer_runs(
                    id, algorithm_id, status, source, created_at, payload_json, updated_at
                 ) VALUES(?1,?2,'completed','local','2026-08-16T00:00:00Z','{}',
                          '2026-08-16T00:00:00Z')",
                rusqlite::params![id, algorithm],
            )
            .unwrap();
        }
        // A run whose algorithm the kernel does not recognise must be left
        // alone: synthesizing a spec for it would only move the failure.
        conn.execute(
            "INSERT INTO optimizer_runs(
                id, algorithm_id, status, source, created_at, payload_json, updated_at
             ) VALUES('unknown-legacy','not-an-algorithm','completed','local',
                      '2026-08-16T00:00:00Z','{}','2026-08-16T00:00:00Z')",
            [],
        )
        .unwrap();
        // An already-admitted run keeps the spec it was admitted under.
        conn.execute(
            "INSERT INTO optimizer_runs(
                id, algorithm_id, status, source, created_at, payload_json, updated_at
             ) VALUES('admitted','gepa','completed','local','2026-08-30T00:00:00Z','{}',
                      '2026-08-30T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO optimizer_run_specs(
                optimizer_run_id, spec_json, spec_digest, authorization_json, admitted_at
             ) VALUES('admitted','{}','sha256:real','{}','2026-08-30T00:00:00Z')",
            [],
        )
        .unwrap();

        apply_migrations(&conn).unwrap();

        let digest = |id: &str| -> Option<String> {
            conn.query_row(
                "SELECT spec_digest FROM optimizer_run_specs WHERE optimizer_run_id = ?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .ok()
        };
        assert_eq!(
            digest("gepa-legacy").as_deref(),
            Some("legacy-local:gepa-legacy")
        );
        assert_eq!(
            digest("eval-legacy").as_deref(),
            Some("legacy-local:eval-legacy")
        );
        assert_eq!(
            digest("unknown-legacy"),
            None,
            "unknown algorithms are left alone"
        );
        assert_eq!(
            digest("admitted").as_deref(),
            Some("sha256:real"),
            "a real admitted spec is never overwritten by the backfill"
        );

        // Provenance stays honest: the synthesized rows say they were adopted,
        // not admitted, so a later audit can tell the two apart.
        let authorization: String = conn
            .query_row(
                "SELECT authorization_json FROM optimizer_run_specs WHERE optimizer_run_id='gepa-legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(authorization.contains("pre_admission_local_run_migration"));
        assert!(authorization.contains("not_required"));
    }

    #[test]
    fn migration_58_backfills_query_fields_with_an_explicit_legacy_clock_witness() {
        let conn = seed_at_version(57);
        conn.execute(
            "INSERT INTO optimizer_runs(
                id, algorithm_id, status, source, created_at, payload_json, updated_at
             ) VALUES('eval-legacy','eval','running','container','2026-08-27T00:00:00Z','{}',
                      '2026-08-27T00:00:00Z')",
            [],
        )
        .unwrap();
        let payload = serde_json::json!({
            "schemaVersion": "optimizer_event.v1",
            "eventId": "eval-legacy:7",
            "type": "eval.trial.event",
            "sequenceNumber": 7,
            "occurredAt": "2026-08-27T00:00:01Z",
            "optimizerRunId": "eval-legacy",
            "algorithmId": "eval",
            "delta": {
                "container_event": {
                    "rollout_id": "rollout-780005",
                    "kind": "frame",
                    "occurred_at": "2026-08-27T00:00:00.500Z",
                    "digest": "sha256:producer",
                    "payload": {"step": 42, "span_id": "span-42"}
                }
            },
            "raw": {}
        })
        .to_string();
        conn.execute(
            "INSERT INTO optimizer_events(
                event_id, optimizer_run_id, sequence_number, event_type,
                algorithm_id, occurred_at, payload_json
             ) VALUES('eval-legacy:7','eval-legacy',7,'eval.trial.event','eval',
                      '2026-08-27T00:00:01Z',?1)",
            [payload],
        )
        .unwrap();

        assert_eq!(apply_migrations(&conn).unwrap(), LATEST_VERSION);
        let fields: (
            String,
            String,
            i64,
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT rollout_id, kind, step, span_id, producer_occurred_at,
                        ingested_at, ingest_witness, producer_digest, payload_cas_digest
                 FROM optimizer_events WHERE event_id='eval-legacy:7'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(fields.0, "rollout-780005");
        assert_eq!(fields.1, "frame");
        assert_eq!(fields.2, 42);
        assert_eq!(fields.3, "span-42");
        assert_eq!(fields.4, "2026-08-27T00:00:00.500Z");
        assert_eq!(fields.5, "2026-08-27T00:00:01Z");
        assert_eq!(fields.6, "legacy_producer_clock");
        assert_eq!(fields.7.as_deref(), Some("sha256:producer"));
        assert_eq!(fields.8, None, "legacy replay payload remains inline");

        for index in [
            "optimizer_events_run_kind",
            "optimizer_events_rollout_step",
            "optimizer_events_ingested",
        ] {
            let present: bool = conn
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM sqlite_master WHERE type='index' AND name=?1
                     )",
                    [index],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(present, "migration 58 must create {index}");
        }
    }
}

/// Backfill admitted specs for runs that predate kernel admission.
///
/// `persist_kernel_projection` refuses to rebuild a projection without a spec
/// digest, and `run_view_v2` needs a projection. A local run created before
/// admission existed therefore has neither, cannot acquire either, and fails
/// every read forever — the visual replays its whole journal, throws, rolls
/// back, and retries. On a developer machine carrying runs from before the
/// cutover that is most of the library.
///
/// MIGRATION_46 already did exactly this for `legacy_campaign_migration`
/// rows. This extends the same treatment to the other pre-admission sources,
/// which were simply not present when that migration was written.
///
/// The synthesized spec is deliberately marked as reconstructed rather than
/// admitted: the digest is namespaced `legacy-local:`, and the authorization
/// records `not_required` with a reason. Provenance stays honest about the
/// difference between a run that was admitted under contract and a run that
/// was adopted afterwards, so a later audit can tell them apart.
const MIGRATION_60: &str = r#"
INSERT OR IGNORE INTO optimizer_run_specs(
    optimizer_run_id, spec_json, spec_digest, authorization_json, admitted_at
)
SELECT
    run.id,
    json_object(
        'schemaVersion', 'legacy_local_run_spec.v1',
        'optimizerRunId', run.id,
        'algorithmId', run.algorithm_id,
        'source', run.source,
        'reconstructed', json('true')
    ),
    'legacy-local:' || run.id,
    '{"state":"not_required","reason":"pre_admission_local_run_migration"}',
    run.created_at
FROM optimizer_runs run
LEFT JOIN optimizer_run_specs spec ON spec.optimizer_run_id = run.id
WHERE spec.optimizer_run_id IS NULL
  AND run.algorithm_id IN ('eval','gepa','go-ex','sft','cispo');
"#;

/// Durable proof that a specific visual revision rendered from complete local
/// evidence.
///
/// The invariant this exists for: *once Workshop has successfully rendered a
/// revision, that revision stays viewable after restart and while the producer
/// is unavailable.* The evidence itself needs no new home — the kernel
/// projection is already durable in SQLite and, since reads stopped taking the
/// write lock, is readable in about a millisecond with no producer involved.
/// Copying it into a second table would create a second authority for product
/// truth that can drift from the projection, which is exactly what the kernel
/// invariants forbid.
///
/// What was missing was the *claim*, not the data. "This rendered" lived as
/// untyped JSON on the mutable `optimizer_runs.summary_json` blob, with no
/// revision, no template version, and no digest — so nothing could tell that a
/// reopened visual was being served evidence older than, or different from,
/// what it had already shown. This table makes that claim typed and checkable:
///
///   · identity   — visual, revision, run, and template it was rendered from;
///   · freshness  — the projection revision it was rendered at, so a lower one
///                  is a detectable regression rather than a silent downgrade;
///   · integrity  — a digest of the rendered projection, so the same revision
///                  carrying different content is detectable;
///   · extent     — the journal tail it had replayed through.
///
/// One row per (visual, revision): a re-render of the same revision replaces
/// it, and `projection_revision` never moves backwards.
const MIGRATION_61: &str = r#"
CREATE TABLE IF NOT EXISTS visual_render_receipts (
    visual_id TEXT NOT NULL,
    visual_revision INTEGER NOT NULL,
    optimizer_run_id TEXT NOT NULL,
    template_id TEXT NOT NULL,
    template_version TEXT NOT NULL,
    projection_revision INTEGER NOT NULL,
    data_digest TEXT NOT NULL,
    tail_cursor INTEGER NOT NULL,
    rendered_at TEXT NOT NULL,
    PRIMARY KEY (visual_id, visual_revision)
);

CREATE INDEX IF NOT EXISTS visual_render_receipts_run
ON visual_render_receipts(optimizer_run_id);
"#;

/// Paid Trace V5 annotation: one single-use, bound, expiring reservation per
/// paid job, settled through the conversation budget; plus the per-launch HMAC
/// secret a container verifies reservation tokens with.
const MIGRATION_62: &str = r#"
CREATE TABLE IF NOT EXISTS annotation_reservations (
    reservation_id TEXT PRIMARY KEY,
    approval_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    container_id TEXT NOT NULL,
    binding_digest TEXT NOT NULL,
    trace_digest TEXT NOT NULL,
    annotator_id TEXT NOT NULL,
    reserved_usd_micros INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('issued', 'forwarded', 'settled', 'released', 'expired')),
    settled_usd_micros INTEGER,
    job_id TEXT,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS annotation_reservations_session ON annotation_reservations(session_id, status);
CREATE INDEX IF NOT EXISTS annotation_reservations_job ON annotation_reservations(container_id, job_id);
CREATE TABLE IF NOT EXISTS annotation_broker_secrets (
    container_id TEXT PRIMARY KEY,
    secret TEXT NOT NULL,
    created_at TEXT NOT NULL
);
"#;
