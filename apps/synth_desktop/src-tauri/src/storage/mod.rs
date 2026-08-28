//! Durable SQLite + content-addressed storage for the Rust CoreRuntime.

pub mod content_store;
mod database;
mod event_journal;
pub mod generation_speed;
#[path = "../migration/mod.rs"]
pub mod legacy_migration;
mod live_spool;
pub(crate) mod migrations;
mod model_performance;
mod models;
pub mod usage_records;


pub use content_store::ContentStore;
pub use database::{app_data_root, Database, Storage};
pub(crate) use event_journal::append_event;
pub use event_journal::{EventAppend, EventJournal};
pub use generation_speed::{GenerationSpeedRepository, GenerationSpeedRow};
pub use live_spool::{
    envelopes_from_event_log, load_live_spool, persist_live_envelopes, replay_frame_from_envelope,
    LiveSpool, LIVE_SPOOL_SCHEMA, LIVE_SPOOL_SCHEMA_V1,
};
pub use model_performance::{
    MeasurementKind, ModelPerformanceRepository, ModelPerformanceSummary,
    ModelPerformanceTurnSample,
};
pub use models::{
    AppEvent, CommandReceiptRecord, CoreDiagnostics, EventSource, RunRecord, SessionRecord,
    APP_EVENT_SCHEMA_VERSION,
};
pub use usage_records::{
    window_start_ms, CostSource, UsageBreakdown, UsageDayPoint, UsageRecord,
    UsageRecordsRepository, UsageSummary,
};
