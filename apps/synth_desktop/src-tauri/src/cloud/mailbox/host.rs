//! Production adapters for the native mailbox.
//!
//! Constructing these performs no network I/O; identity and grant calls
//! happen only when a host method runs. Default boot never calls anything
//! here: the explicit entry points are the eval-driver `/v1/cloud/mailbox/*`
//! routes (instance builds only) until a qualified profile opts in.
use super::grant::{validate_origin, EndpointPolicy, HttpGrantAuthority, SecretToken};
use crate::cloud::identity::IdentityObservation;
use crate::cloud::scoped_runtime::{IdentityVerifier, MailboxDeps, TurnBoundary};
use anyhow::{Context, Result};
use futures_util::future::BoxFuture;
use std::sync::Arc;

/// Fresh read of `/api/v1/desktop/cloud-identity` on every verification.
pub struct ApiIdentityVerifier {
    client: Arc<crate::cloud::intern::InternClient>,
}

impl IdentityVerifier for ApiIdentityVerifier {
    fn verify(&self) -> BoxFuture<'_, Result<IdentityObservation>> {
        Box::pin(async move {
            let document = self.client.identity_observation().await.map_err(|error| anyhow::anyhow!("{error}"))?;
            IdentityObservation::try_from(document)
        })
    }
}

/// A session is at a safe turn boundary when it has no running turn.
pub struct SessionTurnBoundary {
    sessions: crate::domain::SessionService,
}

impl TurnBoundary for SessionTurnBoundary {
    fn session_idle(&self, session_id: String) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            let record = self.sessions.get(session_id).await?.context("bound session no longer exists")?;
            Ok(record.status != "running" && record.active_run_id.is_none())
        })
    }
}

/// Dependencies from the configured backend profile: an https backend
/// origin (or a local slot's loopback origin, see below) and a configured
/// Synth API key. No restricted executor is registered, so Respond-preset
/// requests wait for an operator answer.
pub fn configured_deps(core: &crate::core_runtime::CoreRuntime) -> Result<MailboxDeps> {
    let backend = crate::synth_config::resolve().context("cloud backend configuration unavailable")?;
    let key = backend.api_key.context("Synth API key is not configured")?;
    let url = reqwest::Url::parse(&backend.backend_url).context("invalid backend URL")?;
    let candidate = url.origin().ascii_serialization();
    // Loopback http is accepted only in the backend's exact local-slot form.
    // Every identity verification then requires the backend to report that
    // same origin, which it does only with APP_ENVIRONMENT=local.
    let policy = if super::grant::local_loopback_origin(&candidate).is_some() {
        EndpointPolicy::LOCAL_SLOT
    } else {
        EndpointPolicy::PRODUCTION
    };
    let origin = validate_origin(&candidate, policy)?;
    let client = crate::cloud::intern::InternClient::connect(&backend.backend_url, key.clone(), crate::limits::INTERN_HTTP_TIMEOUT)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(MailboxDeps {
        origin: origin.clone(),
        verifier: Arc::new(ApiIdentityVerifier { client: Arc::new(client) }),
        authority: Arc::new(HttpGrantAuthority::try_new(&origin, SecretToken::new(key), policy)?),
        boundary: Arc::new(SessionTurnBoundary { sessions: core.sessions().clone() }),
        executor: None,
        endpoint_policy: policy,
    })
}

/// [`configured_deps`] plus the production confined executor when it is
/// available, so accepted Respond-preset requests the executor can enforce
/// run confined; everything else still waits for an operator.
pub async fn configured_deps_with_executor(core: &crate::core_runtime::CoreRuntime) -> Result<MailboxDeps> {
    let mut deps = configured_deps(core)?;
    deps.executor = confined_executor().await;
    Ok(deps)
}

static CONFINED_EXECUTOR: tokio::sync::OnceCell<Option<Arc<dyn crate::cloud::scoped_runtime::RestrictedExecutor>>> =
    tokio::sync::OnceCell::const_new();

/// The production confined executor, built and self-checked once per
/// process. It exists only when every precondition holds: macOS seatbelt,
/// a native Codex binary (`SYNTH_CODEX_BIN` or `codex` on PATH), the local
/// Laguna provider on loopback (`SYNTH_LAGUNA_BASE_URL`, default
/// 127.0.0.1:7333) and a passing model-free confined boot check. Otherwise
/// no executor is registered and requests wait for an operator answer.
pub async fn confined_executor() -> Option<Arc<dyn crate::cloud::scoped_runtime::RestrictedExecutor>> {
    CONFINED_EXECUTOR
        .get_or_init(|| async {
            match build_confined_executor().await {
                Ok(executor) => Some(executor),
                Err(error) => {
                    crate::platform::logging::report("mailbox", "eprintln", format!("confined mailbox executor unavailable: {error:#}"));
                    None
                }
            }
        })
        .await
        .clone()
}

async fn build_confined_executor() -> Result<Arc<dyn crate::cloud::scoped_runtime::RestrictedExecutor>> {
    use super::codex_executor::{ConfinedCodexExecutor, LoopbackProvider};
    let launcher = std::env::var_os("SYNTH_CODEX_BIN").map(std::path::PathBuf::from).or_else(|| {
        std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|dir| dir.join("codex")).find(|path| path.is_file()))
    });
    let binary = launcher.as_deref().and_then(ConfinedCodexExecutor::resolve_native).context("no native codex app-server binary")?;
    let base = std::env::var("SYNTH_LAGUNA_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:7333".into());
    let provider = LoopbackProvider {
        name: "local-laguna".into(),
        base_url: format!("{}/v1", base.trim_end_matches('/')),
        model: crate::domain::LOCAL_LAGUNA_MODEL.into(),
        env_key: "SYNTH_LAGUNA_API_KEY".into(),
        api_key: SecretToken::new(std::env::var("SYNTH_LAGUNA_API_KEY").unwrap_or_else(|_| "local".into())),
    };
    let executor = ConfinedCodexExecutor::new(binary, vec![], provider, crate::storage::app_data_root().join("mailbox-turns"))?;
    executor.verify_confined_boot().await?;
    Ok(Arc::new(executor))
}

/// Explicit opt-in: install the registered, shape-verified store into the
/// host runtime. Performs no identity or network request.
pub async fn activate_store(core: &crate::core_runtime::CoreRuntime) -> Result<()> {
    let store = crate::cloud::storage::CloudStore::open(core.storage().database().clone())?;
    core.scoped_cloud().activate_store(store).await
}
