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
    CreateThread, Message, Participant, Principal, PrincipalKind, PublishMessage, Role,
    ScopeBinding, Thread, ThreadId, WakeEvent,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::{principal_from_authorization, principal_for_thread, ThreadOperation};
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
        .route("/v1/threads/{thread_id}/events", get(thread_events))
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

fn actor(state: &AppState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let value = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    principal_from_authorization(&state.auth, value).map_err(|_| ApiError {
        status: StatusCode::UNAUTHORIZED,
        code: "unauthenticated",
    })
}

async fn thread_authority(state: &AppState, headers: &HeaderMap, thread_id: Uuid, operation: ThreadOperation) -> Result<(Principal, Option<u64>), ApiError> {
    let value = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    let (principal, generation) = principal_for_thread(&state.auth, value, thread_id, operation).map_err(|_| ApiError {
        status: StatusCode::UNAUTHORIZED,
        code: "unauthenticated",
    })?;
    if let Some(generation) = generation {
        state.fabric.validate_grant_generation(&principal, ThreadId(thread_id), generation).await?;
    }
    Ok((principal, generation))
}

async fn read_authorized(
    state: &AppState, headers: &HeaderMap, thread_id: Uuid, after_seq: u64, limit: usize,
) -> Result<(Thread, Vec<Message>), ApiError> {
    let (principal, generation) = thread_authority(state, headers, thread_id, ThreadOperation::Read).await?;
    if let Some(generation) = generation {
        return Ok(state.fabric.read_scoped(&principal, ThreadId(thread_id), generation, after_seq, limit).await?);
    }
    let thread = state.fabric.get_thread(&principal, ThreadId(thread_id)).await?;
    let messages = if limit == 0 { Vec::new() } else {
        state.fabric.read_messages(&principal, ThreadId(thread_id), after_seq, limit).await?
    };
    Ok((thread, messages))
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
    let (thread, _) = read_authorized(&state, &headers, thread_id, 0, 0).await?;
    Ok(Json(thread))
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
    let (principal, generation) = thread_authority(&state, &headers, thread_id, ThreadOperation::Publish).await?;
    body.expected_grant_generation = generation;
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
    let (_, messages) = read_authorized(&state, &headers, thread_id, q.after_seq, q.limit.clamp(1, 200)).await?;
    Ok(Json(messages))
}

async fn thread_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<Uuid>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let _ = read_authorized(&state, &headers, thread_id, 0, 0).await?;

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
                // Revalidate the token as well as persisted membership. Quiet
                // streams must not retain authority after credential expiry.
                let authorized = read_authorized(&state, &headers, thread_id, 0, 0).await.is_ok();
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
