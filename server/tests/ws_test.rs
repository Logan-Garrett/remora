//! WebSocket integration tests for remora-server.
//!
//! These tests require a Postgres database. Set `DATABASE_URL` to run them.
//! They are marked `#[ignore]` so `cargo test` skips them by default;
//! run with `cargo test -- --ignored` or `cargo test -- --include-ignored`.

mod common;

use common::TestServer;

// ── Connect to a valid session ───────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn ws_connect_to_valid_session() {
    let server = TestServer::start().await;
    let session_id = server.create_session("ws test").await;

    let (_sink, mut stream) = server.connect_ws(session_id, "alice").await;

    // Should receive backfill/join events (the first events for a fresh session
    // will be the "alice joined" system event delivered via PG NOTIFY).
    // We just need to confirm we receive _something_ — no error frame.
    let event = TestServer::wait_for_event(&mut stream, 3000).await;
    assert!(
        event.is_some(),
        "should receive at least one event after connecting"
    );

    let ev = event.unwrap();
    assert_eq!(ev["type"], "event", "message type should be 'event'");
}

// ── Connect to a nonexistent session ─────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn ws_connect_to_nonexistent_session_returns_error() {
    let server = TestServer::start().await;
    let fake_id = uuid::Uuid::new_v4();

    let (_sink, mut stream) = server.connect_ws(fake_id, "bob").await;

    let event = TestServer::wait_for_event(&mut stream, 3000).await;
    assert!(event.is_some(), "should receive an error message");

    let ev = event.unwrap();
    assert_eq!(ev["type"], "error");
    assert!(
        ev["message"].as_str().unwrap_or("").contains("not found"),
        "error should mention 'not found'"
    );
}

// ── Send a chat message and receive it ───────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn ws_send_chat_and_receive() {
    let server = TestServer::start().await;
    let session_id = server.create_session("chat test").await;

    let (mut sink, mut stream) = server.connect_ws(session_id, "carol").await;

    // Drain any initial join events
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    // Send a chat message
    TestServer::send_chat(&mut sink, "carol", "hello world").await;

    // Wait for the chat event to be echoed back
    let mut found_chat = false;
    for _ in 0..10 {
        if let Some(ev) = TestServer::wait_for_event(&mut stream, 2000).await {
            if ev["type"] == "event" {
                if let Some(data) = ev.get("data") {
                    if data["kind"] == "chat" {
                        assert_eq!(data["payload"]["text"], "hello world");
                        assert_eq!(data["author"], "carol");
                        found_chat = true;
                        break;
                    }
                }
            }
        }
    }
    assert!(found_chat, "should have received the chat event back");
}

// ── Fan-out: two clients see each other's messages ───────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn ws_fanout_two_clients() {
    let server = TestServer::start().await;
    let session_id = server.create_session("fanout test").await;

    let (mut sink_a, mut stream_a) = server.connect_ws(session_id, "alice").await;
    let (_sink_b, mut stream_b) = server.connect_ws(session_id, "bob").await;

    // Drain join events from both
    let _ = TestServer::drain_events(&mut stream_a, 1500).await;
    let _ = TestServer::drain_events(&mut stream_b, 1500).await;

    // Alice sends a chat
    TestServer::send_chat(&mut sink_a, "alice", "hi from alice").await;

    // Bob should see it
    let mut bob_saw_alice = false;
    for _ in 0..10 {
        if let Some(ev) = TestServer::wait_for_event(&mut stream_b, 2000).await {
            if ev["type"] == "event" {
                if let Some(data) = ev.get("data") {
                    if data["kind"] == "chat" && data["payload"]["text"] == "hi from alice" {
                        bob_saw_alice = true;
                        break;
                    }
                }
            }
        }
    }
    assert!(bob_saw_alice, "bob should see alice's chat message");
}

// ── Backfill: reconnecting client sees prior events ──────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn ws_backfill_on_reconnect() {
    let server = TestServer::start().await;
    let session_id = server.create_session("backfill test").await;

    // Connect, send a chat, disconnect
    {
        let (mut sink, mut stream) = server.connect_ws(session_id, "dave").await;
        let _ = TestServer::drain_events(&mut stream, 1500).await;

        TestServer::send_chat(&mut sink, "dave", "message before reconnect").await;

        // Give the server time to process and persist the event
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Wait until we see the chat come back (confirming it was persisted)
        let mut persisted = false;
        for _ in 0..20 {
            if let Some(ev) = TestServer::wait_for_event(&mut stream, 1000).await {
                if ev["type"] == "event" {
                    if let Some(data) = ev.get("data") {
                        if data["kind"] == "chat"
                            && data["payload"]["text"] == "message before reconnect"
                        {
                            persisted = true;
                            break;
                        }
                    }
                }
            }
        }
        assert!(persisted, "chat should have been persisted");

        // Drop sink/stream to disconnect
        drop(sink);
        drop(stream);
    }

    // Small delay to let server process the disconnect
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Reconnect as a new client
    let (_sink2, mut stream2) = server.connect_ws(session_id, "eve").await;

    // The backfill should include the "message before reconnect" chat
    let events = TestServer::drain_events(&mut stream2, 3000).await;
    let has_original_msg = events.iter().any(|ev| {
        ev["type"] == "event"
            && ev.get("data").is_some()
            && ev["data"]["kind"] == "chat"
            && ev["data"]["payload"]["text"] == "message before reconnect"
    });
    assert!(
        has_original_msg,
        "reconnecting client should receive the original message via backfill"
    );
}

// ── Auth: WS connection without token is rejected ────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn ws_connection_without_token_rejected() {
    let server = TestServer::start().await;
    let session_id = server.create_session("auth test").await;

    // Try connecting with a bad token
    let url = format!(
        "{}/sessions/{}?token=wrong-token&name=intruder",
        server.ws_base_url(),
        session_id,
    );

    // The server returns 401 before the WS upgrade, so the handshake should fail.
    let result = tokio_tungstenite::connect_async(&url).await;

    // Depending on the server response, this will either be an HTTP error
    // during the handshake or we'll get an immediate close frame.
    match result {
        Err(_) => {
            // Connection refused or HTTP 401 during upgrade — expected
        }
        Ok((ws, _)) => {
            // If somehow the connection was established, we should get
            // an error frame or the stream should close immediately.
            use futures_util::StreamExt;
            let (_, mut stream) = ws.split();
            let msg =
                tokio::time::timeout(std::time::Duration::from_millis(2000), stream.next()).await;

            // If we got a message, it should be a close or error
            if let Ok(Some(Ok(frame))) = msg {
                match frame {
                    tokio_tungstenite::tungstenite::Message::Close(_) => { /* ok */ }
                    _ => {
                        // The server should not send real data to an unauthorized client
                        panic!("unauthorized client should not receive data");
                    }
                }
            }
        }
    }
}
