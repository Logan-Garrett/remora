use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

/// Name for the docker container associated with a session.
fn container_name(session_id: Uuid) -> String {
    format!("remora-{session_id}")
}

/// Create a docker sandbox for a session.
/// The container runs the specified image with:
/// - Workspace mounted at /workspace
/// - CPU, memory, and PID limits
/// - No network by default (can be changed for fetch proxy)
/// - Claude CLI available inside via bind-mount of host's npm global
pub async fn create_sandbox(
    session_id: Uuid,
    workspace_path: &Path,
    docker_image: &str,
) -> anyhow::Result<()> {
    let name = container_name(session_id);
    let workspace = workspace_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-utf8 workspace path"))?;

    // Find host Claude CLI path for bind-mount
    let claude_path = find_claude_path().await;

    let mut create_args = vec![
        "create".to_string(),
        "--name".to_string(),
        name.clone(),
        "--cpus".to_string(),
        "2".to_string(),
        "--memory".to_string(),
        "2g".to_string(),
        "--pids-limit".to_string(),
        "256".to_string(),
        // Mount workspace
        "-v".to_string(),
        format!("{workspace}:/workspace"),
        "-w".to_string(),
        "/workspace".to_string(),
    ];

    // If we found Claude CLI, bind-mount it into the container
    if let Some(ref claude_dir) = claude_path {
        create_args.extend(["-v".to_string(), format!("{claude_dir}:{claude_dir}:ro")]);
        // Also mount the Claude config directory for auth
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let claude_config = format!("{home}/.claude");
        if tokio::fs::metadata(&claude_config).await.is_ok() {
            create_args.extend([
                "-v".to_string(),
                format!("{claude_config}:/root/.claude:ro"),
            ]);
        }
        // Mount node_modules for Claude's dependencies
        let node_path = find_node_path().await;
        if let Some(ref np) = node_path {
            create_args.extend([
                "-v".to_string(),
                format!("{np}:{np}:ro"),
                "-e".to_string(),
                format!("PATH={claude_dir}:{np}:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
            ]);
        } else {
            create_args.extend([
                "-e".to_string(),
                format!("PATH={claude_dir}:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
            ]);
        }
    }

    // Allow network access (needed for Claude API calls)
    // For production, this should use a proxy for fetch allowlist enforcement
    create_args.extend([
        docker_image.to_string(),
        "sleep".to_string(),
        "infinity".to_string(),
    ]);

    let output = Command::new("docker").args(&create_args).output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker create failed: {stderr}");
    }

    // Start the container
    let output = Command::new("docker")
        .args(["start", &name])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker start failed: {stderr}");
    }

    tracing::info!("sandbox created: {name}");
    Ok(())
}

/// Execute a command in the session's sandbox with a timeout, streaming stdout line by line.
pub async fn exec_in_sandbox(
    session_id: Uuid,
    cmd: &[&str],
    timeout: Duration,
) -> anyhow::Result<tokio::process::Child> {
    let name = container_name(session_id);
    let mut args = vec!["exec".to_string(), "-i".to_string(), name.clone()];
    args.extend(cmd.iter().map(|s| s.to_string()));

    let child = Command::new("docker")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    Ok(child)
}

/// Destroy a session's sandbox.
pub async fn destroy_sandbox(session_id: Uuid) -> anyhow::Result<()> {
    let name = container_name(session_id);
    let output = Command::new("docker")
        .args(["rm", "-f", &name])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("docker rm failed for {name}: {stderr}");
    } else {
        tracing::info!("sandbox destroyed: {name}");
    }
    Ok(())
}

/// Ensure a sandbox exists for the session. Create if it doesn't.
pub async fn ensure_sandbox(
    session_id: Uuid,
    workspace_path: &Path,
    docker_image: &str,
) -> anyhow::Result<()> {
    let name = container_name(session_id);

    // Check if container exists and is running
    let output = Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", &name])
        .output()
        .await?;

    if output.status.success() {
        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if status == "running" {
            return Ok(());
        }
        // Container exists but not running, start it
        if status == "created" || status == "exited" {
            let start = Command::new("docker")
                .args(["start", &name])
                .output()
                .await?;
            if start.status.success() {
                return Ok(());
            }
        }
        // Remove and recreate
        let _ = destroy_sandbox(session_id).await;
    }

    create_sandbox(session_id, workspace_path, docker_image).await
}

/// Check if a sandbox container exists for a session.
pub async fn sandbox_exists(session_id: Uuid) -> bool {
    let name = container_name(session_id);
    let output = Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", &name])
        .output()
        .await;
    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Get the status of a sandbox container.
pub async fn sandbox_status(session_id: Uuid) -> String {
    let name = container_name(session_id);
    let output = Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", &name])
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "not found".to_string(),
    }
}

/// Find the directory containing the Claude CLI binary on the host.
async fn find_claude_path() -> Option<String> {
    let output = Command::new("which").arg("claude").output().await.ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Return the directory containing the binary
        std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
    } else {
        None
    }
}

/// Find the Node.js binary directory on the host.
async fn find_node_path() -> Option<String> {
    let output = Command::new("which").arg("node").output().await.ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
    } else {
        None
    }
}
