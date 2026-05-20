//! Integration tests for RBAC enforcement on WebSocket commands.
//!
//! These tests verify that role-based access control is properly enforced
//! when users send commands over WebSocket:
//! - Viewer: can chat, CANNOT /run
//! - Guest (session token): can chat, CANNOT /run or /session_info
//! - Member: can use /run
//! - Trust/untrust/kick: owner-only (not other members)
//!
//! Requires DATABASE_URL. Marked #[ignore].

mod common;

use common::{TestServer, TEST_TOKEN};

/// Helper: register a user, login, return (access_token, user_id).
async fn register_and_login(base_url: &str, email: &str, display_name: &str) -> (String, String) {
    let client = reqwest::Client::new();
    client
        .post(format!("{base_url}/auth/register"))
        .json(&serde_json::json!({
            "email": email,
            "display_name": display_name,
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{base_url}/auth/login"))
        .json(&serde_json::json!({
            "email": email,
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let token = body["access_token"].as_str().unwrap().to_string();
    let user_id = body["user"]["id"].as_str().unwrap().to_string();
    (token, user_id)
}

/// Helper: set a user's role via the admin endpoint.
async fn set_user_role(base_url: &str, user_id: &str, role: &str) {
    let client = reqwest::Client::new();
    let resp = client
        .put(format!("{base_url}/admin/users/{user_id}/role"))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "role": role }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        204,
        "setting user role to '{}' should succeed, got {}",
        role,
        resp.status()
    );
}

/// Helper: connect to a WS session using a JWT token.
async fn connect_ws_with_jwt(
    server: &TestServer,
    session_id: uuid::Uuid,
    jwt: &str,
) -> (
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::Message,
    >,
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) {
    use futures_util::StreamExt;
    let url = format!(
        "{}/sessions/{}?token={}",
        server.ws_base_url(),
        session_id,
        jwt,
    );
    let (ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws connect with JWT failed");
    ws.split()
}

/// Helper: connect to a WS session using a session invite token.
async fn connect_ws_with_session_token(
    server: &TestServer,
    session_id: uuid::Uuid,
    session_token: &str,
    name: &str,
) -> (
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::Message,
    >,
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) {
    use futures_util::StreamExt;
    let url = format!(
        "{}/sessions/{}?token={}&name={}",
        server.ws_base_url(),
        session_id,
        session_token,
        name,
    );
    let (ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws connect with session token failed");
    ws.split()
}

// ── Viewer: can chat but CANNOT /run ─────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn viewer_can_chat_but_cannot_run() {
    let server = TestServer::start().await;

    // Register a user, then set their role to viewer via admin API
    let (jwt, user_id) =
        register_and_login(&server.base_url(), "viewer-rbac@example.com", "ViewerUser").await;
    set_user_role(&server.base_url(), &user_id, "viewer").await;

    // Re-login to get a JWT with the updated role
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "viewer-rbac@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let jwt = body["access_token"].as_str().unwrap().to_string();

    let session_id = server.create_session("viewer-rbac-test").await;

    let (mut sink, mut stream) = connect_ws_with_jwt(&server, session_id, &jwt).await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    // Viewer sends a chat — should succeed
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({
            "type": "chat",
            "author": "ViewerUser",
            "text": "hello from viewer"
        }),
    )
    .await;

    let chat_ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "chat" && d["payload"]["text"] == "hello from viewer"
                })
        },
        5000,
    )
    .await;
    assert!(
        chat_ev.is_some(),
        "viewer should be able to send chat messages"
    );

    // Viewer tries /run — should get RBAC rejection
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({
            "type": "run",
            "author": "ViewerUser"
        }),
    )
    .await;

    let rbac_err = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("insufficient permissions")
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("viewer")
                })
        },
        5000,
    )
    .await;
    assert!(
        rbac_err.is_some(),
        "viewer should receive RBAC rejection for /run"
    );
}

// ── Guest (session token): can chat but CANNOT /run or /session_info ─

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn guest_can_chat_but_cannot_run_or_info() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Create a session and get the invite token
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "guest-rbac-test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_id: uuid::Uuid = body["id"].as_str().unwrap().parse().unwrap();
    let session_token = body["invite_token"].as_str().unwrap().to_string();

    // Connect as guest via session token
    let (mut sink, mut stream) =
        connect_ws_with_session_token(&server, session_id, &session_token, "GuestUser").await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    // Guest sends a chat — should succeed
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({
            "type": "chat",
            "author": "GuestUser",
            "text": "hello from guest"
        }),
    )
    .await;

    let chat_ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "chat" && d["payload"]["text"] == "hello from guest"
                })
        },
        5000,
    )
    .await;
    assert!(
        chat_ev.is_some(),
        "guest should be able to send chat messages"
    );

    // Guest tries /run — should get RBAC rejection
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({
            "type": "run",
            "author": "GuestUser"
        }),
    )
    .await;

    let rbac_run = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("insufficient permissions")
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("guest")
                })
        },
        5000,
    )
    .await;
    assert!(
        rbac_run.is_some(),
        "guest should receive RBAC rejection for /run"
    );

    // Guest tries /session_info — should get RBAC rejection (guest < viewer)
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({
            "type": "session_info",
            "author": "GuestUser"
        }),
    )
    .await;

    let rbac_info = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("insufficient permissions")
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("/info")
                })
        },
        5000,
    )
    .await;
    assert!(
        rbac_info.is_some(),
        "guest should receive RBAC rejection for /session_info"
    );
}

// ── Member: can use /run ─────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn member_can_use_run() {
    let server = TestServer::start().await;

    // Register a user (first user gets admin, so register a dummy first)
    let _ = register_and_login(
        &server.base_url(),
        "dummy-member-first@example.com",
        "Dummy",
    )
    .await;

    let (jwt, user_id) =
        register_and_login(&server.base_url(), "member-rbac@example.com", "MemberUser").await;

    // Ensure user is a member (should be default for second user)
    set_user_role(&server.base_url(), &user_id, "member").await;

    // Re-login to get fresh JWT
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "member-rbac@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let jwt = body["access_token"].as_str().unwrap().to_string();

    let session_id = server.create_session("member-rbac-test").await;

    let (mut sink, mut stream) = connect_ws_with_jwt(&server, session_id, &jwt).await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    // Member sends /run — should succeed (the server spawns claude run with REMORA_CLAUDE_CMD=echo)
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({
            "type": "run",
            "author": "MemberUser"
        }),
    )
    .await;

    // Should see a system event about starting a Claude run, NOT an RBAC rejection
    let run_ev = TestServer::wait_for_event_matching(
        &mut stream,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("started a Claude run")
                })
        },
        5000,
    )
    .await;
    assert!(
        run_ev.is_some(),
        "member should be allowed to use /run and see 'started a Claude run'"
    );
}

// ── Trust/untrust/kick: only work for session owner ──────────────────

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn trust_untrust_kick_owner_only() {
    let server = TestServer::start().await;

    // Register two users as members
    let _ = register_and_login(
        &server.base_url(),
        "dummy-owner-first@example.com",
        "DummyOwner",
    )
    .await;

    let (jwt_alice, alice_id) =
        register_and_login(&server.base_url(), "alice-owner@example.com", "AliceOwner").await;
    set_user_role(&server.base_url(), &alice_id, "member").await;

    let (jwt_bob, bob_id) = register_and_login(
        &server.base_url(),
        "bob-nonowner@example.com",
        "BobNonOwner",
    )
    .await;
    set_user_role(&server.base_url(), &bob_id, "member").await;

    // Re-login to get JWTs with current roles
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "alice-owner@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let jwt_alice = body["access_token"].as_str().unwrap().to_string();

    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "bob-nonowner@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let jwt_bob = body["access_token"].as_str().unwrap().to_string();

    // Create a session with owner_key, Alice connects with it to become owner
    let (session_id, owner_key) = server.create_session_with_key("owner-only-test").await;

    // Alice connects first with owner_key -> becomes owner
    let url_alice = format!(
        "{}/sessions/{}?token={}&owner_key={}",
        server.ws_base_url(),
        session_id,
        jwt_alice,
        owner_key,
    );
    let (ws_alice, _) = tokio_tungstenite::connect_async(&url_alice)
        .await
        .expect("alice ws connect failed");
    let (mut sink_alice, mut stream_alice) = futures_util::StreamExt::split(ws_alice);
    let _ = TestServer::drain_events(&mut stream_alice, 1500).await;

    // Bob connects without owner_key -> not owner
    let (mut sink_bob, mut stream_bob) = connect_ws_with_jwt(&server, session_id, &jwt_bob).await;
    let _ = TestServer::drain_events(&mut stream_bob, 1500).await;

    // Bob (non-owner member) tries /trust carol -> should get owner rejection
    TestServer::send_msg(
        &mut sink_bob,
        serde_json::json!({
            "type": "trust",
            "author": "BobNonOwner",
            "target": "carol"
        }),
    )
    .await;

    let trust_rejected = TestServer::wait_for_event_matching(
        &mut stream_bob,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("session owner")
                })
        },
        5000,
    )
    .await;
    assert!(
        trust_rejected.is_some(),
        "non-owner member should be rejected for /trust"
    );

    // Bob tries /untrust carol -> should also be rejected
    TestServer::send_msg(
        &mut sink_bob,
        serde_json::json!({
            "type": "untrust",
            "author": "BobNonOwner",
            "target": "carol"
        }),
    )
    .await;

    let untrust_rejected = TestServer::wait_for_event_matching(
        &mut stream_bob,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("session owner")
                })
        },
        5000,
    )
    .await;
    assert!(
        untrust_rejected.is_some(),
        "non-owner member should be rejected for /untrust"
    );

    // Bob tries /kick AliceOwner -> should be rejected (not owner, not admin)
    TestServer::send_msg(
        &mut sink_bob,
        serde_json::json!({
            "type": "kick",
            "author": "BobNonOwner",
            "target": "AliceOwner"
        }),
    )
    .await;

    let kick_rejected = TestServer::wait_for_event_matching(
        &mut stream_bob,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("session owner")
                })
        },
        5000,
    )
    .await;
    assert!(
        kick_rejected.is_some(),
        "non-owner member should be rejected for /kick"
    );

    // Alice (owner) trusts carol -> should succeed
    TestServer::send_msg(
        &mut sink_alice,
        serde_json::json!({
            "type": "trust",
            "author": "AliceOwner",
            "target": "carol"
        }),
    )
    .await;

    let trust_ok = TestServer::wait_for_event_matching(
        &mut stream_alice,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("now trusted")
                })
        },
        5000,
    )
    .await;
    assert!(
        trust_ok.is_some(),
        "owner should successfully /trust a participant"
    );

    // Alice (owner) untrusts carol -> should succeed
    TestServer::send_msg(
        &mut sink_alice,
        serde_json::json!({
            "type": "untrust",
            "author": "AliceOwner",
            "target": "carol"
        }),
    )
    .await;

    let untrust_ok = TestServer::wait_for_event_matching(
        &mut stream_alice,
        |ev| {
            ev["type"] == "event"
                && ev.get("data").is_some_and(|d| {
                    d["kind"] == "system"
                        && d["payload"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .contains("removed from the trusted list")
                })
        },
        5000,
    )
    .await;
    assert!(
        untrust_ok.is_some(),
        "owner should successfully /untrust a participant"
    );
}

// ── Viewer can use /session_info but guest cannot ────────────────────

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn viewer_can_use_session_info() {
    let server = TestServer::start().await;

    let (_, user_id) =
        register_and_login(&server.base_url(), "viewer-info@example.com", "ViewerInfo").await;
    set_user_role(&server.base_url(), &user_id, "viewer").await;

    // Re-login to get JWT with viewer role
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "viewer-info@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let jwt = body["access_token"].as_str().unwrap().to_string();

    let session_id = server.create_session("viewer-info-test").await;

    let (mut sink, mut stream) = connect_ws_with_jwt(&server, session_id, &jwt).await;
    let _ = TestServer::drain_events(&mut stream, 1500).await;

    // Viewer sends /session_info — should succeed (viewer has read access)
    TestServer::send_msg(
        &mut sink,
        serde_json::json!({
            "type": "session_info",
            "author": "ViewerInfo"
        }),
    )
    .await;

    let info_ev = TestServer::wait_for_event_matching(
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
    assert!(
        info_ev.is_some(),
        "viewer should be able to use /session_info"
    );
}
