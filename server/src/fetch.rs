use crate::db::{Database, DatabaseBackend};
use std::sync::Arc;
use uuid::Uuid;

const MAX_FETCH_BYTES: usize = 100 * 1024; // 100KB

/// Domain allowlist check result.
#[derive(Debug, PartialEq, Eq)]
pub enum DomainStatus {
    Allowed,
    Blocked,
    NeedsApproval,
}

/// Check if a domain is allowed for a session.
/// Order: global blocklist > global allowlist > session allowlist > needs approval.
pub async fn check_domain_allowed(
    db: &Arc<DatabaseBackend>,
    session_id: Uuid,
    domain: &str,
) -> anyhow::Result<DomainStatus> {
    // Check global blocklist
    if db.is_domain_blocked(domain).await? {
        return Ok(DomainStatus::Blocked);
    }

    // Check global allowlist
    if db.is_domain_global_allowed(domain).await? {
        return Ok(DomainStatus::Allowed);
    }

    // Check session allowlist
    if db.is_domain_session_allowed(session_id, domain).await? {
        return Ok(DomainStatus::Allowed);
    }

    Ok(DomainStatus::NeedsApproval)
}

/// Fetch a URL and return the body as a string, streaming up to 100KB.
/// Uses chunked reading to avoid downloading the entire response body
/// into memory before truncating.
pub async fn fetch_url(url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client.get(url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }

    let mut buf = Vec::with_capacity(MAX_FETCH_BYTES);
    let mut truncated = false;
    let mut stream = resp;

    while let Some(chunk) = stream.chunk().await? {
        let remaining = MAX_FETCH_BYTES.saturating_sub(buf.len());
        if remaining == 0 {
            truncated = true;
            break;
        }
        if chunk.len() > remaining {
            buf.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        buf.extend_from_slice(&chunk);
    }

    let text = String::from_utf8_lossy(&buf).to_string();
    if truncated {
        Ok(format!("{text}\n\n[truncated at {MAX_FETCH_BYTES} bytes]"))
    } else {
        Ok(text)
    }
}

/// Create a pending approval request for a domain.
pub async fn create_approval_request(
    db: &Arc<DatabaseBackend>,
    session_id: Uuid,
    domain: &str,
    url: &str,
    author: &str,
) -> anyhow::Result<()> {
    db.create_pending_approval(session_id, domain, url, author)
        .await?;

    // Insert event so participants are notified
    crate::ws::insert_event(
        db,
        session_id,
        "system",
        "allowlist_request",
        serde_json::json!({
            "domain": domain,
            "url": url,
            "requested_by": author,
            "text": format!("{author} requested fetch approval for domain: {domain}")
        }),
    )
    .await?;

    Ok(())
}

/// Resolve a pending approval.
pub async fn resolve_approval(
    db: &Arc<DatabaseBackend>,
    session_id: Uuid,
    domain: &str,
    approved: bool,
) -> anyhow::Result<()> {
    db.resolve_approval(session_id, domain, approved).await?;
    Ok(())
}

/// Extract domain from a URL string.
pub fn extract_domain(url_str: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(url_str)?;
    parsed
        .host_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("no host in URL"))
}
