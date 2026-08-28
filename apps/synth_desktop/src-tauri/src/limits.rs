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
// NanoHorizon may retry a transient provider limit up to 17 times with a
// declared bounded backoff. Keep the host request alive through the run
// capability's one-hour authority window; call and spend ceilings still fail
// closed independently.
pub const CONTAINER_POLICY_ROLLOUT_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Native scope used by the DeepSWE Harbor task's 5400-second agent bound.
/// Keeping this value in the host makes the approval disclosure, issued
/// capability, and credential-proxy stream use the same lifetime instead of
/// allowing post-expiry 401s.
pub const DEEPSWE_HARBOR_CAPABILITY_TTL_SECONDS: u32 = 5_400;
pub const DEEPSWE_HARBOR_CAPABILITY_TTL: Duration =
    Duration::from_secs(DEEPSWE_HARBOR_CAPABILITY_TTL_SECONDS as u64);

/// Lifetime of a Workshop-owned (host) approval sheet that requires a human.
///
/// Paid compute, credential access, and container lifecycle sheets are not
/// bound to the agent turn that requested them: the waiter is a Workshop task,
/// not the provider's JSON-RPC peer. Tying them to `turn/completed` gave the
/// operator only the seconds between the model finishing its sentence and the
/// turn closing, which is not enough time to read a digest-bound disclosure.
/// The sheet now lives on its own clock, and `evaluation_start` reports
/// `approval_expired` when that clock runs out.
pub const HOST_APPROVAL_LIFETIME: Duration = Duration::from_secs(900);

/// Contractual floor for [`HOST_APPROVAL_LIFETIME`]. An approval window shorter
/// than this cannot be clicked reliably through an accessibility refresh.
pub const HOST_APPROVAL_LIFETIME_FLOOR: Duration = Duration::from_secs(60);

/// Environment variable carrying the declared source revision into a launched
/// workspace container.
///
/// The container echoes it back on `/info` as its loaded runtime revision.
/// That is what makes staleness detectable: a process from an earlier launch
/// still carries the earlier declaration's value.
pub const CONTAINER_SOURCE_REVISION_ENV: &str = "SYNTH_CONTAINER_SOURCE_REVISION";

/// Account snapshot HTTP budget.
pub const ACCOUNT_CLOUD_TIMEOUT: Duration = Duration::from_secs(12);

/// Credential broker upstream (full streamed cloud turn).
///
/// DeepSWE's Harbor process may legitimately hold one Luna request for the
/// full task lifetime, so this must stay aligned with the approved capability
/// rather than the short default HTTP timeout.
pub const CREDENTIAL_UPSTREAM_TIMEOUT: Duration = DEEPSWE_HARBOR_CAPABILITY_TTL;

/// Floor between provider request starts for one capability.
///
/// This is where a capability's pacer *starts*, not where it stays. A fixed
/// 370-second floor — the cadence one observed DeepSWE trace happened to
/// achieve under token-per-minute pressure — is not a property of the route;
/// it is a property of that transcript at that size. Charging every capability
/// that cadence from its first call makes an approved 80-call ceiling
/// unreachable inside any lifetime an operator would approve, because
/// `1 + 5400/370` is fifteen.
///
/// So the floor is small and the provider sets the real cadence: a 429 raises
/// this capability's interval to whatever `Retry-After` / `X-RateLimit-Reset`
/// actually says, and a clean completion decays it back toward the floor. See
/// [`CREDENTIAL_UPSTREAM_MAX_INTERVAL`].
pub const CREDENTIAL_UPSTREAM_MIN_INTERVAL: Duration = Duration::from_secs(6);

/// Ceiling for one capability's adaptively-raised request interval.
///
/// Bounded so a provider that reports an absurd reset cannot park a run for
/// longer than its own lifetime, which would spend the whole window waiting.
pub const CREDENTIAL_UPSTREAM_MAX_INTERVAL: Duration = Duration::from_secs(600);

/// How much of the raised interval survives one clean completion.
///
/// Decay rather than reset: a single admitted request is evidence the pressure
/// eased, not evidence it is gone. Three quarters walks 370 seconds back to the
/// floor over roughly fifteen successes.
pub const CREDENTIAL_UPSTREAM_INTERVAL_DECAY_NUMERATOR: u32 = 3;
pub const CREDENTIAL_UPSTREAM_INTERVAL_DECAY_DENOMINATOR: u32 = 4;

/// Provider calls a capability can actually send within `lifetime` when every
/// request start is separated by `interval`.
///
/// The first call is free — it starts at t=0 — and each later one costs one
/// full pacing interval. A declared call ceiling above this number is not a
/// ceiling anyone can reach, so admission refuses the pair rather than letting
/// a run discover the shortfall at call sixteen.
pub const fn realizable_provider_calls(lifetime: Duration, interval: Duration) -> u32 {
    let interval_secs = interval.as_secs();
    if interval_secs == 0 {
        return u32::MAX;
    }
    let sends = 1 + (lifetime.as_secs() / interval_secs);
    if sends > u32::MAX as u64 {
        u32::MAX
    } else {
        sends as u32
    }
}

/// Number of additional upstream attempts the proxy makes for a 429 response.
/// The logical capability call is reserved once and remains one call across
/// these provider-level retries.
pub const CREDENTIAL_UPSTREAM_MAX_RATE_LIMIT_RETRIES: u32 = 4;

/// Deterministic floor for rate-limit retry backoff when the provider gives no
/// usable reset hint. The per-capability pacer independently enforces the
/// request-start cadence, and a provider-supplied hint always wins over this.
pub const CREDENTIAL_UPSTREAM_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(30);

/// A paid-compute approval is consent for the exact proposal currently shown,
/// not an indefinitely reusable prompt left open in a restored transcript.
pub const PAID_COMPUTE_APPROVAL_TTL: Duration = Duration::from_secs(120);

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

/// Provider-proxy request body cap. Oversized agent bodies fail closed.
pub const SECRETS_PROXY_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Default lifetime of a run capability issued by the local secrets broker.
pub const SECRETS_CAPABILITY_TTL: Duration = Duration::from_secs(30 * 60);

/// Desktop image preview size cap.
pub const IMAGE_PREVIEW_MAX_BYTES: u64 = 20 * 1024 * 1024;

/// Sealed trace artifact cap for a container import. Above this a trace belongs
/// in a bundle the user moves deliberately, not in a loopback fetch.
pub const MAX_IMPORTED_TRACE_BYTES: u64 = 256 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_proxy_covers_deepswe_capability_lifetime() {
        assert_eq!(CREDENTIAL_UPSTREAM_TIMEOUT, DEEPSWE_HARBOR_CAPABILITY_TTL);
        assert_eq!(
            CREDENTIAL_UPSTREAM_TIMEOUT.as_secs(),
            u64::from(DEEPSWE_HARBOR_CAPABILITY_TTL_SECONDS)
        );
        assert_eq!(CREDENTIAL_UPSTREAM_TIMEOUT.as_secs(), 5_400);
    }

    /// The container reads this exact name. A rename on one side only would
    /// leave every container reporting an unanswered freshness question, which
    /// is quiet rather than loud.
    #[test]
    fn the_source_revision_stamp_name_is_the_one_containers_read() {
        assert_eq!(
            CONTAINER_SOURCE_REVISION_ENV,
            "SYNTH_CONTAINER_SOURCE_REVISION"
        );
    }

    #[test]
    fn host_approval_window_is_clickable() {
        assert!(HOST_APPROVAL_LIFETIME >= HOST_APPROVAL_LIFETIME_FLOOR);
        assert!(HOST_APPROVAL_LIFETIME_FLOOR >= Duration::from_secs(60));
    }

    #[test]
    fn realizable_calls_counts_the_free_first_send() {
        // The shape the prior rollout tripped over: 5400 seconds paced at 370
        // admits fifteen sends, not the eighty its sheet declared.
        assert_eq!(
            realizable_provider_calls(Duration::from_secs(5_400), Duration::from_secs(370)),
            15
        );
        assert_eq!(
            realizable_provider_calls(Duration::ZERO, Duration::ZERO),
            u32::MAX
        );
    }

    #[test]
    fn credential_proxy_rate_limit_guard_is_conservative_and_bounded() {
        assert!(CREDENTIAL_UPSTREAM_MIN_INTERVAL >= Duration::from_secs(6));
        assert!(CREDENTIAL_UPSTREAM_MAX_RATE_LIMIT_RETRIES > 0);
        assert!(CREDENTIAL_UPSTREAM_RATE_LIMIT_BACKOFF >= CREDENTIAL_UPSTREAM_MIN_INTERVAL);
        assert!(CREDENTIAL_UPSTREAM_MAX_INTERVAL > CREDENTIAL_UPSTREAM_MIN_INTERVAL);
        assert!(CREDENTIAL_UPSTREAM_MAX_INTERVAL <= DEEPSWE_HARBOR_CAPABILITY_TTL);
        assert!(
            CREDENTIAL_UPSTREAM_INTERVAL_DECAY_NUMERATOR
                < CREDENTIAL_UPSTREAM_INTERVAL_DECAY_DENOMINATOR
        );
    }

    /// The approved ceiling has to be a number the run can reach. The prior
    /// rollout approved eighty calls against a window that admitted fifteen.
    #[test]
    fn the_deepswe_ceiling_is_reachable_inside_its_own_lifetime() {
        assert!(
            realizable_provider_calls(
                DEEPSWE_HARBOR_CAPABILITY_TTL,
                CREDENTIAL_UPSTREAM_MIN_INTERVAL
            ) >= 80,
            "the approved 80-call ceiling must be realizable at the pacing floor"
        );
    }
}
