use super::migrations::{apply_migrations, schema_version};
use super::models::{CoreDiagnostics, SCHEMA_VERSION};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::task::spawn_blocking;

pub fn app_data_root() -> PathBuf {
    crate::instance::data_root()
}

pub struct Database {
    path: PathBuf,
}

impl Database {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create db dir {}", parent.display()))?;
        }
        let conn = connect(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        apply_migrations(&conn)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connect(&self) -> Result<Connection> {
        connect(&self.path)
    }

    pub fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let conn = self.connect()?;
        f(&conn)
    }

    pub fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let mut conn = self.connect()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("begin immediate sqlite transaction")?;
        let result = f(&tx)?;
        tx.commit().context("commit sqlite transaction")?;
        Ok(result)
    }

    pub async fn run<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let path = self.path.clone();
        spawn_blocking(move || {
            let conn = connect(&path)?;
            f(&conn)
        })
        .await
        .context("database worker join")?
    }

    pub async fn run_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let path = self.path.clone();
        spawn_blocking(move || {
            let mut conn = connect(&path)?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .context("begin immediate sqlite transaction")?;
            let result = f(&tx)?;
            tx.commit().context("commit sqlite transaction")?;
            Ok(result)
        })
        .await
        .context("database worker join")?
    }

    pub fn integrity_ok(&self) -> Result<bool> {
        self.with_conn(|conn| {
            let value: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
            Ok(value == "ok")
        })
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.with_conn(schema_version)
    }
}

fn connect(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("open sqlite {}", path.display()))?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=30000;",
    )?;
    Ok(conn)
}

#[derive(Clone)]
pub struct Storage {
    db: Arc<Database>,
    content_root: PathBuf,
}

impl Storage {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root).with_context(|| format!("create app data {}", root.display()))?;
        let content_root = root.join("store");
        fs::create_dir_all(content_root.join("blobs"))?;
        fs::create_dir_all(content_root.join("previews"))?;
        fs::create_dir_all(content_root.join("traces"))?;
        fs::create_dir_all(content_root.join("exports"))?;
        fs::create_dir_all(root.join("logs"))?;
        let db = Arc::new(Database::open(root.join("synth.sqlite3"))?);
        Ok(Self { db, content_root })
    }

    pub fn open_default() -> Result<Self> {
        Self::open(app_data_root())
    }

    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    pub fn content_root(&self) -> &Path {
        &self.content_root
    }

    pub fn diagnostics(&self) -> Result<CoreDiagnostics> {
        let integrity_ok = self.db.integrity_ok()?;
        let schema = self.db.schema_version()?;
        self.db.with_conn(|conn| {
            let journal_head: i64 = conn
                .query_row("SELECT COALESCE(MAX(sequence), 0) FROM events", [], |row| {
                    row.get(0)
                })
                .optional()?
                .unwrap_or(0);
            let session_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
            let run_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))?;
            let visual_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM visuals", [], |row| row.get(0))?;
            let migration_complete: bool = conn
                .query_row(
                    "SELECT value_json FROM runtime_settings WHERE key = 'migration_complete'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|value| value == "true")
                .unwrap_or(false);
            Ok(CoreDiagnostics {
                database_path: self.db.path().display().to_string(),
                schema_version: schema,
                integrity_ok,
                content_store_path: self.content_root.display().to_string(),
                journal_head,
                session_count,
                run_count,
                visual_count,
                migration_complete,
            })
        })
    }

    pub fn expected_schema_version() -> i64 {
        SCHEMA_VERSION
    }
}

