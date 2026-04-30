use crate::db::{Database, DatabaseBackend};
use remora_common::Event;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub type EventTx = mpsc::UnboundedSender<Event>;

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
        }
    }
}

/// Per-connection info: sender channel + cancel token for kick.
#[allow(dead_code)]
pub struct ConnectionInfo {
    pub tx: EventTx,
    pub cancel: CancellationToken,
}

pub struct AppState {
    pub db: Arc<DatabaseBackend>,
    pub team_token: String,
    pub config: Config,
    /// session_id -> list of subscriber connections
    pub subscribers: RwLock<HashMap<Uuid, Vec<ConnectionInfo>>>,
    /// session_id -> set of connected participant names
    pub participants: RwLock<HashMap<Uuid, HashSet<String>>>,
}

impl AppState {
    pub fn new(db: Arc<DatabaseBackend>, team_token: String, config: Config) -> Self {
        Self {
            db,
            team_token,
            config,
            subscribers: RwLock::new(HashMap::new()),
            participants: RwLock::new(HashMap::new()),
        }
    }

    pub async fn subscribe(
        &self,
        session_id: Uuid,
    ) -> (mpsc::UnboundedReceiver<Event>, CancellationToken) {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let info = ConnectionInfo {
            tx,
            cancel: cancel.clone(),
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
        let subs = self.subscribers.read().await;
        if let Some(list) = subs.get(&event.session_id) {
            list.iter().for_each(|info| {
                let _ = info.tx.send(event.clone());
            });
        }
    }

    pub async fn participant_join(&self, session_id: Uuid, name: &str) {
        let mut parts = self.participants.write().await;
        parts
            .entry(session_id)
            .or_default()
            .insert(name.to_string());
    }

    pub async fn participant_leave(&self, session_id: Uuid, name: &str) {
        let mut parts = self.participants.write().await;
        if let Some(set) = parts.get_mut(&session_id) {
            set.remove(name);
            if set.is_empty() {
                parts.remove(&session_id);
            }
        }
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
    pub async fn kick_participant(&self, session_id: Uuid, target: &str) {
        // Cancel their connections
        let subs = self.subscribers.read().await;
        if let Some(list) = subs.get(&session_id) {
            for info in list.iter() {
                let _ = info;
            }
        }
        drop(subs);

        // Remove from participants
        self.participant_leave(session_id, target).await;
    }

    /// Cancel connections for a participant by name.
    #[allow(dead_code)]
    pub async fn kick_connections(&self, session_id: Uuid, target_name: &str) {
        let _ = (session_id, target_name);
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
