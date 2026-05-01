//! Library re-exports for integration tests.
//!
//! The binary entry point lives in `main.rs`; this crate root
//! exposes the internal modules so that `tests/` integration tests
//! can build routers, create `AppState`, etc.

pub mod claude;
pub mod commands;
pub mod context;
pub mod db;
pub mod fetch;
pub mod quota;
pub mod sandbox;
pub mod state;
pub mod ws;

use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use db::Database;
use remora_common::SessionInfo;
use serde::Deserialize;
use std::sync::Arc;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use state::AppState;

// --- Auth helpers (re-exported for tests) ---

pub fn check_token(state: &AppState, raw: &str) -> bool {
    use subtle::ConstantTimeEq;
    let provided = raw.strip_prefix("Bearer ").unwrap_or(raw);
    provided
        .as_bytes()
        .ct_eq(state.team_token.as_bytes())
        .into()
}

fn extract_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers.get("authorization")?.to_str().ok()
}

// --- REST handlers ---

#[derive(Deserialize)]
struct CreateSession {
    description: Option<String>,
    #[serde(default)]
    repos: Vec<String>,
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateSession>,
) -> impl IntoResponse {
    let Some(token) = extract_token(&headers) else {
        return (StatusCode::UNAUTHORIZED, "missing token").into_response();
    };
    if !check_token(&state, token) {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }

    // Enforce max sessions limit
    match state.db.count_sessions().await {
        Ok(count) if count >= state.config.max_sessions as i64 => {
            return (StatusCode::TOO_MANY_REQUESTS, "session limit reached").into_response();
        }
        Err(e) => {
            tracing::error!("count sessions: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
        _ => {}
    }

    // Validate git URL schemes (reject file://, ftp://, bare paths)
    for git_url in &body.repos {
        if !is_safe_git_url(git_url) {
            return (
                StatusCode::BAD_REQUEST,
                format!("rejected git URL: {git_url} (only https://, ssh://, and git:// schemes are allowed)"),
            )
                .into_response();
        }
    }

    let desc = body.description.unwrap_or_default();
    let result = state.db.create_session(&desc).await;

    match result {
        Ok((id, description, created_at)) => {
            // Create workspace directory for this session
            let session_dir = state.config.workspace_dir.join(id.to_string());
            if let Err(e) = tokio::fs::create_dir_all(&session_dir).await {
                tracing::error!("failed to create workspace for session {id}: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to create workspace",
                )
                    .into_response();
            }

            // Clone repos if provided
            for git_url in &body.repos {
                let repo_name = git_url
                    .rsplit('/')
                    .next()
                    .unwrap_or("repo")
                    .trim_end_matches(".git")
                    .to_string();

                let repo_dir = session_dir.join(&repo_name);
                let output = tokio::process::Command::new("git")
                    .args(["clone", git_url, repo_dir.to_str().unwrap_or(".")])
                    .output()
                    .await;

                match output {
                    Ok(o) if o.status.success() => {
                        let _ = state.db.upsert_repo(id, &repo_name, git_url).await;
                        tracing::info!("cloned {git_url} for session {id}");
                    }
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        tracing::warn!("git clone {git_url} failed: {stderr}");
                    }
                    Err(e) => {
                        tracing::warn!("git clone {git_url} error: {e}");
                    }
                }
            }

            let info = SessionInfo {
                id,
                description,
                created_at,
            };
            (StatusCode::CREATED, Json(info)).into_response()
        }
        Err(e) => {
            tracing::error!("create session: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let Some(token) = extract_token(&headers) else {
        return (StatusCode::UNAUTHORIZED, "missing token").into_response();
    };
    if !check_token(&state, token) {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }

    let result = state.db.list_sessions().await;

    match result {
        Ok(rows) => {
            let sessions: Vec<SessionInfo> = rows
                .into_iter()
                .map(|(id, description, created_at)| SessionInfo {
                    id,
                    description,
                    created_at,
                })
                .collect();
            Json(sessions).into_response()
        }
        Err(e) => {
            tracing::error!("list sessions: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(token) = extract_token(&headers) else {
        return (StatusCode::UNAUTHORIZED, "missing token").into_response();
    };
    if !check_token(&state, token) {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }

    // Destroy sandbox if it exists
    let _ = sandbox::destroy_sandbox(session_id).await;

    // Delete workspace directory
    let session_dir = state.config.workspace_dir.join(session_id.to_string());
    if session_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&session_dir).await;
    }

    // Delete from DB (cascades to events, repos, runs, etc.)
    let result = state.db.delete_session(session_id).await;

    match result {
        Ok(rows_affected) => {
            if rows_affected == 0 {
                (StatusCode::NOT_FOUND, "session not found").into_response()
            } else {
                StatusCode::NO_CONTENT.into_response()
            }
        }
        Err(e) => {
            tracing::error!("delete session {session_id}: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

// --- WebSocket upgrade ---

#[derive(Deserialize)]
struct WsQuery {
    token: String,
    name: Option<String>,
}

async fn ws_upgrade(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if !check_token(&state, &query.token) {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }

    let name = query.name.unwrap_or_else(|| "anon".into());
    ws.on_upgrade(move |socket| ws::handle_socket(state, session_id, name, socket))
        .into_response()
}

// --- Health check (no auth) ---

async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.ping().await {
        Ok(()) => Json(serde_json::json!({
            "status": "ok",
            "db": "connected",
        }))
        .into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "unhealthy",
                "db": format!("{e}"),
            })),
        )
            .into_response(),
    }
}

/// Validate that a git URL uses a safe scheme.
/// Only `https://`, `ssh://`, `git://`, and SSH-style `user@host:path` are allowed.
/// Rejects `file://`, `ftp://`, and absolute/relative bare paths.
pub fn is_safe_git_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    if lower.starts_with("https://") || lower.starts_with("ssh://") || lower.starts_with("git://") {
        return true;
    }
    // SSH-style URLs like git@github.com:user/repo.git
    if url.contains('@') && url.contains(':') && !lower.starts_with('/') {
        return true;
    }
    false
}

/// Build the axum `Router` with the given shared state.
/// Extracted so that integration tests can spin up the server easily.
pub fn build_router(shared: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/sessions", post(create_session))
        .route("/sessions", get(list_sessions))
        .route("/sessions/:id", get(ws_upgrade))
        .route("/sessions/:id", delete(delete_session))
        .layer(ConcurrencyLimitLayer::new(100))
        .layer(CorsLayer::permissive())
        .with_state(shared)
}
