//! Durable SQLite + content-addressed storage for the Rust CoreRuntime.

mod content_store;
mod database;
mod event_journal;
#[path = "../migration/mod.rs"]
pub mod legacy_migration;
mod migrations;
mod model_performance;
mod models;
pub mod usage_records;

#[cfg(test)]
#[path = "contract_tests.rs"]
mod contract_tests;

pub use content_store::ContentStore;
pub use database::{app_data_root, Database, Storage};
pub(crate) use event_journal::append_event;
pub use event_journal::{EventAppend, EventJournal};
pub use model_performance::{MeasurementKind, ModelPerformanceRepository, ModelPerformanceSummary};
pub use usage_records::{
    window_start_ms, CostSource, UsageBreakdown, UsageRecord, UsageRecordsRepository, UsageSummary,
};
pub use models::{
    AppEvent, CommandReceiptRecord, CoreDiagnostics, EventSource, RunRecord, SessionRecord,
    APP_EVENT_SCHEMA_VERSION,
};
