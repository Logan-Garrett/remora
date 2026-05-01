//! AppState unit/integration tests for remora-server.
//!
//! These tests exercise participant tracking and event dispatch on `AppState`
//! directly, without going through WebSocket connections.
//!
//! Set `DATABASE_URL` to run them.

mod common;

use common::TestServer;
use remora_server::db::Database;

// ── participant join + leave ────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn state_try_participant_join_leave() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("state-parts").await.unwrap();

    let config = remora_server::state::Config {
        workspace_dir: std::path::PathBuf::from("/tmp/remora-test-state-parts"),
        run_timeout_secs: 60,
        idle_timeout_secs: 1800,
        global_daily_cap: 10_000_000,
        claude_cmd: "echo".into(),
        docker_image: "ubuntu:22.04".into(),
        skip_permissions: true,
        use_sandbox: false,
        permission_mode: String::new(),
        allowed_tools: vec![],
        backfill_limit: 500,
        max_sessions: 100,
    };

    let state = remora_server::state::AppState::new(db.clone(), "test-token".to_string(), config);

    // Join 3 participants
    state.try_participant_join(sid, "alice").await;
    state.try_participant_join(sid, "bob").await;
    state.try_participant_join(sid, "charlie").await;

    let parts = state.get_participants(sid).await;
    assert_eq!(parts.len(), 3, "should have 3 participants");
    assert!(parts.contains(&"alice".to_string()));
    assert!(parts.contains(&"bob".to_string()));
    assert!(parts.contains(&"charlie".to_string()));

    // Leave one
    state.participant_leave(sid, "alice").await;

    let parts = state.get_participants(sid).await;
    assert_eq!(parts.len(), 2, "should have 2 participants after leave");
    assert!(!parts.contains(&"alice".to_string()));
    assert!(parts.contains(&"bob".to_string()));
    assert!(parts.contains(&"charlie".to_string()));

    // Leaving a non-existent participant is a no-op
    state.participant_leave(sid, "nonexistent").await;
    let parts = state.get_participants(sid).await;
    assert_eq!(parts.len(), 2, "no-op leave should not change count");

    // Duplicate join is rejected
    let dup = state.try_participant_join(sid, "bob").await;
    assert!(!dup, "duplicate join should return false");
    let parts = state.get_participants(sid).await;
    assert_eq!(parts.len(), 2, "duplicate join should not increase count");
}

// ── subscribe receives dispatched events ────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn state_subscribe_receives_dispatched_events() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("state-sub").await.unwrap();

    let config = remora_server::state::Config {
        workspace_dir: std::path::PathBuf::from("/tmp/remora-test-state-sub"),
        run_timeout_secs: 60,
        idle_timeout_secs: 1800,
        global_daily_cap: 10_000_000,
        claude_cmd: "echo".into(),
        docker_image: "ubuntu:22.04".into(),
        skip_permissions: true,
        use_sandbox: false,
        permission_mode: String::new(),
        allowed_tools: vec![],
        backfill_limit: 500,
        max_sessions: 100,
    };

    let state = std::sync::Arc::new(remora_server::state::AppState::new(
        db.clone(),
        "test-token".to_string(),
        config,
    ));

    // Subscribe to the session
    let (mut rx, _cancel) = state.subscribe(sid, "tester").await;

    // Construct an event and dispatch it directly through AppState
    let event = remora_common::Event {
        id: 42,
        session_id: sid,
        timestamp: chrono::Utc::now(),
        author: Some("tester".to_string()),
        kind: "chat".to_string(),
        payload: serde_json::json!({"text": "dispatched message"}),
    };

    state.dispatch(event.clone()).await;

    // The subscriber should receive the event
    let received = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await;

    assert!(received.is_ok(), "should receive event within timeout");
    let got = received.unwrap().expect("channel should not be closed");
    assert_eq!(got.id, 42);
    assert_eq!(got.kind, "chat");
    assert_eq!(got.payload["text"], "dispatched message");

    // Dispatch a second event to make sure the channel stays alive
    let event2 = remora_common::Event {
        id: 43,
        session_id: sid,
        timestamp: chrono::Utc::now(),
        author: Some("tester".to_string()),
        kind: "system".to_string(),
        payload: serde_json::json!({"text": "second event"}),
    };

    state.dispatch(event2).await;

    let received2 = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await;
    assert!(received2.is_ok(), "should receive second event");
    let got2 = received2.unwrap().expect("channel should not be closed");
    assert_eq!(got2.id, 43);
}
