//! One-way import of the retired Python local-runtime SQLite authority.
//!
//! The importer is deliberately separate from schema migrations: it reads a
//! user database, makes a consistent backup, and copies compatible records into
//! the Rust CoreRuntime database without ever changing or deleting the source.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub mod commands;
mod service;

pub use service::{LegacyCandidate, MigrationApplyRequest, MigrationPlan, MigrationService};

const RECEIPT_SCHEMA: &str = "synth.legacy-migration-receipt.v1";
const EXPECTED_TABLES: &[&str] = &[
    "projects",
    "sessions",
    "runs",
    "events",
    "cursors",
    "containers",
    "traces",
    "visuals",
    "usage_ledger",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDetection {
    pub source_path: String,
    pub exists: bool,
    pub is_legacy_runtime: bool,
    pub tables: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct LegacyMigrationOptions {
    pub source_db: PathBuf,
    pub backup_dir: PathBuf,
    /// CoreRuntime's `store` directory (the parent of `blobs`, `traces`, ...).
    pub content_root: PathBuf,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EntityCount {
    #[specta(type = specta_typescript::Number)]
    pub found: u64,
    #[specta(type = specta_typescript::Number)]
    pub imported: u64,
    #[specta(type = specta_typescript::Number)]
    pub existing: u64,
    #[specta(type = specta_typescript::Number)]
    pub skipped: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RollbackMetadata {
    pub source_database: String,
    pub backup_database: String,
    pub receipt_path: String,
    /// IDs inserted by this invocation, grouped by destination table. Deleting
    /// these in reverse dependency order is a manual, auditable rollback.
    pub imported_ids: BTreeMap<String, Vec<String>>,
    pub delete_order: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReceipt {
    pub schema_version: String,
    pub migration_id: String,
    pub source_database: String,
    pub source_fingerprint: String,
    pub destination_database: String,
    pub started_at: String,
    pub completed_at: String,
    pub already_applied: bool,
    pub counts: BTreeMap<String, EntityCount>,
    pub warnings: Vec<String>,
    pub integrity_check: String,
    #[specta(type = specta_typescript::Number)]
    pub foreign_key_violations: u64,
    pub rollback: RollbackMetadata,
}

/// Standard locations used by releases of the Python local-runtime.
pub fn default_legacy_candidates() -> Vec<PathBuf> {
    dirs::home_dir()
        .map(|home| legacy_candidates_for_home(&home))
        .unwrap_or_default()
}

fn legacy_candidates_for_home(home: &Path) -> Vec<PathBuf> {
    let runtime = home.join(".synth-desktop/runtime");
    vec![
        runtime.join("data/runtime.sqlite3"),
        runtime.join("runtime.sqlite3"),
        runtime.join("mcp-data/runtime.sqlite3"),
    ]
}

pub fn detect_legacy_database(path: &Path) -> Result<LegacyDetection> {
    let source_path = path.display().to_string();
    if !path.is_file() {
        return Ok(LegacyDetection {
            source_path,
            exists: false,
            is_legacy_runtime: false,
            tables: Vec::new(),
            warnings: Vec::new(),
        });
    }
    let conn = open_read_only(path)?;
    let mut statement =
        conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name ASC")?;
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let has_sessions = tables.iter().any(|table| table == "sessions");
    let has_legacy_events = table_has_columns(&conn, "events", &["event_kind", "sequence"])?;
    let mut warnings = Vec::new();
    for expected in EXPECTED_TABLES {
        if !tables.iter().any(|table| table == expected) {
            warnings.push(format!(
                "legacy database has no {expected} table; it will be skipped"
            ));
        }
    }
    Ok(LegacyDetection {
        source_path,
        exists: true,
        is_legacy_runtime: has_sessions && has_legacy_events,
        tables,
        warnings,
    })
}

/// Import all compatible legacy records in one destination transaction.
///
/// The destination must already have the current CoreRuntime schema. Existing
/// IDs win, making retries safe. The source is opened read-only and retained.
pub fn migrate_legacy_database(
    destination: &mut Connection,
    destination_path: &Path,
    options: &LegacyMigrationOptions,
) -> Result<MigrationReceipt> {
    let detection = detect_legacy_database(&options.source_db)?;
    if !detection.exists {
        bail!(
            "legacy database does not exist: {}",
            options.source_db.display()
        );
    }
    if !detection.is_legacy_runtime {
        bail!(
            "not a recognized Python local-runtime database: {}",
            options.source_db.display()
        );
    }
    if same_file_path(&options.source_db, destination_path) {
        bail!("source and destination databases must be different");
    }
    ensure_destination_schema(destination)?;

    let source_identity = source_identity(&options.source_db)?;
    let setting_key = format!("legacy_migration:{source_identity}");
    if let Some(raw) = destination
        .query_row(
            "SELECT value_json FROM runtime_settings WHERE key = ?1",
            [&setting_key],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        let mut receipt: MigrationReceipt =
            serde_json::from_str(&raw).context("decode stored legacy migration receipt")?;
        receipt.already_applied = true;
        return Ok(receipt);
    }

    fs::create_dir_all(&options.backup_dir)
        .with_context(|| format!("create backup directory {}", options.backup_dir.display()))?;
    fs::create_dir_all(options.content_root.join("blobs"))?;
    fs::create_dir_all(options.content_root.join("traces"))?;
    let started_at = Utc::now().to_rfc3339();
    let live_source = open_read_only(&options.source_db)?;
    let migration_id = format!("legacy_{}", &source_identity[..24]);
    let backup_path = unique_backup_path(&options.backup_dir, &migration_id);
    consistent_backup(&live_source, &backup_path)?;
    // Fingerprint the consistent SQLite snapshot, not only the main source
    // file: live legacy databases may still have committed pages in WAL.
    let source_fingerprint = fingerprint_file(&backup_path)?;
    // Every imported row comes from the immutable snapshot. Reading the live
    // connection after backup would let concurrent Python writers make the
    // receipt and rollback backup disagree with imported data.
    drop(live_source);
    let source = open_read_only(&backup_path)?;
    let receipt_path = backup_path.with_extension("receipt.json");

    let mut state = ImportState {
        counts: EXPECTED_TABLES
            .iter()
            .map(|table| ((*table).to_owned(), EntityCount::default()))
            .collect(),
        warnings: detection.warnings,
        imported_ids: BTreeMap::new(),
        source_identity: source_identity.clone(),
        source_root: options
            .source_db
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
        content_root: options.content_root.clone(),
    };

    let tx = destination.transaction().context("begin legacy import")?;
    import_projects(&source, &tx, &mut state)?;
    import_sessions(&source, &tx, &mut state)?;
    import_runs(&source, &tx, &mut state)?;
    import_events(&source, &tx, &mut state)?;
    import_cursors(&source, &tx, &mut state)?;
    import_containers(&source, &tx, &mut state)?;
    import_traces(&source, &tx, &mut state)?;
    import_visuals(&source, &tx, &mut state)?;
    import_usage(&source, &tx, &mut state)?;

    let integrity_check: String = tx.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity_check != "ok" {
        bail!("destination integrity check failed: {integrity_check}");
    }
    let foreign_key_violations: u64 = tx
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |_| Ok(()))?
        .count() as u64;
    if foreign_key_violations > 0 {
        bail!("destination has {foreign_key_violations} foreign-key violations");
    }

    let completed_at = Utc::now().to_rfc3339();
    let mut receipt = MigrationReceipt {
        schema_version: RECEIPT_SCHEMA.into(),
        migration_id,
        source_database: options.source_db.display().to_string(),
        source_fingerprint,
        destination_database: destination_path.display().to_string(),
        started_at,
        completed_at: completed_at.clone(),
        already_applied: false,
        counts: state.counts,
        warnings: state.warnings,
        integrity_check,
        foreign_key_violations,
        rollback: RollbackMetadata {
            source_database: options.source_db.display().to_string(),
            backup_database: backup_path.display().to_string(),
            receipt_path: receipt_path.display().to_string(),
            imported_ids: state.imported_ids,
            delete_order: [
                "visual_revisions",
                "visuals",
                "usage_ledger",
                "traces",
                "containers",
                "source_cursors",
                "events",
                "runs",
                "sessions",
                "projects",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        },
    };
    let receipt_json = serde_json::to_string_pretty(&receipt)?;
    tx.execute(
        "INSERT INTO runtime_settings(key, value_json, updated_at) VALUES (?1, ?2, ?3)",
        params![setting_key, receipt_json, completed_at],
    )?;
    tx.execute(
        "INSERT INTO runtime_settings(key, value_json, updated_at) VALUES ('migration_complete', 'true', ?1)
         ON CONFLICT(key) DO UPDATE SET value_json = 'true', updated_at = excluded.updated_at",
        [&receipt.completed_at],
    )?;
    tx.commit().context("commit legacy import")?;

    // The sidecar receipt is intentionally written after commit. A failure here
    // does not invalidate imported data; the authoritative copy is in SQLite.
    if let Err(error) = fs::write(&receipt_path, receipt_json.as_bytes()) {
        receipt.warnings.push(format!(
            "could not write rollback receipt {}: {error}",
            receipt_path.display()
        ));
    }
    Ok(receipt)
}

struct ImportState {
    counts: BTreeMap<String, EntityCount>,
    warnings: Vec<String>,
    imported_ids: BTreeMap<String, Vec<String>>,
    source_identity: String,
    source_root: PathBuf,
    content_root: PathBuf,
}

impl ImportState {
    fn found(&mut self, table: &str) {
        self.counts.entry(table.into()).or_default().found += 1;
    }
    fn imported(&mut self, table: &str, id: String) {
        self.counts.entry(table.into()).or_default().imported += 1;
        self.imported_ids.entry(table.into()).or_default().push(id);
    }
    fn existing(&mut self, table: &str) {
        self.counts.entry(table.into()).or_default().existing += 1;
    }
    fn skipped(&mut self, table: &str, warning: String) {
        self.counts.entry(table.into()).or_default().skipped += 1;
        self.warnings.push(warning);
    }
}

fn import_projects(
    source: &Connection,
    tx: &Transaction<'_>,
    state: &mut ImportState,
) -> Result<()> {
    if !table_exists(source, "projects")? {
        return Ok(());
    }
    let mut stmt = source.prepare("SELECT id,name,path,vcs,metadata_json,created_at,updated_at FROM projects ORDER BY created_at,id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    for row in rows {
        let (id, name, path, vcs, metadata, created, updated) = row?;
        state.found("projects");
        let changed=tx.execute("INSERT OR IGNORE INTO projects(id,name,path,vcs,metadata_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![id,name,path,vcs,valid_json(&metadata,"{}"),created,updated])?;
        if changed == 1 {
            state.imported("projects", id);
        } else {
            state.existing("projects");
        }
    }
    Ok(())
}

fn import_sessions(
    source: &Connection,
    tx: &Transaction<'_>,
    state: &mut ImportState,
) -> Result<()> {
    if !table_exists(source, "sessions")? {
        return Ok(());
    }
    let mut stmt=source.prepare("SELECT id,title,target_json,project_id,remote_id,status,state_generation,latest_cursor,active_run_id,metadata_json,created_at,updated_at FROM sessions ORDER BY created_at,id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, Option<String>>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, String>(10)?,
            r.get::<_, String>(11)?,
        ))
    })?;
    for row in rows {
        let (
            id,
            title,
            target,
            mut project,
            remote,
            status,
            generation,
            cursor,
            active,
            metadata,
            created,
            updated,
        ) = row?;
        state.found("sessions");
        project = valid_fk(
            tx,
            "projects",
            project,
            "sessions",
            &id,
            "project_id",
            state,
        )?;
        let changed=tx.execute("INSERT OR IGNORE INTO sessions(id,title,target_json,project_id,remote_id,codex_thread_id,status,state_generation,latest_cursor,active_run_id,metadata_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,NULL,?6,?7,?8,?9,?10,?11,?12)",params![id,title,valid_json(&target,"{}"),project,remote,status,generation,cursor,active,valid_json(&metadata,"{}"),created,updated])?;
        if changed == 1 {
            state.imported("sessions", id);
        } else {
            state.existing("sessions");
        }
    }
    Ok(())
}

fn import_runs(source: &Connection, tx: &Transaction<'_>, state: &mut ImportState) -> Result<()> {
    if !table_exists(source, "runs")? {
        return Ok(());
    }
    let mut stmt=source.prepare("SELECT id,session_id,mode,status,latest_cursor,checkpoint_json,outcome_json,model,adapter,metadata_json,created_at,started_at,completed_at FROM runs ORDER BY created_at,id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, Option<String>>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, String>(10)?,
            r.get::<_, Option<String>>(11)?,
            r.get::<_, Option<String>>(12)?,
        ))
    })?;
    for row in rows {
        let (
            id,
            session,
            mode,
            status,
            cursor,
            checkpoint,
            outcome,
            model,
            adapter,
            metadata,
            created,
            started,
            completed,
        ) = row?;
        state.found("runs");
        if !id_exists(tx, "sessions", &session)? {
            state.skipped(
                "runs",
                format!("run {id} references missing session {session}"),
            );
            continue;
        }
        let updated = completed
            .clone()
            .or_else(|| started.clone())
            .unwrap_or_else(|| created.clone());
        let changed=tx.execute("INSERT OR IGNORE INTO runs(id,session_id,mode,status,latest_cursor,checkpoint_json,outcome_json,model,adapter,metadata_json,created_at,started_at,completed_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",params![id,session,mode,status,cursor,checkpoint.map(|v|valid_json(&v,"null")),outcome.map(|v|valid_json(&v,"null")),model,adapter,valid_json(&metadata,"{}"),created,started,completed,updated])?;
        if changed == 1 {
            state.imported("runs", id);
        } else {
            state.existing("runs");
        }
    }
    Ok(())
}

fn import_events(source: &Connection, tx: &Transaction<'_>, state: &mut ImportState) -> Result<()> {
    if !table_exists(source, "events")? {
        return Ok(());
    }
    let mut stmt=source.prepare("SELECT session_id,sequence,run_id,source,remote_sequence,event_kind,payload_json,command_id,created_at FROM events ORDER BY created_at,session_id,sequence")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, String>(8)?,
        ))
    })?;
    for row in rows {
        let (session, sequence, mut run, source_name, remote, kind, payload, command, created) =
            row?;
        state.found("events");
        if !id_exists(tx, "sessions", &session)? {
            state.skipped(
                "events",
                format!("event {session}/{sequence} references a missing session"),
            );
            continue;
        }
        run = valid_fk(
            tx,
            "runs",
            run,
            "events",
            &format!("{session}/{sequence}"),
            "run_id",
            state,
        )?;
        let event_id = stable_event_id(&state.source_identity, &session, sequence);
        let changed=tx.execute("INSERT OR IGNORE INTO events(event_id,session_id,session_sequence,run_id,source,kind,payload_json,remote_sequence,command_id,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![event_id,session,sequence,run,normalize_source(&source_name),kind,valid_json(&payload,"{}"),remote,command,created])?;
        if changed == 1 {
            state.imported("events", event_id);
        } else {
            state.existing("events");
        }
    }
    Ok(())
}

fn import_cursors(
    source: &Connection,
    tx: &Transaction<'_>,
    state: &mut ImportState,
) -> Result<()> {
    if !table_exists(source, "cursors")? {
        return Ok(());
    }
    let mut stmt = source
        .prepare("SELECT session_id,source,cursor FROM cursors ORDER BY session_id,source")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (session, source_name, cursor) = row?;
        state.found("cursors");
        if !id_exists(tx, "sessions", &session)? {
            state.skipped(
                "cursors",
                format!("cursor {session}/{source_name} references a missing session"),
            );
            continue;
        }
        let changed = tx.execute(
            "INSERT OR IGNORE INTO source_cursors(session_id,source,cursor) VALUES (?1,?2,?3)",
            params![session, normalize_source(&source_name), cursor],
        )?;
        if changed == 1 {
            state.imported("cursors", format!("{session}:{source_name}"));
        } else {
            state.existing("cursors");
        }
    }
    Ok(())
}

fn import_containers(
    source: &Connection,
    tx: &Transaction<'_>,
    state: &mut ImportState,
) -> Result<()> {
    if !table_exists(source, "containers")? {
        return Ok(());
    }
    let mut stmt=source.prepare("SELECT id,name,location,status,base_url,pool_id,task_family,last_rollout_id,health_json,metadata_json,created_at,updated_at FROM containers ORDER BY created_at,id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, String>(10)?,
            r.get::<_, String>(11)?,
        ))
    })?;
    for row in rows {
        let (
            id,
            name,
            location,
            status,
            url,
            pool,
            family,
            rollout,
            health,
            metadata,
            created,
            updated,
        ) = row?;
        state.found("containers");
        let changed=tx.execute("INSERT OR IGNORE INTO containers(id,name,location,status,base_url,pool_id,task_family,last_rollout_id,health_json,metadata_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![id,name,location,status,url,pool,family,rollout,valid_json(&health,"{}"),valid_json(&metadata,"{}"),created,updated])?;
        if changed == 1 {
            state.imported("containers", id)
        } else {
            state.existing("containers")
        }
    }
    Ok(())
}

fn import_traces(source: &Connection, tx: &Transaction<'_>, state: &mut ImportState) -> Result<()> {
    if !table_exists(source, "traces")? {
        return Ok(());
    }
    let mut stmt=source.prepare("SELECT id,digest,title,source,container_id,session_id,run_id,reward,metrics_json,path,metadata_json,created_at FROM traces ORDER BY created_at,id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<f64>>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, Option<String>>(9)?,
            r.get::<_, String>(10)?,
            r.get::<_, String>(11)?,
        ))
    })?;
    for row in rows {
        let (
            id,
            digest,
            title,
            source_name,
            mut container,
            mut session,
            mut run,
            reward,
            metrics,
            path,
            metadata,
            created,
        ) = row?;
        state.found("traces");
        container = valid_fk(
            tx,
            "containers",
            container,
            "traces",
            &id,
            "container_id",
            state,
        )?;
        session = valid_fk(tx, "sessions", session, "traces", &id, "session_id", state)?;
        run = valid_fk(tx, "runs", run, "traces", &id, "run_id", state)?;
        let imported_path = copy_legacy_content(
            path.as_deref(),
            &state.source_root,
            &state.content_root,
            "traces",
            Some(&digest),
            &id,
            &mut state.warnings,
        )?;
        let metadata = augment_metadata(&metadata, "legacyPath", path.map(Value::String));
        let changed=tx.execute("INSERT OR IGNORE INTO traces(id,digest,title,source,container_id,session_id,run_id,reward,metrics_json,path,metadata_json,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![id,digest,title,source_name,container,session,run,reward,valid_json(&metrics,"[]"),imported_path,metadata,created])?;
        if changed == 1 {
            state.imported("traces", id)
        } else {
            state.existing("traces")
        }
    }
    Ok(())
}

fn import_visuals(
    source: &Connection,
    tx: &Transaction<'_>,
    state: &mut ImportState,
) -> Result<()> {
    if !table_exists(source, "visuals")? {
        return Ok(());
    }
    let mut stmt=source.prepare("SELECT id,template_id,title,bindings_json,tsx_path,metadata_json,created_at,updated_at FROM visuals ORDER BY created_at,id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;
    for row in rows {
        let (id, template, title, bindings, tsx_path, metadata, created, updated) = row?;
        state.found("visuals");
        let content_digest = copy_legacy_content(
            tsx_path.as_deref(),
            &state.source_root,
            &state.content_root,
            "blobs",
            None,
            &id,
            &mut state.warnings,
        )?
        .and_then(|path| {
            Path::new(&path)
                .file_name()
                .map(|v| v.to_string_lossy().into_owned())
        });
        let renderer = if content_digest.is_some() {
            "tsx"
        } else {
            "template"
        };
        let metadata = augment_metadata(&metadata, "legacyTsxPath", tsx_path.map(Value::String));
        let changed=tx.execute("INSERT OR IGNORE INTO visuals(id,current_revision,title,template_id,status,renderer_kind,bindings_json,content_digest,metadata_json,created_at,updated_at) VALUES (?1,1,?2,?3,'saved',?4,?5,?6,?7,?8,?9)",params![id,title,template,renderer,valid_json(&bindings,"{}"),content_digest,metadata,created,updated])?;
        if changed == 1 {
            tx.execute("INSERT INTO visual_revisions(visual_id,revision,template_id,renderer_kind,content_digest,bindings_digest,bindings_json,created_at) VALUES (?1,1,?2,?3,?4,?5,?6,?7)",params![id,template,renderer,content_digest,digest_json_text(&bindings),valid_json(&bindings,"{}"),created])?;
            state.imported("visuals", id)
        } else {
            state.existing("visuals")
        }
    }
    Ok(())
}

fn import_usage(source: &Connection, tx: &Transaction<'_>, state: &mut ImportState) -> Result<()> {
    if !table_exists(source, "usage_ledger")? {
        return Ok(());
    }
    let mut stmt=source.prepare("SELECT id,provider,model,session_id,run_id,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at FROM usage_ledger ORDER BY created_at,id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, Option<f64>>(8)?,
            r.get::<_, String>(9)?,
        ))
    })?;
    for row in rows {
        let (id, provider, model, mut session, mut run, prompt, completion, _total, cost, created) =
            row?;
        state.found("usage_ledger");
        session = valid_fk(
            tx,
            "sessions",
            session,
            "usage_ledger",
            &id,
            "session_id",
            state,
        )?;
        run = valid_fk(tx, "runs", run, "usage_ledger", &id, "run_id", state)?;
        let started_ms = 0_i64;
        let cost_source = if cost.is_some() {
            "provider_reported"
        } else {
            "none"
        };
        let request_id = format!("legacy-ledger:{id}");
        let changed = tx.execute(
            "INSERT OR IGNORE INTO usage_records(
                id, provider, model_id, session_id, run_id, request_id,
                measurement_kind, status, started_at_ms, completed_at_ms,
                input_tokens, output_tokens, billed_cost_usd, estimated_cost_usd,
                cost_source, source, created_at
             ) VALUES (?1,?2,?3,?4,?5,?6,'provider_reported','completed',?7,?7,?8,?9,?10,NULL,?11,'legacy_usage_ledger',?12)",
            params![
                id,
                provider,
                model,
                session,
                run,
                request_id,
                started_ms,
                prompt,
                completion,
                cost,
                cost_source,
                created
            ],
        )?;
        if changed == 1 {
            state.imported("usage_ledger", id)
        } else {
            state.existing("usage_ledger")
        }
    }
    Ok(())
}

fn open_read_only(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open legacy database read-only {}", path.display()))
}

fn ensure_destination_schema(conn: &Connection) -> Result<()> {
    for table in [
        "runtime_settings",
        "projects",
        "sessions",
        "runs",
        "events",
        "source_cursors",
        "containers",
        "traces",
        "visuals",
        "visual_revisions",
        "usage_records",
    ] {
        if !table_exists(conn, table)? {
            bail!("destination CoreRuntime database is missing table {table}");
        }
    }
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn table_has_columns(conn: &Connection, table: &str, expected: &[&str]) -> Result<bool> {
    if !table_exists(conn, table)? {
        return Ok(false);
    }
    let sql = format!("PRAGMA table_info(\"{}\")", table.replace('"', "\"\""));
    let mut stmt = conn.prepare(&sql)?;
    let columns = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(expected
        .iter()
        .all(|item| columns.iter().any(|column| column == item)))
}

fn id_exists(conn: &Connection, table: &str, id: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {table} WHERE id=?1");
    Ok(conn.query_row(&sql, [id], |_| Ok(())).optional()?.is_some())
}

fn valid_fk(
    conn: &Connection,
    table: &str,
    value: Option<String>,
    owner: &str,
    owner_id: &str,
    column: &str,
    state: &mut ImportState,
) -> Result<Option<String>> {
    match value {
        Some(id) if !id_exists(conn, table, &id)? => {
            state.warnings.push(format!(
                "{owner} {owner_id} has missing {column} {id}; imported as null"
            ));
            Ok(None)
        }
        other => Ok(other),
    }
}

fn valid_json(raw: &str, fallback: &str) -> String {
    if serde_json::from_str::<Value>(raw).is_ok() {
        raw.to_owned()
    } else {
        fallback.to_owned()
    }
}

fn augment_metadata(raw: &str, key: &str, value: Option<Value>) -> String {
    let mut object = serde_json::from_str::<Map<String, Value>>(raw).unwrap_or_default();
    if let Some(value) = value {
        object.insert(key.into(), value);
    }
    Value::Object(object).to_string()
}

fn digest_json_text(raw: &str) -> String {
    let normalized = serde_json::from_str::<Value>(raw)
        .unwrap_or_else(|_| json!({}))
        .to_string();
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

fn stable_event_id(source_identity: &str, session: &str, sequence: i64) -> String {
    let raw = format!("{source_identity}:{session}:{sequence}");
    let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
    format!("evt_legacy_{}", &digest[..32])
}

fn normalize_source(source: &str) -> &str {
    match source {
        "remote" => "remote",
        "intern" => "intern",
        "codex" => "codex",
        "system" => "system",
        "mlx" => "mlx",
        "visual" => "visual",
        _ => "local",
    }
}

fn source_identity(path: &Path) -> Result<String> {
    let canonical =
        fs::canonicalize(path).with_context(|| format!("canonicalize {}", path.display()))?;
    Ok(format!(
        "{:x}",
        Sha256::digest(canonical.to_string_lossy().as_bytes())
    ))
}

fn fingerprint_file(path: &Path) -> Result<String> {
    let bytes =
        fs::read(path).with_context(|| format!("read {} for fingerprint", path.display()))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn fingerprint_legacy_state(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
    ] {
        if candidate.is_file() {
            hasher.update(candidate.to_string_lossy().as_bytes());
            hasher.update(
                fs::read(&candidate)
                    .with_context(|| format!("read legacy state file {}", candidate.display()))?,
            );
        }
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn same_file_path(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(a), Ok(b)) => a == b,
        _ => left == right,
    }
}

fn unique_backup_path(dir: &Path, migration_id: &str) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    dir.join(format!("{migration_id}-{stamp}.sqlite3"))
}

fn consistent_backup(source: &Connection, path: &Path) -> Result<()> {
    source
        .execute("VACUUM INTO ?1", [path.to_string_lossy().as_ref()])
        .with_context(|| format!("back up legacy database to {}", path.display()))?;
    Ok(())
}

fn resolve_legacy_path(raw: &str, source_root: &Path) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        source_root.join(path)
    }
}

fn copy_legacy_content(
    raw: Option<&str>,
    source_root: &Path,
    content_root: &Path,
    kind: &str,
    expected_digest: Option<&str>,
    owner_id: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<String>> {
    let Some(raw) = raw.filter(|v| !v.trim().is_empty()) else {
        return Ok(None);
    };
    let source = resolve_legacy_path(raw, source_root);
    if !source.is_file() {
        warnings.push(format!(
            "{owner_id} content file is missing: {}",
            source.display()
        ));
        return Ok(None);
    }
    let bytes = fs::read(&source)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if let Some(expected) = expected_digest {
        if !expected.eq_ignore_ascii_case(&digest) {
            warnings.push(format!(
                "{owner_id} content digest mismatch: recorded {expected}, actual {digest}"
            ));
        }
    }
    let destination = content_root.join(kind).join(&digest[..2]).join(&digest);
    if !destination.exists() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = destination.with_extension("migration.tmp");
        fs::write(&temp, &bytes)?;
        fs::rename(&temp, &destination)?;
    }
    Ok(Some(destination.display().to_string()))
}

