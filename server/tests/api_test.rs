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

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn delete_session_removes_it() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let session_id = server.create_session("to be deleted").await;

    // Delete it (use the same server instance to avoid race with other tests)
    let resp = client
        .delete(format!("{}/sessions/{}", server.base_url(), session_id))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    assert!(
        status == 204 || status == 200,
        "expected 204 or 200, got {status}"
    );

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

// ── C3: Session limit (429) ──────────────────────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn create_session_returns_429_when_limit_reached() {
    // TestServer is configured with max_sessions=100 by default.
    // We need a server with a very small limit to test this efficiently.
    // Since TestServer doesn't expose max_sessions config directly,
    // we set REMORA_MAX_SESSIONS env var and use a custom server setup.
    // However, TestServer::start() uses Config { max_sessions: 100, .. },
    // so we'll create 100 sessions and test the 101st.
    //
    // That's too slow. Instead, we test the behavior with the default
    // TestServer by checking that the endpoint *would* return 429.
    // We'll directly call the DB to create sessions up to the limit,
    // then try via REST.

    use remora_server::db::Database;

    let server = TestServer::start().await;
    let client = reqwest::Client::new();
    let db = server.db();

    // TestServer config has max_sessions = 100.
    // The clean-slate in TestServer::start() already deleted all sessions.
    // Create 100 sessions directly via DB to hit the limit.
    for i in 0..100 {
        db.create_session(&format!("limit-test-{i}")).await.unwrap();
    }

    // The 101st session via REST should be rejected with 429
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "one too many" }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        429,
        "should return 429 when session limit is reached"
    );
}

// ── C4: Create session with invalid git URL ─────────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn create_session_rejects_unsafe_git_url() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({
            "description": "ssrf test",
            "repos": ["file:///etc/passwd"]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        400,
        "file:// git URL should be rejected with 400"
    );

    let body = resp.text().await.unwrap();
    assert!(
        body.contains("rejected git URL"),
        "error body should mention rejected git URL, got: {body}"
    );
}

// ── C9: GET /sessions does not leak owner_key ───────────────────────

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn list_sessions_does_not_leak_owner_key() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Create a session — the create response includes owner_key
    let create_resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "leak test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 201);
    let create_body: serde_json::Value = create_resp.json().await.unwrap();
    assert!(
        create_body["owner_key"].is_string(),
        "create response should include owner_key"
    );

    // List sessions — owner_key must NOT appear
    let list_resp = client
        .get(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(list_resp.status(), 200);

    let list_body: Vec<serde_json::Value> = list_resp.json().await.unwrap();

    for session in &list_body {
        assert!(
            session.get("owner_key").is_none() || session["owner_key"].is_null(),
            "list sessions response must not include owner_key, got: {session}"
        );
    }
}

// ── Auth tests ───────────────────────────────────────────────────────

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
