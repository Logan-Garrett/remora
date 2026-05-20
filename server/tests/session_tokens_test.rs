//! Integration tests for per-session invite tokens.
//!
//! These tests require a database. Set `DATABASE_URL` and `REMORA_DB_PROVIDER`
//! to run them. They are marked `#[ignore]` so `cargo test` skips them by default;
//! run with `cargo test -- --ignored --test-threads=1`.

mod common;

use common::{TestServer, TEST_TOKEN};

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn create_session_includes_invite_token() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "token test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["invite_token"].is_string(),
        "response should include invite_token"
    );
    assert!(!body["invite_token"].as_str().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn create_additional_session_token() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();
    let session_id = server.create_session("extra-token-test").await;
    let resp = client
        .post(format!(
            "{}/sessions/{}/tokens",
            server.base_url(),
            session_id
        ))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "label": "for-bob" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
    assert_eq!(body["label"], "for-bob");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn session_token_cannot_manage_sessions() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "restricted test" }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_token = body["invite_token"].as_str().unwrap().to_string();
    let session_id = body["id"].as_str().unwrap().to_string();

    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {session_token}"))
        .json(&serde_json::json!({ "description": "should fail" }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "session token should not create sessions"
    );

    let resp = client
        .get(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {session_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "session token should not list sessions");

    let resp = client
        .delete(format!("{}/sessions/{}", server.base_url(), session_id))
        .header("Authorization", format!("Bearer {session_token}"))
        .send()
        .await
        .unwrap();
    // Session tokens are valid but lack permission — expect 403 Forbidden
    assert_eq!(
        resp.status(),
        403,
        "session token should not delete sessions"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn ws_join_with_session_token_works() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "ws token test" }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_token = body["invite_token"].as_str().unwrap().to_string();
    let session_id: uuid::Uuid = body["id"].as_str().unwrap().parse().unwrap();

    let url = format!(
        "{}/sessions/{}?token={}&name=bob",
        server.ws_base_url(),
        session_id,
        session_token,
    );
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_ok(), "should connect with session token");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn ws_join_wrong_session_with_session_token_fails() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "session A" }))
        .send()
        .await
        .unwrap();
    let body_a: serde_json::Value = resp.json().await.unwrap();
    let token_a = body_a["invite_token"].as_str().unwrap().to_string();
    let session_b = server.create_session("session B").await;

    let url = format!(
        "{}/sessions/{}?token={}&name=intruder",
        server.ws_base_url(),
        session_b,
        token_a,
    );
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_err(), "should not connect to wrong session");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn ws_revoked_token_fails() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "revoke test" }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_token = body["invite_token"].as_str().unwrap().to_string();
    let session_id: uuid::Uuid = body["id"].as_str().unwrap().parse().unwrap();

    let resp = client
        .get(format!(
            "{}/sessions/{}/tokens",
            server.base_url(),
            session_id
        ))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let tokens: Vec<serde_json::Value> = resp.json().await.unwrap();
    let token_id = tokens[0]["id"].as_i64().unwrap();

    let resp = client
        .delete(format!(
            "{}/sessions/{}/tokens/{}",
            server.base_url(),
            session_id,
            token_id
        ))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    let url = format!(
        "{}/sessions/{}?token={}&name=revoked-user",
        server.ws_base_url(),
        session_id,
        session_token,
    );
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_err(), "should not connect with revoked token");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn admin_token_still_works_for_ws() {
    let server = TestServer::start().await;
    let session_id = server.create_session("admin ws test").await;
    let url = format!(
        "{}/sessions/{}?token={}&name=admin-user",
        server.ws_base_url(),
        session_id,
        TEST_TOKEN,
    );
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_ok(), "admin token should still work");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn session_token_cannot_create_tokens() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "no-create-token test" }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_token = body["invite_token"].as_str().unwrap().to_string();
    let session_id = body["id"].as_str().unwrap().to_string();

    let resp = client
        .post(format!(
            "{}/sessions/{}/tokens",
            server.base_url(),
            session_id
        ))
        .header("Authorization", format!("Bearer {session_token}"))
        .json(&serde_json::json!({ "label": "should-fail" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "session token should not create tokens");
}
