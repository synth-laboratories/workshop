use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use futures_util::stream::Stream;
use mq_core::{
    CreateGrant, CreateThread, EnrollDevice, Enrollment, Grant, GrantFilter, GrantIssuance,
    GrantIssuanceRequest, HistoryAuthority, HistoryPage, Message, Participant, Principal,
    PrincipalKind, PublishMessage, RenewGrant, Role, ScopeBinding, Thread, ThreadId, WakeEvent,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::{
    principal_for_thread, principal_from_authorization, signed_principal_from_authorization, ThreadOperation,
};
use mq_core::{DeliveryCheck, DeliveryCheckRequest};
use crate::error::ApiError;
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/openapi.yaml", get(openapi_yaml))
        .route("/v1/threads", post(create_thread).get(list_threads))
        .route("/v1/threads/ensure", post(ensure_thread))
        .route("/v1/threads/{thread_id}", get(get_thread))
        .route(
            "/v1/threads/{thread_id}/participants",
            post(add_participant),
        )
        .route(
            "/v1/threads/{thread_id}/participants/{principal_kind}/{principal_id}",
            patch(set_participant_role),
        )
        .route(
            "/v1/threads/{thread_id}/messages",
            post(publish_message).get(read_messages),
        )
        .route("/v1/threads/{thread_id}/history", get(read_history))
        .route("/v1/threads/{thread_id}/events", get(thread_events))
        .route("/v1/enrollments", post(enroll).get(list_enrollments))
        .route("/v1/enrollments/{enrollment_id}", get(get_enrollment))
        .route("/v1/enrollments/{enrollment_id}/revoke", post(revoke_enrollment))
        .route("/v1/grants", post(create_grant).get(list_grants))
        .route("/v1/grants/{grant_id}", get(get_grant))
        .route("/v1/grants/{grant_id}/revoke", post(revoke_grant))
        .route("/v1/grants/{grant_id}/restore", post(restore_grant))
        .route("/v1/grants/{grant_id}/renew", post(renew_grant))
        .route("/v1/grants/{grant_id}/issuance", post(grant_issuance))
        .route("/v1/grants/{grant_id}/delivery-check", post(delivery_check))
        .with_state(state)
}

pub fn app() -> Router {
    router(AppState::memory())
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(url) = &state.database_url {
        match sqlx::PgPool::connect(url).await {
            Ok(pool) => match sqlx::query("SELECT 1").execute(&pool).await {
                Ok(_) => StatusCode::OK,
                Err(_) => StatusCode::SERVICE_UNAVAILABLE,
            },
            Err(_) => StatusCode::SERVICE_UNAVAILABLE,
        }
    } else {
        StatusCode::OK
    }
}

async fn openapi_yaml() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/yaml")],
        include_str!("../../../openapi/openapi.yaml"),
    )
}

fn unauthenticated() -> ApiError {
    ApiError {
        status: StatusCode::UNAUTHORIZED,
        code: "unauthenticated",
    }
}

/// Unrestricted principal credential. Scoped and grant credentials refuse here.
fn actor(state: &AppState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let value = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    principal_from_authorization(&state.auth, value).map_err(|_| unauthenticated())
}

async fn thread_authority(
    state: &AppState, headers: &HeaderMap, thread_id: Uuid, operation: ThreadOperation,
) -> Result<(Principal, HistoryAuthority), ApiError> {
    let value = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    let (principal, authority) = principal_for_thread(&state.auth, value, thread_id, operation)
        .map_err(|_| unauthenticated())?;
    if let HistoryAuthority::Scoped { generation } = &authority {
        state.fabric.validate_grant_generation(&principal, ThreadId(thread_id), *generation).await?;
    }
    // Grant credentials are checked atomically with the data access in the store.
    Ok((principal, authority))
}

/// Read authorization without returning messages (thread metadata, SSE rechecks).
async fn authorize_read(state: &AppState, headers: &HeaderMap, thread_id: Uuid) -> Result<Thread, ApiError> {
    let (principal, authority) = thread_authority(state, headers, thread_id, ThreadOperation::Read).await?;
    let thread = ThreadId(thread_id);
    Ok(match authority {
        HistoryAuthority::Membership => state.fabric.get_thread(&principal, thread).await?,
        HistoryAuthority::Scoped { generation } => {
            state.fabric.read_scoped(&principal, thread, generation, 0, 0).await?.0
        }
        HistoryAuthority::Grant(fence) => state.fabric.authorize_grant_read(&principal, thread, fence).await?,
    })
}

async fn create_thread(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateThread>,
) -> Result<(StatusCode, Json<Thread>), ApiError> {
    let principal = actor(&state, &headers)?;
    let thread = state.fabric.create_thread(&principal, body).await?;
    Ok((StatusCode::CREATED, Json(thread)))
}

async fn ensure_thread(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateThread>,
) -> Result<(StatusCode, Json<Thread>), ApiError> {
    let principal = actor(&state, &headers)?;
    let thread = state.fabric.ensure_thread(&principal, body).await?;
    // 200 when idempotent hit, 201 when newly created — both OK; clients key on thread_id.
    Ok((StatusCode::OK, Json(thread)))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    /// Deprecated: ignored. Workspace is always the credential's org_id.
    #[serde(default)]
    org_id: Option<String>,
    scope_kind: Option<mq_core::ScopeKind>,
    scope_id: Option<String>,
}

async fn list_threads(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<Thread>>, ApiError> {
    let principal = actor(&state, &headers)?;
    if let Some(requested) = q.org_id.as_deref() {
        if !requested.is_empty() && requested != principal.org_id {
            return Err(ApiError {
                status: StatusCode::FORBIDDEN,
                code: "org_workspace_mismatch",
            });
        }
    }
    let scope = match (q.scope_kind, q.scope_id) {
        (Some(kind), Some(id)) => Some(ScopeBinding { kind, id }),
        (None, None) => None,
        _ => {
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                code: "invalid_scope_query",
            })
        }
    };
    let threads = state
        .fabric
        .list_threads(&principal, scope.as_ref())
        .await?;
    Ok(Json(threads))
}

async fn get_thread(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
) -> Result<Json<Thread>, ApiError> {
    Ok(Json(authorize_read(&state, &headers, thread_id).await?))
}

async fn add_participant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
    Json(body): Json<Participant>,
) -> Result<StatusCode, ApiError> {
    let principal = actor(&state, &headers)?;
    state
        .fabric
        .add_participant(&principal, ThreadId(thread_id), body)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct SetRoleBody {
    role: Role,
}

async fn set_participant_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((thread_id, principal_kind, principal_id)): Path<(Uuid, String, String)>,
    Json(body): Json<SetRoleBody>,
) -> Result<StatusCode, ApiError> {
    let actor_p = actor(&state, &headers)?;
    let kind = match principal_kind.as_str() {
        "human" => PrincipalKind::Human,
        "intern_async" => PrincipalKind::InternAsync,
        "intern_sync" => PrincipalKind::InternSync,
        "actor" => PrincipalKind::Actor,
        "system" => PrincipalKind::System,
        _ => {
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                code: "invalid_principal_kind",
            })
        }
    };
    let target = Principal {
        kind,
        id: principal_id,
        org_id: actor_p.org_id.clone(),
    };
    state
        .fabric
        .set_participant_role(&actor_p, ThreadId(thread_id), &target, body.role)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn publish_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
    Json(mut body): Json<PublishMessage>,
) -> Result<(StatusCode, Json<Message>), ApiError> {
    let (principal, authority) = thread_authority(&state, &headers, thread_id, ThreadOperation::Publish).await?;
    // Trusted, non-wire authority carried into the atomic commit.
    match authority {
        HistoryAuthority::Membership => {}
        HistoryAuthority::Scoped { generation } => body.expected_grant_generation = Some(generation),
        HistoryAuthority::Grant(fence) => body.grant_fence = Some(fence),
    }
    let message = state
        .fabric
        .publish(&principal, ThreadId(thread_id), body)
        .await?;
    Ok((StatusCode::CREATED, Json(message)))
}

#[derive(Debug, Deserialize)]
struct ReadQuery {
    #[serde(default)]
    after_seq: u64,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

async fn read_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
    Query(q): Query<ReadQuery>,
) -> Result<Json<Vec<Message>>, ApiError> {
    let (principal, authority) = thread_authority(&state, &headers, thread_id, ThreadOperation::Read).await?;
    let thread = ThreadId(thread_id);
    let limit = q.limit.clamp(1, 200);
    let messages = match authority {
        HistoryAuthority::Membership => {
            state.fabric.read_messages(&principal, thread, q.after_seq, limit).await?
        }
        HistoryAuthority::Scoped { generation } => {
            state.fabric.read_scoped(&principal, thread, generation, q.after_seq, limit).await?.1
        }
        HistoryAuthority::Grant(fence) => {
            state.fabric.read_granted_messages(&principal, thread, fence, q.after_seq, limit).await?
        }
    };
    Ok(Json(messages))
}

/// Cursor page with explicit history skips. See docs/WORKSHOP_GRANT_CONTRACT.md §8.
async fn read_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
    Query(q): Query<ReadQuery>,
) -> Result<Json<HistoryPage>, ApiError> {
    let (principal, authority) = thread_authority(&state, &headers, thread_id, ThreadOperation::Read).await?;
    Ok(Json(
        state
            .fabric
            .read_history(&principal, ThreadId(thread_id), authority, q.after_seq, q.limit)
            .await?,
    ))
}

async fn thread_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    authorize_read(&state, &headers, thread_id).await?;

    let rx = state.local_wake.subscribe();
    let checks = tokio::time::interval(Duration::from_secs(5));
    let stream = futures_util::stream::unfold(
        (rx, checks, state, headers, false),
        move |(mut rx, mut checks, state, headers, closed)| async move {
            if closed {
                return None;
            }
            loop {
                let wake = tokio::select! {
                    event = rx.recv() => Some(event),
                    _ = checks.tick() => None,
                };
                // Revalidate the token as well as persisted membership and grant
                // state. Quiet streams must not retain authority after expiry.
                let authorized = authorize_read(&state, &headers, thread_id).await.is_ok();
                if !authorized {
                    return Some((Ok(Event::default().event("revoked").data("authorization_unavailable")),
                        (rx, checks, state, headers, true)));
                }
                let event = match wake {
                    Some(Ok(WakeEvent::Thread(id))) if id == thread_id =>
                        Some(Event::default().event("thread_wake").data(id.to_string())),
                    Some(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) =>
                        Some(Event::default().event("resync").data("read_after_durable_cursor")),
                    Some(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return None,
                    _ => None,
                };
                if let Some(event) = event {
                    return Some((Ok(event), (rx, checks, state, headers, false)));
                }
            }
        },
    );

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

// ---- Enrollment and grant administration (backend calls as the owner) ----

async fn enroll(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EnrollDevice>,
) -> Result<(StatusCode, Json<Enrollment>), ApiError> {
    let principal = actor(&state, &headers)?;
    Ok((StatusCode::CREATED, Json(state.fabric.enroll(&principal, body).await?)))
}

async fn list_enrollments(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Enrollment>>, ApiError> {
    let principal = actor(&state, &headers)?;
    Ok(Json(state.fabric.list_enrollments(&principal).await?))
}

async fn get_enrollment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(enrollment_id): Path<Uuid>,
) -> Result<Json<Enrollment>, ApiError> {
    let principal = actor(&state, &headers)?;
    Ok(Json(state.fabric.get_enrollment(&principal, enrollment_id).await?))
}

/// Device sign-out: revokes every grant and incarnation of the enrollment.
async fn revoke_enrollment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(enrollment_id): Path<Uuid>,
) -> Result<Json<Enrollment>, ApiError> {
    let principal = actor(&state, &headers)?;
    Ok(Json(state.fabric.revoke_enrollment(&principal, enrollment_id).await?))
}

async fn create_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateGrant>,
) -> Result<(StatusCode, Json<Grant>), ApiError> {
    let principal = actor(&state, &headers)?;
    Ok((StatusCode::CREATED, Json(state.fabric.create_grant(&principal, body).await?)))
}

async fn list_grants(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(filter): Query<GrantFilter>,
) -> Result<Json<Vec<Grant>>, ApiError> {
    let principal = actor(&state, &headers)?;
    Ok(Json(state.fabric.list_grants(&principal, &filter).await?))
}

async fn get_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(grant_id): Path<Uuid>,
) -> Result<Json<Grant>, ApiError> {
    let principal = actor(&state, &headers)?;
    Ok(Json(state.fabric.get_grant(&principal, grant_id).await?))
}

async fn revoke_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(grant_id): Path<Uuid>,
) -> Result<Json<Grant>, ApiError> {
    let principal = actor(&state, &headers)?;
    Ok(Json(state.fabric.revoke_grant(&principal, grant_id).await?))
}

async fn restore_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(grant_id): Path<Uuid>,
) -> Result<Json<Grant>, ApiError> {
    let principal = actor(&state, &headers)?;
    Ok(Json(state.fabric.restore_grant(&principal, grant_id).await?))
}

async fn renew_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(grant_id): Path<Uuid>,
    Json(body): Json<RenewGrant>,
) -> Result<Json<Grant>, ApiError> {
    let principal = actor(&state, &headers)?;
    Ok(Json(state.fabric.renew_grant(&principal, grant_id, body.ttl_seconds).await?))
}

/// Bridge-side verification of an envelope grant before acceptance (§6.1).
/// Asymmetric backend signature required; legacy HS256 and dev tokens refuse.
async fn delivery_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(grant_id): Path<Uuid>,
    Json(body): Json<DeliveryCheckRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let value = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    let principal = signed_principal_from_authorization(&state.auth, value).map_err(|_| unauthenticated())?;
    let verified: DeliveryCheck = state.fabric.delivery_check(&principal, grant_id, body).await?;
    Ok(([(axum::http::header::CACHE_CONTROL, "no-store")], Json(verified)))
}

/// Live authority for the backend issuer. Not a credential.
async fn grant_issuance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(grant_id): Path<Uuid>,
    Json(body): Json<GrantIssuanceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let principal = actor(&state, &headers)?;
    let issuance: GrantIssuance = state.fabric.grant_issuance(&principal, grant_id, body).await?;
    Ok(([(axum::http::header::CACHE_CONTROL, "no-store")], Json(issuance)))
}
