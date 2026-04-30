//! Context assembly integration tests for remora-server.
//!
//! These tests exercise `context::assemble_context` with real DB state.
//!
//! Set `DATABASE_URL` to run them.

mod common;

use common::TestServer;
use remora_server::context::{assemble_context, ContextMode};
use remora_server::db::Database;

// ── SinceLast with no boundary events returns all ───────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_since_last_no_boundary_returns_all() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("ctx-all").await.unwrap();

    db.insert_event(sid, "alice", "chat", serde_json::json!({"text": "hello"}))
        .await
        .unwrap();
    db.insert_event(sid, "bob", "chat", serde_json::json!({"text": "world"}))
        .await
        .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::SinceLast)
        .await
        .unwrap();

    assert!(ctx.contains("<untrusted_content source=\"chat\" author=\"alice\">"));
    assert!(ctx.contains("hello"));
    assert!(ctx.contains("<untrusted_content source=\"chat\" author=\"bob\">"));
    assert!(ctx.contains("world"));
}

// ── SinceLast after a claude_response returns only newer events ─────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_since_last_after_claude_response() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("ctx-since").await.unwrap();

    db.insert_event(
        sid,
        "alice",
        "chat",
        serde_json::json!({"text": "old message"}),
    )
    .await
    .unwrap();

    db.insert_event(
        sid,
        "claude",
        "claude_response",
        serde_json::json!({"text": "I did the thing"}),
    )
    .await
    .unwrap();

    db.insert_event(
        sid,
        "bob",
        "chat",
        serde_json::json!({"text": "new message"}),
    )
    .await
    .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::SinceLast)
        .await
        .unwrap();

    assert!(
        !ctx.contains("old message"),
        "old message should not appear after boundary"
    );
    assert!(
        !ctx.contains("[Claude]: I did the thing"),
        "the boundary event itself should not appear"
    );
    assert!(ctx.contains("<untrusted_content source=\"chat\" author=\"bob\">"));
    assert!(ctx.contains("new message"));
}

// ── Full mode returns all events ────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_full_returns_everything() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("ctx-full").await.unwrap();

    db.insert_event(sid, "alice", "chat", serde_json::json!({"text": "first"}))
        .await
        .unwrap();
    db.insert_event(
        sid,
        "claude",
        "claude_response",
        serde_json::json!({"text": "response"}),
    )
    .await
    .unwrap();
    db.insert_event(sid, "bob", "chat", serde_json::json!({"text": "second"}))
        .await
        .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::Full).await.unwrap();

    assert!(ctx.contains("<untrusted_content source=\"chat\" author=\"alice\">"));
    assert!(ctx.contains("first"));
    assert!(ctx.contains("[Claude]: response"));
    assert!(ctx.contains("<untrusted_content source=\"chat\" author=\"bob\">"));
    assert!(ctx.contains("second"));
}

// ── clear_marker acts as context boundary ───────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_clear_marker_boundary() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("ctx-clear").await.unwrap();

    db.insert_event(
        sid,
        "alice",
        "chat",
        serde_json::json!({"text": "before clear"}),
    )
    .await
    .unwrap();
    db.insert_event(
        sid,
        "alice",
        "clear_marker",
        serde_json::json!({"text": "cleared"}),
    )
    .await
    .unwrap();
    db.insert_event(
        sid,
        "bob",
        "chat",
        serde_json::json!({"text": "after clear"}),
    )
    .await
    .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::SinceLast)
        .await
        .unwrap();

    assert!(
        !ctx.contains("before clear"),
        "events before clear_marker should be excluded in SinceLast"
    );
    assert!(ctx.contains("<untrusted_content source=\"chat\" author=\"bob\">"));
    assert!(ctx.contains("after clear"));
}

// ── Event formatting: chat ──────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_format_chat() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("fmt-chat").await.unwrap();

    db.insert_event(
        sid,
        "carol",
        "chat",
        serde_json::json!({"text": "hi there"}),
    )
    .await
    .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::Full).await.unwrap();
    assert_eq!(
        ctx.trim(),
        "<untrusted_content source=\"chat\" author=\"carol\">\nhi there\n</untrusted_content>"
    );
}

// ── Event formatting: file uses untrusted_content tags ──────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_format_file() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("fmt-file").await.unwrap();

    db.insert_event(
        sid,
        "dave",
        "file",
        serde_json::json!({"path": "src/main.rs", "content": "fn main() {}"}),
    )
    .await
    .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::Full).await.unwrap();
    assert!(ctx.contains("<untrusted_content source=\"file\" path=\"src/main.rs\">"));
    assert!(ctx.contains("fn main() {}"));
    assert!(ctx.contains("</untrusted_content>"));
}

// ── Event formatting: fetch uses untrusted_content tags ─────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_format_fetch() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("fmt-fetch").await.unwrap();

    db.insert_event(
        sid,
        "eve",
        "fetch",
        serde_json::json!({"url": "https://example.com", "content": "page body"}),
    )
    .await
    .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::Full).await.unwrap();
    assert!(ctx.contains("<untrusted_content source=\"url\" url=\"https://example.com\">"));
    assert!(ctx.contains("page body"));
    assert!(ctx.contains("</untrusted_content>"));
}

// ── Event formatting: system event ──────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_format_system() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("fmt-system").await.unwrap();

    db.insert_event(
        sid,
        "system",
        "system",
        serde_json::json!({"text": "alice joined"}),
    )
    .await
    .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::Full).await.unwrap();
    assert!(ctx.contains("[system]: alice joined"));
}

// ── Event formatting: unknown kind produces empty string ────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn context_format_unknown_kind_is_empty() {
    let server = TestServer::start().await;
    let db = server.db();

    let (sid, _, _) = db.create_session("fmt-unknown").await.unwrap();

    db.insert_event(
        sid,
        "x",
        "totally_unknown_kind",
        serde_json::json!({"data": 123}),
    )
    .await
    .unwrap();

    let ctx = assemble_context(db, sid, ContextMode::Full).await.unwrap();
    assert!(
        ctx.trim().is_empty(),
        "unknown event kinds should produce no output"
    );
}
