mod common;

/// Health endpoint returns 200 with db status when server is running.
#[tokio::test]
#[ignore]
async fn health_returns_ok() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/health", server.base_url()))
        .send()
        .await
        .expect("health request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["db"], "connected");
}

/// Health endpoint requires no authentication.
#[tokio::test]
#[ignore]
async fn health_requires_no_auth() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();

    // No Authorization header
    let resp = client
        .get(format!("{}/health", server.base_url()))
        .send()
        .await
        .expect("health request failed");

    // Should still succeed (no auth required)
    assert_eq!(resp.status(), 200);
}

/// Migrations apply cleanly on a fresh database (TestServer::start runs them).
#[tokio::test]
#[ignore]
async fn migrations_apply_cleanly() {
    // TestServer::start() runs migrations — if this doesn't panic, migrations are good.
    let server = common::TestServer::start().await;

    // Verify we can actually use the database after migrations
    let session_id = server.create_session("migration-test").await;
    assert!(!session_id.is_nil());
}

/// Server can create, list, and delete sessions after fresh startup.
#[tokio::test]
#[ignore]
async fn full_lifecycle_smoke_test() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();
    let token = common::TEST_TOKEN;

    // Health check
    let resp = client
        .get(format!("{}/health", server.base_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Create session
    let session_id = server.create_session("smoke-test").await;

    // List sessions — should contain our session
    let resp = client
        .get(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let sessions: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(sessions.iter().any(|s| s["id"] == session_id.to_string()));

    // WebSocket connect + send + receive
    let (mut sink, mut stream) = server.connect_ws(session_id, "smoker").await;
    common::TestServer::send_chat(&mut sink, "smoker", "hello").await;
    let event = common::TestServer::wait_for_event(&mut stream, 2000).await;
    assert!(event.is_some(), "should receive the chat event back");

    // Delete session
    let resp = client
        .delete(format!("{}/sessions/{}", server.base_url(), session_id))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}
