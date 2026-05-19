//! Integration tests for Phase 2: team-scoped sessions, cross-team isolation, dashboard.
//!
//! Requires DATABASE_URL. Marked #[ignore] so they run only when explicitly requested.

mod common;

use common::TestServer;

/// Helper: register a user and return (user_id, access_token).
async fn register_and_login(
    base_url: &str,
    email: &str,
    display_name: &str,
) -> (uuid::Uuid, String) {
    let client = reqwest::Client::new();
    let password = "securepass123";

    let resp = client
        .post(format!("{base_url}/auth/register"))
        .json(&serde_json::json!({
            "email": email,
            "display_name": display_name,
            "password": password,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "register {email} should succeed");

    let resp = client
        .post(format!("{base_url}/auth/login"))
        .json(&serde_json::json!({
            "email": email,
            "password": password,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "login {email} should succeed");
    let body: serde_json::Value = resp.json().await.unwrap();
    let user_id: uuid::Uuid = body["user"]["id"].as_str().unwrap().parse().unwrap();
    let token = body["access_token"].as_str().unwrap().to_string();
    (user_id, token)
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn create_session_in_team() {
    let server = TestServer::start().await;
    let (_, token) = register_and_login(&server.base_url(), "ts-admin@test.com", "TSAdmin").await;
    let client = reqwest::Client::new();

    // Create team
    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "session-team" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let team: serde_json::Value = resp.json().await.unwrap();
    let team_id = team["id"].as_str().unwrap();

    // Create session in team
    let resp = client
        .post(format!("{}/teams/{}/sessions", server.base_url(), team_id))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "description": "team session 1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let session: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(session["description"], "team session 1");
    assert!(session["id"].as_str().is_some());
    assert!(session["owner_key"].as_str().is_some());

    // List sessions for team
    let resp = client
        .get(format!("{}/teams/{}/sessions", server.base_url(), team_id))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let sessions: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["description"], "team session 1");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn cross_team_isolation() {
    let server = TestServer::start().await;
    let (_user_a_id, token_a) =
        register_and_login(&server.base_url(), "team-a@test.com", "UserA").await;
    let (_user_b_id, token_b) =
        register_and_login(&server.base_url(), "team-b@test.com", "UserB").await;
    let client = reqwest::Client::new();

    // Create Team A
    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&token_a)
        .json(&serde_json::json!({ "name": "team-alpha" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let team_a: serde_json::Value = resp.json().await.unwrap();
    let team_a_id = team_a["id"].as_str().unwrap();

    // Create Team B
    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&token_b)
        .json(&serde_json::json!({ "name": "team-beta" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let team_b: serde_json::Value = resp.json().await.unwrap();
    let team_b_id = team_b["id"].as_str().unwrap();

    // Create session in Team A
    let resp = client
        .post(format!(
            "{}/teams/{}/sessions",
            server.base_url(),
            team_a_id
        ))
        .bearer_auth(&token_a)
        .json(&serde_json::json!({ "description": "alpha session" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Create session in Team B
    let resp = client
        .post(format!(
            "{}/teams/{}/sessions",
            server.base_url(),
            team_b_id
        ))
        .bearer_auth(&token_b)
        .json(&serde_json::json!({ "description": "beta session" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // User B cannot list Team A sessions
    let resp = client
        .get(format!(
            "{}/teams/{}/sessions",
            server.base_url(),
            team_a_id
        ))
        .bearer_auth(&token_b)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "User B should not access Team A sessions"
    );

    // User A cannot list Team B sessions
    let resp = client
        .get(format!(
            "{}/teams/{}/sessions",
            server.base_url(),
            team_b_id
        ))
        .bearer_auth(&token_a)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "User A should not access Team B sessions"
    );

    // User A sees only Team A sessions in their dashboard
    let resp = client
        .get(format!("{}/dashboard", server.base_url()))
        .bearer_auth(&token_a)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let dashboard: serde_json::Value = resp.json().await.unwrap();
    let sessions = dashboard["sessions"].as_array().unwrap();
    // User A should see: alpha session (from Team A) + any sessions with no team
    let team_sessions: Vec<_> = sessions
        .iter()
        .filter(|s| s["team_name"].as_str() == Some("team-alpha"))
        .collect();
    assert_eq!(team_sessions.len(), 1);
    // Should NOT see beta session
    let beta_sessions: Vec<_> = sessions
        .iter()
        .filter(|s| s["team_name"].as_str() == Some("team-beta"))
        .collect();
    assert_eq!(
        beta_sessions.len(),
        0,
        "User A should not see Team B sessions in dashboard"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn dashboard_shows_all_accessible_sessions() {
    let server = TestServer::start().await;
    let (_user_id, token) =
        register_and_login(&server.base_url(), "dash@test.com", "DashUser").await;
    let client = reqwest::Client::new();

    // Create team
    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "dash-team" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let team: serde_json::Value = resp.json().await.unwrap();
    let team_id = team["id"].as_str().unwrap();

    // Create team session
    let resp = client
        .post(format!("{}/teams/{}/sessions", server.base_url(), team_id))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "description": "team session for dash" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Create a non-team session (via admin token)
    let resp = client
        .post(format!("{}/sessions", server.base_url()))
        .bearer_auth("test-token-abc123")
        .json(&serde_json::json!({ "description": "personal session" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Dashboard should show both: team session and personal (no team) session
    let resp = client
        .get(format!("{}/dashboard", server.base_url()))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let dashboard: serde_json::Value = resp.json().await.unwrap();
    let sessions = dashboard["sessions"].as_array().unwrap();
    assert!(
        sessions.len() >= 2,
        "dashboard should show at least 2 sessions, got {}",
        sessions.len()
    );

    // Check that we have at least one with a team_name and one without
    let has_team_session = sessions
        .iter()
        .any(|s| s["team_name"].as_str() == Some("dash-team"));
    let has_personal_session = sessions.iter().any(|s| s["team_name"].is_null());
    assert!(has_team_session, "should include team session");
    assert!(has_personal_session, "should include personal session");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn viewer_cannot_create_team_session() {
    let server = TestServer::start().await;
    let (_, admin_token) =
        register_and_login(&server.base_url(), "tsv-admin@test.com", "TSVAdmin").await;
    let (viewer_id, viewer_token) =
        register_and_login(&server.base_url(), "tsv-viewer@test.com", "TSVViewer").await;
    let client = reqwest::Client::new();

    // Create team
    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({ "name": "viewer-test-team" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let team: serde_json::Value = resp.json().await.unwrap();
    let team_id = team["id"].as_str().unwrap();

    // Add viewer
    let resp = client
        .post(format!("{}/teams/{}/members", server.base_url(), team_id))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({
            "user_id": viewer_id.to_string(),
            "role": "viewer"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Viewer can list sessions
    let resp = client
        .get(format!("{}/teams/{}/sessions", server.base_url(), team_id))
        .bearer_auth(&viewer_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Viewer cannot create session
    let resp = client
        .post(format!("{}/teams/{}/sessions", server.base_url(), team_id))
        .bearer_auth(&viewer_token)
        .json(&serde_json::json!({ "description": "bad session" }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "viewer should not be able to create sessions"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn team_deletion_detaches_sessions() {
    let server = TestServer::start().await;
    let (_, token) = register_and_login(&server.base_url(), "detach@test.com", "DetachUser").await;
    let client = reqwest::Client::new();

    // Create team
    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "detach-team" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let team: serde_json::Value = resp.json().await.unwrap();
    let team_id = team["id"].as_str().unwrap();

    // Create session in team
    let resp = client
        .post(format!("{}/teams/{}/sessions", server.base_url(), team_id))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "description": "detach session" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let session: serde_json::Value = resp.json().await.unwrap();
    let session_id = session["id"].as_str().unwrap();

    // Delete team
    let resp = client
        .delete(format!("{}/teams/{}", server.base_url(), team_id))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // Session should still be accessible via admin token (now detached from team)
    let resp = client
        .get(format!("{}/sessions", server.base_url()))
        .bearer_auth("test-token-abc123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let sessions: Vec<serde_json::Value> = resp.json().await.unwrap();
    let found = sessions
        .iter()
        .any(|s| s["id"].as_str() == Some(session_id));
    assert!(found, "session should still exist after team deletion");
}
