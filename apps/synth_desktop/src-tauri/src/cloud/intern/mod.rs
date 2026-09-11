//! Desktop-scoped Research Intern protocol and polling support.
//!
//! This is intentionally not the complete `synth-ai` SDK. It is the small,
//! generation-fenced mailbox surface consumed by Synth Desktop.

mod client;
mod ingestion;
mod models;
mod normalize;
mod poller;

pub use client::{InternClient, InternClientError};
pub use ingestion::{
    InternIngestion, InternIngestionState, InternProviderManager, InternSessionBinding,
};
pub use models::*;
pub use normalize::{normalize_event, NormalizedInternEvent};
pub use poller::{InternPoller, PollDelivery, PollUpdate, PollerConfig, PollerHandle};

use std::{sync::Arc, time::Duration};
use synth_api_client::availability::DeferredClient;

/// Core-owned Intern cloud boundary. Reconfiguration first shuts down all
/// mailbox pollers so an old credential/endpoint can never keep running.
pub struct InternRuntime {
    client: DeferredClient,
    poller: Arc<InternPoller>,
}

impl InternRuntime {
    pub fn unconfigured() -> Self {
        Self {
            client: DeferredClient::unavailable(),
            poller: Arc::new(InternPoller::default()),
        }
    }

    pub fn lazy(
        resolve: impl FnOnce() -> Result<Option<InternClient>, InternClientError>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            client: DeferredClient::lazy(resolve),
            poller: Arc::new(InternPoller::default()),
        }
    }

    pub fn configured(
        base_url: &str,
        api_key: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, InternClientError> {
        Ok(Self {
            client: DeferredClient::ready(InternClient::connect(base_url, api_key, timeout)?),
            poller: Arc::new(InternPoller::default()),
        })
    }

    pub async fn client(&self) -> Result<Arc<InternClient>, InternClientError> {
        self.client.client().await
    }

    pub fn poller(&self) -> &InternPoller {
        &self.poller
    }

    pub async fn reconfigure(
        &self,
        base_url: &str,
        api_key: impl Into<String>,
        timeout: Duration,
    ) -> Result<(), InternClientError> {
        self.disable().await;
        let client = InternClient::connect(base_url, api_key, timeout)?;
        self.client.replace(Some(client)).await;
        Ok(())
    }

    pub async fn disable(&self) {
        self.poller.shutdown().await;
        self.client.replace(None).await;
    }
}

impl Default for InternRuntime {
    fn default() -> Self {
        Self::unconfigured()
    }
}
