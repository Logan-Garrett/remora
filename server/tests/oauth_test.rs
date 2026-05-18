//! Integration tests for OAuth flow.
//!
//! Since we cannot mock external OAuth providers in integration tests,
//! these tests verify the URL generation and error handling when OAuth
//! is not configured.

mod common;

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn github_oauth_returns_not_implemented_when_not_configured() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("{}/auth/oauth/github", server.base_url()))
        .send()
        .await
        .unwrap();
    // Without REMORA_OAUTH_GITHUB_CLIENT_ID set, should return 501
    assert_eq!(resp.status(), 501);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn google_oauth_returns_not_implemented_when_not_configured() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("{}/auth/oauth/google", server.base_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 501);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn github_callback_returns_not_implemented_when_not_configured() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{}/auth/oauth/github/callback?code=fakecodefakecodefake&state=test",
            server.base_url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 501);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn google_callback_returns_not_implemented_when_not_configured() {
    let server = common::TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{}/auth/oauth/google/callback?code=fakecodefakecodefake&state=test",
            server.base_url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 501);
}
