//! Integration tests for API keys: create, validate, revoke, list.

mod common;

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn create_and_list_api_keys() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();

    // Register + login
    client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&serde_json::json!({
            "email": "apikey@example.com",
            "display_name": "ApiKeyUser",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "apikey@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let login_body: serde_json::Value = resp.json().await.unwrap();
    let jwt = login_body["access_token"].as_str().unwrap();

    // Create an API key
    let resp = client
        .post(format!("{}/auth/api-keys", server.base_url()))
        .header("Authorization", format!("Bearer {jwt}"))
        .json(&serde_json::json!({ "label": "test-key" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["key"].as_str().unwrap().starts_with("rmk_"));
    assert_eq!(body["label"], "test-key");
    let key_id = body["id"].as_str().unwrap().to_string();

    // List API keys
    let resp = client
        .get(format!("{}/auth/api-keys", server.base_url()))
        .header("Authorization", format!("Bearer {jwt}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let keys: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(!keys.is_empty());
    assert_eq!(keys[0]["label"], "test-key");
    assert_eq!(keys[0]["revoked"], false);

    // Revoke the API key
    let resp = client
        .delete(format!("{}/auth/api-keys/{}", server.base_url(), key_id))
        .header("Authorization", format!("Bearer {jwt}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // List again -- should be revoked
    let resp = client
        .get(format!("{}/auth/api-keys", server.base_url()))
        .header("Authorization", format!("Bearer {jwt}"))
        .send()
        .await
        .unwrap();
    let keys: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(keys[0]["revoked"], true);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn api_key_authenticates_ws() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();

    // Register + login
    client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&serde_json::json!({
            "email": "apikeyws@example.com",
            "display_name": "ApiKeyWsUser",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "apikeyws@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let login_body: serde_json::Value = resp.json().await.unwrap();
    let jwt = login_body["access_token"].as_str().unwrap();

    // Create API key
    let resp = client
        .post(format!("{}/auth/api-keys", server.base_url()))
        .header("Authorization", format!("Bearer {jwt}"))
        .json(&serde_json::json!({ "label": "ws-key" }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let api_key = body["key"].as_str().unwrap();

    // Create a session
    let session_id = server.create_session("api key ws test").await;

    // Connect via WS with API key
    let url = format!(
        "{}/sessions/{}?token={}",
        server.ws_base_url(),
        session_id,
        api_key,
    );
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_ok(), "should connect with API key");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn revoked_api_key_cannot_authenticate() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();

    // Register + login
    client
        .post(format!("{}/auth/register", server.base_url()))
        .json(&serde_json::json!({
            "email": "revokedkey@example.com",
            "display_name": "RevokedKeyUser",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/auth/login", server.base_url()))
        .json(&serde_json::json!({
            "email": "revokedkey@example.com",
            "password": "securepass123"
        }))
        .send()
        .await
        .unwrap();
    let login_body: serde_json::Value = resp.json().await.unwrap();
    let jwt = login_body["access_token"].as_str().unwrap();

    // Create and revoke API key
    let resp = client
        .post(format!("{}/auth/api-keys", server.base_url()))
        .header("Authorization", format!("Bearer {jwt}"))
        .json(&serde_json::json!({ "label": "revoke-me" }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let api_key = body["key"].as_str().unwrap().to_string();
    let key_id = body["id"].as_str().unwrap();

    client
        .delete(format!("{}/auth/api-keys/{}", server.base_url(), key_id))
        .header("Authorization", format!("Bearer {jwt}"))
        .send()
        .await
        .unwrap();

    // Attempt WS connect with revoked key
    let session_id = server.create_session("revoked key test").await;
    let url = format!(
        "{}/sessions/{}?token={}",
        server.ws_base_url(),
        session_id,
        api_key,
    );
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_err(), "revoked API key should not authenticate");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn api_key_endpoints_require_jwt() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();

    // Without any auth
    let resp = client
        .post(format!("{}/auth/api-keys", server.base_url()))
        .json(&serde_json::json!({ "label": "nope" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    let resp = client
        .get(format!("{}/auth/api-keys", server.base_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}
