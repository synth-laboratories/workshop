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
pub use poller::{InternPoller, PollUpdate, PollerConfig, PollerHandle};

use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;

/// Core-owned Intern cloud boundary. Reconfiguration first shuts down all
/// mailbox pollers so an old credential/endpoint can never keep running.
#[derive(Default)]
pub struct InternRuntime {
    client: RwLock<Option<Arc<InternClient>>>,
    poller: Arc<InternPoller>,
}

impl InternRuntime {
    pub fn unconfigured() -> Self {
        Self::default()
    }

    pub fn configured(
        base_url: &str,
        api_key: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, InternClientError> {
        Ok(Self {
            client: RwLock::new(Some(Arc::new(InternClient::connect(
                base_url, api_key, timeout,
            )?))),
            poller: Arc::new(InternPoller::default()),
        })
    }

    pub async fn client(&self) -> Result<Arc<InternClient>, InternClientError> {
        self.client.read().await.clone().ok_or_else(|| {
            InternClientError::Configuration("Synth Intern is not configured".into())
        })
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
        let client = Arc::new(InternClient::connect(base_url, api_key, timeout)?);
        self.poller.shutdown().await;
        *self.client.write().await = Some(client);
        Ok(())
    }

    pub async fn disable(&self) {
        self.poller.shutdown().await;
        *self.client.write().await = None;
    }
}
