#[cfg(feature = "mssql")]
pub mod mssql;
pub mod postgres;
pub mod sqlite;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use remora_common::{ApiKeyInfo, Event, SessionToken, Team, TeamMember, User};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Phase 4: Admin & observability data types
// ---------------------------------------------------------------------------

/// Per-session token usage for the admin dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUsage {
    pub session_id: Uuid,
    pub description: String,
    pub tokens_used_today: i64,
    pub daily_token_cap: i64,
    pub tokens_reset_date: String,
}

/// Global usage totals across all sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalUsage {
    pub total_tokens_today: i64,
    pub total_sessions: i64,
    pub active_sessions: i64,
}

/// Aggregated run analytics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAnalytics {
    pub total_runs: i64,
    pub successful: i64,
    pub failed: i64,
    pub timed_out: i64,
    pub avg_duration_secs: f64,
    pub runs_by_session: Vec<SessionRunCount>,
}

/// Run count per session (for top-N display).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRunCount {
    pub session_id: Uuid,
    pub run_count: i64,
}

/// Full session info for the admin panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminSessionInfo {
    pub id: Uuid,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub status: String,
    pub tokens_used_today: i64,
    pub daily_token_cap: i64,
}

/// An audit event record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: i64,
    pub user_id: Option<Uuid>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub details: Option<Value>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Aggregated data for the Prometheus metrics endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsData {
    pub sessions_total: i64,
    pub sessions_active: i64,
    pub tokens_used_total: i64,
    pub runs_successful: i64,
    pub runs_failed: i64,
    pub runs_timed_out: i64,
}

/// Notification receiver that yields event IDs as they are inserted.
pub type NotificationRx = tokio::sync::mpsc::UnboundedReceiver<i64>;

/// The set of operations every database backend must support.
#[async_trait]
pub trait Database: Send + Sync + 'static {
    // -- health --
    async fn ping(&self) -> anyhow::Result<()>;

    // -- migrations --
    async fn run_migrations(&self) -> anyhow::Result<()>;

    // -- sessions --
    async fn create_session(
        &self,
        description: &str,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)>;

    async fn list_sessions(&self) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String)>>;

    async fn delete_session(&self, session_id: Uuid) -> anyhow::Result<u64>;

    async fn session_exists(&self, session_id: Uuid) -> anyhow::Result<bool>;

    /// Returns the session status: `Some("active")`, `Some("expired")`, or `None` if not found.
    async fn get_session_status(&self, session_id: Uuid) -> anyhow::Result<Option<String>>;

    async fn set_session_expired(&self, session_id: Uuid) -> anyhow::Result<()>;

    async fn reactivate_session(&self, session_id: Uuid) -> anyhow::Result<()>;

    async fn count_sessions(&self) -> anyhow::Result<i64>;

    async fn get_session_info(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<Option<(String, DateTime<Utc>, i64, i64)>>;

    async fn set_idle_since_now(&self, session_id: Uuid) -> anyhow::Result<()>;

    async fn clear_idle_since(&self, session_id: Uuid) -> anyhow::Result<()>;

    // -- events --
    async fn insert_event(
        &self,
        session_id: Uuid,
        author: &str,
        kind: &str,
        payload: Value,
    ) -> anyhow::Result<i64>;

    async fn get_event_by_id(&self, event_id: i64) -> anyhow::Result<Option<Event>>;

    async fn get_events_for_session(&self, session_id: Uuid) -> anyhow::Result<Vec<Event>>;

    async fn get_recent_events_for_session(
        &self,
        session_id: Uuid,
        limit: i64,
    ) -> anyhow::Result<Vec<Event>>;

    async fn get_events_since(
        &self,
        session_id: Uuid,
        since_id: i64,
    ) -> anyhow::Result<Vec<(i64, Option<String>, String, Value)>>;

    async fn get_last_context_boundary(&self, session_id: Uuid) -> anyhow::Result<i64>;

    // -- repos --
    async fn upsert_repo(&self, session_id: Uuid, name: &str, git_url: &str) -> anyhow::Result<()>;

    async fn delete_repo(&self, session_id: Uuid, name: &str) -> anyhow::Result<()>;

    async fn list_repos(&self, session_id: Uuid) -> anyhow::Result<Vec<(String, String)>>;

    async fn list_repo_names(&self, session_id: Uuid) -> anyhow::Result<Vec<String>>;

    // -- runs --
    async fn insert_run(&self, session_id: Uuid, context_mode: &str) -> anyhow::Result<i64>;

    async fn finish_run(&self, run_id: i64, status: &str) -> anyhow::Result<()>;

    async fn is_run_in_flight(&self, session_id: Uuid) -> anyhow::Result<bool>;

    // -- allowlists --
    async fn list_global_allowlist(&self) -> anyhow::Result<Vec<(String, String)>>;

    async fn list_session_allowlist(&self, session_id: Uuid) -> anyhow::Result<Vec<String>>;

    async fn add_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()>;

    async fn remove_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()>;

    async fn is_domain_blocked(&self, domain: &str) -> anyhow::Result<bool>;

    async fn is_domain_global_allowed(&self, domain: &str) -> anyhow::Result<bool>;

    async fn is_domain_session_allowed(
        &self,
        session_id: Uuid,
        domain: &str,
    ) -> anyhow::Result<bool>;

    // -- owner key --
    async fn set_owner_key(&self, session_id: Uuid, key: &str) -> anyhow::Result<()>;
    async fn get_owner_key(&self, session_id: Uuid) -> anyhow::Result<Option<String>>;

    // -- session tokens --
    async fn create_session_token(&self, session_id: Uuid, label: &str) -> anyhow::Result<String>;
    async fn validate_session_token(&self, token: &str) -> anyhow::Result<Option<Uuid>>;
    async fn revoke_session_token(&self, token: &str) -> anyhow::Result<()>;
    async fn revoke_session_token_by_id(
        &self,
        session_id: Uuid,
        token_id: i64,
    ) -> anyhow::Result<()>;
    async fn list_session_tokens(&self, session_id: Uuid) -> anyhow::Result<Vec<SessionToken>>;

    // -- trusted participants --
    async fn trust_participant(&self, session_id: Uuid, name: &str) -> anyhow::Result<()>;
    async fn untrust_participant(&self, session_id: Uuid, name: &str) -> anyhow::Result<()>;
    async fn list_trusted_participants(&self, session_id: Uuid) -> anyhow::Result<Vec<String>>;

    // -- pending approvals --
    async fn create_pending_approval(
        &self,
        session_id: Uuid,
        domain: &str,
        url: &str,
        requested_by: &str,
    ) -> anyhow::Result<()>;

    async fn resolve_approval(
        &self,
        session_id: Uuid,
        domain: &str,
        approved: bool,
    ) -> anyhow::Result<()>;

    async fn get_approved_pending(
        &self,
        session_id: Uuid,
        domain: &str,
    ) -> anyhow::Result<Vec<(String, String)>>;

    // -- quotas --
    async fn reset_tokens_if_needed(&self, session_id: Uuid) -> anyhow::Result<()>;

    async fn get_session_usage(&self, session_id: Uuid) -> anyhow::Result<(i64, i64)>;

    async fn get_global_usage(&self) -> anyhow::Result<i64>;

    async fn add_usage(&self, session_id: Uuid, tokens: i64) -> anyhow::Result<()>;

    async fn get_idle_sessions(&self, idle_timeout_secs: u64) -> anyhow::Result<Vec<Uuid>>;

    async fn clear_idle_since_for(&self, session_id: Uuid) -> anyhow::Result<()>;

    // -- users --
    async fn create_user(
        &self,
        email: &str,
        display_name: &str,
        password_hash: Option<&str>,
        role: &str,
    ) -> anyhow::Result<Uuid>;
    async fn get_user_by_email(&self, email: &str) -> anyhow::Result<Option<User>>;
    async fn get_user_by_id(&self, id: Uuid) -> anyhow::Result<Option<User>>;
    /// Returns the password_hash for login verification (separate from User to avoid leaking it).
    async fn get_password_hash(&self, email: &str) -> anyhow::Result<Option<String>>;

    // -- refresh tokens --
    async fn store_refresh_token(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> anyhow::Result<Uuid>;
    /// Returns (token_id, user_id) if the token is valid and not expired.
    async fn validate_refresh_token(
        &self,
        token_hash: &str,
    ) -> anyhow::Result<Option<(Uuid, Uuid)>>;
    async fn delete_refresh_token(&self, token_id: Uuid) -> anyhow::Result<()>;

    /// Atomically consume a refresh token: DELETE ... WHERE token_hash = $1 AND expires_at > now()
    /// RETURNING user_id. Returns the user_id if a valid token was consumed, None if the token
    /// was already used or expired. This eliminates the validate-then-delete race condition.
    async fn consume_refresh_token(&self, token_hash: &str) -> anyhow::Result<Option<Uuid>>;

    // -- oauth --
    async fn upsert_oauth_connection(
        &self,
        user_id: Uuid,
        provider: &str,
        provider_user_id: &str,
    ) -> anyhow::Result<()>;
    async fn get_user_by_oauth(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> anyhow::Result<Option<User>>;

    // -- api keys --
    async fn create_api_key(
        &self,
        user_id: Uuid,
        key_hash: &str,
        label: &str,
    ) -> anyhow::Result<Uuid>;
    async fn validate_api_key(&self, key_hash: &str) -> anyhow::Result<Option<User>>;
    async fn list_api_keys(&self, user_id: Uuid) -> anyhow::Result<Vec<ApiKeyInfo>>;
    async fn revoke_api_key(&self, key_id: Uuid, user_id: Uuid) -> anyhow::Result<()>;

    // -- teams --
    async fn create_team(
        &self,
        name: &str,
        description: &str,
        created_by: Uuid,
    ) -> anyhow::Result<Uuid>;
    async fn get_team(&self, team_id: Uuid) -> anyhow::Result<Option<Team>>;
    async fn list_teams_for_user(&self, user_id: Uuid) -> anyhow::Result<Vec<Team>>;
    async fn update_team(&self, team_id: Uuid, name: &str, description: &str)
        -> anyhow::Result<()>;
    async fn delete_team(&self, team_id: Uuid) -> anyhow::Result<()>;

    // -- team members --
    async fn add_team_member(&self, team_id: Uuid, user_id: Uuid, role: &str)
        -> anyhow::Result<()>;
    async fn remove_team_member(&self, team_id: Uuid, user_id: Uuid) -> anyhow::Result<()>;
    async fn list_team_members(&self, team_id: Uuid) -> anyhow::Result<Vec<TeamMember>>;
    async fn get_team_member_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> anyhow::Result<Option<String>>;
    async fn update_team_member_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> anyhow::Result<()>;

    // -- team-scoped sessions --
    async fn create_session_for_team(
        &self,
        description: &str,
        team_id: Uuid,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)>;
    async fn list_sessions_for_team(
        &self,
        team_id: Uuid,
    ) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String)>>;
    async fn get_session_team(&self, session_id: Uuid) -> anyhow::Result<Option<Uuid>>;

    // -- user dashboard --
    async fn list_sessions_for_user(
        &self,
        user_id: Uuid,
    ) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String, Option<String>)>>;

    // -- Phase 4: admin & observability --

    /// Per-session usage summary for the admin dashboard.
    async fn get_usage_summary(&self) -> anyhow::Result<Vec<SessionUsage>>;

    /// Global usage totals.
    async fn get_global_usage_summary(&self) -> anyhow::Result<GlobalUsage>;

    /// Run analytics aggregates.
    async fn get_run_analytics(&self) -> anyhow::Result<RunAnalytics>;

    /// List ALL sessions with full details (admin).
    async fn list_all_sessions_admin(&self) -> anyhow::Result<Vec<AdminSessionInfo>>;

    /// Update a session's daily token cap.
    async fn update_session_quota(
        &self,
        session_id: Uuid,
        daily_token_cap: i64,
    ) -> anyhow::Result<()>;

    /// List all users (admin).
    async fn list_all_users(&self) -> anyhow::Result<Vec<User>>;

    /// Update a user's global role.
    async fn update_user_role(&self, user_id: Uuid, role: &str) -> anyhow::Result<()>;

    /// Insert an audit event.
    async fn insert_audit_event(
        &self,
        user_id: Option<Uuid>,
        action: &str,
        target_type: &str,
        target_id: Option<&str>,
        details: Option<Value>,
        ip_address: Option<&str>,
    ) -> anyhow::Result<i64>;

    /// List audit events with pagination.
    async fn list_audit_events(&self, limit: i64, offset: i64) -> anyhow::Result<Vec<AuditEvent>>;

    /// Gather metrics data for Prometheus endpoint.
    async fn get_metrics_data(&self) -> anyhow::Result<MetricsData>;

    // -- notifications --
    /// Start listening for new-event notifications.  Returns a receiver
    /// that yields event IDs.
    async fn subscribe_notifications(&self) -> anyhow::Result<NotificationRx>;
}

/// The concrete backend enum. We dispatch through the trait via `Arc<dyn Database>`.
pub enum DatabaseBackend {
    Postgres(postgres::PostgresDb),
    Sqlite(sqlite::SqliteDb),
    #[cfg(feature = "mssql")]
    Mssql(mssql::MssqlDb),
}

/// Factory: create a backend from a provider name and connection URL.
pub async fn create_backend(provider: &str, url: &str) -> anyhow::Result<DatabaseBackend> {
    match provider {
        "postgres" | "pg" => {
            let db = postgres::PostgresDb::connect(url).await?;
            Ok(DatabaseBackend::Postgres(db))
        }
        "sqlite" => {
            let db = sqlite::SqliteDb::connect(url).await?;
            Ok(DatabaseBackend::Sqlite(db))
        }
        #[cfg(feature = "mssql")]
        "mssql" | "sqlserver" => {
            let db = mssql::MssqlDb::connect(url).await?;
            Ok(DatabaseBackend::Mssql(db))
        }
        other => anyhow::bail!("unsupported REMORA_DB_PROVIDER: {other}"),
    }
}

// Implement the trait for the enum so we can use it directly.
#[async_trait]
impl Database for DatabaseBackend {
    async fn ping(&self) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.ping().await,
            Self::Sqlite(db) => db.ping().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.ping().await,
        }
    }

    async fn run_migrations(&self) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.run_migrations().await,
            Self::Sqlite(db) => db.run_migrations().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.run_migrations().await,
        }
    }

    async fn create_session(
        &self,
        description: &str,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)> {
        match self {
            Self::Postgres(db) => db.create_session(description).await,
            Self::Sqlite(db) => db.create_session(description).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.create_session(description).await,
        }
    }

    async fn list_sessions(&self) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String)>> {
        match self {
            Self::Postgres(db) => db.list_sessions().await,
            Self::Sqlite(db) => db.list_sessions().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_sessions().await,
        }
    }

    async fn delete_session(&self, session_id: Uuid) -> anyhow::Result<u64> {
        match self {
            Self::Postgres(db) => db.delete_session(session_id).await,
            Self::Sqlite(db) => db.delete_session(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.delete_session(session_id).await,
        }
    }

    async fn session_exists(&self, session_id: Uuid) -> anyhow::Result<bool> {
        match self {
            Self::Postgres(db) => db.session_exists(session_id).await,
            Self::Sqlite(db) => db.session_exists(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.session_exists(session_id).await,
        }
    }

    async fn get_session_status(&self, session_id: Uuid) -> anyhow::Result<Option<String>> {
        match self {
            Self::Postgres(db) => db.get_session_status(session_id).await,
            Self::Sqlite(db) => db.get_session_status(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_session_status(session_id).await,
        }
    }

    async fn set_session_expired(&self, session_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.set_session_expired(session_id).await,
            Self::Sqlite(db) => db.set_session_expired(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.set_session_expired(session_id).await,
        }
    }

    async fn reactivate_session(&self, session_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.reactivate_session(session_id).await,
            Self::Sqlite(db) => db.reactivate_session(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.reactivate_session(session_id).await,
        }
    }

    async fn count_sessions(&self) -> anyhow::Result<i64> {
        match self {
            Self::Postgres(db) => db.count_sessions().await,
            Self::Sqlite(db) => db.count_sessions().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.count_sessions().await,
        }
    }

    async fn get_session_info(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<Option<(String, DateTime<Utc>, i64, i64)>> {
        match self {
            Self::Postgres(db) => db.get_session_info(session_id).await,
            Self::Sqlite(db) => db.get_session_info(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_session_info(session_id).await,
        }
    }

    async fn set_idle_since_now(&self, session_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.set_idle_since_now(session_id).await,
            Self::Sqlite(db) => db.set_idle_since_now(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.set_idle_since_now(session_id).await,
        }
    }

    async fn clear_idle_since(&self, session_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.clear_idle_since(session_id).await,
            Self::Sqlite(db) => db.clear_idle_since(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.clear_idle_since(session_id).await,
        }
    }

    async fn insert_event(
        &self,
        session_id: Uuid,
        author: &str,
        kind: &str,
        payload: Value,
    ) -> anyhow::Result<i64> {
        match self {
            Self::Postgres(db) => db.insert_event(session_id, author, kind, payload).await,
            Self::Sqlite(db) => db.insert_event(session_id, author, kind, payload).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.insert_event(session_id, author, kind, payload).await,
        }
    }

    async fn get_event_by_id(&self, event_id: i64) -> anyhow::Result<Option<Event>> {
        match self {
            Self::Postgres(db) => db.get_event_by_id(event_id).await,
            Self::Sqlite(db) => db.get_event_by_id(event_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_event_by_id(event_id).await,
        }
    }

    async fn get_events_for_session(&self, session_id: Uuid) -> anyhow::Result<Vec<Event>> {
        match self {
            Self::Postgres(db) => db.get_events_for_session(session_id).await,
            Self::Sqlite(db) => db.get_events_for_session(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_events_for_session(session_id).await,
        }
    }

    async fn get_recent_events_for_session(
        &self,
        session_id: Uuid,
        limit: i64,
    ) -> anyhow::Result<Vec<Event>> {
        match self {
            Self::Postgres(db) => db.get_recent_events_for_session(session_id, limit).await,
            Self::Sqlite(db) => db.get_recent_events_for_session(session_id, limit).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_recent_events_for_session(session_id, limit).await,
        }
    }

    async fn get_events_since(
        &self,
        session_id: Uuid,
        since_id: i64,
    ) -> anyhow::Result<Vec<(i64, Option<String>, String, Value)>> {
        match self {
            Self::Postgres(db) => db.get_events_since(session_id, since_id).await,
            Self::Sqlite(db) => db.get_events_since(session_id, since_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_events_since(session_id, since_id).await,
        }
    }

    async fn get_last_context_boundary(&self, session_id: Uuid) -> anyhow::Result<i64> {
        match self {
            Self::Postgres(db) => db.get_last_context_boundary(session_id).await,
            Self::Sqlite(db) => db.get_last_context_boundary(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_last_context_boundary(session_id).await,
        }
    }

    async fn upsert_repo(&self, session_id: Uuid, name: &str, git_url: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.upsert_repo(session_id, name, git_url).await,
            Self::Sqlite(db) => db.upsert_repo(session_id, name, git_url).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.upsert_repo(session_id, name, git_url).await,
        }
    }

    async fn delete_repo(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.delete_repo(session_id, name).await,
            Self::Sqlite(db) => db.delete_repo(session_id, name).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.delete_repo(session_id, name).await,
        }
    }

    async fn list_repos(&self, session_id: Uuid) -> anyhow::Result<Vec<(String, String)>> {
        match self {
            Self::Postgres(db) => db.list_repos(session_id).await,
            Self::Sqlite(db) => db.list_repos(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_repos(session_id).await,
        }
    }

    async fn list_repo_names(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        match self {
            Self::Postgres(db) => db.list_repo_names(session_id).await,
            Self::Sqlite(db) => db.list_repo_names(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_repo_names(session_id).await,
        }
    }

    async fn insert_run(&self, session_id: Uuid, context_mode: &str) -> anyhow::Result<i64> {
        match self {
            Self::Postgres(db) => db.insert_run(session_id, context_mode).await,
            Self::Sqlite(db) => db.insert_run(session_id, context_mode).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.insert_run(session_id, context_mode).await,
        }
    }

    async fn finish_run(&self, run_id: i64, status: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.finish_run(run_id, status).await,
            Self::Sqlite(db) => db.finish_run(run_id, status).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.finish_run(run_id, status).await,
        }
    }

    async fn is_run_in_flight(&self, session_id: Uuid) -> anyhow::Result<bool> {
        match self {
            Self::Postgres(db) => db.is_run_in_flight(session_id).await,
            Self::Sqlite(db) => db.is_run_in_flight(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.is_run_in_flight(session_id).await,
        }
    }

    async fn list_global_allowlist(&self) -> anyhow::Result<Vec<(String, String)>> {
        match self {
            Self::Postgres(db) => db.list_global_allowlist().await,
            Self::Sqlite(db) => db.list_global_allowlist().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_global_allowlist().await,
        }
    }

    async fn list_session_allowlist(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        match self {
            Self::Postgres(db) => db.list_session_allowlist(session_id).await,
            Self::Sqlite(db) => db.list_session_allowlist(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_session_allowlist(session_id).await,
        }
    }

    async fn add_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.add_session_allowlist(session_id, domain).await,
            Self::Sqlite(db) => db.add_session_allowlist(session_id, domain).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.add_session_allowlist(session_id, domain).await,
        }
    }

    async fn remove_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.remove_session_allowlist(session_id, domain).await,
            Self::Sqlite(db) => db.remove_session_allowlist(session_id, domain).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.remove_session_allowlist(session_id, domain).await,
        }
    }

    async fn is_domain_blocked(&self, domain: &str) -> anyhow::Result<bool> {
        match self {
            Self::Postgres(db) => db.is_domain_blocked(domain).await,
            Self::Sqlite(db) => db.is_domain_blocked(domain).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.is_domain_blocked(domain).await,
        }
    }

    async fn is_domain_global_allowed(&self, domain: &str) -> anyhow::Result<bool> {
        match self {
            Self::Postgres(db) => db.is_domain_global_allowed(domain).await,
            Self::Sqlite(db) => db.is_domain_global_allowed(domain).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.is_domain_global_allowed(domain).await,
        }
    }

    async fn is_domain_session_allowed(
        &self,
        session_id: Uuid,
        domain: &str,
    ) -> anyhow::Result<bool> {
        match self {
            Self::Postgres(db) => db.is_domain_session_allowed(session_id, domain).await,
            Self::Sqlite(db) => db.is_domain_session_allowed(session_id, domain).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.is_domain_session_allowed(session_id, domain).await,
        }
    }

    async fn set_owner_key(&self, session_id: Uuid, key: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.set_owner_key(session_id, key).await,
            Self::Sqlite(db) => db.set_owner_key(session_id, key).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.set_owner_key(session_id, key).await,
        }
    }

    async fn get_owner_key(&self, session_id: Uuid) -> anyhow::Result<Option<String>> {
        match self {
            Self::Postgres(db) => db.get_owner_key(session_id).await,
            Self::Sqlite(db) => db.get_owner_key(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_owner_key(session_id).await,
        }
    }

    async fn create_session_token(&self, session_id: Uuid, label: &str) -> anyhow::Result<String> {
        match self {
            Self::Postgres(db) => db.create_session_token(session_id, label).await,
            Self::Sqlite(db) => db.create_session_token(session_id, label).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.create_session_token(session_id, label).await,
        }
    }
    async fn validate_session_token(&self, token: &str) -> anyhow::Result<Option<Uuid>> {
        match self {
            Self::Postgres(db) => db.validate_session_token(token).await,
            Self::Sqlite(db) => db.validate_session_token(token).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.validate_session_token(token).await,
        }
    }
    async fn revoke_session_token(&self, token: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.revoke_session_token(token).await,
            Self::Sqlite(db) => db.revoke_session_token(token).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.revoke_session_token(token).await,
        }
    }
    async fn revoke_session_token_by_id(
        &self,
        session_id: Uuid,
        token_id: i64,
    ) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.revoke_session_token_by_id(session_id, token_id).await,
            Self::Sqlite(db) => db.revoke_session_token_by_id(session_id, token_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.revoke_session_token_by_id(session_id, token_id).await,
        }
    }
    async fn list_session_tokens(&self, session_id: Uuid) -> anyhow::Result<Vec<SessionToken>> {
        match self {
            Self::Postgres(db) => db.list_session_tokens(session_id).await,
            Self::Sqlite(db) => db.list_session_tokens(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_session_tokens(session_id).await,
        }
    }

    async fn trust_participant(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.trust_participant(session_id, name).await,
            Self::Sqlite(db) => db.trust_participant(session_id, name).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.trust_participant(session_id, name).await,
        }
    }

    async fn untrust_participant(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.untrust_participant(session_id, name).await,
            Self::Sqlite(db) => db.untrust_participant(session_id, name).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.untrust_participant(session_id, name).await,
        }
    }

    async fn list_trusted_participants(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        match self {
            Self::Postgres(db) => db.list_trusted_participants(session_id).await,
            Self::Sqlite(db) => db.list_trusted_participants(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_trusted_participants(session_id).await,
        }
    }

    async fn create_pending_approval(
        &self,
        session_id: Uuid,
        domain: &str,
        url: &str,
        requested_by: &str,
    ) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => {
                db.create_pending_approval(session_id, domain, url, requested_by)
                    .await
            }
            Self::Sqlite(db) => {
                db.create_pending_approval(session_id, domain, url, requested_by)
                    .await
            }
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => {
                db.create_pending_approval(session_id, domain, url, requested_by)
                    .await
            }
        }
    }

    async fn resolve_approval(
        &self,
        session_id: Uuid,
        domain: &str,
        approved: bool,
    ) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.resolve_approval(session_id, domain, approved).await,
            Self::Sqlite(db) => db.resolve_approval(session_id, domain, approved).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.resolve_approval(session_id, domain, approved).await,
        }
    }

    async fn get_approved_pending(
        &self,
        session_id: Uuid,
        domain: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        match self {
            Self::Postgres(db) => db.get_approved_pending(session_id, domain).await,
            Self::Sqlite(db) => db.get_approved_pending(session_id, domain).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_approved_pending(session_id, domain).await,
        }
    }

    async fn reset_tokens_if_needed(&self, session_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.reset_tokens_if_needed(session_id).await,
            Self::Sqlite(db) => db.reset_tokens_if_needed(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.reset_tokens_if_needed(session_id).await,
        }
    }

    async fn get_session_usage(&self, session_id: Uuid) -> anyhow::Result<(i64, i64)> {
        match self {
            Self::Postgres(db) => db.get_session_usage(session_id).await,
            Self::Sqlite(db) => db.get_session_usage(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_session_usage(session_id).await,
        }
    }

    async fn get_global_usage(&self) -> anyhow::Result<i64> {
        match self {
            Self::Postgres(db) => db.get_global_usage().await,
            Self::Sqlite(db) => db.get_global_usage().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_global_usage().await,
        }
    }

    async fn add_usage(&self, session_id: Uuid, tokens: i64) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.add_usage(session_id, tokens).await,
            Self::Sqlite(db) => db.add_usage(session_id, tokens).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.add_usage(session_id, tokens).await,
        }
    }

    async fn get_idle_sessions(&self, idle_timeout_secs: u64) -> anyhow::Result<Vec<Uuid>> {
        match self {
            Self::Postgres(db) => db.get_idle_sessions(idle_timeout_secs).await,
            Self::Sqlite(db) => db.get_idle_sessions(idle_timeout_secs).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_idle_sessions(idle_timeout_secs).await,
        }
    }

    async fn clear_idle_since_for(&self, session_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.clear_idle_since_for(session_id).await,
            Self::Sqlite(db) => db.clear_idle_since_for(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.clear_idle_since_for(session_id).await,
        }
    }

    // -- users --
    async fn create_user(
        &self,
        email: &str,
        display_name: &str,
        password_hash: Option<&str>,
        role: &str,
    ) -> anyhow::Result<Uuid> {
        match self {
            Self::Postgres(db) => {
                db.create_user(email, display_name, password_hash, role)
                    .await
            }
            Self::Sqlite(db) => {
                db.create_user(email, display_name, password_hash, role)
                    .await
            }
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => {
                db.create_user(email, display_name, password_hash, role)
                    .await
            }
        }
    }
    async fn get_user_by_email(&self, email: &str) -> anyhow::Result<Option<User>> {
        match self {
            Self::Postgres(db) => db.get_user_by_email(email).await,
            Self::Sqlite(db) => db.get_user_by_email(email).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_user_by_email(email).await,
        }
    }
    async fn get_user_by_id(&self, id: Uuid) -> anyhow::Result<Option<User>> {
        match self {
            Self::Postgres(db) => db.get_user_by_id(id).await,
            Self::Sqlite(db) => db.get_user_by_id(id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_user_by_id(id).await,
        }
    }
    async fn get_password_hash(&self, email: &str) -> anyhow::Result<Option<String>> {
        match self {
            Self::Postgres(db) => db.get_password_hash(email).await,
            Self::Sqlite(db) => db.get_password_hash(email).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_password_hash(email).await,
        }
    }

    // -- refresh tokens --
    async fn store_refresh_token(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> anyhow::Result<Uuid> {
        match self {
            Self::Postgres(db) => {
                db.store_refresh_token(user_id, token_hash, expires_at)
                    .await
            }
            Self::Sqlite(db) => {
                db.store_refresh_token(user_id, token_hash, expires_at)
                    .await
            }
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => {
                db.store_refresh_token(user_id, token_hash, expires_at)
                    .await
            }
        }
    }
    async fn validate_refresh_token(
        &self,
        token_hash: &str,
    ) -> anyhow::Result<Option<(Uuid, Uuid)>> {
        match self {
            Self::Postgres(db) => db.validate_refresh_token(token_hash).await,
            Self::Sqlite(db) => db.validate_refresh_token(token_hash).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.validate_refresh_token(token_hash).await,
        }
    }
    async fn delete_refresh_token(&self, token_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.delete_refresh_token(token_id).await,
            Self::Sqlite(db) => db.delete_refresh_token(token_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.delete_refresh_token(token_id).await,
        }
    }

    async fn consume_refresh_token(&self, token_hash: &str) -> anyhow::Result<Option<Uuid>> {
        match self {
            Self::Postgres(db) => db.consume_refresh_token(token_hash).await,
            Self::Sqlite(db) => db.consume_refresh_token(token_hash).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.consume_refresh_token(token_hash).await,
        }
    }

    // -- oauth --
    async fn upsert_oauth_connection(
        &self,
        user_id: Uuid,
        provider: &str,
        provider_user_id: &str,
    ) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => {
                db.upsert_oauth_connection(user_id, provider, provider_user_id)
                    .await
            }
            Self::Sqlite(db) => {
                db.upsert_oauth_connection(user_id, provider, provider_user_id)
                    .await
            }
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => {
                db.upsert_oauth_connection(user_id, provider, provider_user_id)
                    .await
            }
        }
    }
    async fn get_user_by_oauth(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> anyhow::Result<Option<User>> {
        match self {
            Self::Postgres(db) => db.get_user_by_oauth(provider, provider_user_id).await,
            Self::Sqlite(db) => db.get_user_by_oauth(provider, provider_user_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_user_by_oauth(provider, provider_user_id).await,
        }
    }

    // -- api keys --
    async fn create_api_key(
        &self,
        user_id: Uuid,
        key_hash: &str,
        label: &str,
    ) -> anyhow::Result<Uuid> {
        match self {
            Self::Postgres(db) => db.create_api_key(user_id, key_hash, label).await,
            Self::Sqlite(db) => db.create_api_key(user_id, key_hash, label).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.create_api_key(user_id, key_hash, label).await,
        }
    }
    async fn validate_api_key(&self, key_hash: &str) -> anyhow::Result<Option<User>> {
        match self {
            Self::Postgres(db) => db.validate_api_key(key_hash).await,
            Self::Sqlite(db) => db.validate_api_key(key_hash).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.validate_api_key(key_hash).await,
        }
    }
    async fn list_api_keys(&self, user_id: Uuid) -> anyhow::Result<Vec<ApiKeyInfo>> {
        match self {
            Self::Postgres(db) => db.list_api_keys(user_id).await,
            Self::Sqlite(db) => db.list_api_keys(user_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_api_keys(user_id).await,
        }
    }
    async fn revoke_api_key(&self, key_id: Uuid, user_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.revoke_api_key(key_id, user_id).await,
            Self::Sqlite(db) => db.revoke_api_key(key_id, user_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.revoke_api_key(key_id, user_id).await,
        }
    }

    // -- teams --
    async fn create_team(
        &self,
        name: &str,
        description: &str,
        created_by: Uuid,
    ) -> anyhow::Result<Uuid> {
        match self {
            Self::Postgres(db) => db.create_team(name, description, created_by).await,
            Self::Sqlite(db) => db.create_team(name, description, created_by).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.create_team(name, description, created_by).await,
        }
    }
    async fn get_team(&self, team_id: Uuid) -> anyhow::Result<Option<Team>> {
        match self {
            Self::Postgres(db) => db.get_team(team_id).await,
            Self::Sqlite(db) => db.get_team(team_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_team(team_id).await,
        }
    }
    async fn list_teams_for_user(&self, user_id: Uuid) -> anyhow::Result<Vec<Team>> {
        match self {
            Self::Postgres(db) => db.list_teams_for_user(user_id).await,
            Self::Sqlite(db) => db.list_teams_for_user(user_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_teams_for_user(user_id).await,
        }
    }
    async fn update_team(
        &self,
        team_id: Uuid,
        name: &str,
        description: &str,
    ) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.update_team(team_id, name, description).await,
            Self::Sqlite(db) => db.update_team(team_id, name, description).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.update_team(team_id, name, description).await,
        }
    }
    async fn delete_team(&self, team_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.delete_team(team_id).await,
            Self::Sqlite(db) => db.delete_team(team_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.delete_team(team_id).await,
        }
    }

    // -- team members --
    async fn add_team_member(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.add_team_member(team_id, user_id, role).await,
            Self::Sqlite(db) => db.add_team_member(team_id, user_id, role).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.add_team_member(team_id, user_id, role).await,
        }
    }
    async fn remove_team_member(&self, team_id: Uuid, user_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.remove_team_member(team_id, user_id).await,
            Self::Sqlite(db) => db.remove_team_member(team_id, user_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.remove_team_member(team_id, user_id).await,
        }
    }
    async fn list_team_members(&self, team_id: Uuid) -> anyhow::Result<Vec<TeamMember>> {
        match self {
            Self::Postgres(db) => db.list_team_members(team_id).await,
            Self::Sqlite(db) => db.list_team_members(team_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_team_members(team_id).await,
        }
    }
    async fn get_team_member_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> anyhow::Result<Option<String>> {
        match self {
            Self::Postgres(db) => db.get_team_member_role(team_id, user_id).await,
            Self::Sqlite(db) => db.get_team_member_role(team_id, user_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_team_member_role(team_id, user_id).await,
        }
    }
    async fn update_team_member_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.update_team_member_role(team_id, user_id, role).await,
            Self::Sqlite(db) => db.update_team_member_role(team_id, user_id, role).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.update_team_member_role(team_id, user_id, role).await,
        }
    }

    // -- team-scoped sessions --
    async fn create_session_for_team(
        &self,
        description: &str,
        team_id: Uuid,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)> {
        match self {
            Self::Postgres(db) => db.create_session_for_team(description, team_id).await,
            Self::Sqlite(db) => db.create_session_for_team(description, team_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.create_session_for_team(description, team_id).await,
        }
    }
    async fn list_sessions_for_team(
        &self,
        team_id: Uuid,
    ) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String)>> {
        match self {
            Self::Postgres(db) => db.list_sessions_for_team(team_id).await,
            Self::Sqlite(db) => db.list_sessions_for_team(team_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_sessions_for_team(team_id).await,
        }
    }
    async fn get_session_team(&self, session_id: Uuid) -> anyhow::Result<Option<Uuid>> {
        match self {
            Self::Postgres(db) => db.get_session_team(session_id).await,
            Self::Sqlite(db) => db.get_session_team(session_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_session_team(session_id).await,
        }
    }

    // -- user dashboard --
    async fn list_sessions_for_user(
        &self,
        user_id: Uuid,
    ) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String, Option<String>)>> {
        match self {
            Self::Postgres(db) => db.list_sessions_for_user(user_id).await,
            Self::Sqlite(db) => db.list_sessions_for_user(user_id).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_sessions_for_user(user_id).await,
        }
    }

    // -- Phase 4: admin & observability --
    async fn get_usage_summary(&self) -> anyhow::Result<Vec<SessionUsage>> {
        match self {
            Self::Postgres(db) => db.get_usage_summary().await,
            Self::Sqlite(db) => db.get_usage_summary().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_usage_summary().await,
        }
    }
    async fn get_global_usage_summary(&self) -> anyhow::Result<GlobalUsage> {
        match self {
            Self::Postgres(db) => db.get_global_usage_summary().await,
            Self::Sqlite(db) => db.get_global_usage_summary().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_global_usage_summary().await,
        }
    }
    async fn get_run_analytics(&self) -> anyhow::Result<RunAnalytics> {
        match self {
            Self::Postgres(db) => db.get_run_analytics().await,
            Self::Sqlite(db) => db.get_run_analytics().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_run_analytics().await,
        }
    }
    async fn list_all_sessions_admin(&self) -> anyhow::Result<Vec<AdminSessionInfo>> {
        match self {
            Self::Postgres(db) => db.list_all_sessions_admin().await,
            Self::Sqlite(db) => db.list_all_sessions_admin().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_all_sessions_admin().await,
        }
    }
    async fn update_session_quota(
        &self,
        session_id: Uuid,
        daily_token_cap: i64,
    ) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.update_session_quota(session_id, daily_token_cap).await,
            Self::Sqlite(db) => db.update_session_quota(session_id, daily_token_cap).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.update_session_quota(session_id, daily_token_cap).await,
        }
    }
    async fn list_all_users(&self) -> anyhow::Result<Vec<User>> {
        match self {
            Self::Postgres(db) => db.list_all_users().await,
            Self::Sqlite(db) => db.list_all_users().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_all_users().await,
        }
    }
    async fn update_user_role(&self, user_id: Uuid, role: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.update_user_role(user_id, role).await,
            Self::Sqlite(db) => db.update_user_role(user_id, role).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.update_user_role(user_id, role).await,
        }
    }
    async fn insert_audit_event(
        &self,
        user_id: Option<Uuid>,
        action: &str,
        target_type: &str,
        target_id: Option<&str>,
        details: Option<Value>,
        ip_address: Option<&str>,
    ) -> anyhow::Result<i64> {
        match self {
            Self::Postgres(db) => {
                db.insert_audit_event(user_id, action, target_type, target_id, details, ip_address)
                    .await
            }
            Self::Sqlite(db) => {
                db.insert_audit_event(user_id, action, target_type, target_id, details, ip_address)
                    .await
            }
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => {
                db.insert_audit_event(user_id, action, target_type, target_id, details, ip_address)
                    .await
            }
        }
    }
    async fn list_audit_events(&self, limit: i64, offset: i64) -> anyhow::Result<Vec<AuditEvent>> {
        match self {
            Self::Postgres(db) => db.list_audit_events(limit, offset).await,
            Self::Sqlite(db) => db.list_audit_events(limit, offset).await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.list_audit_events(limit, offset).await,
        }
    }
    async fn get_metrics_data(&self) -> anyhow::Result<MetricsData> {
        match self {
            Self::Postgres(db) => db.get_metrics_data().await,
            Self::Sqlite(db) => db.get_metrics_data().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.get_metrics_data().await,
        }
    }

    async fn subscribe_notifications(&self) -> anyhow::Result<NotificationRx> {
        match self {
            Self::Postgres(db) => db.subscribe_notifications().await,
            Self::Sqlite(db) => db.subscribe_notifications().await,
            #[cfg(feature = "mssql")]
            Self::Mssql(db) => db.subscribe_notifications().await,
        }
    }
}
