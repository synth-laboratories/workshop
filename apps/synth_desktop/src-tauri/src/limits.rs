//! Named operational parameters for Synth Desktop.
//!
//! Timeouts, body caps, poll intervals, and TTLs live here so call sites do not
//! invent silent literals or lean on library defaults.

use std::time::Duration;

/// Default reqwest client timeout when a call site does not need a tighter bound.
pub const HTTP_DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Container `/health` + `/info` probe budget (Inventory hydrate + visuals IPC).
pub const CONTAINER_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Re-fetch task catalogs / metadata when older than this.
pub const CONTAINER_METADATA_REFRESH: Duration = Duration::from_secs(300);

/// Short visuals IPC hop to a registered loopback container.
pub const VISUALS_IPC_HOP_TIMEOUT: Duration = Duration::from_secs(3);

/// Longer visuals IPC hop (rollout / dataset pulls).
pub const VISUALS_IPC_ROLL_TIMEOUT: Duration = Duration::from_secs(10);

/// Account snapshot HTTP budget.
pub const ACCOUNT_CLOUD_TIMEOUT: Duration = Duration::from_secs(12);

/// Credential broker upstream (full streamed cloud turn).
pub const CREDENTIAL_UPSTREAM_TIMEOUT: Duration = Duration::from_secs(900);

/// Desktop update manifest fetch.
pub const UPDATE_MANIFEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Device-code / OAuth token exchange.
pub const DEVICE_AUTH_TIMEOUT: Duration = Duration::from_secs(20);

/// Default Intern HTTP client timeout when CoreRuntime configures one.
pub const INTERN_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Model-performance telemetry request.
pub const MODEL_PERFORMANCE_TIMEOUT: Duration = Duration::from_secs(10);

/// Optimizers cloud list/control.
pub const OPTIMIZERS_CLOUD_TIMEOUT: Duration = Duration::from_secs(30);

/// Laguna ready-wait ceiling after sidecar start.
pub const LAGUNA_READY_WAIT: Duration = Duration::from_secs(90);

/// Laguna `/health` probe.
pub const LAGUNA_HEALTH_TIMEOUT: Duration = Duration::from_millis(1200);

/// Laguna generation / chat request.
pub const LAGUNA_INFERENCE_TIMEOUT: Duration = Duration::from_secs(20);

/// Laguna admin control plane.
pub const LAGUNA_ADMIN_TIMEOUT: Duration = Duration::from_millis(2000);

/// Laguna unload / stop request.
pub const LAGUNA_STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Whisper worker idle unload.
pub const WHISPER_IDLE_UNLOAD: Duration = Duration::from_secs(15 * 60);

/// Loopback JSON IPC header cap (visuals + eval driver).
pub const LOOPBACK_MAX_HEADER_BYTES: usize = 32 * 1024;

/// Visuals IPC body cap.
pub const VISUALS_IPC_MAX_BODY_BYTES: usize = 1024 * 1024;

/// Eval driver body cap (larger for trace bundles).
pub const EVAL_DRIVER_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Desktop image preview size cap.
pub const IMAGE_PREVIEW_MAX_BYTES: u64 = 20 * 1024 * 1024;
