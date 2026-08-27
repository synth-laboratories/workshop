use std::sync::OnceLock;

use super::record::{LogLevel, LogRecord};
use super::runtime::LogRuntime;

static LOGS: OnceLock<LogRuntime> = OnceLock::new();

pub fn install(logs: LogRuntime) {
    let _ = LOGS.set(logs);
}

/// Structured operational log. Replaces process-stderr writes once a runtime is installed.
/// Before install, the message is dropped — bootstrap uses the emergency NDJSON sink.
pub fn report(component: &str, event: &str, message: impl Into<String>) {
    report_level(LogLevel::Error, component, event, message);
}

pub fn report_info(component: &str, event: &str, message: impl Into<String>) {
    report_level(LogLevel::Info, component, event, message);
}

fn report_level(level: LogLevel, component: &str, event: &str, message: impl Into<String>) {
    if let Some(logs) = LOGS.get() {
        let _ = logs.emit(LogRecord::new(level, component, event, message.into()));
    }
}
