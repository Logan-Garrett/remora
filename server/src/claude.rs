use crate::context::{self, ContextMode};
use crate::db::Database;
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
    let run_id = db.insert_run(session_id, context_mode.as_str()).await?;

    // Assemble context
    let prompt = match context::assemble_context(db, session_id, context_mode).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("failed to assemble context for session {session_id}: {e}");
            let _ = db.finish_run(run_id, "failed").await;
            insert_event(
                db,
                session_id,
                "system",
                "system",
                serde_json::json!({"text": format!("Failed to assemble context: {e}")}),
            )
            .await?;
            return Ok(());
        }
    };

    if prompt.trim().is_empty() {
        let _ = db.finish_run(run_id, "completed").await;
        insert_event(
            db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": "No context to send to Claude."}),
        )
        .await?;
        return Ok(());
    }

    // Run Claude CLI — either in sandbox container or directly on host
    let workspace_path = state.config.workspace_dir.join(session_id.to_string());
    let claude_cmd = &state.config.claude_cmd;
    let timeout = Duration::from_secs(state.config.run_timeout_secs);

    // Build Claude args — permission handling:
    // 1. skip_permissions=true → --dangerously-skip-permissions (legacy, not allowed as root)
    // 2. permission_mode set → --permission-mode <mode> (e.g. "auto", "acceptEdits")
    // 3. allowed_tools set → --allowedTools <tools> (fine-grained control)
    // 4. none of the above → Claude runs with default interactive permissions (will hang in non-TTY)
    let mut claude_args: Vec<String> = Vec::new();
    if state.config.skip_permissions {
        claude_args.push("--dangerously-skip-permissions".into());
    } else if !state.config.permission_mode.is_empty() {
        claude_args.extend([
            "--permission-mode".into(),
            state.config.permission_mode.clone(),
        ]);
    }
    if !state.config.allowed_tools.is_empty() {
        claude_args.push("--allowedTools".into());
        claude_args.push(state.config.allowed_tools.join(" "));
    }
    claude_args.extend([
        "-p".into(),
        prompt.clone(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--max-turns".into(),
        "5".into(),
    ]);

    let mut child = if state.config.use_sandbox {
        // Ensure sandbox container exists
        if let Err(e) =
            crate::sandbox::ensure_sandbox(session_id, &workspace_path, &state.config.docker_image)
                .await
        {
            let _ = db.finish_run(run_id, "failed").await;
            insert_event(
                db,
                session_id,
                "system",
                "system",
                serde_json::json!({"text": format!("Failed to create sandbox: {e}")}),
            )
            .await?;
            return Ok(());
        }

        insert_event(
            db, session_id, "system", "system",
            serde_json::json!({"text": format!("Sandbox container running for session {session_id}")}),
        ).await?;

        // Build docker exec command
        let mut exec_args: Vec<&str> = vec![claude_cmd.as_str()];
        exec_args.extend(claude_args.iter().map(|s| s.as_str()));

        match crate::sandbox::exec_in_sandbox(session_id, &exec_args, timeout).await {
            Ok(child) => child,
            Err(e) => {
                let _ = db.finish_run(run_id, "failed").await;
                insert_event(
                    db,
                    session_id,
                    "system",
                    "system",
                    serde_json::json!({"text": format!("Failed to exec in sandbox: {e}")}),
                )
                .await?;
                return Ok(());
            }
        }
    } else {
        // Run directly on host
        match tokio::process::Command::new(claude_cmd)
            .args(claude_args.iter().map(|s| s.as_str()))
            .current_dir(&workspace_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                let _ = db.finish_run(run_id, "failed").await;
                insert_event(
                    db,
                    session_id,
                    "system",
                    "system",
                    serde_json::json!({"text": format!("Failed to start Claude: {e}")}),
                )
                .await?;
                return Ok(());
            }
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
    let mut saw_activity = false;

    let read_result = tokio::time::timeout(timeout, async {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match event_type {
                    // Assistant message with content blocks (text or tool_use)
                    "assistant" => {
                        saw_activity = true;
                        if let Some(message) = json.get("message") {
                            if let Some(content) =
                                message.get("content").and_then(|c| c.as_array())
                            {
                                let mut turn_text = String::new();
                                for block in content {
                                    let block_type = block
                                        .get("type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if block_type == "text" {
                                        if let Some(text) =
                                            block.get("text").and_then(|v| v.as_str())
                                        {
                                            turn_text.push_str(text);
                                        }
                                    } else if block_type == "tool_use" {
                                        let tool = block
                                            .get("name")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown");
                                        let args = block
                                            .get("input")
                                            .cloned()
                                            .unwrap_or(serde_json::Value::Null);
                                        let _ = insert_event(
                                            db,
                                            session_id,
                                            "claude",
                                            "tool_call",
                                            serde_json::json!({"tool": tool, "args": args}),
                                        )
                                        .await;
                                    }
                                }
                                // Stream text immediately so users see it in real-time
                                if !turn_text.is_empty() {
                                    let _ = insert_event(
                                        db,
                                        session_id,
                                        "claude",
                                        "claude_response",
                                        serde_json::json!({"text": turn_text}),
                                    )
                                    .await;
                                    response_text.push_str(&turn_text);
                                }
                            }
                        }
                    }
                    // Tool result from Claude's tool execution
                    "user" => {
                        if let Some(message) = json.get("message") {
                            if let Some(content) =
                                message.get("content").and_then(|c| c.as_array())
                            {
                                for block in content {
                                    if block.get("type").and_then(|v| v.as_str())
                                        == Some("tool_result")
                                    {
                                        let output = block
                                            .get("content")
                                            .map(|v| {
                                                if v.is_string() {
                                                    v.as_str().unwrap_or("").to_string()
                                                } else {
                                                    v.to_string()
                                                }
                                            })
                                            .unwrap_or_default();
                                        let is_error = block
                                            .get("is_error")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false);
                                        let line_count = output.lines().count();
                                        let _ = insert_event(
                                            db,
                                            session_id,
                                            "claude",
                                            "tool_result",
                                            serde_json::json!({"output": output, "lines": line_count, "is_error": is_error}),
                                        )
                                        .await;
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
                            let input = usage
                                .get("input_tokens")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);
                            let output = usage
                                .get("output_tokens")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);
                            let cache_create = usage
                                .get("cache_creation_input_tokens")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);
                            let cache_read = usage
                                .get("cache_read_input_tokens")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);
                            total_tokens = input + output + cache_create + cache_read;
                        }
                    }
                    _ => {}
                }
            }
        }
        child.wait().await
    })
    .await;

    // Record token usage
    if total_tokens > 0 {
        let _ = quota::record_usage(db, session_id, total_tokens).await;
    }

    // Collect stderr
    let stderr_output = stderr_handle.await.unwrap_or_default();
    if !stderr_output.trim().is_empty() {
        tracing::warn!(
            "claude stderr for session {session_id}: {}",
            stderr_output.trim()
        );
    }

    // Update run status
    match read_result {
        Ok(Ok(status)) => {
            if status.success() || !response_text.is_empty() || saw_activity {
                let _ = db.finish_run(run_id, "completed").await;
                insert_event(
                    db,
                    session_id,
                    "system",
                    "system",
                    serde_json::json!({"text": "Claude run completed."}),
                )
                .await?;
            } else {
                let _ = db.finish_run(run_id, "failed").await;
                let msg = if stderr_output.trim().is_empty() {
                    format!("Claude exited with {status}")
                } else {
                    format!("Claude failed: {}", stderr_output.trim())
                };
                insert_event(
                    db,
                    session_id,
                    "system",
                    "system",
                    serde_json::json!({"text": msg}),
                )
                .await?;
            }
        }
        Ok(Err(e)) => {
            let _ = db.finish_run(run_id, "failed").await;
            insert_event(
                db,
                session_id,
                "system",
                "system",
                serde_json::json!({"text": format!("Claude run failed: {e}")}),
            )
            .await?;
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = db.finish_run(run_id, "timeout").await;
            insert_event(
                db,
                session_id,
                "system",
                "system",
                serde_json::json!({"text": "Claude run timed out."}),
            )
            .await?;
        }
    }

    Ok(())
}
