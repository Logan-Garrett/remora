use crate::context::{self, ContextMode};
use crate::quota;
use crate::state::AppState;
use crate::ws::insert_event;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

/// Run Claude directly on the host (sandbox support is M6).
/// This is meant to be called from a spawned task.
pub async fn run_claude(
    state: Arc<AppState>,
    session_id: Uuid,
    context_mode: ContextMode,
) -> anyhow::Result<()> {
    let db = &state.db;

    // Check no run currently in flight
    if state.is_run_in_flight(session_id).await {
        insert_event(
            db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": "A run is already in progress for this session."}),
        )
        .await?;
        return Ok(());
    }

    // Check quota
    if let Err(e) = quota::check_quota(db, session_id, state.config.global_daily_cap).await {
        insert_event(
            db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": format!("Quota exceeded: {e}")}),
        )
        .await?;
        return Ok(());
    }

    // Insert run record
    let run_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO session_runs (session_id, status, context_mode) \
         VALUES ($1, 'running', $2) RETURNING id",
    )
    .bind(session_id)
    .bind(context_mode.as_str())
    .fetch_one(db)
    .await?;

    // Assemble context
    let prompt = match context::assemble_context(db, session_id, context_mode).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("failed to assemble context for session {session_id}: {e}");
            let _ = finish_run(db, run_id, "failed").await;
            insert_event(
                db, session_id, "system", "system",
                serde_json::json!({"text": format!("Failed to assemble context: {e}")}),
            ).await?;
            return Ok(());
        }
    };

    if prompt.trim().is_empty() {
        let _ = finish_run(db, run_id, "completed").await;
        insert_event(
            db, session_id, "system", "system",
            serde_json::json!({"text": "No context to send to Claude."}),
        ).await?;
        return Ok(());
    }

    // Run Claude CLI directly on the host, with CWD set to the session workspace
    let workspace_path = state.config.workspace_dir.join(session_id.to_string());
    let claude_cmd = &state.config.claude_cmd;
    let timeout = Duration::from_secs(state.config.run_timeout_secs);

    let mut child = match tokio::process::Command::new(claude_cmd)
        .args([
            "--dangerously-skip-permissions",
            "-p",
            &prompt,
            "--output-format",
            "stream-json",
            "--verbose",
            "--max-turns",
            "5",
        ])
        .current_dir(&workspace_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            let _ = finish_run(db, run_id, "failed").await;
            insert_event(
                db, session_id, "system", "system",
                serde_json::json!({"text": format!("Failed to start Claude: {e}")}),
            ).await?;
            return Ok(());
        }
    };

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // Spawn a task to collect stderr
    let stderr_handle = tokio::spawn(async move {
        let mut buf = String::new();
        let mut reader = BufReader::new(stderr);
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut buf).await;
        buf
    });

    let mut response_text = String::new();
    let mut total_tokens: i64 = 0;
    let mut saw_activity = false; // true if Claude did anything (tool calls, text, etc.)

    let read_result = tokio::time::timeout(timeout, async {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match event_type {
                    // Assistant message with content blocks (text or tool_use)
                    "assistant" => { saw_activity = true;
                        if let Some(message) = json.get("message") {
                            if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                                let mut turn_text = String::new();
                                for block in content {
                                    let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                    if block_type == "text" {
                                        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                            turn_text.push_str(text);
                                        }
                                    } else if block_type == "tool_use" {
                                        let tool = block.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                                        let args = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
                                        let _ = insert_event(db, session_id, "claude", "tool_call",
                                            serde_json::json!({"tool": tool, "args": args})).await;
                                    }
                                }
                                // Stream text immediately so users see it in real-time
                                if !turn_text.is_empty() {
                                    let _ = insert_event(db, session_id, "claude", "claude_response",
                                        serde_json::json!({"text": turn_text})).await;
                                    response_text.push_str(&turn_text);
                                }
                            }
                        }
                    }
                    // Tool result from Claude's tool execution
                    "user" => {
                        if let Some(message) = json.get("message") {
                            if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                                for block in content {
                                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                                        let output = block.get("content")
                                            .map(|v| if v.is_string() { v.as_str().unwrap_or("").to_string() } else { v.to_string() })
                                            .unwrap_or_default();
                                        let is_error = block.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                                        let line_count = output.lines().count();
                                        let _ = insert_event(db, session_id, "claude", "tool_result",
                                            serde_json::json!({"output": output, "lines": line_count, "is_error": is_error})).await;
                                    }
                                }
                            }
                        }
                    }
                    // Final result with usage stats
                    "result" => {
                        if let Some(text) = json.get("result").and_then(|v| v.as_str()) {
                            if !text.is_empty() {
                                // Only overwrite if we didn't get text from assistant events
                                if response_text.is_empty() {
                                    response_text = text.to_string();
                                }
                            }
                        }
                        if let Some(usage) = json.get("usage") {
                            let input = usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                            let output = usage.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                            let cache_create = usage.get("cache_creation_input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                            let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                            total_tokens = input + output + cache_create + cache_read;
                        }
                    }
                    _ => {}
                }
            }
        }
        child.wait().await
    }).await;

    // Insert final response only if it came from the result event (not already streamed)
    // Text from assistant events is already streamed per-turn above

    // Record token usage
    if total_tokens > 0 {
        let _ = quota::record_usage(db, session_id, total_tokens).await;
    }

    // Collect stderr
    let stderr_output = stderr_handle.await.unwrap_or_default();
    if !stderr_output.trim().is_empty() {
        tracing::warn!("claude stderr for session {session_id}: {}", stderr_output.trim());
    }

    // Update run status
    // Claude CLI exits 1 for max_turns and other non-fatal conditions,
    // so treat it as completed if we got any response text
    match read_result {
        Ok(Ok(status)) => {
            if status.success() || !response_text.is_empty() || saw_activity {
                let _ = finish_run(db, run_id, "completed").await;
                insert_event(db, session_id, "system", "system",
                    serde_json::json!({"text": "Claude run completed."})).await?;
            } else {
                let _ = finish_run(db, run_id, "failed").await;
                let msg = if stderr_output.trim().is_empty() {
                    format!("Claude exited with {status}")
                } else {
                    format!("Claude failed: {}", stderr_output.trim())
                };
                insert_event(db, session_id, "system", "system",
                    serde_json::json!({"text": msg})).await?;
            }
        }
        Ok(Err(e)) => {
            let _ = finish_run(db, run_id, "failed").await;
            insert_event(db, session_id, "system", "system",
                serde_json::json!({"text": format!("Claude run failed: {e}")})).await?;
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = finish_run(db, run_id, "timeout").await;
            insert_event(db, session_id, "system", "system",
                serde_json::json!({"text": "Claude run timed out."})).await?;
        }
    }

    Ok(())
}

async fn finish_run(db: &sqlx::PgPool, run_id: i64, status: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE session_runs SET status = $1, finished_at = now() WHERE id = $2")
        .bind(status)
        .bind(run_id)
        .execute(db)
        .await?;
    Ok(())
}
