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

/// Maximum accepted age of a container health + capability observation before
/// `rollouts.prepare` refuses and asks for `container_probe`. Deliberately
/// wider than `CONTAINER_METADATA_REFRESH` so a record that is merely due for
/// its next `/info` refresh is not reported as stale.
pub const CONTAINER_CAPABILITY_MAX_AGE: Duration = Duration::from_secs(900);

/// Short visuals IPC hop to a registered loopback container.
pub const VISUALS_IPC_HOP_TIMEOUT: Duration = Duration::from_secs(3);

/// Longer visuals IPC hop (rollout / dataset pulls).
pub const VISUALS_IPC_ROLL_TIMEOUT: Duration = Duration::from_secs(10);

/// End-to-end live policy rollout budget. Containers may make several
/// sequential provider calls (each with its own bounded timeout) while the
/// subscribed visual continues to receive partial trace and frame events.
/// This must not reuse the short dataset/engine hop timeout or a successful
/// paid rollout will be reported to the MCP caller as a transport failure.
pub const CONTAINER_POLICY_ROLLOUT_TIMEOUT: Duration = Duration::from_secs(900);

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

/// Optimizer sidecar `/health` probe.
pub const OPTIMIZER_SIDECAR_HEALTH_TIMEOUT: Duration = Duration::from_millis(1200);

/// Optimizer sidecar ready-wait after start.
// A freshly staged local optimizer project may need to compile its native
// extension on first launch. Keep this bounded, but allow cold starts to
// complete instead of repeatedly killing the build at five seconds.
pub const OPTIMIZER_SIDECAR_READY_WAIT: Duration = Duration::from_secs(60);

/// How long a spawned recipe child may run without its run becoming visible to
/// the optimizer service the host polls.
// The producer registers its durable index shortly after spawn, so this only
// has to outlast that registration. Past it the run is not merely slow: the
// child is writing somewhere the polled service cannot see, and every further
// second is paid rollouts whose events can never be ingested. Bounded here so
// that a contract failure costs a known amount instead of a whole run.
pub const OPTIMIZER_RUN_INDEX_WAIT: Duration = Duration::from_secs(90);

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

/// Sealed trace artifact cap for a container import. Above this a trace belongs
/// in a bundle the user moves deliberately, not in a loopback fetch.
pub const MAX_IMPORTED_TRACE_BYTES: u64 = 256 * 1024 * 1024;
