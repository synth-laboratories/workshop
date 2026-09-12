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
/// origin and a configured Synth API key. No restricted executor is
/// registered, so Respond-preset requests wait for an operator answer.
pub fn configured_deps(core: &crate::core_runtime::CoreRuntime) -> Result<MailboxDeps> {
    let backend = crate::synth_config::resolve().context("cloud backend configuration unavailable")?;
    let key = backend.api_key.context("Synth API key is not configured")?;
    let url = reqwest::Url::parse(&backend.backend_url).context("invalid backend URL")?;
    let policy = EndpointPolicy::PRODUCTION;
    let origin = validate_origin(&url.origin().ascii_serialization(), policy)?;
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

/// Explicit opt-in: install the registered, shape-verified store into the
/// host runtime. Performs no identity or network request.
pub async fn activate_store(core: &crate::core_runtime::CoreRuntime) -> Result<()> {
    let store = crate::cloud::storage::CloudStore::open(core.storage().database().clone())?;
    core.scoped_cloud().activate_store(store).await
}
