//! Phase 4: Admin & Observability endpoints.
//!
//! All `/admin/*` endpoints require admin access (admin team token, or a JWT/API key
//! with role == "admin"). The `/metrics` endpoint is unauthenticated (like `/health`).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::Database;
use crate::state::AppState;
use crate::{check_any_token, extract_token, TokenKind};

// ── Auth helper ─────────────────────────────────────────────────────

/// Verify that the request carries admin credentials.
/// Accepts: admin team token, JWT with role=="admin", or API key with role=="admin".
async fn require_admin(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), (StatusCode, &'static str)> {
    let token = extract_token(headers).ok_or((StatusCode::UNAUTHORIZED, "missing token"))?;
    match check_any_token(state, token).await {
        TokenKind::Admin => Ok(()),
        TokenKind::UserJwt(user) | TokenKind::ApiKey(user) if user.role == "admin" => Ok(()),
        _ => Err((StatusCode::FORBIDDEN, "admin access required")),
    }
}

// ── Usage dashboard ─────────────────────────────────────────────────

pub async fn get_usage(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &headers).await {
        return e.into_response();
    }

    let sessions = match state.db.get_usage_summary().await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("get_usage_summary: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    };

    let global = match state.db.get_global_usage_summary().await {
        Ok(g) => g,
        Err(e) => {
            tracing::error!("get_global_usage_summary: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    };

    Json(serde_json::json!({
        "sessions": sessions,
        "global": global,
    }))
    .into_response()
}

// ── Run analytics ───────────────────────────────────────────────────

pub async fn get_analytics(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &headers).await {
        return e.into_response();
    }

    match state.db.get_run_analytics().await {
        Ok(analytics) => Json(analytics).into_response(),
        Err(e) => {
            tracing::error!("get_run_analytics: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

// ── Admin sessions ──────────────────────────────────────────────────

pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &headers).await {
        return e.into_response();
    }

    match state.db.list_all_sessions_admin().await {
        Ok(sessions) => Json(sessions).into_response(),
        Err(e) => {
            tracing::error!("list_all_sessions_admin: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateQuotaBody {
    pub daily_token_cap: i64,
}

pub async fn update_session_quota(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<Uuid>,
    Json(body): Json<UpdateQuotaBody>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &headers).await {
        return e.into_response();
    }

    if body.daily_token_cap < 0 {
        return (
            StatusCode::BAD_REQUEST,
            "daily_token_cap must be non-negative",
        )
            .into_response();
    }

    match state.db.session_exists(session_id).await {
        Ok(false) => return (StatusCode::NOT_FOUND, "session not found").into_response(),
        Err(e) => {
            tracing::error!("session_exists: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
        _ => {}
    }

    if let Err(e) = state
        .db
        .update_session_quota(session_id, body.daily_token_cap)
        .await
    {
        tracing::error!("update_session_quota: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
    }

    // Audit log
    let _ = state
        .db
        .insert_audit_event(
            None,
            "update_quota",
            "session",
            Some(&session_id.to_string()),
            Some(serde_json::json!({"daily_token_cap": body.daily_token_cap})),
            None,
        )
        .await;

    StatusCode::NO_CONTENT.into_response()
}

pub async fn force_delete_session(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &headers).await {
        return e.into_response();
    }

    // Destroy sandbox if it exists
    let _ = crate::sandbox::destroy_sandbox(session_id).await;

    // Delete workspace directory
    let session_dir = state.config.workspace_dir.join(session_id.to_string());
    if session_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&session_dir).await;
    }

    // Clean up in-memory ownership tracking
    state.clear_session_owner(session_id).await;

    match state.db.delete_session(session_id).await {
        Ok(0) => (StatusCode::NOT_FOUND, "session not found").into_response(),
        Ok(_) => {
            // Audit log
            let _ = state
                .db
                .insert_audit_event(
                    None,
                    "admin_delete_session",
                    "session",
                    Some(&session_id.to_string()),
                    None,
                    None,
                )
                .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            tracing::error!("admin delete_session {session_id}: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

pub async fn force_expire_session(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &headers).await {
        return e.into_response();
    }

    let status = state
        .db
        .get_session_status(session_id)
        .await
        .unwrap_or(None);
    match status.as_deref() {
        None => return (StatusCode::NOT_FOUND, "session not found").into_response(),
        Some("expired") => {
            return (StatusCode::BAD_REQUEST, "session is already expired").into_response()
        }
        _ => {}
    }

    if let Err(e) = state.db.set_session_expired(session_id).await {
        tracing::error!("force_expire_session {session_id}: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
    }

    // Audit log
    let _ = state
        .db
        .insert_audit_event(
            None,
            "admin_expire_session",
            "session",
            Some(&session_id.to_string()),
            None,
            None,
        )
        .await;

    StatusCode::NO_CONTENT.into_response()
}

// ── Admin users ─────────────────────────────────────────────────────

pub async fn list_users(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &headers).await {
        return e.into_response();
    }

    match state.db.list_all_users().await {
        Ok(users) => Json(users).into_response(),
        Err(e) => {
            tracing::error!("list_all_users: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateRoleBody {
    pub role: String,
}

pub async fn update_user_role(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(user_id): Path<Uuid>,
    Json(body): Json<UpdateRoleBody>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &headers).await {
        return e.into_response();
    }

    match body.role.as_str() {
        "admin" | "member" | "viewer" | "guest" => {}
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "role must be admin, member, viewer, or guest",
            )
                .into_response();
        }
    }

    match state.db.get_user_by_id(user_id).await {
        Ok(None) => return (StatusCode::NOT_FOUND, "user not found").into_response(),
        Err(e) => {
            tracing::error!("get_user_by_id: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
        Ok(Some(_)) => {}
    }

    if let Err(e) = state.db.update_user_role(user_id, &body.role).await {
        tracing::error!("update_user_role: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
    }

    // Audit log
    let _ = state
        .db
        .insert_audit_event(
            None,
            "update_user_role",
            "user",
            Some(&user_id.to_string()),
            Some(serde_json::json!({"role": body.role})),
            None,
        )
        .await;

    StatusCode::NO_CONTENT.into_response()
}

// ── Audit log ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AuditQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

pub async fn list_audit_events(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<AuditQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &headers).await {
        return e.into_response();
    }

    let limit = query.limit.clamp(1, 500);
    let offset = query.offset.max(0);

    match state.db.list_audit_events(limit, offset).await {
        Ok(events) => Json(events).into_response(),
        Err(e) => {
            tracing::error!("list_audit_events: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

// ── Prometheus metrics ──────────────────────────────────────────────

pub async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let data = match state.db.get_metrics_data().await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("get_metrics_data: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to gather metrics",
            )
                .into_response();
        }
    };

    // Count WebSocket subscribers across all sessions
    let ws_connections = {
        let subs = state.subscribers.read().await;
        subs.values().map(|list| list.len()).sum::<usize>()
    };

    let body = format!(
        "# HELP remora_sessions_total Total number of sessions.\n\
         # TYPE remora_sessions_total gauge\n\
         remora_sessions_total {}\n\
         # HELP remora_sessions_active Number of active sessions.\n\
         # TYPE remora_sessions_active gauge\n\
         remora_sessions_active {}\n\
         # HELP remora_websocket_connections Current WebSocket connections.\n\
         # TYPE remora_websocket_connections gauge\n\
         remora_websocket_connections {}\n\
         # HELP remora_tokens_used_total Total tokens used today.\n\
         # TYPE remora_tokens_used_total counter\n\
         remora_tokens_used_total {}\n\
         # HELP remora_runs_total Total runs by status.\n\
         # TYPE remora_runs_total counter\n\
         remora_runs_total{{status=\"success\"}} {}\n\
         remora_runs_total{{status=\"failed\"}} {}\n\
         remora_runs_total{{status=\"timeout\"}} {}\n",
        data.sessions_total,
        data.sessions_active,
        ws_connections,
        data.tokens_used_total,
        data.runs_successful,
        data.runs_failed,
        data.runs_timed_out,
    );

    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}
