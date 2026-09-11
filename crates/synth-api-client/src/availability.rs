//! Lazy cloud initialization with caller-owned configuration resolution.
use crate::{InternClient, InternClientError};
use std::sync::Arc;
use tokio::sync::RwLock;

type Resolver = Box<dyn FnOnce() -> Result<Option<InternClient>, InternClientError> + Send + Sync>;

enum State {
    Pending(Resolver),
    Ready(Arc<InternClient>),
    CloudUnavailable,
}

pub struct DeferredClient(RwLock<State>);

impl DeferredClient {
    pub fn lazy(
        resolve: impl FnOnce() -> Result<Option<InternClient>, InternClientError>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self(RwLock::new(State::Pending(Box::new(resolve))))
    }

    pub fn unavailable() -> Self {
        Self(RwLock::new(State::CloudUnavailable))
    }

    pub fn ready(client: InternClient) -> Self {
        Self(RwLock::new(State::Ready(Arc::new(client))))
    }

    pub async fn client(&self) -> Result<Arc<InternClient>, InternClientError> {
        let mut state = self.0.write().await;
        if matches!(*state, State::Pending(_)) {
            let State::Pending(resolve) = std::mem::replace(&mut *state, State::CloudUnavailable)
            else {
                unreachable!()
            };
            // Never retain or display resolver error strings: TOML errors can
            // include credential-bearing source lines.
            if let Ok(Some(client)) = resolve() {
                *state = State::Ready(Arc::new(client));
            }
        }
        match &*state {
            State::Ready(client) => Ok(client.clone()),
            _ => Err(InternClientError::CloudUnavailable),
        }
    }

    pub async fn replace(&self, client: Option<InternClient>) {
        *self.0.write().await = client
            .map(|c| State::Ready(Arc::new(c)))
            .unwrap_or(State::CloudUnavailable);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    #[tokio::test]
    async fn bad_configuration_is_lazy_cached_and_redacted() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let cloud = DeferredClient::lazy(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Err(InternClientError::Configuration(
                "secret source text".into(),
            ))
        });
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        for _ in 0..2 {
            let error = cloud.client().await.err().unwrap();
            assert!(matches!(error, InternClientError::CloudUnavailable));
            assert!(!error.to_string().contains("secret"));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn malformed_missing_and_offline_config_do_not_require_network() {
        for url in ["not a url", "file:///tmp/backend"] {
            let cloud = DeferredClient::lazy(move || {
                InternClient::connect(url, "fixture", Duration::from_secs(1)).map(Some)
            });
            assert!(matches!(
                cloud.client().await,
                Err(InternClientError::CloudUnavailable)
            ));
        }
        assert!(DeferredClient::lazy(|| Ok(None)).client().await.is_err());
        let cloud = DeferredClient::lazy(|| {
            InternClient::connect("http://127.0.0.1:1", "fixture", Duration::from_secs(1)).map(Some)
        });
        assert!(cloud.client().await.is_ok());
        cloud.replace(None).await;
        assert!(cloud.client().await.is_err());
    }
}
