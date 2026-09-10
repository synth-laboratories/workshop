use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use crate::platform::failure::FailureRuntime;
use crate::platform::logging::{emergency_sink, LogRuntime, ObservabilityMode};
use crate::storage::Database;

#[derive(Clone)]
pub struct ObservabilityRuntime {
    pub failures: Option<FailureRuntime>,
    pub logs: LogRuntime,
    pub mode: ObservabilityMode,
}

impl ObservabilityRuntime {
    pub fn open(db: Arc<Database>, data_root: PathBuf) -> Result<Self> {
        let logs = LogRuntime::durable(db.clone(), data_root.clone());
        db.with_conn(crate::platform::failure::repository::migrate_historical_failures)?;
        if emergency_sink::exists(&data_root) {
            db.transaction(|conn| logs.import_emergency(conn))?;
        }
        let _ = crate::platform::logging::retention::write_defaults(&data_root);
        crate::platform::logging::install(logs.clone());
        Ok(Self {
            failures: Some(FailureRuntime::new(db)),
            logs,
            mode: ObservabilityMode::Durable,
        })
    }

    pub fn emergency(data_root: PathBuf) -> Self {
        let _ = emergency_sink::write_line(
            &data_root,
            "bootstrap",
            "sqlite_unavailable",
            "SQLite is unavailable; writing bounded emergency NDJSON",
        );
        let logs = LogRuntime::emergency(data_root);
        crate::platform::logging::install(logs.clone());
        Self {
            failures: None,
            logs,
            mode: ObservabilityMode::EmergencyFile,
        }
    }

    pub fn failures(&self) -> Option<&FailureRuntime> {
        self.failures.as_ref()
    }
}
