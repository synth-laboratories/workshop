use anyhow::Result;
use chrono::{Duration, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicy {
    pub unresolved_failures: bool,
    pub terminal_failure_days: i64,
    pub warning_days: i64,
    pub info_days: i64,
    pub debug_hours: i64,
    pub ceiling_bytes: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            unresolved_failures: true,
            terminal_failure_days: 90,
            warning_days: 30,
            info_days: 7,
            debug_hours: 24,
            ceiling_bytes: 250 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionReceipt {
    pub trimmed_logs: i64,
    pub trimmed_failures: i64,
    pub at: String,
}

pub fn trim(conn: &Connection, policy: &RetentionPolicy) -> Result<RetentionReceipt> {
    let now = Utc::now();
    let debug_before = (now - Duration::hours(policy.debug_hours)).to_rfc3339();
    let info_before = (now - Duration::days(policy.info_days)).to_rfc3339();
    let warn_before = (now - Duration::days(policy.warning_days)).to_rfc3339();
    let terminal_before = (now - Duration::days(policy.terminal_failure_days)).to_rfc3339();
    let trimmed_logs = conn.execute(
        "DELETE FROM log_records WHERE
            (level = 'debug' AND at < ?1) OR
            (level = 'info' AND at < ?2) OR
            (level = 'warn' AND at < ?3)",
        params![debug_before, info_before, warn_before],
    )? as i64;
    let trimmed_failures = conn.execute(
        "DELETE FROM failure_occurrences WHERE lifecycle_state IN ('resolved','terminalized','superseded')
         AND updated_at < ?1",
        params![terminal_before],
    )? as i64;
    Ok(RetentionReceipt {
        trimmed_logs,
        trimmed_failures,
        at: now.to_rfc3339(),
    })
}

pub fn write_defaults(data_root: &std::path::Path) -> Result<RetentionPolicy> {
    let policy = RetentionPolicy::default();
    let path = data_root.join("observability.toml");
    if !path.exists() {
        std::fs::create_dir_all(data_root)?;
        std::fs::write(
            path,
            r#"# Workshop observability defaults. See notes/specifications/workshop/failure_runtime.md
[retention]
unresolved_failures = true
terminal_failure_days = 90
warning_days = 30
info_days = 7
debug_hours = 24
ceiling_bytes = 262144000
"#,
        )?;
    }
    Ok(policy)
}
