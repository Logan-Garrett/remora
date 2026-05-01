//! Database trait integration tests for remora-server.
//!
//! These tests exercise every `Database` trait method through the
//! TestServer's database handle. They must pass on both Postgres and SQLite.
//!
//! Set `DATABASE_URL` (and optionally `REMORA_DB_PROVIDER`) to run them.

mod common;

use common::TestServer;
use remora_server::db::Database;

// ── Sessions: create + list + delete ────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_create_and_list_sessions() {
    let server = TestServer::start().await;
    let db = server.db();

    let (id1, desc1, _) = db.create_session("alpha").await.unwrap();
    let (id2, desc2, _) = db.create_session("beta").await.unwrap();

    assert_eq!(desc1, "alpha");
    assert_eq!(desc2, "beta");
    assert_ne!(id1, id2);

    let sessions = db.list_sessions().await.unwrap();
    let ids: Vec<uuid::Uuid> = sessions.iter().map(|(id, _, _)| *id).collect();
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_delete_session() {
    let server = TestServer::start().await;
    let db = server.db();

    let (id, _, _) = db.create_session("delete-me").await.unwrap();
    assert!(db.session_exists(id).await.unwrap());

    let deleted = db.delete_session(id).await.unwrap();
    assert_eq!(deleted, 1);
    assert!(!db.session_exists(id).await.unwrap());
}

// ── session_exists ──────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_session_exists_true_and_false() {
    let server = TestServer::start().await;
    let db = server.db();

    let (id, _, _) = db.create_session("exists-test").await.unwrap();
    assert!(db.session_exists(id).await.unwrap());

    let fake = uuid::Uuid::new_v4();
    assert!(!db.session_exists(fake).await.unwrap());
}

// ── Events: insert + get_by_id + get_for_session ────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_insert_and_get_events() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("event-test").await.unwrap();

    let eid1 = db
        .insert_event(sid, "alice", "chat", serde_json::json!({"text": "hello"}))
        .await
        .unwrap();
    let eid2 = db
        .insert_event(sid, "bob", "chat", serde_json::json!({"text": "world"}))
        .await
        .unwrap();

    assert!(eid2 > eid1);

    // get_event_by_id
    let ev = db.get_event_by_id(eid1).await.unwrap().unwrap();
    assert_eq!(ev.id, eid1);
    assert_eq!(ev.session_id, sid);
    assert_eq!(ev.kind, "chat");
    assert_eq!(ev.author, Some("alice".to_string()));
    assert_eq!(ev.payload["text"], "hello");

    // Non-existent event
    let missing = db.get_event_by_id(999_999_999).await.unwrap();
    assert!(missing.is_none());

    // get_events_for_session
    let events = db.get_events_for_session(sid).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].id, eid1);
    assert_eq!(events[1].id, eid2);
}

// ── get_events_since ────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_get_events_since() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("since-test").await.unwrap();

    let eid1 = db
        .insert_event(sid, "a", "chat", serde_json::json!({"text": "1"}))
        .await
        .unwrap();
    let eid2 = db
        .insert_event(sid, "b", "chat", serde_json::json!({"text": "2"}))
        .await
        .unwrap();
    let _eid3 = db
        .insert_event(sid, "c", "chat", serde_json::json!({"text": "3"}))
        .await
        .unwrap();

    // Since eid1 => should get eid2 and eid3
    let since = db.get_events_since(sid, eid1).await.unwrap();
    assert_eq!(since.len(), 2);
    assert_eq!(since[0].0, eid2);

    // Since eid2 => should get only eid3
    let since2 = db.get_events_since(sid, eid2).await.unwrap();
    assert_eq!(since2.len(), 1);
}

// ── get_last_context_boundary ───────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_last_context_boundary() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("boundary-test").await.unwrap();

    // No events => boundary is 0
    let boundary = db.get_last_context_boundary(sid).await.unwrap();
    assert_eq!(boundary, 0);

    // Insert a chat (not a boundary)
    db.insert_event(sid, "x", "chat", serde_json::json!({"text": "hi"}))
        .await
        .unwrap();
    let boundary = db.get_last_context_boundary(sid).await.unwrap();
    assert_eq!(boundary, 0);

    // Insert a clear_marker
    let clear_id = db
        .insert_event(
            sid,
            "x",
            "clear_marker",
            serde_json::json!({"text": "cleared"}),
        )
        .await
        .unwrap();
    let boundary = db.get_last_context_boundary(sid).await.unwrap();
    assert_eq!(boundary, clear_id);

    // Insert a claude_response
    let cr_id = db
        .insert_event(
            sid,
            "claude",
            "claude_response",
            serde_json::json!({"text": "response"}),
        )
        .await
        .unwrap();
    let boundary = db.get_last_context_boundary(sid).await.unwrap();
    assert_eq!(boundary, cr_id);
}

// ── Repos: upsert + list + list_names + delete ──────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_repos_crud() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("repo-test").await.unwrap();

    // Empty initially
    assert!(db.list_repos(sid).await.unwrap().is_empty());
    assert!(db.list_repo_names(sid).await.unwrap().is_empty());

    // Upsert
    db.upsert_repo(sid, "frontend", "https://github.com/x/frontend.git")
        .await
        .unwrap();
    db.upsert_repo(sid, "backend", "https://github.com/x/backend.git")
        .await
        .unwrap();

    let repos = db.list_repos(sid).await.unwrap();
    assert_eq!(repos.len(), 2);

    let names = db.list_repo_names(sid).await.unwrap();
    assert!(names.contains(&"frontend".to_string()));
    assert!(names.contains(&"backend".to_string()));

    // Upsert same name updates URL
    db.upsert_repo(sid, "frontend", "https://github.com/y/frontend.git")
        .await
        .unwrap();
    let repos = db.list_repos(sid).await.unwrap();
    let frontend_url = repos.iter().find(|(n, _)| n == "frontend").unwrap();
    assert_eq!(frontend_url.1, "https://github.com/y/frontend.git");
    assert_eq!(repos.len(), 2); // still 2, not 3

    // Delete
    db.delete_repo(sid, "frontend").await.unwrap();
    let names = db.list_repo_names(sid).await.unwrap();
    assert_eq!(names.len(), 1);
    assert!(names.contains(&"backend".to_string()));
}

// ── Runs: insert + is_in_flight + finish ────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_runs_lifecycle() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("run-test").await.unwrap();

    assert!(!db.is_run_in_flight(sid).await.unwrap());

    let run_id = db.insert_run(sid, "since_last").await.unwrap();
    assert!(db.is_run_in_flight(sid).await.unwrap());

    db.finish_run(run_id, "completed").await.unwrap();
    assert!(!db.is_run_in_flight(sid).await.unwrap());
}

// ── Session allowlist: add + list + remove + is_domain_session_allowed

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_session_allowlist_crud() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("allowlist-test").await.unwrap();

    assert!(db.list_session_allowlist(sid).await.unwrap().is_empty());
    assert!(!db
        .is_domain_session_allowed(sid, "example.com")
        .await
        .unwrap());

    db.add_session_allowlist(sid, "example.com").await.unwrap();
    db.add_session_allowlist(sid, "test.org").await.unwrap();

    let list = db.list_session_allowlist(sid).await.unwrap();
    assert_eq!(list.len(), 2);
    assert!(list.contains(&"example.com".to_string()));
    assert!(list.contains(&"test.org".to_string()));

    assert!(db
        .is_domain_session_allowed(sid, "example.com")
        .await
        .unwrap());
    assert!(!db
        .is_domain_session_allowed(sid, "other.net")
        .await
        .unwrap());

    // Duplicate insert should be idempotent
    db.add_session_allowlist(sid, "example.com").await.unwrap();
    let list = db.list_session_allowlist(sid).await.unwrap();
    assert_eq!(list.len(), 2);

    db.remove_session_allowlist(sid, "example.com")
        .await
        .unwrap();
    let list = db.list_session_allowlist(sid).await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(!db
        .is_domain_session_allowed(sid, "example.com")
        .await
        .unwrap());
}

// ── Domain checks with empty tables ─────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_domain_blocked_and_global_allowed_empty() {
    let server = TestServer::start().await;
    let db = server.db();

    // With empty global_allowlist table, nothing should be blocked or globally allowed
    assert!(!db.is_domain_blocked("anything.com").await.unwrap());
    assert!(!db.is_domain_global_allowed("anything.com").await.unwrap());
}

// ── Pending approvals: create + resolve + get_approved ──────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_pending_approval_flow() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("approval-test").await.unwrap();

    // No approved pending initially
    let approved = db.get_approved_pending(sid, "example.com").await.unwrap();
    assert!(approved.is_empty());

    // Create a pending approval
    db.create_pending_approval(sid, "example.com", "https://example.com/page", "alice")
        .await
        .unwrap();

    // Not yet approved
    let approved = db.get_approved_pending(sid, "example.com").await.unwrap();
    assert!(approved.is_empty());

    // Resolve as approved
    db.resolve_approval(sid, "example.com", true).await.unwrap();

    let approved = db.get_approved_pending(sid, "example.com").await.unwrap();
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].0, "https://example.com/page");
    assert_eq!(approved[0].1, "alice");

    // Domain should now be in session allowlist (resolve_approval adds it)
    assert!(db
        .is_domain_session_allowed(sid, "example.com")
        .await
        .unwrap());
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_pending_approval_denied() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("denial-test").await.unwrap();

    db.create_pending_approval(sid, "bad.com", "https://bad.com/x", "bob")
        .await
        .unwrap();

    db.resolve_approval(sid, "bad.com", false).await.unwrap();

    // Should not be approved
    let approved = db.get_approved_pending(sid, "bad.com").await.unwrap();
    assert!(approved.is_empty());

    // Should not be in session allowlist
    assert!(!db.is_domain_session_allowed(sid, "bad.com").await.unwrap());
}

// ── Quotas: reset + get_usage + add_usage + global ──────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_quota_usage() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("quota-test").await.unwrap();

    // Reset tokens (idempotent for today)
    db.reset_tokens_if_needed(sid).await.unwrap();

    let (used, cap) = db.get_session_usage(sid).await.unwrap();
    assert_eq!(used, 0);
    assert!(cap > 0);

    db.add_usage(sid, 500).await.unwrap();
    let (used, _) = db.get_session_usage(sid).await.unwrap();
    assert_eq!(used, 500);

    db.add_usage(sid, 300).await.unwrap();
    let (used, _) = db.get_session_usage(sid).await.unwrap();
    assert_eq!(used, 800);

    // Global usage should include this session
    let global = db.get_global_usage().await.unwrap();
    assert!(global >= 800);
}

// ── Idle sessions: set + get + clear ────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_idle_sessions() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("idle-test").await.unwrap();

    // Initially no sessions are idle (even with 0 timeout, idle_since is NULL)
    let idle = db.get_idle_sessions(0).await.unwrap();
    assert!(!idle.contains(&sid));

    // Set idle
    db.set_idle_since_now(sid).await.unwrap();

    // With a very large timeout, it should not yet be considered idle
    let idle = db.get_idle_sessions(999_999).await.unwrap();
    assert!(!idle.contains(&sid));

    // Clear idle
    db.clear_idle_since_for(sid).await.unwrap();

    let idle = db.get_idle_sessions(0).await.unwrap();
    assert!(!idle.contains(&sid));
}

// ── Cascade delete ──────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_cascade_delete_session() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("cascade-test").await.unwrap();

    // Insert related data
    let eid = db
        .insert_event(sid, "a", "chat", serde_json::json!({"text": "hi"}))
        .await
        .unwrap();
    db.upsert_repo(sid, "myrepo", "https://github.com/x/y.git")
        .await
        .unwrap();
    let rid = db.insert_run(sid, "full").await.unwrap();
    db.add_session_allowlist(sid, "example.com").await.unwrap();

    // Verify data exists
    assert!(db.get_event_by_id(eid).await.unwrap().is_some());
    assert_eq!(db.list_repos(sid).await.unwrap().len(), 1);
    assert!(db.is_run_in_flight(sid).await.unwrap());
    assert!(db
        .is_domain_session_allowed(sid, "example.com")
        .await
        .unwrap());

    // Delete the session
    db.delete_session(sid).await.unwrap();

    // All related data should be gone
    assert!(db.get_event_by_id(eid).await.unwrap().is_none());
    assert!(db.list_repos(sid).await.unwrap().is_empty());

    // finish_run should not fail even if the run was cascade-deleted
    let _ = db.finish_run(rid, "cancelled").await;

    // Session allowlist should be gone
    assert!(!db
        .is_domain_session_allowed(sid, "example.com")
        .await
        .unwrap());
}

// ── Participant tracking via AppState ───────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_participant_tracking() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("participant-test").await.unwrap();

    // Build an AppState directly for testing participant methods
    let config = remora_server::state::Config {
        workspace_dir: std::path::PathBuf::from("/tmp/remora-test-participant"),
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

    // Initially no participants
    let parts = state.get_participants(sid).await;
    assert!(parts.is_empty(), "should start with no participants");

    // Join three participants
    state.try_participant_join(sid, "alice").await;
    state.try_participant_join(sid, "bob").await;
    state.try_participant_join(sid, "charlie").await;

    let parts = state.get_participants(sid).await;
    assert_eq!(parts.len(), 3);
    assert!(parts.contains(&"alice".to_string()));
    assert!(parts.contains(&"bob".to_string()));
    assert!(parts.contains(&"charlie".to_string()));

    // Leave one
    state.participant_leave(sid, "bob").await;

    let parts = state.get_participants(sid).await;
    assert_eq!(parts.len(), 2);
    assert!(!parts.contains(&"bob".to_string()));
    assert!(parts.contains(&"alice".to_string()));
    assert!(parts.contains(&"charlie".to_string()));

    // Leave all
    state.participant_leave(sid, "alice").await;
    state.participant_leave(sid, "charlie").await;

    let parts = state.get_participants(sid).await;
    assert!(parts.is_empty(), "all participants should be gone");
}

// ── Subscribe and dispatch ─────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_subscribe_and_dispatch() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("subscribe-test").await.unwrap();

    // Build an AppState and subscribe
    let config = remora_server::state::Config {
        workspace_dir: std::path::PathBuf::from("/tmp/remora-test-subscribe"),
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

    // Start the event notification listener
    let listener_state = std::sync::Arc::clone(&state);
    tokio::spawn(async move {
        let _ = remora_server::state::run_event_listener(listener_state).await;
    });

    // Give the listener time to subscribe to DB notifications
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let (mut rx, _cancel) = state.subscribe(sid, "tester").await;

    // Insert an event into the DB (this triggers the notification channel)
    let event_id = db
        .insert_event(sid, "tester", "chat", serde_json::json!({"text": "hello"}))
        .await
        .unwrap();

    // The subscriber should receive the dispatched event
    let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await;

    assert!(received.is_ok(), "should receive event within timeout");
    let event = received.unwrap().expect("channel should not be closed");
    assert_eq!(event.id, event_id);
    assert_eq!(event.kind, "chat");
    assert_eq!(event.payload["text"], "hello");
}

// ── get_session_info ────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn db_get_session_info() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("info-test").await.unwrap();

    let info = db.get_session_info(sid).await.unwrap();
    assert!(info.is_some());
    let (desc, _created, used, cap) = info.unwrap();
    assert_eq!(desc, "info-test");
    assert_eq!(used, 0);
    assert!(cap > 0);

    // Non-existent session
    let missing = db.get_session_info(uuid::Uuid::new_v4()).await.unwrap();
    assert!(missing.is_none());
}
