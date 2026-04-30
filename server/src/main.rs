use remora_server::db;
use remora_server::state::{AppState, Config};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let team_token = std::env::var("REMORA_TEAM_TOKEN").expect("REMORA_TEAM_TOKEN must be set");
    let bind = std::env::var("REMORA_BIND").unwrap_or_else(|_| "0.0.0.0:7200".into());
    let db_provider = std::env::var("REMORA_DB_PROVIDER").unwrap_or_else(|_| "postgres".into());

    let config = Config::from_env();
    tracing::info!("workspace dir: {:?}", config.workspace_dir);
    tracing::info!("run timeout: {}s", config.run_timeout_secs);
    tracing::info!("idle timeout: {}s", config.idle_timeout_secs);
    tracing::info!("db provider: {db_provider}");
    tracing::info!("skip permissions: {}", config.skip_permissions);

    let backend = db::create_backend(&db_provider, &database_url).await?;
    let db_arc = Arc::new(backend);

    // Run migrations
    {
        use db::Database;
        db_arc.run_migrations().await?;
    }
    tracing::info!("migrations applied");

    // Ensure workspace directory exists
    tokio::fs::create_dir_all(&config.workspace_dir).await?;

    let state = AppState::new(db_arc, team_token, config);
    let shared = Arc::new(state);

    // Spawn the event notification dispatcher
    let listener_state = Arc::clone(&shared);
    tokio::spawn(async move {
        if let Err(e) = remora_server::state::run_event_listener(listener_state).await {
            tracing::error!("event listener died: {e}");
        }
    });

    // Spawn idle session cleanup task (every 60 seconds)
    let cleanup_state = Arc::clone(&shared);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(e) = remora_server::quota::check_idle_sessions(
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

    let app = remora_server::build_router(shared);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("remora server listening on {bind}");
    axum::serve(listener, app).await?;
    Ok(())
}
