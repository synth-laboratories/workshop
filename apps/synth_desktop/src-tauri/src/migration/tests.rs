use super::*;
use crate::storage::Database;
use tempfile::tempdir;

pub(super) fn legacy_fixture(path: &Path, content_path: &Path) {
    fs::write(
        content_path,
        "export default function Legacy(){ return null }",
    )
    .unwrap();
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(r#"
      PRAGMA foreign_keys=OFF;
      CREATE TABLE projects(id TEXT PRIMARY KEY,name TEXT NOT NULL,path TEXT NOT NULL UNIQUE,vcs TEXT,metadata_json TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
      CREATE TABLE sessions(id TEXT PRIMARY KEY,title TEXT NOT NULL,target_json TEXT NOT NULL,project_id TEXT,remote_id TEXT,status TEXT NOT NULL,state_generation INTEGER,latest_cursor INTEGER NOT NULL,active_run_id TEXT,metadata_json TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
      CREATE TABLE runs(id TEXT PRIMARY KEY,session_id TEXT NOT NULL,mode TEXT NOT NULL,status TEXT NOT NULL,latest_cursor INTEGER NOT NULL,checkpoint_json TEXT,outcome_json TEXT,model TEXT,adapter TEXT,metadata_json TEXT NOT NULL,created_at TEXT NOT NULL,started_at TEXT,completed_at TEXT);
      CREATE TABLE events(session_id TEXT NOT NULL,sequence INTEGER NOT NULL,run_id TEXT,source TEXT NOT NULL,remote_sequence INTEGER,event_kind TEXT NOT NULL,payload_json TEXT NOT NULL,command_id TEXT,created_at TEXT NOT NULL,PRIMARY KEY(session_id,sequence));
      CREATE TABLE cursors(session_id TEXT NOT NULL,source TEXT NOT NULL,cursor INTEGER NOT NULL,PRIMARY KEY(session_id,source));
      CREATE TABLE containers(id TEXT PRIMARY KEY,name TEXT NOT NULL,location TEXT NOT NULL,status TEXT NOT NULL,base_url TEXT,pool_id TEXT,task_family TEXT,last_rollout_id TEXT,health_json TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,metadata_json TEXT NOT NULL);
      CREATE TABLE traces(id TEXT PRIMARY KEY,digest TEXT NOT NULL UNIQUE,title TEXT NOT NULL,source TEXT NOT NULL,container_id TEXT,session_id TEXT,run_id TEXT,reward REAL,metrics_json TEXT NOT NULL,created_at TEXT NOT NULL,path TEXT,metadata_json TEXT NOT NULL);
      CREATE TABLE visuals(id TEXT PRIMARY KEY,template_id TEXT NOT NULL,title TEXT NOT NULL,bindings_json TEXT NOT NULL,tsx_path TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,metadata_json TEXT NOT NULL);
      CREATE TABLE usage_ledger(id TEXT PRIMARY KEY,provider TEXT NOT NULL,model TEXT NOT NULL,session_id TEXT,run_id TEXT,prompt_tokens INTEGER NOT NULL,completion_tokens INTEGER NOT NULL,total_tokens INTEGER NOT NULL,cost_usd REAL,created_at TEXT NOT NULL);
      INSERT INTO projects VALUES('proj_1','Workshop','/tmp/workshop','git','{}','2026-01-01','2026-01-01');
      INSERT INTO sessions VALUES('ses_1','Legacy','{"kind":"intern","mode":"sync"}','proj_1','remote_1','ready',2,1,'run_1','{}','2026-01-01','2026-01-02');
      INSERT INTO runs VALUES('run_1','ses_1','sync','completed',1,NULL,'{"ok":true}',NULL,NULL,'{}','2026-01-01','2026-01-01','2026-01-02');
      INSERT INTO events VALUES('ses_1',1,'run_1','intern',9,'message.completed','{"text":"hello"}',NULL,'2026-01-01T00:00:01Z');
      INSERT INTO cursors VALUES('ses_1','intern',9);
      INSERT INTO containers VALUES('ctr_1','Craftax','local','ready','http://127.0.0.1:8098',NULL,'craftax',NULL,'{}','2026-01-01','2026-01-01','{}');
      INSERT INTO visuals VALUES('vis_1','eval.overview.v1','Overview','{}',NULL,'2026-01-01','2026-01-01','{}');
      INSERT INTO usage_ledger VALUES('use_1','openrouter','model','ses_1','run_1',10,4,14,0.02,'2026-01-01');
      INSERT INTO usage_ledger VALUES('use_orphan','openrouter','model','missing',NULL,1,1,2,NULL,'2026-01-01');
    "#).unwrap();
    let tsx = content_path.display().to_string();
    conn.execute("UPDATE visuals SET tsx_path=?1 WHERE id='vis_1'", [tsx])
        .unwrap();
    let trace_bytes = b"{\"reward\":1}";
    let digest = format!("{:x}", Sha256::digest(trace_bytes));
    let trace_path = path.parent().unwrap().join("trace.json");
    fs::write(&trace_path, trace_bytes).unwrap();
    conn.execute("INSERT INTO traces VALUES('tr_1',?1,'Trace','local','ctr_1','ses_1','run_1',1.0,'[]','2026-01-01',?2,'{}')",params![digest,trace_path.display().to_string()]).unwrap();
}

#[test]
fn detects_only_legacy_shape() {
    let dir = tempdir().unwrap();
    let missing = detect_legacy_database(&dir.path().join("missing.sqlite3")).unwrap();
    assert!(!missing.exists);
    let random = dir.path().join("random.sqlite3");
    Connection::open(&random)
        .unwrap()
        .execute("CREATE TABLE sessions(id TEXT)", [])
        .unwrap();
    assert!(!detect_legacy_database(&random).unwrap().is_legacy_runtime);
    let legacy = dir.path().join("runtime.sqlite3");
    legacy_fixture(&legacy, &dir.path().join("legacy.tsx"));
    let detected = detect_legacy_database(&legacy).unwrap();
    assert!(detected.is_legacy_runtime);
    assert!(detected.tables.contains(&"visuals".into()));
}

#[test]
fn default_discovery_prefers_the_live_data_directory() {
    let home = Path::new("/test/home");
    assert_eq!(
        legacy_candidates_for_home(home),
        vec![
            home.join(".synth-desktop/runtime/data/runtime.sqlite3"),
            home.join(".synth-desktop/runtime/runtime.sqlite3"),
            home.join(".synth-desktop/runtime/mcp-data/runtime.sqlite3"),
        ]
    );
}

#[test]
fn imports_every_compatible_domain_and_is_idempotent() {
    let dir = tempdir().unwrap();
    let legacy = dir.path().join("runtime.sqlite3");
    legacy_fixture(&legacy, &dir.path().join("legacy.tsx"));
    let rust_root = dir.path().join("rust");
    let database = Database::open(rust_root.join("synth.sqlite3")).unwrap();
    let mut target = database.connect().unwrap();
    let options = LegacyMigrationOptions {
        source_db: legacy.clone(),
        backup_dir: rust_root.join("backups"),
        content_root: rust_root.join("store"),
    };
    let first = migrate_legacy_database(&mut target, database.path(), &options).unwrap();
    assert!(!first.already_applied);
    assert_eq!(first.integrity_check, "ok");
    assert_eq!(first.foreign_key_violations, 0);
    assert_eq!(first.counts["events"].imported, 1);
    assert_eq!(first.counts["visuals"].imported, 1);
    assert!(Path::new(&first.rollback.backup_database).is_file());
    assert!(Path::new(&first.rollback.receipt_path).is_file());
    assert!(legacy.is_file());
    let counts:(i64,i64,i64,i64,i64,i64,i64,i64,i64)=target.query_row("SELECT (SELECT count(*) FROM projects),(SELECT count(*) FROM sessions),(SELECT count(*) FROM runs),(SELECT count(*) FROM events),(SELECT count(*) FROM source_cursors),(SELECT count(*) FROM containers),(SELECT count(*) FROM traces),(SELECT count(*) FROM visuals),(SELECT count(*) FROM usage_ledger)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).unwrap();
    assert_eq!(counts, (1, 1, 1, 1, 1, 1, 1, 1, 2));
    let orphan_session: Option<String> = target
        .query_row(
            "SELECT session_id FROM usage_ledger WHERE id='use_orphan'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(orphan_session.is_none());
    let content_digest: String = target
        .query_row(
            "SELECT content_digest FROM visuals WHERE id='vis_1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(rust_root
        .join("store/blobs")
        .join(&content_digest[..2])
        .join(content_digest)
        .is_file());
    let second = migrate_legacy_database(&mut target, database.path(), &options).unwrap();
    assert!(second.already_applied);
    let event_count: i64 = target
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(event_count, 1);
}

#[test]
fn refuses_source_destination_alias() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("same.sqlite3");
    legacy_fixture(&path, &dir.path().join("legacy.tsx"));
    let mut conn = Connection::open(&path).unwrap();
    let options = LegacyMigrationOptions {
        source_db: path.clone(),
        backup_dir: dir.path().join("backup"),
        content_root: dir.path().join("store"),
    };
    let error = migrate_legacy_database(&mut conn, &path, &options).unwrap_err();
    assert!(error.to_string().contains("must be different"));
}

#[test]
fn rolls_back_all_database_rows_when_a_late_table_is_malformed() {
    let dir = tempdir().unwrap();
    let legacy = dir.path().join("runtime.sqlite3");
    legacy_fixture(&legacy, &dir.path().join("legacy.tsx"));
    let legacy_conn = Connection::open(&legacy).unwrap();
    legacy_conn
        .execute_batch("DROP TABLE containers; CREATE TABLE containers(id TEXT); INSERT INTO containers VALUES ('bad');")
        .unwrap();

    let rust_root = dir.path().join("rust");
    let database = Database::open(rust_root.join("synth.sqlite3")).unwrap();
    let mut target = database.connect().unwrap();
    let options = LegacyMigrationOptions {
        source_db: legacy.clone(),
        backup_dir: rust_root.join("backups"),
        content_root: rust_root.join("store"),
    };
    assert!(migrate_legacy_database(&mut target, database.path(), &options).is_err());
    let sessions: i64 = target
        .query_row("SELECT count(*) FROM sessions", [], |row| row.get(0))
        .unwrap();
    let settings: i64 = target
        .query_row(
            "SELECT count(*) FROM runtime_settings WHERE key LIKE 'legacy_migration:%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(sessions, 0);
    assert_eq!(settings, 0);
    assert!(legacy.is_file());
}

#[test]
fn backup_and_import_include_commits_still_in_a_live_wal() {
    let dir = tempdir().unwrap();
    let legacy = dir.path().join("runtime.sqlite3");
    legacy_fixture(&legacy, &dir.path().join("legacy.tsx"));
    let live_writer = Connection::open(&legacy).unwrap();
    live_writer
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    live_writer
        .pragma_update(None, "wal_autocheckpoint", 0)
        .unwrap();
    live_writer
        .execute(
            "INSERT INTO events VALUES('ses_1',2,'run_1','intern',10,'message.completed','{}',NULL,'2026-01-01T00:00:02Z')",
            [],
        )
        .unwrap();
    assert!(PathBuf::from(format!("{}-wal", legacy.display())).is_file());

    let rust_root = dir.path().join("rust");
    let database = Database::open(rust_root.join("synth.sqlite3")).unwrap();
    let mut target = database.connect().unwrap();
    let receipt = migrate_legacy_database(
        &mut target,
        database.path(),
        &LegacyMigrationOptions {
            source_db: legacy,
            backup_dir: rust_root.join("backups"),
            content_root: rust_root.join("store"),
        },
    )
    .unwrap();
    assert_eq!(receipt.counts["events"].imported, 2);
    let snapshot = Connection::open_with_flags(
        &receipt.rollback.backup_database,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let snapshot_events: i64 = snapshot
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(snapshot_events, 2);
    drop(live_writer);
}
