use anyhow::Result;
use std::path::{Path, PathBuf};

use super::runtime::ObservabilityRuntime;
use crate::platform::logging::emergency_sink;
use crate::storage::{Database, Storage};

/// Open storage, recording an explicit emergency file if SQLite cannot start.
pub fn open_storage(root: impl Into<PathBuf>) -> Result<(Storage, ObservabilityRuntime)> {
    let root = root.into();
    match Storage::open(&root) {
        Ok(storage) => {
            let runtime = ObservabilityRuntime::open(storage.database().clone(), root)?;
            Ok((storage, runtime))
        }
        Err(error) => {
            let _ = emergency_sink::write_line(
                &root,
                "bootstrap",
                "sqlite_unavailable",
                &format!("{error:#}"),
            );
            Err(error)
        }
    }
}

pub fn record_bootstrap_failure(root: &Path, error: &anyhow::Error) {
    let _ = emergency_sink::write_line(
        root,
        "bootstrap",
        "sqlite_unavailable",
        &format!("{error:#}"),
    );
}

pub fn open_database(path: impl Into<PathBuf>) -> Result<Database> {
    Database::open(path)
}
