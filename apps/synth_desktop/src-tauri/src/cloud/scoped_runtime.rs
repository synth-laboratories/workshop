//! Host-owned scoped cloud lifecycle. Production starts qualification-gated;
//! offline fixtures install the candidate store explicitly. No migration or
//! network client is constructed here.
use super::{
    identity::IdentityObservation,
    storage::{CloudScopeIdentity, CloudStore, ScopeLease},
};
use crate::{
    domain::ExecutionLocation,
    storage::{AppEvent, SessionRecord},
};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::{watch, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    QualificationRequired,
    SignedOut,
    Ready,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ScopeView {
    pub generation: u32,
    pub availability: Availability,
}
struct Active {
    identity: CloudScopeIdentity,
    lease: ScopeLease,
    until: DateTime<Utc>,
}
struct State {
    store: Option<CloudStore>,
    active: Option<Active>,
    view: ScopeView,
    attempt: u64,
}
#[derive(Clone)]
pub struct ScopedCloudRuntime {
    state: Arc<Mutex<State>>,
    changes: watch::Sender<ScopeView>,
}
#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ScopedSessions {
    pub generation: u32,
    pub sessions: Vec<SessionRecord>,
}
#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ScopedEvents {
    pub generation: u32,
    pub events: Vec<AppEvent>,
}

impl ScopedCloudRuntime {
    pub fn qualification_gated() -> Self {
        let view = ScopeView {
            generation: 0,
            availability: Availability::QualificationRequired,
        };
        let (changes, _) = watch::channel(view);
        Self {
            state: Arc::new(Mutex::new(State {
                store: None,
                active: None,
                view,
                attempt: 0,
            })),
            changes,
        }
    }
    pub fn subscribe(&self) -> watch::Receiver<ScopeView> {
        self.changes.subscribe()
    }
    pub async fn view(&self) -> Result<ScopeView> {
        let mut state = self.state.lock().await;
        self.expire(&mut state).await?;
        Ok(state.view)
    }
    // There is intentionally no production activation entry point until the
    // deployment/profile contract and registered migration are qualified.
    #[cfg(test)]
    pub async fn install_fixture(&self, store: CloudStore) -> Result<()> {
        let mut state = self.state.lock().await;
        state.store = Some(store);
        self.reset(&mut state).await?;
        Ok(())
    }
    pub async fn invalidate(&self) -> Result<ScopeView> {
        let mut state = self.state.lock().await;
        self.reset(&mut state).await?;
        Ok(state.view)
    }
    async fn reset(&self, state: &mut State) -> Result<()> {
        state.attempt = state
            .attempt
            .checked_add(1)
            .context("identity attempt exhausted")?;
        state.active = None;
        state.view.generation = state
            .view
            .generation
            .checked_add(1)
            .context("scope view generation exhausted")?;
        state.view.availability = if state.store.is_some() {
            Availability::SignedOut
        } else {
            Availability::QualificationRequired
        };
        // Publish the cache reset even if persistence reports a failure. The
        // coordinator will no longer authorize reads from the old scope.
        self.changes.send_replace(state.view);
        if let Some(store) = state.store.clone() {
            tokio::task::spawn_blocking(move || store.sign_out())
                .await
                .context("join scope invalidation")??;
        }
        Ok(())
    }
    async fn expire(&self, state: &mut State) -> Result<()> {
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.until <= Utc::now())
        {
            self.reset(state).await?;
        }
        Ok(())
    }
    /// The injected verifier must fetch the authority afresh. A sign-out or a
    /// newer request supersedes its eventual result; late responses cannot log
    /// the host back into a previous account.
    pub async fn revalidate_with<F, Fut>(
        &self,
        expected_origin: &str,
        verify: F,
    ) -> Result<ScopeView>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<IdentityObservation>>,
    {
        let attempt = {
            let mut state = self.state.lock().await;
            self.expire(&mut state).await?;
            if state.store.is_none() {
                bail!("cloud profile qualification is required");
            }
            state.attempt = state
                .attempt
                .checked_add(1)
                .context("identity attempt exhausted")?;
            state.attempt
        };
        let observed = verify().await.and_then(|observation| {
            let identity = observation.validate(expected_origin, Utc::now())?;
            Ok((identity, observation.valid_until))
        });
        let mut state = self.state.lock().await;
        if state.attempt != attempt {
            bail!("identity observation was superseded");
        }
        let (identity, until) = match observed {
            Ok(value) => value,
            Err(_) => {
                self.reset(&mut state).await?;
                bail!("cloud identity authority unavailable");
            }
        };
        self.expire(&mut state).await?;
        let store = state
            .store
            .clone()
            .context("cloud profile qualification is required")?;
        let unchanged = state
            .active
            .as_ref()
            .filter(|active| active.identity == identity)
            .map(|active| active.lease.clone());
        let worker_identity = identity.clone();
        let result = tokio::task::spawn_blocking(move || {
            if let Some(lease) = unchanged {
                store.refresh_verified_until(&lease, &worker_identity, until)?;
                Ok(lease)
            } else {
                store.activate_verified_until(&worker_identity, until)
            }
        })
        .await
        .context("join identity activation")?;
        let lease = match result {
            Ok(lease) => lease,
            Err(_) => {
                self.reset(&mut state).await?;
                bail!("cloud identity activation failed");
            }
        };
        let changed = state
            .active
            .as_ref()
            .is_none_or(|active| active.identity != identity);
        state.active = Some(Active {
            identity,
            lease,
            until,
        });
        if changed {
            state.view.generation = state
                .view
                .generation
                .checked_add(1)
                .context("scope view generation exhausted")?;
        }
        state.view.availability = Availability::Ready;
        self.changes.send_replace(state.view);
        // Expiration must reset an idle renderer too, not only the next reader.
        // A renewed observation schedules its own timer; an older timer simply
        // checks the current deadline and cannot revoke the renewed lease.
        let runtime = self.clone();
        let delay = (until - Utc::now()).to_std().unwrap_or_default();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let mut state = runtime.state.lock().await;
            let _ = runtime.expire(&mut state).await;
        });
        Ok(state.view)
    }
    /// Scope-tagged projection for gated IPC routing. Local execution is
    /// independent of provider; unknown kinds and unowned legacy cloud rows
    /// never enter this result.
    pub async fn project_sessions(
        &self,
        service: &crate::domain::SessionService,
    ) -> Result<ScopedSessions> {
        let mut state = self.state.lock().await;
        let _ = self.expire(&mut state).await;
        let allowed =
            if let (Some(store), Some(active)) = (state.store.clone(), state.active.as_ref()) {
                let lease = active.lease.clone();
                match tokio::task::spawn_blocking(move || store.bound_sessions(&lease)).await {
                    Ok(Ok(ids)) => ids.into_iter().collect::<HashSet<_>>(),
                    _ => {
                        let _ = self.reset(&mut state).await;
                        HashSet::new()
                    }
                }
            } else {
                HashSet::new()
            };
        let records = service
            .list_scoped(allowed.iter().cloned().collect())
            .await?;
        // The DB worker may outlive the observation while holding the host
        // lock; recheck before returning any cloud rows to the renderer.
        let _ = self.expire(&mut state).await;
        let cloud_available = state.active.is_some();
        let sessions = records
            .into_iter()
            .filter(|record| match record.execution_location() {
                Ok(ExecutionLocation::Local) => true,
                Ok(ExecutionLocation::Cloud) => cloud_available && allowed.contains(&record.id),
                Err(_) => false,
            })
            .collect();
        Ok(ScopedSessions {
            generation: state.view.generation,
            sessions,
        })
    }
    pub async fn read_session_events<F, Fut>(
        &self,
        session: &SessionRecord,
        read: F,
    ) -> Result<ScopedEvents>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<AppEvent>>>,
    {
        if session.execution_location()? == ExecutionLocation::Local {
            let generation = self.state.lock().await.view.generation;
            return Ok(ScopedEvents {
                generation,
                events: read().await?,
            });
        }
        let (store, lease, generation) = {
            let mut state = self.state.lock().await;
            self.expire(&mut state).await?;
            (
                state
                    .store
                    .clone()
                    .context("cloud profile qualification is required")?,
                state
                    .active
                    .as_ref()
                    .context("cloud identity is unavailable")?
                    .lease
                    .clone(),
                state.view.generation,
            )
        };
        let session_id = session.id.clone();
        authorize(store.clone(), lease.clone(), session_id.clone()).await?;
        let events = read().await?;
        let mut state = self.state.lock().await;
        self.expire(&mut state).await?;
        if state.view.generation != generation || state.active.is_none() {
            bail!("cloud history response was superseded");
        }
        authorize(store, lease, session_id).await?;
        Ok(ScopedEvents { generation, events })
    }
}
async fn authorize(store: CloudStore, lease: ScopeLease, session_id: String) -> Result<()> {
    tokio::task::spawn_blocking(move || store.authorize_session(&lease, &session_id))
        .await
        .context("join scoped session authorization")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cloud::storage::{Adapter, Stream, MIGRATION_CANDIDATE},
        core_runtime::CoreRuntime,
        domain::{RuntimeTarget, SessionCreate, SessionKind, SessionStatus},
        storage::{EventAppend, EventSource},
    };
    use serde_json::json;
    fn observation(account: u128) -> IdentityObservation {
        let now = Utc::now();
        serde_json::from_value(json!({"schema_version":"synth.desktop-cloud-identity.v1","backend_origin":"https://fixture.invalid","backend_id":uuid::Uuid::from_u128(1).to_string(),"account_id":uuid::Uuid::from_u128(account).to_string(),"org_id":uuid::Uuid::from_u128(3).to_string(),"profile_id":uuid::Uuid::from_u128(4).to_string(),"verified_at":now.to_rfc3339(),"valid_until":(now+chrono::Duration::seconds(55)).to_rfc3339(),"credential_expiry":null,"revalidate_before_remote_operation":true,"revocation_contract":"fresh_database_key_and_membership_check"})).unwrap()
    }
    async fn setup() -> (tempfile::TempDir, CoreRuntime, CloudStore) {
        let dir = tempfile::tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        let db = core.storage().database().clone();
        db.transaction(|conn| {
            conn.execute_batch(MIGRATION_CANDIDATE)?;
            Ok(())
        })
        .unwrap();
        let store = CloudStore::open(db).unwrap();
        core.scoped_cloud()
            .install_fixture(store.clone())
            .await
            .unwrap();
        (dir, core, store)
    }
    async fn local(core: &CoreRuntime) -> String {
        let id = "native-hosted-model".to_owned();
        core.sessions()
            .create_or_update(SessionCreate {
                id: id.clone(),
                title: "Local".into(),
                kind: SessionKind::Codex,
                target: RuntimeTarget::CloudRuntime {
                    model: "hosted".into(),
                    adapter: None,
                },
                project_id: None,
                remote_id: None,
                codex_thread_id: None,
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        id
    }
    #[tokio::test]
    async fn gated_core_never_fetches_identity_or_requires_candidate_schema() {
        let dir = tempfile::tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        let id = local(&core).await;
        assert!(core
            .scoped_cloud()
            .revalidate_with("https://fixture.invalid", || async {
                panic!("gated host must not access network")
            })
            .await
            .is_err());
        let rows = core.scoped_session_history().await.unwrap();
        assert_eq!(rows.sessions.len(), 1);
        assert_eq!(rows.sessions[0].id, id);
        assert_eq!(
            core.scoped_cloud().view().await.unwrap().availability,
            Availability::QualificationRequired
        );
        assert!(CloudStore::open(core.storage().database().clone()).is_err());
    }
    #[tokio::test]
    async fn legacy_cloud_rows_cannot_crowd_local_history_out_before_filtering() {
        let dir = tempfile::tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        let local_id = local(&core).await;
        core.storage().database().transaction(|conn| {
            conn.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<2001)
                INSERT INTO sessions (id,title,kind,target_json,status,metadata_json,created_at,updated_at)
                SELECT 'legacy-' || x, 'Legacy', 'intern', '{}', 'ready', '{}', '2099-01-01', '2099-01-01' FROM n")?;
            Ok(())
        }).unwrap();
        assert_eq!(core.sessions().list(2000).await.unwrap().len(), 2000);
        let visible = core.scoped_session_history().await.unwrap();
        assert_eq!(visible.sessions.len(), 1);
        assert_eq!(visible.sessions[0].id, local_id);
    }

    #[tokio::test]
    async fn host_history_is_scope_owned_and_local_survives_authority_failure() {
        let (_dir, core, store) = setup().await;
        let local_id = local(&core).await;
        let view = core
            .scoped_cloud()
            .revalidate_with("https://fixture.invalid", || async { Ok(observation(10)) })
            .await
            .unwrap();
        let lease = core
            .scoped_cloud()
            .state
            .lock()
            .await
            .active
            .as_ref()
            .unwrap()
            .lease
            .clone();
        let remote = store
            .create_conversation(
                &lease,
                &Stream {
                    adapter: Adapter::InternSync,
                    external_id: "same-runtime".into(),
                },
                "A",
            )
            .unwrap();
        core.append_and_broadcast(EventAppend {
            event_id: None,
            session_id: Some(remote.clone()),
            run_id: None,
            source: EventSource::Intern,
            kind: "agent.message".into(),
            payload: json!({"body":"A private"}),
            remote_sequence: None,
            command_id: None,
            created_at: None,
        })
        .await
        .unwrap();
        let history = core.scoped_session_history().await.unwrap();
        assert_eq!(history.generation, view.generation);
        assert_eq!(history.sessions.len(), 2);
        assert!(core
            .scoped_session_events_after(remote.clone(), 0, 10)
            .await
            .unwrap()
            .events
            .iter()
            .any(|e| e.payload == json!({"body":"A private"})));
        let refreshed = core
            .scoped_cloud()
            .revalidate_with("https://fixture.invalid", || async { Ok(observation(10)) })
            .await
            .unwrap();
        assert_eq!(refreshed.generation, view.generation);
        core.scoped_cloud()
            .revalidate_with("https://fixture.invalid", || async { Ok(observation(11)) })
            .await
            .unwrap();
        let history = core.scoped_session_history().await.unwrap();
        assert_eq!(history.sessions.len(), 1);
        assert_eq!(history.sessions[0].id, local_id);
        assert!(core
            .scoped_session_events_after(remote, 0, 10)
            .await
            .is_err());
        assert!(core
            .scoped_cloud()
            .revalidate_with("https://fixture.invalid", || async {
                bail!("fixture revoked")
            })
            .await
            .is_err());
        assert_eq!(
            core.scoped_session_history().await.unwrap().sessions.len(),
            1
        );
        assert!(core
            .scoped_session_events_after(local_id, 0, 10)
            .await
            .is_ok());
    }
    #[tokio::test]
    async fn credential_boundary_disables_legacy_client_even_if_persistence_fails() {
        for fail_persistence in [false, true] {
            let (_dir, core, _store) = setup().await;
            let local_id = local(&core).await;
            core.scoped_cloud()
                .revalidate_with("https://fixture.invalid", || async { Ok(observation(10)) })
                .await
                .unwrap();
            if fail_persistence {
                core.storage()
                    .database()
                    .transaction(|conn| {
                        conn.execute_batch(
                            "CREATE TRIGGER refuse_scope_reset BEFORE UPDATE ON cloud_auth_state
                        BEGIN SELECT RAISE(ABORT, 'fixture persistence failure'); END",
                        )?;
                        Ok(())
                    })
                    .unwrap();
            }
            assert_eq!(
                core.disable_cloud_runtime().await.is_err(),
                fail_persistence
            );
            assert_eq!(
                core.scoped_cloud().view().await.unwrap().availability,
                Availability::SignedOut
            );
            assert!(matches!(
                core.intern().client().await,
                Err(crate::cloud::intern::InternClientError::CloudUnavailable)
            ));
            assert_eq!(
                core.scoped_session_history().await.unwrap().sessions[0].id,
                local_id
            );
        }
    }

    #[tokio::test]
    async fn idle_observation_expiry_publishes_reset_without_a_history_read() {
        let (_dir, core, _store) = setup().await;
        let runtime = core.scoped_cloud();
        let mut changes = runtime.subscribe();
        let ready = runtime
            .revalidate_with("https://fixture.invalid", || async {
                let mut document = observation(10);
                document.valid_until = Utc::now() + chrono::Duration::milliseconds(400);
                Ok(document)
            })
            .await
            .unwrap();
        changes.borrow_and_update();
        tokio::time::timeout(std::time::Duration::from_secs(3), changes.changed())
            .await
            .unwrap()
            .unwrap();
        let expired = *changes.borrow_and_update();
        assert_eq!(expired.availability, Availability::SignedOut);
        assert!(expired.generation > ready.generation);
    }

    #[tokio::test]
    async fn signout_supersedes_late_identity_and_inflight_history() {
        let (_dir, core, store) = setup().await;
        let runtime = core.scoped_cloud().clone();
        let worker = runtime.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let pending = tokio::spawn(async move {
            worker
                .revalidate_with("https://fixture.invalid", || async move {
                    started.send(()).unwrap();
                    released.await.unwrap();
                    Ok(observation(10))
                })
                .await
        });
        ready.await.unwrap();
        runtime.invalidate().await.unwrap();
        release.send(()).unwrap();
        assert!(pending.await.unwrap().is_err());
        assert_eq!(
            runtime.view().await.unwrap().availability,
            Availability::SignedOut
        );
        runtime
            .revalidate_with("https://fixture.invalid", || async { Ok(observation(10)) })
            .await
            .unwrap();
        let lease = runtime
            .state
            .lock()
            .await
            .active
            .as_ref()
            .unwrap()
            .lease
            .clone();
        let id = store
            .create_conversation(
                &lease,
                &Stream {
                    adapter: Adapter::InternSync,
                    external_id: "history-runtime".into(),
                },
                "fixture",
            )
            .unwrap();
        let session = core.sessions().get(id).await.unwrap().unwrap();
        let worker = runtime.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let pending = tokio::spawn(async move {
            worker
                .read_session_events(&session, || async move {
                    started.send(()).unwrap();
                    released.await.unwrap();
                    Ok(Vec::new())
                })
                .await
        });
        ready.await.unwrap();
        runtime.invalidate().await.unwrap();
        release.send(()).unwrap();
        assert!(pending.await.unwrap().is_err());
    }
}
