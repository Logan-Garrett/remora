//! REST API integration tests for remora-server.
//!
//! These tests require a Postgres database. Set `DATABASE_URL` to run them.
//! They are marked `#[ignore]` so `cargo test` skips them by default;
//! run with `cargo test -- --ignored` or `cargo test -- --include-ignored`.

mod common;

use common::{TestServer, TEST_TOKEN};

// ── POST /sessions ───────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn create_session_returns_valid_json() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "test session" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["id"].is_string(), "response should contain id");
    assert_eq!(body["description"], "test session");
    assert!(
        body["created_at"].is_string(),
        "response should contain created_at"
    );

    // The id should be a valid UUID
    let id_str = body["id"].as_str().unwrap();
    let _: uuid::Uuid = id_str.parse().expect("id should be a valid UUID");
}

// ── GET /sessions ────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn list_sessions_returns_array() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Create two sessions
    server.create_session("session alpha").await;
    server.create_session("session beta").await;

    let resp = client
        .get(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(body.len() >= 2, "should have at least 2 sessions");

    let descriptions: Vec<&str> = body
        .iter()
        .filter_map(|s| s["description"].as_str())
        .collect();
    assert!(descriptions.contains(&"session alpha"));
    assert!(descriptions.contains(&"session beta"));
}

// ── DELETE /sessions/:id ─────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn delete_session_removes_it() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let session_id = server.create_session("to be deleted").await;

    // Delete it
    let resp = client
        .delete(format!("{}/sessions/{}", server.base_url(), session_id))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // List should no longer contain it
    let resp = client
        .get(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .send()
        .await
        .unwrap();
    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    let ids: Vec<&str> = body.iter().filter_map(|s| s["id"].as_str()).collect();
    assert!(
        !ids.contains(&session_id.to_string().as_str()),
        "deleted session should not appear in list"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn delete_nonexistent_session_returns_404() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let fake_id = uuid::Uuid::new_v4();
    let resp = client
        .delete(format!("{}/sessions/{}", server.base_url(), fake_id))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

// ── Auth tests ───────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn request_without_token_returns_401() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // POST without auth header
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .json(&serde_json::json!({ "description": "no auth" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // GET without auth header
    let resp = client
        .get(format!("{}/sessions", server.base_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn request_with_wrong_token_returns_401() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", "Bearer wrong-token-value")
        .json(&serde_json::json!({ "description": "bad auth" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    let resp = client
        .get(format!("{}/sessions", server.base_url()))
        .header("Authorization", "Bearer wrong-token-value")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}
