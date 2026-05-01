use crate::db::{Database, DatabaseBackend};
use crate::sandbox;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

/// Check if the session is within its token quota.
/// Returns Ok(()) if allowed, Err with a message if over quota.
pub async fn check_quota(
    db: &Arc<DatabaseBackend>,
    session_id: Uuid,
    global_cap: i64,
) -> anyhow::Result<()> {
    // Reset date if needed
    db.reset_tokens_if_needed(session_id).await?;

    // Check session cap
    let (used, cap) = db.get_session_usage(session_id).await?;

    if used >= cap {
        anyhow::bail!("Session daily token cap reached ({used}/{cap})");
    }

    // Check global daily cap (sum of all sessions today)
    let global_used = db.get_global_usage().await?;

    if global_used >= global_cap {
        anyhow::bail!("Global daily token cap reached ({global_used}/{global_cap})");
    }

    Ok(())
}

/// Record token usage for a session.
pub async fn record_usage(
    db: &Arc<DatabaseBackend>,
    session_id: Uuid,
    tokens: i64,
) -> anyhow::Result<()> {
    // Reset date if needed, then add
    db.reset_tokens_if_needed(session_id).await?;
    db.add_usage(session_id, tokens).await?;
    Ok(())
}

/// Find sessions idle longer than `idle_timeout` seconds, destroy sandbox, delete workspace.
pub async fn check_idle_sessions(
    db: &Arc<DatabaseBackend>,
    workspace_dir: &Path,
    idle_timeout_secs: u64,
) -> anyhow::Result<()> {
    let idle_sessions = db.get_idle_sessions(idle_timeout_secs).await?;

    for session_id in idle_sessions {
        tracing::info!("cleaning up idle session {session_id}");

        // Destroy sandbox
        if let Err(e) = sandbox::destroy_sandbox(session_id).await {
            tracing::warn!("failed to destroy sandbox for idle session {session_id}: {e}");
        }

        // Delete workspace directory
        let session_dir = workspace_dir.join(session_id.to_string());
        if session_dir.exists() {
            if let Err(e) = tokio::fs::remove_dir_all(&session_dir).await {
                tracing::warn!("failed to remove workspace for idle session {session_id}: {e}");
            }
        }

        // Mark session as expired and clear idle_since so we don't repeatedly try
        let _ = db.set_session_expired(session_id).await;
        let _ = db.clear_idle_since_for(session_id).await;
    }

    Ok(())
}
