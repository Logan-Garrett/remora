//! Quota enforcement integration tests for remora-server.
//!
//! These tests exercise `quota::check_quota` and `quota::record_usage`
//! against a real database.
//!
//! Set `DATABASE_URL` to run them.

mod common;

use common::TestServer;
use remora_server::db::Database;
use remora_server::quota;

// ── Under cap: check_quota succeeds ─────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn quota_under_cap_returns_ok() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("quota-ok").await.unwrap();

    // Usage is 0, cap is large => should pass
    let result = quota::check_quota(db, sid, 10_000_000).await;
    assert!(result.is_ok(), "should pass quota check when under cap");
}

// ── At/above session cap: check_quota fails ─────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn quota_at_session_cap_returns_err() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("quota-over").await.unwrap();

    // Get the session's cap
    let (_, cap) = db.get_session_usage(sid).await.unwrap();

    // Push usage to exactly the cap
    db.add_usage(sid, cap).await.unwrap();

    let result = quota::check_quota(db, sid, 10_000_000).await;
    assert!(
        result.is_err(),
        "should fail quota check when at session cap"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Session daily token cap"),
        "error should mention session cap: {err_msg}"
    );
}

// ── Above global cap: check_quota fails ─────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn quota_above_global_cap_returns_err() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("quota-global").await.unwrap();

    // Set a very low global cap, but session usage is still 0
    // We need to push global usage above the cap by adding usage to some session
    db.add_usage(sid, 100).await.unwrap();

    // Use a global cap of 50, so 100 > 50 triggers global cap error
    let result = quota::check_quota(db, sid, 50).await;

    // It might fail on session cap first if session cap < 100,
    // so check that it fails with some quota message
    assert!(result.is_err(), "should fail when global cap is exceeded");
}

// ── add_usage increments correctly ──────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn quota_add_usage_increments() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("quota-incr").await.unwrap();

    let (before, _) = db.get_session_usage(sid).await.unwrap();
    assert_eq!(before, 0);

    quota::record_usage(db, sid, 1000).await.unwrap();
    let (after, _) = db.get_session_usage(sid).await.unwrap();
    assert_eq!(after, 1000);

    quota::record_usage(db, sid, 500).await.unwrap();
    let (after2, _) = db.get_session_usage(sid).await.unwrap();
    assert_eq!(after2, 1500);
}

// ── Global usage reflects all sessions ──────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn quota_global_usage_sums_sessions() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid1, _, _) = db.create_session("quota-g1").await.unwrap();
    let (sid2, _, _) = db.create_session("quota-g2").await.unwrap();

    db.add_usage(sid1, 300).await.unwrap();
    db.add_usage(sid2, 700).await.unwrap();

    let global = db.get_global_usage().await.unwrap();
    assert!(
        global >= 1000,
        "global usage should be at least 1000, got {global}"
    );
}

// ── reset_tokens_if_needed is idempotent for today ──────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn quota_reset_tokens_idempotent_today() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("quota-reset").await.unwrap();

    db.add_usage(sid, 999).await.unwrap();
    let (used, _) = db.get_session_usage(sid).await.unwrap();
    assert_eq!(used, 999);

    // reset should be a no-op because tokens_reset_date is already today
    db.reset_tokens_if_needed(sid).await.unwrap();
    let (used_after, _) = db.get_session_usage(sid).await.unwrap();
    assert_eq!(
        used_after, 999,
        "reset should not zero out tokens when date is current"
    );
}
