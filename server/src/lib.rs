//! Library re-exports for integration tests.
//!
//! The binary entry point lives in `main.rs`; this crate root
//! exposes the internal modules so that `tests/` integration tests
//! can build routers, create `AppState`, etc.

pub mod admin;
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
    routing::{delete, get, post, put},
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
    // Session tokens cannot create new sessions
    match check_any_token(&state, token).await {
        TokenKind::Admin | TokenKind::UserJwt(_) | TokenKind::ApiKey(_) => {}
        _ => return (StatusCode::UNAUTHORIZED, "bad token").into_response(),
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
    match check_any_token(&state, token).await {
        TokenKind::Admin | TokenKind::UserJwt(_) | TokenKind::ApiKey(_) => {}
        _ => return (StatusCode::UNAUTHORIZED, "bad token").into_response(),
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
    let token_kind = check_any_token(&state, token).await;

    // Only admin token, admin-role users, or the session owner may delete
    let allowed = match &token_kind {
        TokenKind::Admin => true,
        TokenKind::UserJwt(user) | TokenKind::ApiKey(user) => {
            if auth::role_level(&user.role) >= auth::role_level("admin") {
                true
            } else {
                // Check in-memory owner (display_name match)
                state
                    .get_session_owner(session_id)
                    .await
                    .map(|owner| owner == user.display_name)
                    .unwrap_or(false)
            }
        }
        _ => false,
    };
    if !allowed {
        return (
            StatusCode::FORBIDDEN,
            "only session owner or admin can delete",
        )
            .into_response();
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
    match check_any_token(&state, token).await {
        TokenKind::Admin | TokenKind::UserJwt(_) | TokenKind::ApiKey(_) => {}
        _ => return (StatusCode::UNAUTHORIZED, "bad token").into_response(),
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

    // Cross-team isolation: if session belongs to a team, check membership.
    // Admin token bypasses team checks. Session tokens are already scoped.
    if let Ok(Some(team_id)) = state.db.get_session_team(session_id).await {
        match &token_kind {
            TokenKind::Admin | TokenKind::Session(_) => {
                // Admin and session tokens bypass team checks
            }
            TokenKind::UserJwt(user) | TokenKind::ApiKey(user) => {
                match state.db.get_team_member_role(team_id, user.id).await {
                    Ok(None) => {
                        return (StatusCode::FORBIDDEN, "not a member of the session's team")
                            .into_response();
                    }
                    Err(e) => {
                        tracing::error!("get_team_member_role (ws): {e}");
                        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
                    }
                    Ok(Some(_)) => {} // authorized
                }
            }
            TokenKind::Invalid => unreachable!(),
        }
    }

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
    // Session tokens cannot create new tokens
    match check_any_token(&state, token).await {
        TokenKind::Admin | TokenKind::UserJwt(_) | TokenKind::ApiKey(_) => {}
        _ => {
            return (StatusCode::UNAUTHORIZED, "admin token required").into_response();
        }
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

// --- Team types ---

#[derive(Deserialize)]
struct CreateTeamBody {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct UpdateTeamBody {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct AddTeamMemberBody {
    user_id: Uuid,
    #[serde(default = "default_member_role")]
    role: String,
}

fn default_member_role() -> String {
    "member".to_string()
}

#[derive(Deserialize)]
struct UpdateTeamMemberBody {
    role: String,
}

#[derive(Deserialize)]
struct CreateTeamSessionBody {
    #[serde(default)]
    description: String,
}

/// Extract the authenticated user from the request (JWT or API key).
/// Returns None if the user is not authenticated via user credentials.
async fn extract_user(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Option<remora_common::User> {
    let token = extract_token(headers)?;
    match check_any_token(state, token).await {
        TokenKind::UserJwt(user) | TokenKind::ApiKey(user) => Some(user),
        _ => None,
    }
}

// --- Team REST handlers ---

async fn create_team(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateTeamBody>,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    if body.name.is_empty() {
        return (StatusCode::BAD_REQUEST, "team name required").into_response();
    }

    match state
        .db
        .create_team(&body.name, &body.description, user.id)
        .await
    {
        Ok(team_id) => {
            // Add creator as admin
            if let Err(e) = state.db.add_team_member(team_id, user.id, "admin").await {
                tracing::error!("add_team_member (creator): {e}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
            }
            match state.db.get_team(team_id).await {
                Ok(Some(team)) => (StatusCode::CREATED, Json(team)).into_response(),
                Ok(None) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "team created but not found",
                )
                    .into_response(),
                Err(e) => {
                    tracing::error!("get_team: {e}");
                    (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
                }
            }
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("UNIQUE") || msg.contains("unique") || msg.contains("duplicate") {
                (StatusCode::CONFLICT, "team name already exists").into_response()
            } else {
                tracing::error!("create_team: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
            }
        }
    }
}

async fn list_teams(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    match state.db.list_teams_for_user(user.id).await {
        Ok(teams) => Json(teams).into_response(),
        Err(e) => {
            tracing::error!("list_teams_for_user: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

async fn get_team(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(team_id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    // Check membership
    match state.db.get_team_member_role(team_id, user.id).await {
        Ok(None) => return (StatusCode::FORBIDDEN, "not a team member").into_response(),
        Err(e) => {
            tracing::error!("get_team_member_role: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
        Ok(Some(_)) => {}
    }

    match state.db.get_team(team_id).await {
        Ok(Some(team)) => Json(team).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "team not found").into_response(),
        Err(e) => {
            tracing::error!("get_team: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

async fn update_team_endpoint(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(team_id): Path<Uuid>,
    Json(body): Json<UpdateTeamBody>,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    // Check admin role
    match state.db.get_team_member_role(team_id, user.id).await {
        Ok(Some(role)) if role == "admin" => {}
        Ok(Some(_)) => {
            return (StatusCode::FORBIDDEN, "team admin required").into_response();
        }
        Ok(None) => return (StatusCode::FORBIDDEN, "not a team member").into_response(),
        Err(e) => {
            tracing::error!("get_team_member_role: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    }

    if body.name.is_empty() {
        return (StatusCode::BAD_REQUEST, "team name required").into_response();
    }

    match state
        .db
        .update_team(team_id, &body.name, &body.description)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("UNIQUE") || msg.contains("unique") || msg.contains("duplicate") {
                (StatusCode::CONFLICT, "team name already exists").into_response()
            } else {
                tracing::error!("update_team: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
            }
        }
    }
}

async fn delete_team_endpoint(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(team_id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    // Check admin role
    match state.db.get_team_member_role(team_id, user.id).await {
        Ok(Some(role)) if role == "admin" => {}
        Ok(Some(_)) => {
            return (StatusCode::FORBIDDEN, "team admin required").into_response();
        }
        Ok(None) => return (StatusCode::FORBIDDEN, "not a team member").into_response(),
        Err(e) => {
            tracing::error!("get_team_member_role: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    }

    match state.db.delete_team(team_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("delete_team: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

// --- Team member REST handlers ---

async fn add_team_member(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(team_id): Path<Uuid>,
    Json(body): Json<AddTeamMemberBody>,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    // Check admin role
    match state.db.get_team_member_role(team_id, user.id).await {
        Ok(Some(role)) if role == "admin" => {}
        Ok(Some(_)) => {
            return (StatusCode::FORBIDDEN, "team admin required").into_response();
        }
        Ok(None) => return (StatusCode::FORBIDDEN, "not a team member").into_response(),
        Err(e) => {
            tracing::error!("get_team_member_role: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    }

    let role = if body.role.is_empty() {
        "member"
    } else {
        match body.role.as_str() {
            "admin" | "member" | "viewer" => body.role.as_str(),
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    "role must be admin, member, or viewer",
                )
                    .into_response();
            }
        }
    };

    match state.db.add_team_member(team_id, body.user_id, role).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => {
            tracing::error!("add_team_member: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

async fn list_team_members(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(team_id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    // Check membership
    match state.db.get_team_member_role(team_id, user.id).await {
        Ok(None) => return (StatusCode::FORBIDDEN, "not a team member").into_response(),
        Err(e) => {
            tracing::error!("get_team_member_role: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
        Ok(Some(_)) => {}
    }

    match state.db.list_team_members(team_id).await {
        Ok(members) => Json(members).into_response(),
        Err(e) => {
            tracing::error!("list_team_members: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

async fn update_team_member(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((team_id, member_uid)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateTeamMemberBody>,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    // Check admin role
    match state.db.get_team_member_role(team_id, user.id).await {
        Ok(Some(role)) if role == "admin" => {}
        Ok(Some(_)) => {
            return (StatusCode::FORBIDDEN, "team admin required").into_response();
        }
        Ok(None) => return (StatusCode::FORBIDDEN, "not a team member").into_response(),
        Err(e) => {
            tracing::error!("get_team_member_role: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    }

    match body.role.as_str() {
        "admin" | "member" | "viewer" => {}
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "role must be admin, member, or viewer",
            )
                .into_response();
        }
    }

    match state
        .db
        .update_team_member_role(team_id, member_uid, &body.role)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("update_team_member_role: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

async fn remove_team_member_endpoint(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((team_id, member_uid)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    // Allow self-removal or admin removal
    let is_self = user.id == member_uid;
    if !is_self {
        match state.db.get_team_member_role(team_id, user.id).await {
            Ok(Some(role)) if role == "admin" => {}
            Ok(Some(_)) => {
                return (StatusCode::FORBIDDEN, "team admin required").into_response();
            }
            Ok(None) => return (StatusCode::FORBIDDEN, "not a team member").into_response(),
            Err(e) => {
                tracing::error!("get_team_member_role: {e}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
            }
        }
    }

    match state.db.remove_team_member(team_id, member_uid).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("remove_team_member: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

// --- Team-scoped session handlers ---

async fn create_team_session(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(team_id): Path<Uuid>,
    Json(body): Json<CreateTeamSessionBody>,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    // Check membership (member or admin can create sessions; viewer cannot)
    match state.db.get_team_member_role(team_id, user.id).await {
        Ok(Some(role)) if role == "admin" || role == "member" => {}
        Ok(Some(_)) => {
            return (StatusCode::FORBIDDEN, "team member or admin role required").into_response();
        }
        Ok(None) => return (StatusCode::FORBIDDEN, "not a team member").into_response(),
        Err(e) => {
            tracing::error!("get_team_member_role: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
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

    match state
        .db
        .create_session_for_team(&body.description, team_id)
        .await
    {
        Ok((id, description, created_at)) => {
            // Generate and persist the owner_key
            let owner_key = Uuid::new_v4().to_string();
            if let Err(e) = state.db.set_owner_key(id, &owner_key).await {
                tracing::error!("failed to set owner_key for team session {id}: {e}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
            }

            // Create workspace directory
            let session_dir = state.config.workspace_dir.join(id.to_string());
            if let Err(e) = tokio::fs::create_dir_all(&session_dir).await {
                tracing::error!("failed to create workspace for team session {id}: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to create workspace",
                )
                    .into_response();
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
            tracing::error!("create_session_for_team: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

async fn list_team_sessions(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(team_id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    // Check membership
    match state.db.get_team_member_role(team_id, user.id).await {
        Ok(None) => return (StatusCode::FORBIDDEN, "not a team member").into_response(),
        Err(e) => {
            tracing::error!("get_team_member_role: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
        Ok(Some(_)) => {}
    }

    match state.db.list_sessions_for_team(team_id).await {
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
            tracing::error!("list_sessions_for_team: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

// --- Dashboard handler ---

async fn user_dashboard(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let Some(user) = extract_user(&state, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "JWT or API key required").into_response();
    };

    match state.db.list_sessions_for_user(user.id).await {
        Ok(rows) => {
            let sessions: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(id, description, created_at, status, team_name)| {
                    serde_json::json!({
                        "id": id,
                        "description": description,
                        "created_at": created_at,
                        "status": status,
                        "team_name": team_name,
                    })
                })
                .collect();
            Json(serde_json::json!({
                "user": user,
                "sessions": sessions,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("list_sessions_for_user: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
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
        // Teams
        .route("/teams", post(create_team))
        .route("/teams", get(list_teams))
        .route("/teams/:id", get(get_team))
        .route("/teams/:id", put(update_team_endpoint))
        .route("/teams/:id", delete(delete_team_endpoint))
        // Team members
        .route("/teams/:id/members", post(add_team_member))
        .route("/teams/:id/members", get(list_team_members))
        .route("/teams/:id/members/:uid", put(update_team_member))
        .route(
            "/teams/:id/members/:uid",
            delete(remove_team_member_endpoint),
        )
        // Team sessions
        .route("/teams/:id/sessions", post(create_team_session))
        .route("/teams/:id/sessions", get(list_team_sessions))
        // Dashboard
        .route("/dashboard", get(user_dashboard))
        // Admin endpoints
        .route("/admin/usage", get(admin::get_usage))
        .route("/admin/analytics", get(admin::get_analytics))
        .route("/admin/sessions", get(admin::list_sessions))
        .route(
            "/admin/sessions/:id/quota",
            put(admin::update_session_quota),
        )
        .route("/admin/sessions/:id", delete(admin::force_delete_session))
        .route(
            "/admin/sessions/:id/expire",
            post(admin::force_expire_session),
        )
        .route("/admin/users", get(admin::list_users))
        .route("/admin/users/:id/role", put(admin::update_user_role))
        .route("/admin/audit", get(admin::list_audit_events))
        .route("/metrics", get(admin::metrics))
        .layer(ConcurrencyLimitLayer::new(100))
        // NOTE: Permissive CORS is a pre-existing configuration (predates auth branch).
        // Should be tightened to specific origins in production. Tracked separately.
        .layer(CorsLayer::permissive())
        .with_state(shared)
}
