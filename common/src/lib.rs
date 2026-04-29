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
}

/// Messages the server sends to the client over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    #[serde(rename = "event")]
    Event { data: Event },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Session metadata returned by REST endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: Uuid,
    pub description: String,
    pub created_at: DateTime<Utc>,
}
