use crate::claude;
use crate::context::ContextMode;
use crate::db::Database;
use crate::fetch;
use crate::state::AppState;
use crate::ws::insert_event;
use remora_common::ClientMsg;
use std::sync::Arc;
use uuid::Uuid;

/// Dispatch a client message to the appropriate handler.
/// Each handler inserts events into the DB (which triggers NOTIFY -> broadcast).
pub async fn dispatch(state: Arc<AppState>, session_id: Uuid, msg: ClientMsg) {
    let result = match msg {
        ClientMsg::Chat { author, text } => handle_chat(&state, session_id, &author, &text).await,
        ClientMsg::Run { author } => {
            handle_run(state.clone(), session_id, &author, ContextMode::SinceLast).await
        }
        ClientMsg::RunAll { author } => {
            handle_run(state.clone(), session_id, &author, ContextMode::Full).await
        }
        ClientMsg::Clear { author } => handle_clear(&state, session_id, &author).await,
        ClientMsg::Add { author, path } => handle_add(&state, session_id, &author, &path).await,
        ClientMsg::Diff { author } => handle_diff(&state, session_id, &author).await,
        ClientMsg::Fetch { author, url } => handle_fetch(&state, session_id, &author, &url).await,
        ClientMsg::RepoAdd { author, git_url } => {
            handle_repo_add(&state, session_id, &author, &git_url).await
        }
        ClientMsg::RepoRemove { author, name } => {
            handle_repo_remove(&state, session_id, &author, &name).await
        }
        ClientMsg::RepoList { author } => handle_repo_list(&state, session_id, &author).await,
        ClientMsg::Allowlist { author } => handle_allowlist(&state, session_id, &author).await,
        ClientMsg::AllowlistAdd { author, domain } => {
            handle_allowlist_add(&state, session_id, &author, &domain).await
        }
        ClientMsg::AllowlistRemove { author, domain } => {
            handle_allowlist_remove(&state, session_id, &author, &domain).await
        }
        ClientMsg::Approve {
            author,
            domain,
            approved,
        } => handle_approve(&state, session_id, &author, &domain, approved).await,
        ClientMsg::Who { author } => handle_who(&state, session_id, &author).await,
        ClientMsg::Kick { author, target } => {
            handle_kick(&state, session_id, &author, &target).await
        }
        ClientMsg::SessionInfo { author } => handle_session_info(&state, session_id, &author).await,
    };

    if let Err(e) = result {
        tracing::error!("command error in session {session_id}: {e}");
        let _ = insert_event(
            &state.db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": format!("Error: {e}")}),
        )
        .await;
    }
}

async fn handle_chat(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    text: &str,
) -> anyhow::Result<()> {
    insert_event(
        &state.db,
        session_id,
        author,
        "chat",
        serde_json::json!({"text": text}),
    )
    .await?;

    // Update idle_since
    let _ = state.db.clear_idle_since(session_id).await;

    Ok(())
}

async fn handle_run(
    state: Arc<AppState>,
    session_id: Uuid,
    author: &str,
    mode: ContextMode,
) -> anyhow::Result<()> {
    insert_event(
        &state.db,
        session_id,
        author,
        "system",
        serde_json::json!({"text": format!("{author} started a Claude run (mode: {})", mode.as_str())}),
    )
    .await?;

    // Spawn the run in a background task
    let state_clone = state.clone();
    tokio::spawn(async move {
        if let Err(e) = claude::run_claude(state_clone, session_id, mode).await {
            tracing::error!("claude run error for session {session_id}: {e}");
        }
    });

    Ok(())
}

async fn handle_clear(state: &AppState, session_id: Uuid, author: &str) -> anyhow::Result<()> {
    insert_event(
        &state.db,
        session_id,
        author,
        "clear_marker",
        serde_json::json!({"text": format!("{author} cleared context")}),
    )
    .await?;
    Ok(())
}

async fn handle_add(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    path: &str,
) -> anyhow::Result<()> {
    let workspace = state.config.workspace_dir.join(session_id.to_string());
    let file_path = workspace.join(path);

    // Security: ensure the resolved path is under the workspace
    let canonical = match tokio::fs::canonicalize(&file_path).await {
        Ok(p) => p,
        Err(e) => {
            insert_event(
                &state.db,
                session_id,
                "system",
                "system",
                serde_json::json!({"text": format!("File not found: {path} ({e})")}),
            )
            .await?;
            return Ok(());
        }
    };

    let canonical_workspace = tokio::fs::canonicalize(&workspace)
        .await
        .unwrap_or(workspace);
    if !canonical.starts_with(&canonical_workspace) {
        insert_event(
            &state.db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": format!("Path escapes workspace: {path}")}),
        )
        .await?;
        return Ok(());
    }

    let content = match tokio::fs::read_to_string(&canonical).await {
        Ok(c) => c,
        Err(e) => {
            insert_event(
                &state.db,
                session_id,
                "system",
                "system",
                serde_json::json!({"text": format!("Failed to read {path}: {e}")}),
            )
            .await?;
            return Ok(());
        }
    };

    insert_event(
        &state.db,
        session_id,
        author,
        "file",
        serde_json::json!({"path": path, "content": content}),
    )
    .await?;
    Ok(())
}

async fn handle_diff(state: &AppState, session_id: Uuid, author: &str) -> anyhow::Result<()> {
    let workspace = state.config.workspace_dir.join(session_id.to_string());

    // Get repos for the session
    let repo_names = state.db.list_repo_names(session_id).await?;

    let mut all_diffs = Vec::new();

    if repo_names.is_empty() {
        // Try running diff in the workspace root
        let output = tokio::process::Command::new("git")
            .args(["diff"])
            .current_dir(&workspace)
            .output()
            .await;

        if let Ok(output) = output {
            let diff = String::from_utf8_lossy(&output.stdout).to_string();
            if !diff.is_empty() {
                all_diffs.push(diff);
            }
        }
    } else {
        for repo_name in repo_names {
            let repo_dir = workspace.join(&repo_name);
            let output = tokio::process::Command::new("git")
                .args(["diff"])
                .current_dir(&repo_dir)
                .output()
                .await;

            if let Ok(output) = output {
                let diff = String::from_utf8_lossy(&output.stdout).to_string();
                if !diff.is_empty() {
                    all_diffs.push(format!("--- {repo_name} ---\n{diff}"));
                }
            }
        }
    }

    let diff_text = if all_diffs.is_empty() {
        "No changes detected.".to_string()
    } else {
        all_diffs.join("\n\n")
    };

    insert_event(
        &state.db,
        session_id,
        author,
        "diff",
        serde_json::json!({"text": diff_text}),
    )
    .await?;
    Ok(())
}

async fn handle_fetch(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    url: &str,
) -> anyhow::Result<()> {
    let domain = match fetch::extract_domain(url) {
        Ok(d) => d,
        Err(e) => {
            insert_event(
                &state.db,
                session_id,
                "system",
                "system",
                serde_json::json!({"text": format!("Invalid URL: {e}")}),
            )
            .await?;
            return Ok(());
        }
    };

    let status = fetch::check_domain_allowed(&state.db, session_id, &domain).await?;

    match status {
        fetch::DomainStatus::Blocked => {
            insert_event(
                &state.db,
                session_id,
                "system",
                "system",
                serde_json::json!({"text": format!("Domain {domain} is blocked.")}),
            )
            .await?;
        }
        fetch::DomainStatus::NeedsApproval => {
            fetch::create_approval_request(&state.db, session_id, &domain, url, author).await?;
        }
        fetch::DomainStatus::Allowed => match fetch::fetch_url(url).await {
            Ok(content) => {
                insert_event(
                    &state.db,
                    session_id,
                    author,
                    "fetch",
                    serde_json::json!({"url": url, "content": content}),
                )
                .await?;
            }
            Err(e) => {
                insert_event(
                    &state.db,
                    session_id,
                    "system",
                    "system",
                    serde_json::json!({"text": format!("Fetch failed: {e}")}),
                )
                .await?;
            }
        },
    }

    Ok(())
}

async fn handle_repo_add(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    git_url: &str,
) -> anyhow::Result<()> {
    // Extract repo name from URL
    let name = git_url
        .rsplit('/')
        .next()
        .unwrap_or("repo")
        .trim_end_matches(".git")
        .to_string();

    let workspace = state.config.workspace_dir.join(session_id.to_string());
    tokio::fs::create_dir_all(&workspace).await?;

    let repo_dir = workspace.join(&name);

    // Clone the repo
    let output = tokio::process::Command::new("git")
        .args(["clone", git_url, repo_dir.to_str().unwrap_or(".")])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        insert_event(
            &state.db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": format!("git clone failed: {stderr}")}),
        )
        .await?;
        return Ok(());
    }

    // Insert into session_repos
    state.db.upsert_repo(session_id, &name, git_url).await?;

    insert_event(
        &state.db,
        session_id,
        author,
        "repo_change",
        serde_json::json!({"action": "add", "name": name, "git_url": git_url}),
    )
    .await?;
    Ok(())
}

async fn handle_repo_remove(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    name: &str,
) -> anyhow::Result<()> {
    let workspace = state.config.workspace_dir.join(session_id.to_string());
    let repo_dir = workspace.join(name);

    // Delete directory
    if repo_dir.exists() {
        tokio::fs::remove_dir_all(&repo_dir).await?;
    }

    // Remove from session_repos
    state.db.delete_repo(session_id, name).await?;

    insert_event(
        &state.db,
        session_id,
        author,
        "repo_change",
        serde_json::json!({"action": "remove", "name": name}),
    )
    .await?;
    Ok(())
}

async fn handle_repo_list(state: &AppState, session_id: Uuid, author: &str) -> anyhow::Result<()> {
    let repos = state.db.list_repos(session_id).await?;

    let text = if repos.is_empty() {
        "No repos in this session.".to_string()
    } else {
        repos
            .iter()
            .map(|(name, url)| format!("  {name}: {url}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    insert_event(
        &state.db,
        session_id,
        author,
        "system",
        serde_json::json!({"text": format!("Repos:\n{text}")}),
    )
    .await?;
    Ok(())
}

async fn handle_allowlist(state: &AppState, session_id: Uuid, author: &str) -> anyhow::Result<()> {
    let global = state.db.list_global_allowlist().await?;
    let session_entries = state.db.list_session_allowlist(session_id).await?;

    let mut text = String::from("Global allowlist:\n");
    if global.is_empty() {
        text.push_str("  (empty)\n");
    } else {
        for (domain, kind) in &global {
            text.push_str(&format!("  {domain} [{kind}]\n"));
        }
    }
    text.push_str("\nSession allowlist:\n");
    if session_entries.is_empty() {
        text.push_str("  (empty)\n");
    } else {
        for domain in &session_entries {
            text.push_str(&format!("  {domain}\n"));
        }
    }

    insert_event(
        &state.db,
        session_id,
        author,
        "system",
        serde_json::json!({"text": text.trim_end()}),
    )
    .await?;
    Ok(())
}

async fn handle_allowlist_add(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    domain: &str,
) -> anyhow::Result<()> {
    state.db.add_session_allowlist(session_id, domain).await?;

    insert_event(
        &state.db,
        session_id,
        author,
        "allowlist_update",
        serde_json::json!({"action": "add", "domain": domain}),
    )
    .await?;
    Ok(())
}

async fn handle_allowlist_remove(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    domain: &str,
) -> anyhow::Result<()> {
    state
        .db
        .remove_session_allowlist(session_id, domain)
        .await?;

    insert_event(
        &state.db,
        session_id,
        author,
        "allowlist_update",
        serde_json::json!({"action": "remove", "domain": domain}),
    )
    .await?;
    Ok(())
}

async fn handle_approve(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    domain: &str,
    approved: bool,
) -> anyhow::Result<()> {
    fetch::resolve_approval(&state.db, session_id, domain, approved).await?;

    let status_text = if approved { "approved" } else { "denied" };
    insert_event(
        &state.db,
        session_id,
        author,
        "system",
        serde_json::json!({"text": format!("{author} {status_text} fetch access to {domain}")}),
    )
    .await?;

    // If approved, proceed with any pending fetches for this domain
    if approved {
        let pending = state.db.get_approved_pending(session_id, domain).await?;

        for (url, requested_by) in pending {
            match fetch::fetch_url(&url).await {
                Ok(content) => {
                    let _ = insert_event(
                        &state.db,
                        session_id,
                        &requested_by,
                        "fetch",
                        serde_json::json!({"url": url, "content": content}),
                    )
                    .await;
                }
                Err(e) => {
                    let _ = insert_event(
                        &state.db,
                        session_id,
                        "system",
                        "system",
                        serde_json::json!({"text": format!("Fetch failed for {url}: {e}")}),
                    )
                    .await;
                }
            }
        }
    }

    Ok(())
}

async fn handle_who(state: &AppState, session_id: Uuid, _author: &str) -> anyhow::Result<()> {
    let participants = state.get_participants(session_id).await;
    let text = if participants.is_empty() {
        "No participants connected.".to_string()
    } else {
        format!("Connected: {}", participants.join(", "))
    };

    insert_event(
        &state.db,
        session_id,
        "system",
        "system",
        serde_json::json!({"text": text}),
    )
    .await?;
    Ok(())
}

async fn handle_kick(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    target: &str,
) -> anyhow::Result<()> {
    // Insert kick event (the WS handler watches for these)
    insert_event(
        &state.db,
        session_id,
        author,
        "kick",
        serde_json::json!({"target": target, "text": format!("{author} kicked {target}")}),
    )
    .await?;

    // Remove participant
    state.kick_participant(session_id, target).await;

    Ok(())
}

async fn handle_session_info(
    state: &AppState,
    session_id: Uuid,
    _author: &str,
) -> anyhow::Result<()> {
    let row = state.db.get_session_info(session_id).await?;

    let text = match row {
        Some((desc, created, used, cap)) => {
            let participants = state.get_participants(session_id).await;
            let run_in_flight = state.is_run_in_flight(session_id).await;

            let repo_names = state.db.list_repo_names(session_id).await?;

            format!(
                "Session: {session_id}\n\
                 Description: {desc}\n\
                 Created: {created}\n\
                 Tokens: {used}/{cap}\n\
                 Repos: {}\n\
                 Participants: {}\n\
                 Run in flight: {run_in_flight}",
                if repo_names.is_empty() {
                    "(none)".to_string()
                } else {
                    repo_names.join(", ")
                },
                if participants.is_empty() {
                    "(none)".to_string()
                } else {
                    participants.join(", ")
                }
            )
        }
        None => "Session not found.".to_string(),
    };

    insert_event(
        &state.db,
        session_id,
        "system",
        "system",
        serde_json::json!({"text": text}),
    )
    .await?;
    Ok(())
}
