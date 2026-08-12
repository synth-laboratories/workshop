//! `ManagedService` vocabulary + supervisor registry.
//!
//! Laguna and Whisper implement the trait directly. The composition root drains
//! registered services on [`tauri::RunEvent::ExitRequested`].

use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Shared lifecycle for Desktop-owned sidecars (Laguna, Whisper, loopback IPC, …).
pub trait ManagedService: Send + Sync {
    fn name(&self) -> &'static str;

    fn spawn(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn probe(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async { Ok(true) })
    }

    fn wait_ready(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>>;

    fn restart(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            self.stop().await?;
            self.spawn().await
        })
    }
}

/// Process-wide registry of managed services. Injected via `app.manage`.
#[derive(Default)]
pub struct ServiceSupervisor {
    services: Mutex<Vec<Arc<dyn ManagedService>>>,
}

impl ServiceSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, service: Arc<dyn ManagedService>) {
        self.services
            .lock()
            .expect("service supervisor lock")
            .push(service);
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.services
            .lock()
            .expect("service supervisor lock")
            .iter()
            .map(|service| service.name())
            .collect()
    }

    /// Best-effort stop of every registered service. Failures are reported but
    /// do not abort the remaining drain — quit must continue.
    pub async fn drain_all(&self) {
        let services = self
            .services
            .lock()
            .expect("service supervisor lock")
            .clone();
        for service in services {
            if let Err(error) = service.stop().await {
                eprintln!(
                    "synth-desktop: managed service '{}' failed to stop on exit: {error:#}",
                    service.name()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ManagedService, ServiceSupervisor};
    use anyhow::Result;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct FlagService(Arc<AtomicBool>);

    impl ManagedService for FlagService {
        fn name(&self) -> &'static str {
            "flag"
        }

        fn stop(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async move {
                self.0.store(true, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn drain_stops_every_registered_service() {
        let stopped = Arc::new(AtomicBool::new(false));
        let supervisor = ServiceSupervisor::new();
        supervisor.register(Arc::new(FlagService(stopped.clone())));
        assert_eq!(supervisor.names(), vec!["flag"]);
        supervisor.drain_all().await;
        assert!(stopped.load(Ordering::SeqCst));
    }
}
