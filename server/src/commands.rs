use crate::auth;
use crate::claude;
use crate::context::ContextMode;
use crate::db::Database;
use crate::fetch;
use crate::state::AppState;
use crate::ws::insert_event;
use remora_common::ClientMsg;
use std::sync::Arc;
use uuid::Uuid;

/// Return the command name for a `ClientMsg` variant (used in RBAC error messages).
fn command_name(msg: &ClientMsg) -> &'static str {
    match msg {
        ClientMsg::Chat { .. } => "chat",
        ClientMsg::Run { .. } => "/run",
        ClientMsg::RunAll { .. } => "/run-all",
        ClientMsg::Clear { .. } => "/clear",
        ClientMsg::Add { .. } => "/add",
        ClientMsg::Diff { .. } => "/diff",
        ClientMsg::Fetch { .. } => "/fetch",
        ClientMsg::RepoAdd { .. } => "/repo add",
        ClientMsg::RepoRemove { .. } => "/repo remove",
        ClientMsg::RepoList { .. } => "/repo list",
        ClientMsg::Allowlist { .. } => "/allowlist",
        ClientMsg::AllowlistAdd { .. } => "/allowlist add",
        ClientMsg::AllowlistRemove { .. } => "/allowlist remove",
        ClientMsg::Approve { .. } => "/approve",
        ClientMsg::Who { .. } => "/who",
        ClientMsg::Kick { .. } => "/kick",
        ClientMsg::SessionInfo { .. } => "/info",
        ClientMsg::Help { .. } => "/help",
        ClientMsg::Trust { .. } => "/trust",
        ClientMsg::Untrust { .. } => "/untrust",
    }
}

/// Check whether the given role has permission to execute the command.
/// Returns `Ok(())` if permitted, or `Err(message)` if denied.
///
/// Role hierarchy: admin(4) > member(3) > viewer(2) > guest(1)
///
/// Permission mapping:
/// - **admin**: all commands
/// - **member**: chat, run, run_all, add, fetch, repo_add, repo_remove, repo_list,
///   diff, help, who, session_info, clear, allowlist, allowlist_add,
///   allowlist_remove, approve, kick, trust, untrust
/// - **viewer**: chat, help, who, session_info (read-only)
/// - **guest**: chat, help, who (minimal)
///
/// Note: kick/trust/untrust additionally require admin role or session owner status,
/// which is enforced inside the respective handlers.
fn check_rbac(role: &str, msg: &ClientMsg) -> Result<(), String> {
    let level = auth::role_level(role);

    match msg {
        // All roles can use: chat, help, who
        ClientMsg::Chat { .. } | ClientMsg::Help { .. } | ClientMsg::Who { .. } => Ok(()),

        // viewer(2)+ can use /info
        ClientMsg::SessionInfo { .. } => {
            if level >= auth::role_level("viewer") {
                Ok(())
            } else {
                Err(format!(
                    "insufficient permissions: {} cannot use {}",
                    role,
                    command_name(msg)
                ))
            }
        }

        // member(3)+ can use most commands
        ClientMsg::Run { .. }
        | ClientMsg::RunAll { .. }
        | ClientMsg::Clear { .. }
        | ClientMsg::Add { .. }
        | ClientMsg::Diff { .. }
        | ClientMsg::Fetch { .. }
        | ClientMsg::RepoAdd { .. }
        | ClientMsg::RepoRemove { .. }
        | ClientMsg::RepoList { .. }
        | ClientMsg::Allowlist { .. }
        | ClientMsg::AllowlistAdd { .. }
        | ClientMsg::AllowlistRemove { .. }
        | ClientMsg::Approve { .. } => {
            if level >= auth::role_level("member") {
                Ok(())
            } else {
                Err(format!(
                    "insufficient permissions: {} cannot use {}",
                    role,
                    command_name(msg)
                ))
            }
        }

        // kick/trust/untrust: member(3)+ at the RBAC layer.
        // The handlers additionally enforce admin-or-owner for trust/untrust/kick.
        ClientMsg::Kick { .. } | ClientMsg::Trust { .. } | ClientMsg::Untrust { .. } => {
            if level >= auth::role_level("member") {
                Ok(())
            } else {
                Err(format!(
                    "insufficient permissions: {} cannot use {}",
                    role,
                    command_name(msg)
                ))
            }
        }
    }
}

/// Dispatch a client message to the appropriate handler.
/// Each handler inserts events into the DB (which triggers NOTIFY -> broadcast).
///
/// `verified_author` is the authenticated name from the WebSocket connection
/// and is used for ALL event insertions, ignoring any client-supplied author field.
///
/// `verified_role` is the user's role from authentication (admin/member/viewer/guest)
/// and is used for RBAC enforcement before dispatching to command handlers.
pub async fn dispatch(
    state: Arc<AppState>,
    session_id: Uuid,
    msg: ClientMsg,
    verified_author: &str,
    verified_role: &str,
) {
    // RBAC: check if the user's role permits this command
    if let Err(err_msg) = check_rbac(verified_role, &msg) {
        let _ = insert_event(
            &state.db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": err_msg}),
        )
        .await;
        return;
    }

    let author = verified_author;
    let role = verified_role;
    let result = match msg {
        ClientMsg::Chat { text, .. } => handle_chat(&state, session_id, author, &text).await,
        ClientMsg::Run { .. } => {
            handle_run(state.clone(), session_id, author, ContextMode::SinceLast).await
        }
        ClientMsg::RunAll { .. } => {
            handle_run(state.clone(), session_id, author, ContextMode::Full).await
        }
        ClientMsg::Clear { .. } => handle_clear(&state, session_id, author).await,
        ClientMsg::Add { path, .. } => handle_add(&state, session_id, author, &path).await,
        ClientMsg::Diff { .. } => handle_diff(&state, session_id, author).await,
        ClientMsg::Fetch { url, .. } => handle_fetch(&state, session_id, author, &url).await,
        ClientMsg::RepoAdd { git_url, .. } => {
            handle_repo_add(&state, session_id, author, &git_url).await
        }
        ClientMsg::RepoRemove { name, .. } => {
            handle_repo_remove(&state, session_id, author, &name).await
        }
        ClientMsg::RepoList { .. } => handle_repo_list(&state, session_id, author).await,
        ClientMsg::Allowlist { .. } => handle_allowlist(&state, session_id, author).await,
        ClientMsg::AllowlistAdd { domain, .. } => {
            handle_allowlist_add(&state, session_id, author, &domain).await
        }
        ClientMsg::AllowlistRemove { domain, .. } => {
            handle_allowlist_remove(&state, session_id, author, &domain).await
        }
        ClientMsg::Approve {
            domain, approved, ..
        } => handle_approve(&state, session_id, author, &domain, approved).await,
        ClientMsg::Who { .. } => handle_who(&state, session_id, author).await,
        ClientMsg::Kick { target, .. } => {
            handle_kick(&state, session_id, author, role, &target).await
        }
        ClientMsg::SessionInfo { .. } => handle_session_info(&state, session_id, author).await,
        ClientMsg::Help { .. } => handle_help(&state, session_id, author).await,
        ClientMsg::Trust { target, .. } => {
            handle_trust(&state, session_id, author, role, &target).await
        }
        ClientMsg::Untrust { target, .. } => {
            handle_untrust(&state, session_id, author, role, &target).await
        }
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
    // Validate git URL scheme (SSRF prevention)
    if !crate::is_safe_git_url(git_url) {
        insert_event(
            &state.db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": format!("Rejected git URL: {git_url} (only https://, ssh://, and git:// schemes are allowed)")}),
        )
        .await?;
        return Ok(());
    }

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
    // Validate that the repo name contains no path separators or traversal sequences
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        anyhow::bail!("Invalid repo name: must not contain path separators or '..'");
    }

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
    let mut text = if participants.is_empty() {
        "No participants connected.".to_string()
    } else {
        format!("Connected: {}", participants.join(", "))
    };

    if let Some(owner) = state.get_session_owner(session_id).await {
        text.push_str(&format!("\nOwner: {owner}"));
    }

    let trusted = state
        .db
        .list_trusted_participants(session_id)
        .await
        .unwrap_or_default();
    if !trusted.is_empty() {
        text.push_str(&format!("\nTrusted: {}", trusted.join(", ")));
    }

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
    role: &str,
    target: &str,
) -> anyhow::Result<()> {
    // Only admin or session owner can kick
    let is_admin = auth::role_level(role) >= auth::role_level("admin");
    let is_owner = state
        .get_session_owner(session_id)
        .await
        .map(|o| o == author)
        .unwrap_or(false);

    if !is_admin && !is_owner {
        insert_event(
            &state.db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": "Only the session owner or an admin can kick participants."}),
        )
        .await?;
        return Ok(());
    }

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
    author: &str,
) -> anyhow::Result<()> {
    let row = state.db.get_session_info(session_id).await?;

    let text = match row {
        Some((desc, created, used, cap)) => {
            let participants = state.get_participants(session_id).await;
            let run_in_flight = state.is_run_in_flight(session_id).await;

            let repo_names = state.db.list_repo_names(session_id).await?;

            let mut info = format!(
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
            );

            // Show owner_key only to the session owner
            let is_owner = state
                .get_session_owner(session_id)
                .await
                .map(|o| o == author)
                .unwrap_or(false);
            if is_owner {
                if let Ok(Some(key)) = state.db.get_owner_key(session_id).await {
                    info.push_str(&format!("\nOwner key: {key}"));
                }
            }

            info
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

async fn handle_help(state: &AppState, session_id: Uuid, _author: &str) -> anyhow::Result<()> {
    let help = "\
Commands:
  /run          \u{2014} Run Claude (context since last run)
  /run-all      \u{2014} Run Claude (full context)
  /clear        \u{2014} Insert context boundary
  /who          \u{2014} List connected participants
  /info         \u{2014} Session info (id, tokens, repos, status)
  /diff         \u{2014} Show git diff for session repos
  /add <path>   \u{2014} Add a file to context
  /fetch <url>  \u{2014} Fetch a URL into context
  /repo list    \u{2014} List repos in this session
  /repo add <url>    \u{2014} Clone a git repo into the session
  /repo remove <name> \u{2014} Remove a repo
  /allowlist         \u{2014} Show allowed domains
  /allowlist add <domain>    \u{2014} Allow a domain for fetch
  /allowlist remove <domain> \u{2014} Remove from allowlist
  /approve <domain>  \u{2014} Approve a pending fetch request
  /deny <domain>     \u{2014} Deny a pending fetch request
  /kick <name>       \u{2014} Disconnect a participant
  /trust <name>      \u{2014} Mark a participant as trusted (owner only)
  /untrust <name>    \u{2014} Remove a participant from the trusted list (owner only)
  /help              \u{2014} Show this help";

    insert_event(
        &state.db,
        session_id,
        "system",
        "system",
        serde_json::json!({"text": help}),
    )
    .await?;
    Ok(())
}

async fn handle_trust(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    role: &str,
    target: &str,
) -> anyhow::Result<()> {
    // Only the session owner can trust participants
    let is_owner = state
        .get_session_owner(session_id)
        .await
        .map(|o| o == author)
        .unwrap_or(false);

    if !is_owner {
        insert_event(
            &state.db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": "Only the session owner or an admin can trust/untrust participants."}),
        )
        .await?;
        return Ok(());
    }

    state.db.trust_participant(session_id, target).await?;
    insert_event(
        &state.db,
        session_id,
        "system",
        "system",
        serde_json::json!({"text": format!("{target} is now trusted \u{2014} their messages will be treated as instructions to Claude.")}),
    )
    .await?;
    Ok(())
}

async fn handle_untrust(
    state: &AppState,
    session_id: Uuid,
    author: &str,
    role: &str,
    target: &str,
) -> anyhow::Result<()> {
    // Only admin or session owner can untrust participants
    let is_admin = auth::role_level(role) >= auth::role_level("admin");
    let is_owner = state
        .get_session_owner(session_id)
        .await
        .map(|o| o == author)
        .unwrap_or(false);

    if !is_admin && !is_owner {
        insert_event(
            &state.db,
            session_id,
            "system",
            "system",
            serde_json::json!({"text": "Only the session owner or an admin can trust/untrust participants."}),
        )
        .await?;
        return Ok(());
    }

    state.db.untrust_participant(session_id, target).await?;
    insert_event(
        &state.db,
        session_id,
        "system",
        "system",
        serde_json::json!({"text": format!("{target} has been removed from the trusted list.")}),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use remora_common::ClientMsg;

    #[test]
    fn rbac_admin_allows_all() {
        let commands = vec![
            ClientMsg::Chat {
                text: "hi".into(),
                author: String::new(),
            },
            ClientMsg::Run {
                author: String::new(),
            },
            ClientMsg::RunAll {
                author: String::new(),
            },
            ClientMsg::Clear {
                author: String::new(),
            },
            ClientMsg::Add {
                path: "f".into(),
                author: String::new(),
            },
            ClientMsg::Diff {
                author: String::new(),
            },
            ClientMsg::Fetch {
                url: "u".into(),
                author: String::new(),
            },
            ClientMsg::Who {
                author: String::new(),
            },
            ClientMsg::Help {
                author: String::new(),
            },
            ClientMsg::SessionInfo {
                author: String::new(),
            },
            ClientMsg::Kick {
                target: "t".into(),
                author: String::new(),
            },
            ClientMsg::Trust {
                target: "t".into(),
                author: String::new(),
            },
            ClientMsg::Untrust {
                target: "t".into(),
                author: String::new(),
            },
        ];
        for cmd in commands {
            assert!(
                check_rbac("admin", &cmd).is_ok(),
                "admin should be allowed: {}",
                command_name(&cmd)
            );
        }
    }

    #[test]
    fn rbac_viewer_blocked_from_run() {
        let cmd = ClientMsg::Run {
            author: String::new(),
        };
        let result = check_rbac("viewer", &cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("viewer cannot use /run"));
    }

    #[test]
    fn rbac_viewer_allowed_info() {
        let cmd = ClientMsg::SessionInfo {
            author: String::new(),
        };
        assert!(check_rbac("viewer", &cmd).is_ok());
    }

    #[test]
    fn rbac_guest_blocked_from_info() {
        let cmd = ClientMsg::SessionInfo {
            author: String::new(),
        };
        let result = check_rbac("guest", &cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("guest cannot use /info"));
    }

    #[test]
    fn rbac_guest_allowed_chat_who_help() {
        assert!(check_rbac(
            "guest",
            &ClientMsg::Chat {
                text: "hi".into(),
                author: String::new()
            }
        )
        .is_ok());
        assert!(check_rbac(
            "guest",
            &ClientMsg::Who {
                author: String::new()
            }
        )
        .is_ok());
        assert!(check_rbac(
            "guest",
            &ClientMsg::Help {
                author: String::new()
            }
        )
        .is_ok());
    }

    #[test]
    fn rbac_member_allowed_run_and_kick() {
        assert!(check_rbac(
            "member",
            &ClientMsg::Run {
                author: String::new()
            }
        )
        .is_ok());
        assert!(check_rbac(
            "member",
            &ClientMsg::Kick {
                target: "t".into(),
                author: String::new()
            }
        )
        .is_ok());
    }

    #[test]
    fn rbac_unknown_role_blocked() {
        let cmd = ClientMsg::Run {
            author: String::new(),
        };
        assert!(check_rbac("unknown", &cmd).is_err());
    }
}
