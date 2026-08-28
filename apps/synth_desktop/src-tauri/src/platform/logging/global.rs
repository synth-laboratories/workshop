use std::sync::OnceLock;

use super::record::{LogLevel, LogRecord};
use super::runtime::LogRuntime;
use crate::platform::failure::FailureId;

static LOGS: OnceLock<LogRuntime> = OnceLock::new();

pub fn install(logs: LogRuntime) {
    let _ = LOGS.set(logs);
}

/// Structured operational log. Replaces process-stderr writes once a runtime is installed.
/// Before install, the message is dropped — bootstrap uses the emergency NDJSON sink.
pub fn report(component: &str, event: &str, message: impl Into<String>) {
    report_level(LogLevel::Error, component, event, message, None);
}

pub fn report_info(component: &str, event: &str, message: impl Into<String>) {
    report_level(LogLevel::Info, component, event, message, None);
}

pub fn report_failure(
    component: &str,
    event: &str,
    message: impl Into<String>,
    failure_id: FailureId,
) {
    report_level(LogLevel::Error, component, event, message, Some(failure_id));
}

pub fn report_info_for_failure(
    component: &str,
    event: &str,
    message: impl Into<String>,
    failure_id: FailureId,
) {
    report_level(LogLevel::Info, component, event, message, Some(failure_id));
}

fn report_level(
    level: LogLevel,
    component: &str,
    event: &str,
    message: impl Into<String>,
    failure_id: Option<FailureId>,
) {
    if let Some(logs) = LOGS.get() {
        let mut record = LogRecord::new(level, component, event, message.into());
        record.failure_id = failure_id;
        let _ = logs.emit(record);
    }
}
