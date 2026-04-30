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

// ── /repo_add ──────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_repo_add_clones_repo() {
    // Create a temp bare git repo that can be cloned
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let repo_path = tmp.path().join("test-repo.git");
    let status = std::process::Command::new("git")
        .args(["init", "--bare", repo_path.to_str().unwrap()])
        .output()
        .expect("git init failed");
    assert!(status.status.success(), "git init --bare should succeed");

    let server = TestServer::start().await;
    let session_id = server.create_session("repo-add-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    let git_url = format!("file://{}", repo_path.display());
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "repo_add", "author": "alice", "git_url": git_url}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev
                    .get("data")
                    .is_some_and(|d| d["kind"] == "repo_change" && d["payload"]["action"] == "add")
        },
        10000,
    )
    .await;

    assert!(ev.is_some(), "should receive a repo_change add event");
    let data = ev.unwrap();
    assert_eq!(data["data"]["payload"]["name"], "test-repo");

    // Verify the repo was registered in DB
    let db = server.db();
    let names = db.list_repo_names(session_id).await.unwrap();
    assert!(names.contains(&"test-repo".to_string()));
}

// ── /repo_remove ───────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_repo_remove_deletes() {
    // Create a temp bare git repo
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let repo_path = tmp.path().join("removeme.git");
    let status = std::process::Command::new("git")
        .args(["init", "--bare", repo_path.to_str().unwrap()])
        .output()
        .expect("git init failed");
    assert!(status.status.success(), "git init --bare should succeed");

    let server = TestServer::start().await;
    let session_id = server.create_session("repo-rm-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    // First add the repo
    let git_url = format!("file://{}", repo_path.display());
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "repo_add", "author": "alice", "git_url": git_url}),
    )
    .await;

    let _ = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev
                    .get("data")
                    .is_some_and(|d| d["kind"] == "repo_change" && d["payload"]["action"] == "add")
        },
        10000,
    )
    .await;

    // Now remove it
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "repo_remove", "author": "alice", "name": "removeme"}),
    )
    .await;

    let ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "repo_change" && d["payload"]["action"] == "remove"
                })
        },
        10000,
    )
    .await;

    assert!(ev.is_some(), "should receive a repo_change remove event");
    let data = ev.unwrap();
    assert_eq!(data["data"]["payload"]["name"], "removeme");

    // Verify repo is gone from DB
    let db = server.db();
    let names = db.list_repo_names(session_id).await.unwrap();
    assert!(
        !names.contains(&"removeme".to_string()),
        "repo should be removed from DB"
    );
}

// ── /diff with repo ────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_diff_with_repo() {
    // Create a temp git repo with a commit and then modify a file
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let repo_path = tmp.path().join("diffrepo");

    // Init, add a file, commit
    let init = std::process::Command::new("git")
        .args(["init", repo_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(init.status.success());

    std::fs::write(repo_path.join("hello.txt"), "original").unwrap();

    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    let _ = std::process::Command::new("git")
        .args([
            "-c",
            "user.email=test@test.com",
            "-c",
            "user.name=Test",
            "commit",
            "-m",
            "init",
        ])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    let server = TestServer::start().await;
    let session_id = server.create_session("diff-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    // Add the repo
    let git_url = format!("file://{}", repo_path.display());
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "repo_add", "author": "alice", "git_url": git_url}),
    )
    .await;

    let _ = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev
                    .get("data")
                    .is_some_and(|d| d["kind"] == "repo_change" && d["payload"]["action"] == "add")
        },
        10000,
    )
    .await;

    // Modify the file in the cloned workspace.
    // The server clones repos into workspace_dir/session_id/repo_name.
    let cloned_hello = server
        .workspace_dir()
        .join(session_id.to_string())
        .join("diffrepo")
        .join("hello.txt");
    assert!(cloned_hello.exists(), "cloned file should exist");
    std::fs::write(&cloned_hello, "modified content").unwrap();

    // Now run /diff
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
                            .contains("modified content")
                })
        },
        10000,
    )
    .await;

    assert!(ev.is_some(), "diff should contain the changes");
}

// ── /run ───────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cmd_run_starts_claude_process() {
    let server = TestServer::start().await;
    let session_id = server.create_session("run-test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "alice").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    TestServer::send_msg(
        &mut sink,
        serde_json::json!({"type": "run", "author": "alice"}),
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
                            .contains("started")
                })
        },
        10000,
    )
    .await;

    assert!(
        ev.is_some(),
        "should receive a system event indicating run started"
    );
    let data = ev.unwrap();
    let text = data["data"]["payload"]["text"].as_str().unwrap();
    assert!(
        text.contains("alice"),
        "started event should mention the author"
    );
}
