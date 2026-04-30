//! Edge-case integration tests for remora-server.
//!
//! These cover race conditions, path traversal guards, boundary conditions,
//! and multi-subscriber fan-out.
//!
//! Set `DATABASE_URL` to run them.

mod common;

use common::TestServer;
use remora_server::db::Database;

// ── Concurrent /run: atomic insert_run guard ─────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn test_concurrent_run_only_one_starts() {
    let server = TestServer::start().await;
    let session_id = server.create_session("concurrent-run").await;

    // Test the DB-level guard directly: insert_run should succeed once,
    // then fail while the first is still 'running'.
    let run_id = server
        .db()
        .insert_run(session_id, "since_last")
        .await
        .expect("first insert_run should succeed");
    assert!(run_id > 0);

    // Second insert_run while first is still 'running' should fail
    let result = server.db().insert_run(session_id, "since_last").await;
    assert!(
        result.is_err(),
        "second insert_run should fail while first is in-flight"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("already in progress"),
        "error should mention 'already in progress', got: {err_msg}"
    );

    // After finishing the first run, a new one should succeed
    server
        .db()
        .finish_run(run_id, "completed")
        .await
        .expect("finish_run should succeed");

    let run_id_2 = server
        .db()
        .insert_run(session_id, "full")
        .await
        .expect("insert_run after finish should succeed");
    assert!(run_id_2 > run_id);

    // Cleanup
    server.db().finish_run(run_id_2, "completed").await.unwrap();
}

// ── /add path traversal blocked ────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn test_add_path_traversal_blocked() {
    let server = TestServer::start().await;
    let session_id = server.create_session("traversal-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "add", "author": "alice", "path": "../../etc/passwd"}),
    )
    .await;

    // Should get an error event about path escaping workspace or file not found
    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("escape")
                        || d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("not found")
                        || d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("File not found")
                })
        },
        5000,
    )
    .await;

    assert!(
        ev.is_some(),
        "path traversal attempt should be rejected with an error event"
    );

    // Verify no file event was emitted
    let remaining = TestServer::drain_events(&mut stream, 1000).await;
    let has_file_event = remaining
        .iter()
        .any(|ev| ev["type"] == "event" && ev.get("data").is_some_and(|d| d["kind"] == "file"));
    assert!(
        !has_file_event,
        "no file event should be emitted for a traversal path"
    );
}

// ── /add nonexistent file ──────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn test_add_nonexistent_file() {
    let server = TestServer::start().await;
    let session_id = server.create_session("add-nofile-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "add", "author": "alice", "path": "no_such_file.rs"}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && (d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("not found")
                            || d["payload"]["text"]
                                .as_str()
                                .unwrap_or("")
                                .contains("File not found")
                            || d["payload"]["text"]
                                .as_str()
                                .unwrap_or("")
                                .contains("No such file"))
                })
        },
        5000,
    )
    .await;

    assert!(
        ev.is_some(),
        "adding a nonexistent file should produce an error event"
    );
}

// ── Unicode chat round-trip ────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn test_unicode_chat_roundtrip() {
    let server = TestServer::start().await;
    let session_id = server.create_session("unicode-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    let unicode_text = "hello \u{1f389} \u{4e16}\u{754c}";
    TestServer::send_chat(&mut sink, "alice", unicode_text).await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "chat" && d["payload"]["text"].as_str() == Some(unicode_text)
                })
        },
        5000,
    )
    .await;

    assert!(
        ev.is_some(),
        "unicode chat message should round-trip intact"
    );
    let data = ev.unwrap();
    assert_eq!(
        data["data"]["payload"]["text"].as_str().unwrap(),
        unicode_text
    );
}

// ── /diff on empty session ─────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn test_diff_no_repos() {
    let server = TestServer::start().await;
    let session_id = server.create_session("diff-empty-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "diff", "author": "alice"}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "diff"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("No changes")
                })
        },
        5000,
    )
    .await;

    assert!(
        ev.is_some(),
        "diff on empty session should return 'No changes' event"
    );
}

// ── WS leave sets idle_since ───────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn test_ws_leave_sets_idle_since() {
    let server = TestServer::start().await;
    let session_id = server.create_session("idle-test").await;
    let db = server.db();

    // Verify idle_since is initially NULL (via get_idle_sessions with 0 timeout,
    // which would only return sessions that already have idle_since set)
    // Instead, just check that no idle sessions are returned for a very short timeout.

    // Connect and disconnect
    {
        let (sink, stream) = server.connect_ws(session_id, "alice").await;
        // Give the server time to process join
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        drop(sink);
        drop(stream);
    }

    // Give the server time to process the disconnect and set idle_since
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // Now the session should appear as idle (with a very large cutoff so any
    // idle_since in the past qualifies)
    let idle = db.get_idle_sessions(0).await.unwrap();
    assert!(
        idle.contains(&session_id),
        "after last participant leaves, session should have idle_since set"
    );
}

// ── Quota at exact boundary ────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn test_quota_at_exact_boundary() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("quota-boundary").await.unwrap();

    // Get the session's cap
    let (_, cap) = db.get_session_usage(sid).await.unwrap();

    // Set usage to cap - 1
    db.add_usage(sid, cap - 1).await.unwrap();

    // Should still pass at cap - 1 (use i64::MAX for global cap so only session cap matters)
    let result = remora_server::quota::check_quota(db, sid, i64::MAX).await;
    assert!(
        result.is_ok(),
        "should pass quota check when usage is cap - 1"
    );

    // Add 1 more to reach exactly the cap
    db.add_usage(sid, 1).await.unwrap();

    // Should now fail at exactly the cap
    let result = remora_server::quota::check_quota(db, sid, i64::MAX).await;
    assert!(
        result.is_err(),
        "should fail quota check when usage equals cap"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Session daily token cap"),
        "error should mention session cap: {err_msg}"
    );
}

// ── Multiple subscribers fan-out ───────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn test_multiple_subscribers_fanout() {
    let server = TestServer::start().await;
    let session_id = server.create_session("fanout-5-test").await;

    // Connect 5 clients
    let mut sinks = Vec::new();
    let mut streams = Vec::new();

    for i in 0..5 {
        let name = format!("user{i}");
        let (sink, stream) = server.connect_ws(session_id, &name).await;
        sinks.push(sink);
        streams.push(stream);
    }

    // Drain join events from all streams
    for stream in &mut streams {
        let _ = TestServer::drain_events(stream, 1500).await;
    }

    // User0 sends a chat
    TestServer::send_chat(&mut sinks[0], "user0", "broadcast test").await;

    // All 5 clients should receive the chat event
    let mut received_count = 0;
    for stream in &mut streams {
        let ev = TestServer::wait_for_event_matching(
            stream,
            |ev| {
                ev["type"] == "event"
                    && ev.get("data").is_some_and(|d| {
                        d["kind"] == "chat" && d["payload"]["text"] == "broadcast test"
                    })
            },
            5000,
        )
        .await;
        if ev.is_some() {
            received_count += 1;
        }
    }

    assert_eq!(
        received_count, 5,
        "all 5 subscribers should receive the chat event, but only {received_count} did"
    );
}
