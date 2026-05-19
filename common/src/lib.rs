use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A persisted event from the session log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub session_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub author: Option<String>,
    pub kind: String,
    pub payload: serde_json::Value,
}

/// Messages the client sends to the server over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    #[serde(rename = "chat")]
    Chat { author: String, text: String },
    #[serde(rename = "run")]
    Run { author: String },
    #[serde(rename = "run_all")]
    RunAll { author: String },
    #[serde(rename = "clear")]
    Clear { author: String },
    #[serde(rename = "add")]
    Add { author: String, path: String },
    #[serde(rename = "diff")]
    Diff { author: String },
    #[serde(rename = "fetch")]
    Fetch { author: String, url: String },
    #[serde(rename = "repo_add")]
    RepoAdd { author: String, git_url: String },
    #[serde(rename = "repo_remove")]
    RepoRemove { author: String, name: String },
    #[serde(rename = "repo_list")]
    RepoList { author: String },
    #[serde(rename = "allowlist")]
    Allowlist { author: String },
    #[serde(rename = "allowlist_add")]
    AllowlistAdd { author: String, domain: String },
    #[serde(rename = "allowlist_remove")]
    AllowlistRemove { author: String, domain: String },
    #[serde(rename = "approve")]
    Approve {
        author: String,
        domain: String,
        approved: bool,
    },
    #[serde(rename = "who")]
    Who { author: String },
    #[serde(rename = "kick")]
    Kick { author: String, target: String },
    #[serde(rename = "session_info")]
    SessionInfo { author: String },
    #[serde(rename = "help")]
    Help { author: String },
    #[serde(rename = "trust")]
    Trust { author: String, target: String },
    #[serde(rename = "untrust")]
    Untrust { author: String, target: String },
}

/// Messages the server sends to the client over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    #[serde(rename = "event")]
    Event { data: Event },
    #[serde(rename = "error")]
    Error { message: String },
    /// Claude has started generating a response (ephemeral, not persisted).
    #[serde(rename = "stream_start")]
    StreamStart { session_id: Uuid },
    /// A partial text chunk from Claude (ephemeral, not persisted).
    #[serde(rename = "stream_delta")]
    StreamDelta { session_id: Uuid, delta: String },
    /// Claude has finished generating; the final event follows via `Event` (ephemeral).
    #[serde(rename = "stream_end")]
    StreamEnd { session_id: Uuid },
}

/// Session metadata returned by REST endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: Uuid,
    pub description: String,
    pub created_at: DateTime<Utc>,
    /// Session status: "active" or "expired".
    #[serde(default = "default_status")]
    pub status: String,
    /// Only included in the create-session response; `None` in list responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToken {
    pub id: i64,
    pub session_id: Uuid,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub revoked: bool,
}

fn default_status() -> String {
    "active".to_string()
}

/// A registered user account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

/// Summary of an API key (the actual secret is never stored or returned).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub id: Uuid,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked: bool,
}

/// A team for multi-tenancy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub daily_token_cap: i64,
    pub created_at: DateTime<Utc>,
}

/// A member of a team.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub user_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}
