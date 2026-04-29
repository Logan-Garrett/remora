use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

/// Name for the docker container associated with a session.
fn container_name(session_id: Uuid) -> String {
    format!("remora-{session_id}")
}

/// Create a docker sandbox for a session.
pub async fn create_sandbox(
    session_id: Uuid,
    workspace_path: &Path,
    docker_image: &str,
) -> anyhow::Result<()> {
    let name = container_name(session_id);
    let workspace = workspace_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-utf8 workspace path"))?;

    let output = Command::new("docker")
        .args([
            "create",
            "--name",
            &name,
            "--cpus",
            "2",
            "--memory",
            "2g",
            "--pids-limit",
            "256",
            "--network",
            "none",
            "-v",
            &format!("{workspace}:/workspace"),
            "-w",
            "/workspace",
            docker_image,
            "sleep",
            "infinity",
        ])
        .output()
        .await?;

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

/// Execute a command in the session's sandbox with a timeout.
#[allow(dead_code)]
pub async fn exec_in_sandbox(
    session_id: Uuid,
    cmd: &[&str],
    timeout: Duration,
) -> anyhow::Result<String> {
    let name = container_name(session_id);

    let mut args = vec!["exec", &name];
    args.extend(cmd);

    let child = Command::new("docker")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("exec in sandbox {name} returned non-zero: {stderr}");
            }
            Ok(stdout)
        }
        Ok(Err(e)) => Err(anyhow::anyhow!("exec failed: {e}")),
        Err(_) => {
            // Timeout: kill the exec (container still runs)
            tracing::warn!("exec in sandbox {name} timed out");
            Err(anyhow::anyhow!("exec timed out"))
        }
    }
}

/// Execute a command in the sandbox, streaming stdout line by line via a callback.
pub async fn exec_in_sandbox_streaming(
    session_id: Uuid,
    cmd: &[&str],
    timeout: Duration,
    mut on_line: impl FnMut(String) + Send,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let name = container_name(session_id);
    let mut args = vec!["exec".to_string(), name.clone()];
    args.extend(cmd.iter().map(|s| s.to_string()));

    let mut child = Command::new("docker")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdout"))?;

    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    let read_fut = async {
        while let Ok(Some(line)) = lines.next_line().await {
            on_line(line);
        }
        // Wait for process to finish
        child.wait().await
    };

    match tokio::time::timeout(timeout, read_fut).await {
        Ok(Ok(status)) => {
            if !status.success() {
                tracing::warn!("streaming exec in sandbox {name} exited with {status}");
            }
            Ok(())
        }
        Ok(Err(e)) => Err(anyhow::anyhow!("exec failed: {e}")),
        Err(_) => {
            let _ = child.kill().await;
            Err(anyhow::anyhow!("exec timed out"))
        }
    }
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
