use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::platform::failure::definition::FailureId;
use crate::platform::operations::OperationId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "debug" => Self::Debug,
            "warn" => Self::Warn,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    pub log_id: String,
    pub level: LogLevel,
    pub component: String,
    pub event: String,
    pub message: String,
    pub operation_id: Option<OperationId>,
    pub failure_id: Option<FailureId>,
    pub fields: serde_json::Value,
    pub at: DateTime<Utc>,
}

impl LogRecord {
    pub fn new(
        level: LogLevel,
        component: impl Into<String>,
        event: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            log_id: format!("log_{}", uuid::Uuid::new_v4().simple()),
            level,
            component: component.into(),
            event: event.into(),
            message: message.into(),
            operation_id: None,
            failure_id: None,
            fields: serde_json::Value::Null,
            at: Utc::now(),
        }
    }

    pub fn with_operation(mut self, operation_id: OperationId) -> Self {
        self.operation_id = Some(operation_id);
        self
    }

    pub fn with_failure(mut self, failure_id: FailureId) -> Self {
        self.failure_id = Some(failure_id);
        self
    }
}
