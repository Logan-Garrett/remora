use crate::sandbox;
use sqlx::PgPool;
use std::path::Path;
use uuid::Uuid;

/// Check if the session is within its token quota.
/// Returns Ok(()) if allowed, Err with a message if over quota.
pub async fn check_quota(db: &PgPool, session_id: Uuid, global_cap: i64) -> anyhow::Result<()> {
    // Reset date if needed
    sqlx::query(
        "UPDATE sessions SET tokens_used_today = 0, tokens_reset_date = CURRENT_DATE \
         WHERE id = $1 AND tokens_reset_date < CURRENT_DATE",
    )
    .bind(session_id)
    .execute(db)
    .await?;

    // Check session cap
    let (used, cap): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(tokens_used_today, 0), COALESCE(daily_token_cap, 1000000) \
         FROM sessions WHERE id = $1",
    )
    .bind(session_id)
    .fetch_one(db)
    .await?;

    if used >= cap {
        anyhow::bail!("Session daily token cap reached ({used}/{cap})");
    }

    // Check global daily cap (sum of all sessions today)
    let global_used: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(tokens_used_today), 0)::BIGINT FROM sessions \
         WHERE tokens_reset_date = CURRENT_DATE",
    )
    .fetch_one(db)
    .await?;

    if global_used >= global_cap {
        anyhow::bail!("Global daily token cap reached ({global_used}/{global_cap})");
    }

    Ok(())
}

/// Record token usage for a session.
pub async fn record_usage(db: &PgPool, session_id: Uuid, tokens: i64) -> anyhow::Result<()> {
    // Reset date if needed, then add
    sqlx::query(
        "UPDATE sessions SET tokens_used_today = 0, tokens_reset_date = CURRENT_DATE \
         WHERE id = $1 AND tokens_reset_date < CURRENT_DATE",
    )
    .bind(session_id)
    .execute(db)
    .await?;

    sqlx::query(
        "UPDATE sessions SET tokens_used_today = tokens_used_today + $1 WHERE id = $2",
    )
    .bind(tokens)
    .bind(session_id)
    .execute(db)
    .await?;

    Ok(())
}

/// Find sessions idle longer than `idle_timeout` seconds, destroy sandbox, delete workspace.
pub async fn check_idle_sessions(
    db: &PgPool,
    workspace_dir: &Path,
    idle_timeout_secs: u64,
) -> anyhow::Result<()> {
    let interval = format!("{idle_timeout_secs} seconds");
    let rows = sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM sessions WHERE idle_since IS NOT NULL \
         AND idle_since < now() - ($1::text || ' seconds')::interval",
    )
    .bind(&interval)
    .fetch_all(db)
    .await?;

    for (session_id,) in rows {
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

        // Clear idle_since so we don't repeatedly try
        let _ = sqlx::query("UPDATE sessions SET idle_since = NULL WHERE id = $1")
            .bind(session_id)
            .execute(db)
            .await;
    }

    Ok(())
}
