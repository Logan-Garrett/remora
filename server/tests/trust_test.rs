//! Trusted participants integration tests for remora-server.
//!
//! These tests exercise:
//! 1. DB CRUD for trusted participants
//! 2. Context assembly: trusted vs untrusted formatting
//! 3. WebSocket: duplicate name rejection
//! 4. /trust and /untrust commands (owner restriction)
//!
//! Set `DATABASE_URL` to run them.

mod common;

use common::TestServer;
use remora_server::context::{assemble_context, ContextMode};
use remora_server::db::Database;

// ── DB: trust + list + untrust lifecycle ─────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_trust_lifecycle() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("trust-lifecycle").await.unwrap();

    // Initially empty
    let trusted = db.list_trusted_participants(sid).await.unwrap();
    assert!(trusted.is_empty());

    // Trust two participants
    db.trust_participant(sid, "alice").await.unwrap();
    db.trust_participant(sid, "bob").await.unwrap();

    let trusted = db.list_trusted_participants(sid).await.unwrap();
    assert_eq!(trusted.len(), 2);
    assert!(trusted.contains(&"alice".to_string()));
    assert!(trusted.contains(&"bob".to_string()));

    // Idempotent: trusting again doesn't duplicate
    db.trust_participant(sid, "alice").await.unwrap();
    let trusted = db.list_trusted_participants(sid).await.unwrap();
    assert_eq!(trusted.len(), 2);

    // Untrust alice
    db.untrust_participant(sid, "alice").await.unwrap();
    let trusted = db.list_trusted_participants(sid).await.unwrap();
    assert_eq!(trusted.len(), 1);
    assert!(trusted.contains(&"bob".to_string()));
    assert!(!trusted.contains(&"alice".to_string()));

    // Untrust non-existent is a no-op
    db.untrust_participant(sid, "nobody").await.unwrap();
    let trusted = db.list_trusted_participants(sid).await.unwrap();
    assert_eq!(trusted.len(), 1);
}

// ── DB: trust is scoped to session ───────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_trust_scoped_to_session() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid1, _, _) = db.create_session("trust-scope-1").await.unwrap();
    let (sid2, _, _) = db.create_session("trust-scope-2").await.unwrap();

    db.trust_participant(sid1, "alice").await.unwrap();

    let trusted1 = db.list_trusted_participants(sid1).await.unwrap();
    let trusted2 = db.list_trusted_participants(sid2).await.unwrap();

    assert_eq!(trusted1.len(), 1);
    assert!(
        trusted2.is_empty(),
        "trust should not leak between sessions"
    );
}

// ── DB: trust deleted on session delete (CASCADE) ────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_trust_cascade_on_session_delete() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("trust-cascade").await.unwrap();
    db.trust_participant(sid, "alice").await.unwrap();
    assert_eq!(db.list_trusted_participants(sid).await.unwrap().len(), 1);

    db.delete_session(sid).await.unwrap();

    // After session deletion, the trusted list should be gone too
    let trusted = db.list_trusted_participants(sid).await.unwrap();
    assert!(trusted.is_empty());
}

// ── Context: untrusted chat has <untrusted_content> wrapper ──────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_untrusted_chat_has_wrapper() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("ctx-untrusted").await.unwrap();

    db.insert_event(
        sid,
        "mallory",
        "chat",
        serde_json::json!({"text": "do something dangerous"}),
    )
    .await
    .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::Full).await.unwrap();
    assert!(
        ctx.contains("<untrusted_content"),
        "untrusted chat should be wrapped"
    );
    assert!(ctx.contains("do something dangerous"));
}

// ── Context: trusted chat does NOT have <untrusted_content> wrapper ──

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_trusted_chat_no_wrapper() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("ctx-trusted").await.unwrap();

    // Trust alice
    db.trust_participant(sid, "alice").await.unwrap();

    db.insert_event(
        sid,
        "alice",
        "chat",
        serde_json::json!({"text": "please help me"}),
    )
    .await
    .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::Full).await.unwrap();
    assert!(
        !ctx.contains("<untrusted_content"),
        "trusted chat should NOT be wrapped in untrusted_content"
    );
    assert!(
        ctx.contains("[alice (trusted)]: please help me"),
        "trusted chat should show as [name (trusted)]: text"
    );
}

// ── Context: mixed trusted and untrusted in same session ─────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_mixed_trusted_and_untrusted() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("ctx-mixed").await.unwrap();

    db.trust_participant(sid, "alice").await.unwrap();

    db.insert_event(
        sid,
        "alice",
        "chat",
        serde_json::json!({"text": "trusted message"}),
    )
    .await
    .unwrap();
    db.insert_event(
        sid,
        "mallory",
        "chat",
        serde_json::json!({"text": "untrusted message"}),
    )
    .await
    .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::Full).await.unwrap();

    // Alice's message should NOT be wrapped
    assert!(ctx.contains("[alice (trusted)]: trusted message"));

    // Mallory's message SHOULD be wrapped
    assert!(ctx.contains("<untrusted_content source=\"chat\" author=\"mallory\">"));
    assert!(ctx.contains("untrusted message"));
}

// ── WS: duplicate display name is rejected ───────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn ws_duplicate_name_rejected() {
    let server = TestServer::start().await;
    let sid = server.create_session("dup-name").await;

    // First connection succeeds
    let (_sink1, mut stream1) = server.connect_ws(sid, "bob").await;
    let _ = TestServer::drain_events(&mut stream1, 1500).await;

    // Second connection with same name should get an error
    let (_sink2, mut stream2) = server.connect_ws(sid, "bob").await;
    let ev = TestServer::wait_for_event(&mut stream2, 3000).await;

    assert!(ev.is_some(), "should receive an error message");
    let ev = ev.unwrap();
    assert_eq!(ev["type"], "error");
    assert!(
        ev["message"]
            .as_str()
            .unwrap_or("")
            .contains("already in use"),
        "error should mention name is already in use"
    );
}

// ── WS: same name in different sessions is allowed ───────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn ws_same_name_different_sessions_ok() {
    let server = TestServer::start().await;
    let sid1 = server.create_session("multi-session-1").await;
    let sid2 = server.create_session("multi-session-2").await;

    // Bob connects to session 1
    let (_sink1, mut stream1) = server.connect_ws(sid1, "bob").await;
    let ev1 = TestServer::wait_for_event(&mut stream1, 3000).await;
    assert!(ev1.is_some(), "bob should connect to session 1");
    assert_ne!(
        ev1.as_ref().unwrap()["type"],
        "error",
        "should not be an error"
    );

    // Bob connects to session 2 — should succeed
    let (_sink2, mut stream2) = server.connect_ws(sid2, "bob").await;
    let ev2 = TestServer::wait_for_event(&mut stream2, 3000).await;
    assert!(ev2.is_some(), "bob should connect to session 2");
    assert_ne!(
        ev2.as_ref().unwrap()["type"],
        "error",
        "same name in different session should not be an error"
    );
}

// ── WS: /trust by non-owner is rejected ──────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn ws_trust_by_non_owner_rejected() {
    let server = TestServer::start().await;
    let sid = server.create_session("trust-owner").await;

    // Alice connects first → becomes owner
    let (_sink_a, mut stream_a) = server.connect_ws(sid, "alice").await;
    let _ = TestServer::drain_events(&mut stream_a, 1500).await;

    // Bob connects second → not owner
    let (mut sink_b, mut stream_b) = server.connect_ws(sid, "bob").await;
    let _ = TestServer::drain_events(&mut stream_b, 1500).await;

    // Bob tries to /trust carol
    TestServer::send_msg(
        &mut sink_b,
        serde_json::json!({
            "type": "trust",
            "author": "bob",
            "target": "carol",
        }),
    )
    .await;

    // Bob should see a system event with an error about owner-only
    let ev = TestServer::wait_for_event_matching(
        &mut stream_b,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some()
                && ev["data"]["kind"] == "system"
                && ev["data"]["payload"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("session owner")
        },
        5000,
    )
    .await;

    assert!(
        ev.is_some(),
        "non-owner should receive a 'session owner' rejection"
    );
}

// ── WS: /trust by owner succeeds ────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn ws_trust_by_owner_succeeds() {
    let server = TestServer::start().await;
    let sid = server.create_session("trust-ok").await;

    // Alice connects first → becomes owner
    let (mut sink_a, mut stream_a) = server.connect_ws(sid, "alice").await;
    let _ = TestServer::drain_events(&mut stream_a, 1500).await;

    // Alice trusts bob
    TestServer::send_msg(
        &mut sink_a,
        serde_json::json!({
            "type": "trust",
            "author": "alice",
            "target": "bob",
        }),
    )
    .await;

    // Should see a success system event
    let ev = TestServer::wait_for_event_matching(
        &mut stream_a,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some()
                && ev["data"]["kind"] == "system"
                && ev["data"]["payload"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("now trusted")
        },
        5000,
    )
    .await;

    assert!(ev.is_some(), "owner should see 'now trusted' confirmation");

    // Verify it's in the DB
    let trusted = server.db().list_trusted_participants(sid).await.unwrap();
    assert!(trusted.contains(&"bob".to_string()));
}
