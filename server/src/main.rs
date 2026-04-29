mod claude;
mod commands;
mod context;
mod fetch;
mod quota;
mod sandbox;
mod state;
mod ws;

use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use remora_common::SessionInfo;
use state::{AppState, Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let team_token = std::env::var("REMORA_TEAM_TOKEN").expect("REMORA_TEAM_TOKEN must be set");
    let bind = std::env::var("REMORA_BIND").unwrap_or_else(|_| "0.0.0.0:7200".into());

    let config = Config::from_env();
    tracing::info!("workspace dir: {:?}", config.workspace_dir);
    tracing::info!("run timeout: {}s", config.run_timeout_secs);
    tracing::info!("idle timeout: {}s", config.idle_timeout_secs);

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&database_url)
        .await?;

    sqlx::migrate!("../migrations").run(&pool).await?;
    tracing::info!("migrations applied");

    // Ensure workspace directory exists
    tokio::fs::create_dir_all(&config.workspace_dir).await?;

    let state = AppState::new(pool.clone(), team_token, config);
    let shared = Arc::new(state);

    // Spawn the LISTEN/NOTIFY dispatcher
    let listener_state = Arc::clone(&shared);
    tokio::spawn(async move {
        if let Err(e) = state::run_pg_listener(listener_state).await {
            tracing::error!("pg listener died: {e}");
        }
    });

    // Spawn idle session cleanup task (every 60 seconds)
    let cleanup_state = Arc::clone(&shared);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(e) = quota::check_idle_sessions(
                &cleanup_state.db,
                &cleanup_state.config.workspace_dir,
                cleanup_state.config.idle_timeout_secs,
            )
            .await
            {
                tracing::warn!("idle cleanup error: {e}");
            }
        }
    });

    let app = Router::new()
        .route("/sessions", post(create_session))
        .route("/sessions", get(list_sessions))
        .route("/sessions/:id", get(ws_upgrade))
        .route("/sessions/:id", delete(delete_session))
        .with_state(shared);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("remora server listening on {bind}");
    axum::serve(listener, app).await?;
    Ok(())
}

// --- Auth helper ---

fn check_token(state: &AppState, raw: &str) -> bool {
    use subtle::ConstantTimeEq;
    let provided = raw.strip_prefix("Bearer ").unwrap_or(raw);
    provided.as_bytes().ct_eq(state.team_token.as_bytes()).into()
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

    let desc = body.description.unwrap_or_default();
    let result = sqlx::query_as::<_, (Uuid, String, chrono::DateTime<chrono::Utc>)>(
        "INSERT INTO sessions (description) VALUES ($1) RETURNING id, description, created_at",
    )
    .bind(&desc)
    .fetch_one(&state.db)
    .await;

    match result {
        Ok((id, description, created_at)) => {
            // Create workspace directory for this session
            let session_dir = state.config.workspace_dir.join(id.to_string());
            if let Err(e) = tokio::fs::create_dir_all(&session_dir).await {
                tracing::error!("failed to create workspace for session {id}: {e}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "failed to create workspace")
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
                    .args([
                        "clone",
                        git_url,
                        repo_dir.to_str().unwrap_or("."),
                    ])
                    .output()
                    .await;

                match output {
                    Ok(o) if o.status.success() => {
                        let _ = sqlx::query(
                            "INSERT INTO session_repos (session_id, name, git_url) \
                             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                        )
                        .bind(id)
                        .bind(&repo_name)
                        .bind(git_url)
                        .execute(&state.db)
                        .await;
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

    let result = sqlx::query_as::<_, (Uuid, String, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, description, created_at FROM sessions ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await;

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
    let result = sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session_id)
        .execute(&state.db)
        .await;

    match result {
        Ok(r) => {
            if r.rows_affected() == 0 {
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
