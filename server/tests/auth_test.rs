//! Integration tests for user auth: register, login, JWT validation, refresh, expired tokens.
//!
//! Requires DATABASE_URL. Marked #[ignore] so they run only when explicitly requested.

mod common;

use common::{TestServer, TEST_TOKEN};

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn register_and_login() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Register
    let resp = client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&serde_json::json!({
            "email": "alice@example.com",
            "display_name": "Alice",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "register should succeed");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["email"], "alice@example.com");
    assert_eq!(body["display_name"], "Alice");
    assert_eq!(body["role"], "member");

    // Login
    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "alice@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "login should succeed");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["access_token"].is_string());
    assert!(body["refresh_token"].is_string());
    assert_eq!(body["user"]["email"], "alice@example.com");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn register_duplicate_email_fails() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "email": "dup@example.com",
        "display_name": "Dup",
        "password": "securepass123"
    });

    let resp = client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "duplicate email should fail");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn register_short_password_fails() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&serde_json::json!({
            "email": "short@example.com",
            "display_name": "Short",
            "password": "abc"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "short password should be rejected");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn login_wrong_password_fails() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Register
    client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&serde_json::json!({
            "email": "wrongpw@example.com",
            "display_name": "WrongPW",
            "password": "correctpass123"
        }))
        .send()
        .await
        .unwrap();

    // Login with wrong password
    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "wrongpw@example.com",
            "password": "wrongpass123"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn login_nonexistent_user_fails() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "nobody@example.com",
            "password": "whatever123"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn jwt_me_endpoint() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Register and login
    client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&serde_json::json!({
            "email": "me@example.com",
            "display_name": "MeUser",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "me@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let login_body: serde_json::Value = resp.json().await.unwrap();
    let access_token = login_body["access_token"].as_str().unwrap();

    // GET /auth/me with JWT
    let resp = client
        .get(format!("{}/auth/me", server.base_url()))
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["email"], "me@example.com");
    assert_eq!(body["display_name"], "MeUser");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn me_without_jwt_fails() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/auth/me", server.base_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn me_with_invalid_jwt_fails() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/auth/me", server.base_url()))
        .header("Authorization", "Bearer eyJinvalid.token.here")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn refresh_token_flow() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Register + login
    client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&serde_json::json!({
            "email": "refresh@example.com",
            "display_name": "Refresh",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "refresh@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let login_body: serde_json::Value = resp.json().await.unwrap();
    let refresh_token = login_body["refresh_token"].as_str().unwrap();

    // Use refresh token to get new access token
    let resp = client
        .post(format!("{}/auth/refresh", server.base_url()))
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["access_token"].is_string());

    // Old refresh token should be consumed (token rotation)
    let resp = client
        .post(format!("{}/auth/refresh", server.base_url()))
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "used refresh token should be rejected");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn invalid_refresh_token_fails() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/auth/refresh", server.base_url()))
        .json(&serde_json::json!({ "refresh_token": "completely-bogus-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires DATABASE_URL"]
async fn ws_connect_with_jwt() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Register + login
    client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&serde_json::json!({
            "email": "wsjwt@example.com",
            "display_name": "JwtWsUser",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "wsjwt@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let login_body: serde_json::Value = resp.json().await.unwrap();
    let access_token = login_body["access_token"].as_str().unwrap();

    // Create a session (with admin token)
    let session_id = server.create_session("jwt ws test").await;

    // Connect via WS with JWT (name comes from JWT, not query param)
    let url = format!(
        "{}/sessions/{}?token={}",
        server.ws_base_url(),
        session_id,
        access_token,
    );
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_ok(), "should connect with JWT");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn admin_token_still_works_for_sessions() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Admin token can still create sessions
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "admin still works" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
}
