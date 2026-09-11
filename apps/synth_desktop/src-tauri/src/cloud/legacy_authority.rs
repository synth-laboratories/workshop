//! Volatile creation provenance, never a persisted account-ownership claim.
//! Historical rows require a future authoritative verification/rebind contract.
use anyhow::{bail, Context, Result};
use std::{collections::HashMap, future::Future, sync::{Arc, Mutex}};
use tokio::sync::{watch, RwLock};
use super::intern::InternClient;

pub struct LegacyAuthority {
    epoch: watch::Sender<u64>,
    admitted: Mutex<HashMap<String, (u64, Arc<InternClient>)>>,
    // Serializes provider startup with client replacement, not Local work.
    pub provider_gate: RwLock<()>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stale_epoch_never_constructs_transport_or_admits_creation() {
        let authority = LegacyAuthority::default();
        let generation = authority.generation();
        let client = Arc::new(InternClient::connect("https://fixture.invalid", "fixture", std::time::Duration::from_secs(1)).unwrap());
        authority.invalidate();
        let result = authority.run(generation, || { panic!("stale transport constructor"); #[allow(unreachable_code)] async {} }).await;
        assert!(result.is_err());
        assert!(authority.admit_created("late", generation, client).is_err());
        assert!(authority.session("late").is_err());
    }

    #[test]
    fn conditional_invalidation_ignores_superseded_generations() {
        let authority = LegacyAuthority::default();
        let stale = authority.generation();
        authority.invalidate();
        let current = authority.generation();
        assert_eq!(authority.invalidate_if_current(stale), None);
        assert_eq!(authority.generation(), current);
        assert_eq!(authority.invalidate_if_current(current), Some(current + 1));
        assert!(authority.ensure(current).is_err());
    }
}

impl Default for LegacyAuthority {
    fn default() -> Self {
        Self { epoch: watch::channel(0).0, admitted: Mutex::new(HashMap::new()), provider_gate: RwLock::new(()) }
    }
}

impl LegacyAuthority {
    pub fn generation(&self) -> u64 { *self.epoch.borrow() }

    pub fn invalidate(&self) {
        self.epoch.send_modify(|epoch| *epoch = epoch.saturating_add(1));
        self.admitted.lock().unwrap().clear();
    }

    /// Retire `generation` only if it is still current, atomically with the
    /// check. Returns the fenced epoch, or `None` when a transition already
    /// superseded it; the caller must then leave the newer configuration alone.
    pub fn invalidate_if_current(&self, generation: u64) -> Option<u64> {
        let mut fenced = None;
        self.epoch.send_if_modified(|epoch| {
            if *epoch != generation || generation == u64::MAX {
                return false;
            }
            *epoch = epoch.saturating_add(1);
            fenced = Some(*epoch);
            true
        });
        if fenced.is_some() {
            self.admitted.lock().unwrap().clear();
        }
        fenced
    }

    pub fn ensure(&self, generation: u64) -> Result<()> {
        if generation == u64::MAX || self.generation() != generation {
            bail!("Intern configuration changed; ownership verification is required");
        }
        Ok(())
    }

    pub fn admit_created(&self, id: &str, generation: u64, client: Arc<InternClient>) -> Result<()> {
        let epoch = self.epoch.borrow();
        if *epoch != generation || generation == u64::MAX {
            bail!("Intern creation was superseded; ownership verification is required");
        }
        self.admitted.lock().unwrap().insert(id.into(), (generation, client));
        Ok(())
    }

    pub fn session(&self, id: &str) -> Result<(u64, Arc<InternClient>)> {
        let epoch = self.epoch.borrow();
        self.admitted.lock().unwrap().get(id)
            .filter(|(generation, _)| *generation == *epoch && *epoch != u64::MAX)
            .cloned().context("Historical Intern conversation requires ownership verification before reuse or remote operations")
    }

    /// Construct only after checking the epoch. The future owns its transport;
    /// invalidation drops it and never implies that the remote operation failed.
    pub async fn run<T, F, Fut>(&self, generation: u64, construct: F) -> Result<T>
    where F: FnOnce() -> Fut, Fut: Future<Output = T> {
        let mut changes = self.epoch.subscribe();
        let operation = {
            let epoch = changes.borrow_and_update();
            if *epoch != generation || generation == u64::MAX {
                bail!("Intern configuration changed before dispatch");
            }
            construct()
        };
        tokio::pin!(operation);
        tokio::select! {
            biased;
            _ = changes.changed() => bail!("Intern configuration changed during dispatch; remote outcome is unknown"),
            result = &mut operation => { self.ensure(generation)?; Ok(result) }
        }
    }
}
