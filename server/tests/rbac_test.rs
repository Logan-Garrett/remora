//! Integration tests for role-based access control.
//!
//! Tests that the role hierarchy is enforced and that different token types
//! get the appropriate access levels.

mod common;

use common::TEST_TOKEN;

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

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn jwt_user_can_access_me() {
    let server = common::TestServer::start().await;
    let (jwt, _) = register_and_login(&server.base_url(), "rbac-me@example.com", "RbacUser").await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/auth/me", server.base_url()))
        .header("Authorization", format!("Bearer {jwt}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn jwt_member_and_admin_can_create_sessions() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();

    let (jwt, _) =
        register_and_login(&server.base_url(), "rbac-sess@example.com", "SessUser").await;

    // JWT user (member role) can create sessions
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {jwt}"))
        .json(&serde_json::json!({ "description": "member creates" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Admin token also works
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "admin creates" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn session_token_cannot_access_auth_endpoints() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();

    // Create a session with admin token
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .header("Authorization", format!("Bearer {TEST_TOKEN}"))
        .json(&serde_json::json!({ "description": "rbac session" }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_token = body["invite_token"].as_str().unwrap();

    // Session token should not be able to access /auth/me
    let resp = client
        .get(format!("{}/auth/me", server.base_url()))
        .header("Authorization", format!("Bearer {session_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn role_hierarchy_unit() {
    // Unit test for role_level (doesn't need DB)
    use remora_server::auth::role_level;
    assert!(role_level("admin") > role_level("member"));
    assert!(role_level("member") > role_level("viewer"));
    assert!(role_level("viewer") > role_level("guest"));
    assert_eq!(role_level("unknown"), 0);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn jwt_token_works_for_ws_connect() {
    let server = common::TestServer::start().await;
    let (jwt, _) =
        register_and_login(&server.base_url(), "rbac-ws@example.com", "WsRbacUser").await;

    let session_id = server.create_session("rbac ws test").await;

    let url = format!(
        "{}/sessions/{}?token={}",
        server.ws_base_url(),
        session_id,
        jwt,
    );
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_ok(), "JWT should work for WS connection");
}
