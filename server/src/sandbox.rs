use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

/// Docker image name for the sandbox. Built from Dockerfile.sandbox.
const SANDBOX_IMAGE: &str = "remora-sandbox";

/// Name for the docker container associated with a session.
fn container_name(session_id: Uuid) -> String {
    format!("remora-{session_id}")
}

/// Build the sandbox Docker image if it doesn't exist.
pub async fn ensure_image() -> anyhow::Result<()> {
    // Check if image exists
    let output = Command::new("docker")
        .args(["image", "inspect", SANDBOX_IMAGE])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;

    if output.success() {
        return Ok(());
    }

    tracing::info!("building sandbox image '{SANDBOX_IMAGE}' from Dockerfile.sandbox...");

    // Find the Dockerfile — look relative to the server binary, then cwd
    let dockerfile = find_dockerfile().await?;

    let output = Command::new("docker")
        .args(["build", "-t", SANDBOX_IMAGE, "-f", &dockerfile, "."])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to build sandbox image: {stderr}");
    }

    tracing::info!("sandbox image built successfully");
    Ok(())
}

/// Create a docker sandbox for a session.
pub async fn create_sandbox(
    session_id: Uuid,
    workspace_path: &Path,
    _docker_image: &str, // unused now — we use SANDBOX_IMAGE
) -> anyhow::Result<()> {
    // Ensure the sandbox image is built
    ensure_image().await?;

    let name = container_name(session_id);
    let workspace = workspace_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-utf8 workspace path"))?;

    let mut create_args = vec![
        "create".to_string(),
        "--name".to_string(),
        name.clone(),
        "--cpus".to_string(),
        "2".to_string(),
        "--memory".to_string(),
        "2g".to_string(),
        "--pids-limit".to_string(),
        "512".to_string(),
        "-v".to_string(),
        format!("{workspace}:/workspace"),
        "-w".to_string(),
        "/workspace".to_string(),
    ];

    // Pass API key into the container (required for sandbox auth).
    // Claude Code's OAuth session can't be exported to containers —
    // an ANTHROPIC_API_KEY is the only reliable auth method for sandboxed runs.
    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
        create_args.extend(["-e".to_string(), format!("ANTHROPIC_API_KEY={api_key}")]);
    } else {
        tracing::warn!(
            "ANTHROPIC_API_KEY not set — Claude inside the sandbox may not be able to authenticate. \
             Set ANTHROPIC_API_KEY or disable sandbox (REMORA_USE_SANDBOX=false)."
        );
    }

    // Use the pre-built sandbox image (has Claude + Node installed)
    create_args.push(SANDBOX_IMAGE.to_string());

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

/// Execute a command in the session's sandbox, returning the child process for streaming.
pub async fn exec_in_sandbox(
    session_id: Uuid,
    cmd: &[&str],
    _timeout: Duration,
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

    let output = Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", &name])
        .output()
        .await?;

    if output.status.success() {
        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if status == "running" {
            return Ok(());
        }
        if status == "created" || status == "exited" {
            let start = Command::new("docker")
                .args(["start", &name])
                .output()
                .await?;
            if start.status.success() {
                return Ok(());
            }
        }
        let _ = destroy_sandbox(session_id).await;
    }

    create_sandbox(session_id, workspace_path, docker_image).await
}

/// Check if a sandbox container exists for a session.
#[allow(dead_code)]
pub async fn sandbox_exists(session_id: Uuid) -> bool {
    let name = container_name(session_id);
    Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", &name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Find the Dockerfile.sandbox in likely locations.
async fn find_dockerfile() -> anyhow::Result<String> {
    let candidates = [
        "Dockerfile.sandbox",
        "../Dockerfile.sandbox",
        "../../Dockerfile.sandbox",
    ];
    for path in &candidates {
        if tokio::fs::metadata(path).await.is_ok() {
            return Ok(path.to_string());
        }
    }
    anyhow::bail!(
        "Dockerfile.sandbox not found. Build the image manually: \
         docker build -t remora-sandbox -f Dockerfile.sandbox ."
    )
}
