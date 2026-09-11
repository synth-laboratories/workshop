//! Host composition of durable creation/outbox with volatile scope fencing.
//! Every remote operation revalidates identity; no production gate is opened.
use super::*;
use crate::cloud::storage::{
    CommandIntent, CreationIntent, CreationReceipt, CreationRecord, DeliveryReceipt,
    PendingCommand, Stream,
};
use std::future::Future;

pub struct ScopedCreation {
    pub generation: u32,
    pub record: CreationRecord,
}

impl ScopedCloudRuntime {
    async fn scoped_transaction<T, F>(&self, generation: u32, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(CloudStore, ScopeLease) -> Result<T> + Send + 'static,
    {
        let mut state = self.state.lock().await;
        self.expire(&mut state).await?;
        if state.view.generation != generation {
            bail!("cloud operation was superseded");
        }
        let lease = state
            .active
            .as_ref()
            .context("cloud identity unavailable")?
            .lease
            .clone();
        let store = state
            .store
            .clone()
            .context("cloud profile qualification is required")?;
        // Retain the host fence through persistence, including after a failed
        // durable sign-out. No network work is allowed in this closure.
        let result = tokio::task::spawn_blocking(move || operation(store, lease))
            .await
            .context("join scoped operation persistence")?;
        self.expire(&mut state).await?;
        if state.view.generation != generation || state.active.is_none() {
            bail!("cloud operation expired during persistence");
        }
        result
    }

    /// The callback returns a lazy, cancellation-owned future. Detached
    /// transport tasks require their own cancellation contract and are not
    /// supported by this composition.
    pub(super) async fn await_scoped<T, F, Fut>(&self, generation: u32, construct: F) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let (response, mut changes) = {
            let mut state = self.state.lock().await;
            self.expire(&mut state).await?;
            if state.view.generation != generation || state.active.is_none() {
                bail!("cloud operation was superseded");
            }
            let changes = self.subscribe();
            (construct(), changes)
        };
        tokio::pin!(response);
        loop {
            let view = *changes.borrow_and_update();
            if view.generation != generation || view.availability != Availability::Ready {
                bail!("cloud operation was superseded");
            }
            tokio::select! {
                biased;
                changed = changes.changed() => { changed.context("cloud scope observer closed")?; }
                result = &mut response => {
                    if result.as_ref().err().is_some_and(|error| {
                        error.downcast_ref::<crate::cloud::intern::InternClientError>()
                            .is_some_and(|cause| cause.is_auth_failure())
                    }) {
                        // Revocation may happen after the preflight identity
                        // read. A remote denial must clear the cached scope.
                        let _ = self.invalidate().await;
                    }
                    return result;
                }
            }
        }
    }

    pub async fn create_once_with<V, VF, S, SF>(
        &self,
        origin: &str,
        intent: CreationIntent,
        verify: V,
        send: S,
    ) -> Result<ScopedCreation>
    where
        V: FnOnce() -> VF,
        VF: Future<Output = Result<IdentityObservation>>,
        S: FnOnce(CreationRecord) -> SF,
        SF: Future<Output = Result<CreationReceipt>>,
    {
        let generation = self.revalidate_with(origin, verify).await?.generation;
        let plan = intent.clone();
        let request = self
            .scoped_transaction(generation, move |store, lease| {
                store.stage_creation(&lease, &plan)?;
                store.begin_creation(&lease, &plan.creation_id)
            })
            .await?;
        let receipt = self.await_scoped(generation, || send(request)).await?;
        if receipt.creation_id != intent.creation_id
            || receipt.adapter != intent.adapter
            || receipt.idempotency_key != intent.idempotency_key
        {
            bail!("creation receipt identity drift");
        }
        let record = self
            .scoped_transaction(generation, move |store, lease| {
                store.bind_created(&lease, &intent.creation_id, &receipt.external_id)
            })
            .await?;
        Ok(ScopedCreation { generation, record })
    }

    pub async fn send_once_with<V, VF, S, SF>(
        &self,
        origin: &str,
        expected_generation: u32,
        intent: CommandIntent,
        verify: V,
        send: S,
    ) -> Result<PendingCommand>
    where
        V: FnOnce() -> VF,
        VF: Future<Output = Result<IdentityObservation>>,
        S: FnOnce(PendingCommand) -> SF,
        SF: Future<Output = Result<DeliveryReceipt>>,
    {
        let generation = self.revalidate_with(origin, verify).await?.generation;
        if generation != expected_generation {
            bail!("cloud command scope was superseded");
        }
        let plan = intent.clone();
        let request = self
            .scoped_transaction(generation, move |store, lease| {
                store.enqueue(&lease, &plan)?;
                store.begin_send(&lease, &plan.command_id)
            })
            .await?;
        let receipt = self.await_scoped(generation, || send(request)).await?;
        if receipt.command_id != intent.command_id || receipt.stream != intent.stream {
            bail!("command receipt identity drift");
        }
        self.scoped_transaction(generation, move |store, lease| {
            store.record_receipt(&lease, &intent.command_id, receipt.stage, &receipt.detail)?;
            store.command(&lease, &intent.command_id)
        })
        .await
    }

    /// Creation binding and first-command staging are atomic. A second fresh
    /// identity read is mandatory before that original first command is sent.
    pub async fn create_and_first_send_with<V, VF, C, CF, S, SF>(
        &self,
        origin: &str,
        intent: CreationIntent,
        mut verify: V,
        create: C,
        send: S,
    ) -> Result<(ScopedCreation, PendingCommand)>
    where
        V: FnMut() -> VF,
        VF: Future<Output = Result<IdentityObservation>>,
        C: FnOnce(CreationRecord) -> CF,
        CF: Future<Output = Result<CreationReceipt>>,
        S: FnOnce(PendingCommand) -> SF,
        SF: Future<Output = Result<DeliveryReceipt>>,
    {
        let created = self
            .create_once_with(origin, intent, || verify(), create)
            .await?;
        let first = &created.record.intent.first;
        let command = CommandIntent {
            command_id: first.command_id.clone(),
            stream: Stream {
                adapter: created.record.intent.adapter,
                external_id: created
                    .record
                    .external_id
                    .clone()
                    .context("creation is not bound")?,
            },
            operation_id: first.operation_id.clone(),
            idempotency_key: first.idempotency_key.clone(),
            body: first.body.clone(),
            expected_generation: first.expected_generation,
        };
        let sent = self
            .send_once_with(origin, created.generation, command, || verify(), send)
            .await?;
        Ok((created, sent))
    }
}
