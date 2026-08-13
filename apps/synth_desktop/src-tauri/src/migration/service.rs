use super::{
    default_legacy_candidates, detect_legacy_database, fingerprint_legacy_state,
    migrate_legacy_database, open_read_only, source_identity, LegacyDetection,
    LegacyMigrationOptions, MigrationReceipt, EXPECTED_TABLES,
};
use crate::storage::Database;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use uuid::Uuid;

const PLAN_TTL_MINUTES: i64 = 10;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LegacyCandidate {
    pub detection: LegacyDetection,
    pub source_fingerprint: Option<String>,
    pub already_migrated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPlan {
    pub confirmation_token: String,
    pub confirmation_phrase: String,
    pub source_database: String,
    pub source_fingerprint: String,
    pub destination_database: String,
    pub backup_directory: String,
    pub expires_at: String,
    #[specta(type = specta_typescript::Unknown)]
    pub estimated_counts: BTreeMap<String, u64>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MigrationApplyRequest {
    pub confirmation_token: String,
    pub confirmation_phrase: String,
}

#[derive(Clone)]
pub struct MigrationService {
    database: Arc<Database>,
    content_root: PathBuf,
    backup_dir: PathBuf,
    pending: Arc<Mutex<HashMap<String, PendingPlan>>>,
}

#[derive(Clone)]
struct PendingPlan {
    source_db: PathBuf,
    source_fingerprint: String,
    confirmation_phrase: String,
    expires_at: DateTime<Utc>,
}

impl MigrationService {
    pub fn new(
        database: Arc<Database>,
        content_root: impl Into<PathBuf>,
        backup_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            database,
            content_root: content_root.into(),
            backup_dir: backup_dir.into(),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn scan_default_candidates(&self) -> Result<Vec<LegacyCandidate>> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            default_legacy_candidates()
                .into_iter()
                .map(|path| service.inspect_candidate(&path))
                .collect()
        })
        .await
        .context("legacy migration scan worker")?
    }

    pub async fn prepare(&self, source_db: PathBuf) -> Result<MigrationPlan> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || service.prepare_sync(source_db))
            .await
            .context("legacy migration prepare worker")?
    }

    pub async fn apply(&self, request: MigrationApplyRequest) -> Result<MigrationReceipt> {
        let pending = {
            let mut plans = self.pending.lock().expect("migration plan lock poisoned");
            plans.retain(|_, plan| plan.expires_at > Utc::now());
            let plan = plans
                .get(&request.confirmation_token)
                .cloned()
                .context("migration confirmation token is invalid or expired")?;
            if request.confirmation_phrase != plan.confirmation_phrase {
                bail!("migration confirmation phrase does not match");
            }
            plans.remove(&request.confirmation_token);
            plan
        };
        let service = self.clone();
        tokio::task::spawn_blocking(move || service.apply_sync(pending))
            .await
            .context("legacy migration apply worker")?
    }

    pub fn cancel(&self, confirmation_token: &str) -> bool {
        self.pending
            .lock()
            .expect("migration plan lock poisoned")
            .remove(confirmation_token)
            .is_some()
    }

    fn inspect_candidate(&self, path: &Path) -> Result<LegacyCandidate> {
        let mut detection = detect_legacy_database(path)?;
        if detection.is_legacy_runtime && legacy_sidecars_present(path) {
            detection.warnings.push(
                "SQLite WAL/SHM sidecars are present. Apply uses a consistent snapshot and imports only from that backup; close the legacy Python runtime first when possible so the confirmed counts cannot change before snapshotting."
                    .into(),
            );
        }
        let fingerprint = detection
            .is_legacy_runtime
            .then(|| fingerprint_legacy_state(path))
            .transpose()?;
        let already_migrated = if detection.is_legacy_runtime {
            self.has_receipt(path)?
        } else {
            false
        };
        Ok(LegacyCandidate {
            detection,
            source_fingerprint: fingerprint,
            already_migrated,
        })
    }

    fn prepare_sync(&self, source_db: PathBuf) -> Result<MigrationPlan> {
        let candidate = self.inspect_candidate(&source_db)?;
        if !candidate.detection.exists {
            bail!("legacy database does not exist: {}", source_db.display());
        }
        if !candidate.detection.is_legacy_runtime {
            bail!("selected file is not a recognized Python local-runtime database");
        }
        if candidate.already_migrated {
            bail!("this legacy database already has a completed migration receipt");
        }
        let source_fingerprint = candidate
            .source_fingerprint
            .context("recognized legacy database has no fingerprint")?;
        let estimated_counts = source_counts(&source_db)?;
        let total: u64 = estimated_counts.values().sum();
        let confirmation_phrase = format!("IMPORT {total} LEGACY RECORDS");
        let confirmation_token = Uuid::new_v4().simple().to_string();
        let expires_at = Utc::now() + Duration::minutes(PLAN_TTL_MINUTES);
        self.pending
            .lock()
            .expect("migration plan lock poisoned")
            .insert(
                confirmation_token.clone(),
                PendingPlan {
                    source_db: source_db.clone(),
                    source_fingerprint: source_fingerprint.clone(),
                    confirmation_phrase: confirmation_phrase.clone(),
                    expires_at,
                },
            );
        Ok(MigrationPlan {
            confirmation_token,
            confirmation_phrase,
            source_database: source_db.display().to_string(),
            source_fingerprint,
            destination_database: self.database.path().display().to_string(),
            backup_directory: self.backup_dir.display().to_string(),
            expires_at: expires_at.to_rfc3339(),
            estimated_counts,
            warnings: candidate.detection.warnings,
        })
    }

    fn apply_sync(&self, pending: PendingPlan) -> Result<MigrationReceipt> {
        let current_fingerprint = fingerprint_legacy_state(&pending.source_db)?;
        if current_fingerprint != pending.source_fingerprint {
            bail!("legacy database changed after confirmation; inspect and confirm again");
        }
        let mut connection = self.database.connect()?;
        migrate_legacy_database(
            &mut connection,
            self.database.path(),
            &LegacyMigrationOptions {
                source_db: pending.source_db,
                backup_dir: self.backup_dir.clone(),
                content_root: self.content_root.clone(),
            },
        )
    }

    fn has_receipt(&self, path: &Path) -> Result<bool> {
        let key = format!("legacy_migration:{}", source_identity(path)?);
        self.database.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM runtime_settings WHERE key = ?1",
                    [key],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
    }
}

fn legacy_sidecars_present(path: &Path) -> bool {
    [
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ]
    .iter()
    .any(|candidate| candidate.exists())
}

fn source_counts(path: &Path) -> Result<BTreeMap<String, u64>> {
    let conn = open_read_only(path)?;
    EXPECTED_TABLES
        .iter()
        .map(|table| {
            let exists = conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
                    [*table],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            let count = if exists {
                let sql = format!("SELECT COUNT(*) FROM {table}");
                conn.query_row(&sql, [], |row| row.get::<_, u64>(0))?
            } else {
                0
            };
            Ok(((*table).to_owned(), count))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use tempfile::tempdir;

    #[tokio::test]
    async fn requires_exact_confirmation_and_consumes_the_plan() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("runtime.sqlite3");
        super::super::tests::legacy_fixture(&source, &dir.path().join("legacy.tsx"));
        let database = Arc::new(Database::open(dir.path().join("rust/synth.sqlite3")).unwrap());
        let service = MigrationService::new(
            database,
            dir.path().join("rust/store"),
            dir.path().join("rust/backups"),
        );
        let plan = service.prepare(source).await.unwrap();
        let wrong = service
            .apply(MigrationApplyRequest {
                confirmation_token: plan.confirmation_token.clone(),
                confirmation_phrase: "IMPORT IT".into(),
            })
            .await
            .unwrap_err();
        assert!(wrong.to_string().contains("phrase"));
        let receipt = service
            .apply(MigrationApplyRequest {
                confirmation_token: plan.confirmation_token.clone(),
                confirmation_phrase: plan.confirmation_phrase,
            })
            .await
            .unwrap();
        assert!(!receipt.already_applied);
        assert!(!service.cancel(&plan.confirmation_token));
    }

    #[tokio::test]
    async fn rejects_a_source_changed_after_confirmation() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("runtime.sqlite3");
        super::super::tests::legacy_fixture(&source, &dir.path().join("legacy.tsx"));
        let database = Arc::new(Database::open(dir.path().join("rust/synth.sqlite3")).unwrap());
        let service = MigrationService::new(
            database,
            dir.path().join("rust/store"),
            dir.path().join("rust/backups"),
        );
        let plan = service.prepare(source.clone()).await.unwrap();
        assert!(!dir.path().join("rust/backups").exists());
        assert_eq!(
            fingerprint_legacy_state(&source).unwrap(),
            plan.source_fingerprint
        );
        rusqlite::Connection::open(source)
            .unwrap()
            .execute("UPDATE sessions SET title='changed' WHERE id='ses_1'", [])
            .unwrap();
        let error = service
            .apply(MigrationApplyRequest {
                confirmation_token: plan.confirmation_token,
                confirmation_phrase: plan.confirmation_phrase,
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("changed after confirmation"));
        assert!(!dir.path().join("rust/backups").exists());
    }
}
