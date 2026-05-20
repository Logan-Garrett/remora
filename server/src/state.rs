use crate::db::{Database, DatabaseBackend};
use remora_common::{Event, ServerMsg};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub type EventTx = mpsc::UnboundedSender<ServerMsg>;

/// Server configuration derived from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub workspace_dir: PathBuf,
    pub run_timeout_secs: u64,
    pub idle_timeout_secs: u64,
    pub global_daily_cap: i64,
    pub claude_cmd: String,
    pub docker_image: String,
    pub skip_permissions: bool,
    pub use_sandbox: bool,
    pub permission_mode: String,
    pub allowed_tools: Vec<String>,
    pub backfill_limit: i64,
    pub max_sessions: usize,
    // JWT
    pub jwt_secret: String,
    pub jwt_expiry_secs: u64,
    pub refresh_expiry_secs: u64,
    // OAuth
    pub oauth_github_client_id: Option<String>,
    pub oauth_github_client_secret: Option<String>,
    pub oauth_google_client_id: Option<String>,
    pub oauth_google_client_secret: Option<String>,
    pub oauth_redirect_base_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            workspace_dir: PathBuf::from(
                std::env::var("REMORA_WORKSPACE_DIR")
                    .unwrap_or_else(|_| "/var/lib/remora/workspaces".into()),
            ),
            run_timeout_secs: std::env::var("REMORA_RUN_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(600),
            idle_timeout_secs: std::env::var("REMORA_IDLE_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1800),
            global_daily_cap: std::env::var("REMORA_GLOBAL_DAILY_CAP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10_000_000),
            claude_cmd: std::env::var("REMORA_CLAUDE_CMD").unwrap_or_else(|_| "claude".into()),
            docker_image: std::env::var("REMORA_DOCKER_IMAGE")
                .unwrap_or_else(|_| "ubuntu:22.04".into()),
            skip_permissions: std::env::var("REMORA_SKIP_PERMISSIONS")
                .map(|v| v != "false" && v != "0")
                .unwrap_or(true),
            use_sandbox: std::env::var("REMORA_USE_SANDBOX")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            permission_mode: std::env::var("REMORA_PERMISSION_MODE")
                .unwrap_or_else(|_| String::new()),
            allowed_tools: std::env::var("REMORA_ALLOWED_TOOLS")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            backfill_limit: std::env::var("REMORA_BACKFILL_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(500),
            max_sessions: std::env::var("REMORA_MAX_SESSIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100),
            jwt_secret: std::env::var("REMORA_JWT_SECRET").unwrap_or_else(|_| {
                let secret = uuid::Uuid::new_v4().to_string();
                tracing::warn!(
                    "REMORA_JWT_SECRET not set, using auto-generated secret (tokens will not survive restarts)"
                );
                secret
            }),
            jwt_expiry_secs: std::env::var("REMORA_JWT_EXPIRY_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600),
            refresh_expiry_secs: std::env::var("REMORA_REFRESH_EXPIRY_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(604800),
            oauth_github_client_id: std::env::var("REMORA_OAUTH_GITHUB_CLIENT_ID").ok(),
            oauth_github_client_secret: std::env::var("REMORA_OAUTH_GITHUB_CLIENT_SECRET").ok(),
            oauth_google_client_id: std::env::var("REMORA_OAUTH_GOOGLE_CLIENT_ID").ok(),
            oauth_google_client_secret: std::env::var("REMORA_OAUTH_GOOGLE_CLIENT_SECRET").ok(),
            oauth_redirect_base_url: std::env::var("REMORA_OAUTH_REDIRECT_URL")
                .unwrap_or_else(|_| "http://localhost:7200".into()),
        }
    }
}

/// Per-connection info: sender channel + cancel token for kick.
pub struct ConnectionInfo {
    pub tx: EventTx,
    pub cancel: CancellationToken,
    pub name: String,
}

pub struct AppState {
    pub db: Arc<DatabaseBackend>,
    pub team_token: String,
    pub config: Config,
    /// session_id -> list of subscriber connections
    pub subscribers: RwLock<HashMap<Uuid, Vec<ConnectionInfo>>>,
    /// session_id -> set of connected participant names
    pub participants: RwLock<HashMap<Uuid, HashSet<String>>>,
    /// session_id -> display name of the session owner (first participant to join)
    pub session_owners: RwLock<HashMap<Uuid, String>>,
    /// (session_id, participant_name) -> role string for RBAC enforcement
    pub participant_roles: RwLock<HashMap<(Uuid, String), String>>,
}

impl AppState {
    pub fn new(db: Arc<DatabaseBackend>, team_token: String, config: Config) -> Self {
        Self {
            db,
            team_token,
            config,
            subscribers: RwLock::new(HashMap::new()),
            participants: RwLock::new(HashMap::new()),
            session_owners: RwLock::new(HashMap::new()),
            participant_roles: RwLock::new(HashMap::new()),
        }
    }

    pub async fn subscribe(
        &self,
        session_id: Uuid,
        name: &str,
    ) -> (mpsc::UnboundedReceiver<ServerMsg>, CancellationToken) {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let info = ConnectionInfo {
            tx,
            cancel: cancel.clone(),
            name: name.to_string(),
        };
        let mut subs = self.subscribers.write().await;
        subs.entry(session_id).or_default().push(info);
        (rx, cancel)
    }

    pub async fn unsubscribe_closed(&self, session_id: Uuid) {
        let mut subs = self.subscribers.write().await;
        if let Some(list) = subs.get_mut(&session_id) {
            list.retain(|info| !info.tx.is_closed());
            if list.is_empty() {
                subs.remove(&session_id);
            }
        }
    }

    pub async fn dispatch(&self, event: Event) {
        let session_id = event.session_id;
        let msg = ServerMsg::Event { data: event };
        let subs = self.subscribers.read().await;
        if let Some(list) = subs.get(&session_id) {
            list.iter().for_each(|info| {
                let _ = info.tx.send(msg.clone());
            });
        }
    }

    /// Broadcast an ephemeral streaming message to all subscribers of a session.
    /// These are NOT persisted to the database — they are fire-and-forget.
    pub async fn broadcast_stream(&self, session_id: Uuid, msg: ServerMsg) {
        let subs = self.subscribers.read().await;
        if let Some(list) = subs.get(&session_id) {
            list.iter().for_each(|info| {
                let _ = info.tx.send(msg.clone());
            });
        }
    }

    /// Attempt to join a session. Returns `false` if the name is already taken.
    /// This combines the uniqueness check and insertion under a single write lock
    /// to prevent TOCTOU races between concurrent WebSocket handlers.
    pub async fn try_participant_join(&self, session_id: Uuid, name: &str) -> bool {
        let mut parts = self.participants.write().await;
        let set = parts.entry(session_id).or_default();
        if set.contains(name) {
            false
        } else {
            set.insert(name.to_string());
            true
        }
    }

    pub async fn participant_leave(&self, session_id: Uuid, name: &str) {
        // Clean up role entry
        self.remove_participant_role(session_id, name).await;

        let mut parts = self.participants.write().await;
        if let Some(set) = parts.get_mut(&session_id) {
            set.remove(name);
            if set.is_empty() {
                parts.remove(&session_id);
                // Clear ownership when the last participant leaves so the next
                // person to join becomes the new owner.
                drop(parts); // release participants lock before acquiring owners lock
                let mut owners = self.session_owners.write().await;
                owners.remove(&session_id);
            }
        }
    }

    /// Set the session owner if one has not been set yet.
    /// Returns `true` if the caller was set as owner, `false` if an owner already exists.
    pub async fn set_session_owner(&self, session_id: Uuid, name: &str) -> bool {
        use std::collections::hash_map::Entry;
        let mut owners = self.session_owners.write().await;
        if let Entry::Vacant(e) = owners.entry(session_id) {
            e.insert(name.to_string());
            true
        } else {
            false
        }
    }

    /// Forcefully set the session owner, overwriting any existing owner.
    /// Used when a valid owner_key is provided on WebSocket connect.
    pub async fn force_set_session_owner(&self, session_id: Uuid, name: &str) {
        let mut owners = self.session_owners.write().await;
        owners.insert(session_id, name.to_string());
    }

    /// Get the session owner's display name, if set.
    pub async fn get_session_owner(&self, session_id: Uuid) -> Option<String> {
        let owners = self.session_owners.read().await;
        owners.get(&session_id).cloned()
    }

    /// Remove the session owner entry (e.g. on session delete).
    pub async fn clear_session_owner(&self, session_id: Uuid) {
        let mut owners = self.session_owners.write().await;
        owners.remove(&session_id);
    }

    /// Store the role for a participant in a session.
    pub async fn set_participant_role(&self, session_id: Uuid, name: &str, role: &str) {
        let mut roles = self.participant_roles.write().await;
        roles.insert((session_id, name.to_string()), role.to_string());
    }

    /// Get the role for a participant in a session.
    pub async fn get_participant_role(&self, session_id: Uuid, name: &str) -> Option<String> {
        let roles = self.participant_roles.read().await;
        roles.get(&(session_id, name.to_string())).cloned()
    }

    /// Remove the role entry for a participant leaving a session.
    pub async fn remove_participant_role(&self, session_id: Uuid, name: &str) {
        let mut roles = self.participant_roles.write().await;
        roles.remove(&(session_id, name.to_string()));
    }

    pub async fn get_participants(&self, session_id: Uuid) -> Vec<String> {
        let parts = self.participants.read().await;
        parts
            .get(&session_id)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Check if a Claude run is currently in flight for the given session.
    pub async fn is_run_in_flight(&self, session_id: Uuid) -> bool {
        self.db.is_run_in_flight(session_id).await.unwrap_or(false)
    }

    /// Kick all connections for a named participant in a session.
    ///
    /// The WS handler's send loop already detects kick events and disconnects
    /// the targeted client after forwarding the event. This method acts as a
    /// delayed fallback: it collects the cancel tokens for the target, then
    /// spawns a task that waits briefly before cancelling them, giving the
    /// notification pipeline time to deliver the kick event first.
    pub async fn kick_participant(&self, session_id: Uuid, target: &str) {
        // Collect cancel tokens for the target's connections
        let tokens: Vec<CancellationToken> = {
            let subs = self.subscribers.read().await;
            subs.get(&session_id)
                .map(|list| {
                    list.iter()
                        .filter(|info| info.name == target)
                        .map(|info| info.cancel.clone())
                        .collect()
                })
                .unwrap_or_default()
        };

        // Remove from participants immediately
        self.participant_leave(session_id, target).await;

        // Spawn a delayed cancel so the kick event has time to be delivered
        // through the notification pipeline before we forcibly close the connection.
        if !tokens.is_empty() {
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                for token in tokens {
                    token.cancel();
                }
            });
        }
    }
}

/// Runs the notification listener (Postgres LISTEN/NOTIFY or in-process broadcast)
/// and dispatches events to WebSocket subscribers.
pub async fn run_event_listener(state: Arc<AppState>) -> anyhow::Result<()> {
    let mut rx = state.db.subscribe_notifications().await?;

    while let Some(event_id) = rx.recv().await {
        match state.db.get_event_by_id(event_id).await {
            Ok(Some(event)) => {
                state.dispatch(event).await;
            }
            Ok(None) => tracing::warn!("event {event_id} not found after notify"),
            Err(e) => tracing::error!("fetch event {event_id}: {e}"),
        }
    }
    Ok(())
}
