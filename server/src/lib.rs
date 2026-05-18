//! Library re-exports for integration tests.
//!
//! The binary entry point lives in `main.rs`; this crate root
//! exposes the internal modules so that `tests/` integration tests
//! can build routers, create `AppState`, etc.

pub mod auth;
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

#[derive(Debug, Clone)]
pub enum TokenKind {
    Admin,
    Session(Uuid),
    UserJwt(remora_common::User),
    ApiKey(remora_common::User),
    Invalid,
}
pub async fn check_any_token(state: &AppState, raw: &str) -> TokenKind {
    let provided = raw.strip_prefix("Bearer ").unwrap_or(raw);

    // 1. Admin team token (constant-time compare)
    if check_token(state, provided) {
        return TokenKind::Admin;
    }

    // 2. JWT decode (JWTs start with "ey")
    if provided.starts_with("ey") {
        if let Some(claims) = auth::decode_jwt(provided, &state.config.jwt_secret) {
            if let Ok(user_id) = claims.sub.parse::<Uuid>() {
                if let Ok(Some(user)) = state.db.get_user_by_id(user_id).await {
                    return TokenKind::UserJwt(user);
                }
            }
        }
    }

    // 3. Session token (DB lookup)
    if let Ok(Some(session_id)) = state.db.validate_session_token(provided).await {
        return TokenKind::Session(session_id);
    }

    // 4. API key (hash then DB lookup)
    let key_hash = auth::sha256_hex(provided);
    if let Ok(Some(user)) = state.db.validate_api_key(&key_hash).await {
        return TokenKind::ApiKey(user);
    }

    TokenKind::Invalid
}

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
            // Generate and persist the owner_key
            let owner_key = Uuid::new_v4().to_string();
            if let Err(e) = state.db.set_owner_key(id, &owner_key).await {
                tracing::error!("failed to set owner_key for session {id}: {e}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
            }

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

            let invite_token = match state.db.create_session_token(id, "initial").await {
                Ok(tok) => Some(tok),
                Err(e) => {
                    tracing::error!("failed to create initial session token for {id}: {e}");
                    None
                }
            };

            let info = SessionInfo {
                id,
                description,
                created_at,
                status: "active".to_string(),
                owner_key: Some(owner_key),
                invite_token,
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
                .map(|(id, description, created_at, status)| SessionInfo {
                    id,
                    description,
                    created_at,
                    status,
                    owner_key: None,
                    invite_token: None,
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

    // Clean up in-memory ownership tracking
    state.clear_session_owner(session_id).await;

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

async fn reactivate_session(
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

    // Enforce max sessions limit (reactivation adds an active session)
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

    let status = state
        .db
        .get_session_status(session_id)
        .await
        .unwrap_or(None);
    match status.as_deref() {
        None => {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        }
        Some("active") => {
            return (StatusCode::BAD_REQUEST, "session is already active").into_response();
        }
        Some("expired") => {}
        Some(_) => {
            return (StatusCode::BAD_REQUEST, "session cannot be reactivated").into_response();
        }
    }

    if let Err(e) = state.db.reactivate_session(session_id).await {
        tracing::error!("reactivate session {session_id}: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
    }

    let session_dir = state.config.workspace_dir.join(session_id.to_string());
    if let Err(e) = tokio::fs::create_dir_all(&session_dir).await {
        tracing::error!("failed to create workspace for reactivated session {session_id}: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create workspace",
        )
            .into_response();
    }

    tracing::info!("session {session_id} reactivated");
    StatusCode::NO_CONTENT.into_response()
}

// --- WebSocket upgrade ---

#[derive(Deserialize)]
struct WsQuery {
    token: String,
    name: Option<String>,
    owner_key: Option<String>,
}

async fn ws_upgrade(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let token_kind = check_any_token(&state, &query.token).await;
    let jwt_name = match &token_kind {
        TokenKind::Admin => None,
        TokenKind::Session(token_session_id) => {
            if *token_session_id != session_id {
                return (StatusCode::UNAUTHORIZED, "token not valid for this session")
                    .into_response();
            }
            None
        }
        TokenKind::UserJwt(user) => Some(user.display_name.clone()),
        TokenKind::ApiKey(user) => Some(user.display_name.clone()),
        TokenKind::Invalid => {
            return (StatusCode::UNAUTHORIZED, "bad token").into_response();
        }
    };

    // If authenticated via JWT/API key, use the user's display_name;
    // otherwise fall back to ?name= query param.
    let name = jwt_name.unwrap_or_else(|| query.name.unwrap_or_else(|| "anon".into()));
    let owner_key = query.owner_key;
    ws.on_upgrade(move |socket| ws::handle_socket(state, session_id, name, owner_key, socket))
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

#[derive(Deserialize)]
struct CreateTokenBody {
    #[serde(default)]
    label: String,
}
async fn create_session_token_endpoint(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<Uuid>,
    Json(body): Json<CreateTokenBody>,
) -> impl IntoResponse {
    let Some(token) = extract_token(&headers) else {
        return (StatusCode::UNAUTHORIZED, "missing token").into_response();
    };
    if !check_token(&state, token) {
        return (StatusCode::UNAUTHORIZED, "admin token required").into_response();
    }
    match state.db.session_exists(session_id).await {
        Ok(false) => return (StatusCode::NOT_FOUND, "session not found").into_response(),
        Err(e) => {
            tracing::error!("session_exists: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
        _ => {}
    }
    match state.db.create_session_token(session_id, &body.label).await {
        Ok(tok) => Json(serde_json::json!({ "token": tok, "label": body.label })).into_response(),
        Err(e) => {
            tracing::error!("create_session_token: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}
async fn list_session_tokens_endpoint(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(raw_token) = extract_token(&headers) else {
        return (StatusCode::UNAUTHORIZED, "missing token").into_response();
    };
    let is_admin = check_token(&state, raw_token);
    if !is_admin {
        let provided = raw_token.strip_prefix("Bearer ").unwrap_or(raw_token);
        let db_key = state.db.get_owner_key(session_id).await.unwrap_or(None);
        if db_key.as_deref() != Some(provided) {
            return (StatusCode::UNAUTHORIZED, "admin or owner token required").into_response();
        }
    }
    match state.db.list_session_tokens(session_id).await {
        Ok(tokens) => Json(serde_json::json!(tokens)).into_response(),
        Err(e) => {
            tracing::error!("list_session_tokens: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}
async fn revoke_session_token_endpoint(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((session_id, token_id)): Path<(Uuid, i64)>,
) -> impl IntoResponse {
    let Some(raw_token) = extract_token(&headers) else {
        return (StatusCode::UNAUTHORIZED, "missing token").into_response();
    };
    let is_admin = check_token(&state, raw_token);
    if !is_admin {
        let provided = raw_token.strip_prefix("Bearer ").unwrap_or(raw_token);
        let db_key = state.db.get_owner_key(session_id).await.unwrap_or(None);
        if db_key.as_deref() != Some(provided) {
            return (StatusCode::UNAUTHORIZED, "admin or owner token required").into_response();
        }
    }
    let tokens = match state.db.list_session_tokens(session_id).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("list_session_tokens for revoke: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    };
    if !tokens.iter().any(|t| t.id == token_id) {
        return (StatusCode::NOT_FOUND, "token not found for this session").into_response();
    }
    match state
        .db
        .revoke_session_token_by_id(session_id, token_id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("revoke_session_token: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
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
        // Session management
        .route("/sessions", post(create_session))
        .route("/sessions", get(list_sessions))
        .route("/sessions/:id", get(ws_upgrade))
        .route("/sessions/:id", delete(delete_session))
        .route("/sessions/:id/reactivate", post(reactivate_session))
        .route("/sessions/:id/tokens", post(create_session_token_endpoint))
        .route("/sessions/:id/tokens", get(list_session_tokens_endpoint))
        .route(
            "/sessions/:id/tokens/:token_id",
            delete(revoke_session_token_endpoint),
        )
        // Auth endpoints
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/me", get(auth::me))
        .route("/auth/api-keys", post(auth::create_api_key))
        .route("/auth/api-keys", get(auth::list_api_keys))
        .route("/auth/api-keys/:id", delete(auth::revoke_api_key_endpoint))
        // OAuth
        .route("/auth/oauth/github", get(auth::oauth_github_redirect))
        .route(
            "/auth/oauth/github/callback",
            get(auth::oauth_github_callback),
        )
        .route("/auth/oauth/google", get(auth::oauth_google_redirect))
        .route(
            "/auth/oauth/google/callback",
            get(auth::oauth_google_callback),
        )
        .layer(ConcurrencyLimitLayer::new(100))
        .layer(CorsLayer::permissive())
        .with_state(shared)
}
