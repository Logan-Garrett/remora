//! Allowlist and fetch integration tests for remora-server.
//!
//! These tests exercise `fetch::check_domain_allowed`, the approval flow,
//! and `fetch::extract_domain` against a real database.
//!
//! Set `DATABASE_URL` to run them.

mod common;

use common::TestServer;
use remora_server::db::Database;
use remora_server::fetch::{self, DomainStatus};

// ── Domain in global blocklist returns Blocked ──────────────────────
// Note: global_allowlist is a static table we can't easily populate
// through the Database trait. We test the check logic with what we can
// control (session allowlist) and verify the empty-table behavior for
// global lists.

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn fetch_unknown_domain_needs_approval() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("fetch-unknown").await.unwrap();

    // A domain not in any list should need approval
    let status = fetch::check_domain_allowed(db, sid, "unknown-domain.xyz")
        .await
        .unwrap();
    assert_eq!(status, DomainStatus::NeedsApproval);
}

// ── Domain in session allowlist returns Allowed ─────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn fetch_session_allowed_domain() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("fetch-allowed").await.unwrap();

    db.add_session_allowlist(sid, "trusted.com").await.unwrap();

    let status = fetch::check_domain_allowed(db, sid, "trusted.com")
        .await
        .unwrap();
    assert_eq!(status, DomainStatus::Allowed);
}

// ── Session allowlist does not leak across sessions ─────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn fetch_session_allowlist_isolation() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid1, _, _) = db.create_session("fetch-iso-1").await.unwrap();
    let (sid2, _, _) = db.create_session("fetch-iso-2").await.unwrap();

    db.add_session_allowlist(sid1, "only-for-s1.com")
        .await
        .unwrap();

    // sid1 should be allowed
    let s1 = fetch::check_domain_allowed(db, sid1, "only-for-s1.com")
        .await
        .unwrap();
    assert_eq!(s1, DomainStatus::Allowed);

    // sid2 should need approval
    let s2 = fetch::check_domain_allowed(db, sid2, "only-for-s1.com")
        .await
        .unwrap();
    assert_eq!(s2, DomainStatus::NeedsApproval);
}

// ── Global blocked/allowed with empty table ─────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn fetch_global_empty_not_blocked_not_allowed() {
    let server = TestServer::start().await;
    let db = server.db();

    // With empty global_allowlist, nothing is blocked or globally allowed
    assert!(!db.is_domain_blocked("anything.com").await.unwrap());
    assert!(!db.is_domain_global_allowed("anything.com").await.unwrap());
}

// ── Approval flow: create + resolve approved + domain in allowlist ──

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn fetch_approval_flow_approve() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("fetch-approve").await.unwrap();

    // Domain starts as needing approval
    let status = fetch::check_domain_allowed(db, sid, "new-domain.io")
        .await
        .unwrap();
    assert_eq!(status, DomainStatus::NeedsApproval);

    // Create a pending approval
    db.create_pending_approval(sid, "new-domain.io", "https://new-domain.io/page", "alice")
        .await
        .unwrap();

    // Still needs approval (not yet resolved)
    let status = fetch::check_domain_allowed(db, sid, "new-domain.io")
        .await
        .unwrap();
    assert_eq!(status, DomainStatus::NeedsApproval);

    // Resolve as approved (this also adds to session_allowlist)
    fetch::resolve_approval(db, sid, "new-domain.io", true)
        .await
        .unwrap();

    // Now the domain should be allowed
    let status = fetch::check_domain_allowed(db, sid, "new-domain.io")
        .await
        .unwrap();
    assert_eq!(status, DomainStatus::Allowed);

    // Pending should show as approved
    let approved = db.get_approved_pending(sid, "new-domain.io").await.unwrap();
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].0, "https://new-domain.io/page");
}

// ── Approval flow: create + resolve denied + domain still blocked ───

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn fetch_approval_flow_deny() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("fetch-deny").await.unwrap();

    db.create_pending_approval(sid, "bad-site.io", "https://bad-site.io/x", "bob")
        .await
        .unwrap();

    fetch::resolve_approval(db, sid, "bad-site.io", false)
        .await
        .unwrap();

    // Domain should still need approval (denied does not add to allowlist)
    let status = fetch::check_domain_allowed(db, sid, "bad-site.io")
        .await
        .unwrap();
    assert_eq!(status, DomainStatus::NeedsApproval);
}

// ── extract_domain ──────────────────────────────────────────────────

#[test]
fn fetch_extract_domain_basic() {
    assert_eq!(
        fetch::extract_domain("https://example.com/page").unwrap(),
        "example.com"
    );
    assert_eq!(
        fetch::extract_domain("http://sub.domain.org/path?q=1").unwrap(),
        "sub.domain.org"
    );
    assert!(fetch::extract_domain("not a url").is_err());
}

// ── extract_domain: various URL forms ──────────────────────────────

#[test]
fn fetch_extract_domain_various_urls() {
    // Standard https with path
    assert_eq!(
        fetch::extract_domain("https://example.com/path").unwrap(),
        "example.com"
    );

    // http with subdomain and port
    assert_eq!(
        fetch::extract_domain("http://sub.domain.org:8080/").unwrap(),
        "sub.domain.org"
    );

    // ftp scheme is parsed by the url crate but we accept any scheme
    assert_eq!(
        fetch::extract_domain("ftp://files.example.net/data").unwrap(),
        "files.example.net"
    );

    // Empty string should error
    assert!(
        fetch::extract_domain("").is_err(),
        "empty string should fail"
    );

    // No-scheme string should error (url crate requires scheme)
    assert!(
        fetch::extract_domain("example.com/path").is_err(),
        "no-scheme string should fail"
    );

    // URL with query and fragment
    assert_eq!(
        fetch::extract_domain("https://search.example.com/q?a=1#frag").unwrap(),
        "search.example.com"
    );

    // URL with auth info
    assert_eq!(
        fetch::extract_domain("https://user:pass@secure.example.com/").unwrap(),
        "secure.example.com"
    );
}
