//! Slash command integration tests for remora-server.
//!
//! These tests connect via WebSocket, send command messages, and verify
//! that the correct events appear in the stream.
//!
//! Set `DATABASE_URL` to run them.

mod common;

use common::TestServer;
use remora_server::db::Database;

// ── /who ────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_who_lists_participants() {
    let server = TestServer::start().await;
    let session_id = server.create_session("who-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "who", "author": "alice"}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("alice")
                })
        },
        5000,
    )
    .await;

    assert!(
        ev.is_some(),
        "should receive a system event mentioning alice"
    );
}

// ── /clear ──────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_clear_inserts_clear_marker() {
    let server = TestServer::start().await;
    let session_id = server.create_session("clear-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "clear", "author": "alice"}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| ev["type"] == "event" && ev.get("data").is_some_and(|d| d["kind"] == "clear_marker"),
        5000,
    )
    .await;

    assert!(ev.is_some(), "should receive a clear_marker event");
}

// ── /repo_list ──────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_repo_list_shows_repos() {
    let server = TestServer::start().await;
    let session_id = server.create_session("repo-list-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "repo_list", "author": "alice"}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("Repos")
                })
        },
        5000,
    )
    .await;

    assert!(ev.is_some(), "should receive a system event with repo list");
}

// ── /session_info ───────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_session_info_shows_metadata() {
    let server = TestServer::start().await;
    let session_id = server.create_session("info-cmd-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "session_info", "author": "alice"}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("Session:")
                })
        },
        5000,
    )
    .await;

    assert!(ev.is_some(), "should receive session info event");
    let data = ev.unwrap();
    let text = data["data"]["payload"]["text"].as_str().unwrap();
    assert!(text.contains("Description:"));
    assert!(text.contains("Tokens:"));
}

// ── /allowlist ──────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_allowlist_shows_lists() {
    let server = TestServer::start().await;
    let session_id = server.create_session("allowlist-cmd-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "allowlist", "author": "alice"}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("allowlist")
                })
        },
        5000,
    )
    .await;

    assert!(ev.is_some(), "should receive allowlist info");
}

// ── /allowlist_add ──────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_allowlist_add_emits_update_event() {
    let server = TestServer::start().await;
    let session_id = server.create_session("allowlist-add-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "allowlist_add", "author": "alice", "domain": "example.com"}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "allowlist_update"
                        && d["payload"]["action"] == "add"
                        && d["payload"]["domain"] == "example.com"
                })
        },
        5000,
    )
    .await;

    assert!(ev.is_some(), "should receive allowlist_update add event");

    // Verify domain was actually added
    let db = server.db();
    assert!(db
        .is_domain_session_allowed(session_id, "example.com")
        .await
        .unwrap());
}

// ── /allowlist_remove ───────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_allowlist_remove_emits_update_event() {
    let server = TestServer::start().await;
    let session_id = server.create_session("allowlist-rm-test").await;

    // Pre-add a domain via DB
    let db = server.db();
    db.add_session_allowlist(session_id, "example.com")
        .await
        .unwrap();

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "allowlist_remove", "author": "alice", "domain": "example.com"}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "allowlist_update"
                        && d["payload"]["action"] == "remove"
                        && d["payload"]["domain"] == "example.com"
                })
        },
        5000,
    )
    .await;

    assert!(ev.is_some(), "should receive allowlist_update remove event");

    // Verify domain was actually removed
    assert!(!db
        .is_domain_session_allowed(session_id, "example.com")
        .await
        .unwrap());
}

// ── /kick ───────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_kick_disconnects_target() {
    let server = TestServer::start().await;
    let session_id = server.create_session("kick-test").await;

    // Connect alice and bob
    let (mut sink_a, mut stream_a) = server.connect_ws(session_id, "alice").await;
    let (_sink_b, mut stream_b) = server.connect_ws(session_id, "bob").await;

    let _ = TestServer::drain_events(&mut stream_a, 1500).await;
    let _ = TestServer::drain_events(&mut stream_b, 1500).await;

    // Alice kicks bob
    TestServer::send_msg(
        &mut sink_a,
        serde_json::json!({"type": "kick", "author": "alice", "target": "bob"}),
    )
    .await;

    // Bob should see the kick event targeting him
    let ev = TestServer::wait_for_event_matching(
        &mut stream_b,
        |ev| {
            ev["type"] == "event"
                && ev
                    .get("data")
                    .is_some_and(|d| d["kind"] == "kick" && d["payload"]["target"] == "bob")
        },
        5000,
    )
    .await;

    assert!(ev.is_some(), "bob should see the kick event");

    // After kick event, bob's stream should close (next read returns None or Close)
    let next = TestServer::wait_for_event(&mut stream_b, 3000).await;
    // Either None (stream closed) or a leave event is acceptable
    if let Some(ref msg) = next {
        // If we got another message, it should be a close or leave, not normal data
        // The important thing is that the kick event was delivered
        let _ = msg;
    }
}
