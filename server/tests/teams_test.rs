//! Integration tests for Phase 2: teams, team members, role enforcement, team deletion.
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
async fn create_team_and_list() {
    let server = TestServer::start().await;
    let (_, token) = register_and_login(&server.base_url(), "team1@test.com", "TeamUser1").await;
    let client = reqwest::Client::new();

    // Create team
    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "name": "alpha-team",
            "description": "Alpha squad"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create team should succeed");
    let team: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(team["name"], "alpha-team");
    assert_eq!(team["description"], "Alpha squad");
    let team_id = team["id"].as_str().unwrap();

    // List teams for user (should include the one we created)
    let resp = client
        .get(format!("{}/teams", server.base_url()))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let teams: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0]["name"], "alpha-team");

    // Get team by ID
    let resp = client
        .get(format!("{}/teams/{}", server.base_url(), team_id))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let fetched: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(fetched["name"], "alpha-team");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn duplicate_team_name_rejected() {
    let server = TestServer::start().await;
    let (_, token) = register_and_login(&server.base_url(), "dup@test.com", "DupUser").await;
    let client = reqwest::Client::new();

    let body = serde_json::json!({ "name": "unique-team-dup-test" });
    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "duplicate team name should be rejected");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn team_member_management() {
    let server = TestServer::start().await;
    let (_admin_id, admin_token) =
        register_and_login(&server.base_url(), "admin@team.com", "TeamAdmin").await;
    let (member_id, member_token) =
        register_and_login(&server.base_url(), "member@team.com", "TeamMember").await;
    let client = reqwest::Client::new();

    // Create team
    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({ "name": "member-test-team" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let team: serde_json::Value = resp.json().await.unwrap();
    let team_id = team["id"].as_str().unwrap();

    // Creator should be admin - list members
    let resp = client
        .get(format!("{}/teams/{}/members", server.base_url(), team_id))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let members: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["role"], "admin");

    // Add member
    let resp = client
        .post(format!("{}/teams/{}/members", server.base_url(), team_id))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({
            "user_id": member_id.to_string(),
            "role": "member"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "add member should succeed");

    // Verify member is now listed
    let resp = client
        .get(format!("{}/teams/{}/members", server.base_url(), team_id))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    let members: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(members.len(), 2);

    // Member cannot add other members (not admin)
    let (viewer_id, _) =
        register_and_login(&server.base_url(), "viewer@team.com", "TeamViewer").await;
    let resp = client
        .post(format!("{}/teams/{}/members", server.base_url(), team_id))
        .bearer_auth(&member_token)
        .json(&serde_json::json!({
            "user_id": viewer_id.to_string(),
            "role": "viewer"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "non-admin should not be able to add members"
    );

    // Update member role (admin can)
    let resp = client
        .put(format!(
            "{}/teams/{}/members/{}",
            server.base_url(),
            team_id,
            member_id
        ))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({ "role": "viewer" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // Self-remove
    let resp = client
        .delete(format!(
            "{}/teams/{}/members/{}",
            server.base_url(),
            team_id,
            member_id
        ))
        .bearer_auth(&member_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204, "self-removal should succeed");

    // Verify member removed
    let resp = client
        .get(format!("{}/teams/{}/members", server.base_url(), team_id))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    let members: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(members.len(), 1, "only admin should remain");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn team_update_and_delete() {
    let server = TestServer::start().await;
    let (_, admin_token) = register_and_login(&server.base_url(), "del@team.com", "DelAdmin").await;
    let (_, member_token) =
        register_and_login(&server.base_url(), "del-member@team.com", "DelMember").await;
    let client = reqwest::Client::new();

    // Create team
    let resp = client
        .post(format!("{}/teams", server.base_url()))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({ "name": "del-test-team" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let team: serde_json::Value = resp.json().await.unwrap();
    let team_id = team["id"].as_str().unwrap();

    // Update team name
    let resp = client
        .put(format!("{}/teams/{}", server.base_url(), team_id))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({
            "name": "renamed-team",
            "description": "updated desc"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // Non-member cannot update
    let resp = client
        .put(format!("{}/teams/{}", server.base_url(), team_id))
        .bearer_auth(&member_token)
        .json(&serde_json::json!({ "name": "bad-rename" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);

    // Delete team (admin)
    let resp = client
        .delete(format!("{}/teams/{}", server.base_url(), team_id))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // Team should no longer exist
    let resp = client
        .get(format!("{}/teams/{}", server.base_url(), team_id))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "team should not be accessible after deletion"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL"]
async fn unauthenticated_team_access_rejected() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // No auth header
    let resp = client
        .get(format!("{}/teams", server.base_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Admin token (not JWT) should be rejected for team endpoints
    let resp = client
        .get(format!("{}/teams", server.base_url()))
        .bearer_auth("test-token-abc123")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "admin token should not work for team endpoints"
    );
}
