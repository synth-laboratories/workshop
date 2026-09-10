use super::migrations::{apply_migrations, schema_version};
use super::models::{CoreDiagnostics, SCHEMA_VERSION};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::task::spawn_blocking;

/// Time spent waiting for a transaction to begin, split by intent.
///
/// This is the number that mattered and that nothing measured. A read that
/// should cost a millisecond cost as long as the producer held its append
/// transaction, and from the renderer that was indistinguishable from a dead
/// producer — so the visual reported the wrong owner for the delay. It is not
/// query time, deserialize time, or IPC time; it is the interval between
/// asking for a transaction and getting one.
///
/// Process-global and lock-free because it is read far less often than it is
/// written: every transaction in the app updates it, and it is projected once
/// per `core_diagnostics` call.
#[derive(Debug, Default)]
pub struct LockWaitCounters {
    pub reads: AtomicU64,
    pub read_wait_us: AtomicU64,
    pub read_wait_max_us: AtomicU64,
    pub writes: AtomicU64,
    pub write_wait_us: AtomicU64,
    pub write_wait_max_us: AtomicU64,
    /// Transactions that gave up rather than acquiring, at either budget.
    pub timeouts: AtomicU64,
}

pub static LOCK_WAIT: LockWaitCounters = LockWaitCounters {
    reads: AtomicU64::new(0),
    read_wait_us: AtomicU64::new(0),
    read_wait_max_us: AtomicU64::new(0),
    writes: AtomicU64::new(0),
    write_wait_us: AtomicU64::new(0),
    write_wait_max_us: AtomicU64::new(0),
    timeouts: AtomicU64::new(0),
};

fn note_wait(read: bool, started: Instant, acquired: bool) {
    if !acquired {
        LOCK_WAIT.timeouts.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let micros = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
    let (count, total, max) = if read {
        (
            &LOCK_WAIT.reads,
            &LOCK_WAIT.read_wait_us,
            &LOCK_WAIT.read_wait_max_us,
        )
    } else {
        (
            &LOCK_WAIT.writes,
            &LOCK_WAIT.write_wait_us,
            &LOCK_WAIT.write_wait_max_us,
        )
    };
    count.fetch_add(1, Ordering::Relaxed);
    total.fetch_add(micros, Ordering::Relaxed);
    max.fetch_max(micros, Ordering::Relaxed);
}

/// Snapshot of [`LOCK_WAIT`], for the diagnostics projection.
pub fn lock_wait_snapshot() -> (u64, u64, u64, u64, u64, u64, u64) {
    (
        LOCK_WAIT.reads.load(Ordering::Relaxed),
        LOCK_WAIT.read_wait_us.load(Ordering::Relaxed),
        LOCK_WAIT.read_wait_max_us.load(Ordering::Relaxed),
        LOCK_WAIT.writes.load(Ordering::Relaxed),
        LOCK_WAIT.write_wait_us.load(Ordering::Relaxed),
        LOCK_WAIT.write_wait_max_us.load(Ordering::Relaxed),
        LOCK_WAIT.timeouts.load(Ordering::Relaxed),
    )
}

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
        let started = Instant::now();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .inspect_err(|_| note_wait(false, started, false))
            .context("begin immediate sqlite transaction")?;
        note_wait(false, started, true);
        let result = f(&tx)?;
        tx.commit().context("commit sqlite transaction")?;
        Ok(result)
    }

    /// A consistent multi-statement **read**.
    ///
    /// `Deferred` is the whole point: the database is in WAL mode, where a
    /// reader takes a snapshot and never contends with the writer. Asking for
    /// `Immediate` — as every write path here correctly does — acquires the
    /// write lock at `BEGIN`, before a single row is read, which serializes
    /// every reader behind the producer that is appending to the run they are
    /// trying to display. A projection read that costs a millisecond then
    /// costs exactly as long as the producer holds its transaction.
    ///
    /// The closure must not write. A `Deferred` transaction that attempts one
    /// upgrades to a write lock mid-transaction and can fail with
    /// `SQLITE_BUSY_SNAPSHOT` rather than waiting, which is the correct and
    /// loud outcome for a read path that was mislabelled.
    pub fn read_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let mut conn = connect_read(&self.path)?;
        let started = Instant::now();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .inspect_err(|_| note_wait(true, started, false))
            .context("begin deferred sqlite read transaction")?;
        note_wait(true, started, true);
        let result = f(&tx)?;
        tx.commit().context("commit sqlite read transaction")?;
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

    /// Async counterpart to [`Database::read_transaction`]. Every read that
    /// spans more than one statement — a projection plus the run row it is
    /// projected against, an evidence page plus the cursor it is bounded by —
    /// belongs here rather than in `run_transaction`.
    pub async fn run_read<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let path = self.path.clone();
        spawn_blocking(move || {
            let mut conn = connect_read(&path)?;
            let started = Instant::now();
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Deferred)
                .inspect_err(|_| note_wait(true, started, false))
                .context("begin deferred sqlite read transaction")?;
            note_wait(true, started, true);
            let result = f(&tx)?;
            tx.commit().context("commit sqlite read transaction")?;
            Ok(result)
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
            let started = Instant::now();
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .inspect_err(|_| note_wait(false, started, false))
                .context("begin immediate sqlite transaction")?;
            note_wait(false, started, true);
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

/// A reader's lock budget.
///
/// Deliberately shorter than a writer's, and shorter than the renderer's stall
/// watchdog (`STALL_TIMEOUT_MS`, 15s in runProgress/subscription.ts). At the
/// writer's 30s, SQLite was still patiently waiting for a lock long after the
/// UI had given up and told the user the producer had stopped answering — a
/// lock wait and a dead producer were indistinguishable to the person looking
/// at the pane. Bounding the reader means a genuine contention timeout
/// surfaces as a database error attributed to its own stage, with watchdog
/// budget left over for real producer silence.
///
/// Writers keep the full 30s: a write that gives up is lost work, and a long
/// checkpoint or a busy import is a reason to wait, not to fail.
const READ_BUSY_TIMEOUT_MS: u32 = 5_000;

fn connect_read(path: &Path) -> Result<Connection> {
    let conn = connect(path)?;
    conn.busy_timeout(std::time::Duration::from_millis(u64::from(
        READ_BUSY_TIMEOUT_MS,
    )))?;
    Ok(conn)
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
                lock_wait: {
                    let (reads, read_us, read_max, writes, write_us, write_max, timeouts) =
                        lock_wait_snapshot();
                    let avg = |total: u64, count: u64| -> i64 {
                        if count == 0 {
                            0
                        } else {
                            (total / count).min(i64::MAX as u64) as i64
                        }
                    };
                    let clamp = |value: u64| value.min(i64::MAX as u64) as i64;
                    super::models::LockWaitDiagnostics {
                        read_transactions: clamp(reads),
                        read_wait_avg_us: avg(read_us, reads),
                        read_wait_max_us: clamp(read_max),
                        write_transactions: clamp(writes),
                        write_wait_avg_us: avg(write_us, writes),
                        write_wait_max_us: clamp(write_max),
                        timeouts: clamp(timeouts),
                    }
                },
            })
        })
    }

    pub fn expected_schema_version() -> i64 {
        SCHEMA_VERSION
    }
}

