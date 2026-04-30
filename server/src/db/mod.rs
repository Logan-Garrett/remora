pub mod postgres;
pub mod sqlite;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use remora_common::Event;
use serde_json::Value;
use uuid::Uuid;

/// Notification receiver that yields event IDs as they are inserted.
pub type NotificationRx = tokio::sync::mpsc::UnboundedReceiver<i64>;

/// The set of operations every database backend must support.
#[async_trait]
pub trait Database: Send + Sync + 'static {
    // -- migrations --
    async fn run_migrations(&self) -> anyhow::Result<()>;

    // -- sessions --
    async fn create_session(
        &self,
        description: &str,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)>;

    async fn list_sessions(&self) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>)>>;

    async fn delete_session(&self, session_id: Uuid) -> anyhow::Result<u64>;

    async fn session_exists(&self, session_id: Uuid) -> anyhow::Result<bool>;

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

    // -- notifications --
    /// Start listening for new-event notifications.  Returns a receiver
    /// that yields event IDs.
    async fn subscribe_notifications(&self) -> anyhow::Result<NotificationRx>;
}

/// The concrete backend enum. We dispatch through the trait via `Arc<dyn Database>`.
pub enum DatabaseBackend {
    Postgres(postgres::PostgresDb),
    Sqlite(sqlite::SqliteDb),
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
        other => anyhow::bail!("unsupported REMORA_DB_PROVIDER: {other}"),
    }
}

// Implement the trait for the enum so we can use it directly.
#[async_trait]
impl Database for DatabaseBackend {
    async fn run_migrations(&self) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.run_migrations().await,
            Self::Sqlite(db) => db.run_migrations().await,
        }
    }

    async fn create_session(
        &self,
        description: &str,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)> {
        match self {
            Self::Postgres(db) => db.create_session(description).await,
            Self::Sqlite(db) => db.create_session(description).await,
        }
    }

    async fn list_sessions(&self) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>)>> {
        match self {
            Self::Postgres(db) => db.list_sessions().await,
            Self::Sqlite(db) => db.list_sessions().await,
        }
    }

    async fn delete_session(&self, session_id: Uuid) -> anyhow::Result<u64> {
        match self {
            Self::Postgres(db) => db.delete_session(session_id).await,
            Self::Sqlite(db) => db.delete_session(session_id).await,
        }
    }

    async fn session_exists(&self, session_id: Uuid) -> anyhow::Result<bool> {
        match self {
            Self::Postgres(db) => db.session_exists(session_id).await,
            Self::Sqlite(db) => db.session_exists(session_id).await,
        }
    }

    async fn get_session_info(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<Option<(String, DateTime<Utc>, i64, i64)>> {
        match self {
            Self::Postgres(db) => db.get_session_info(session_id).await,
            Self::Sqlite(db) => db.get_session_info(session_id).await,
        }
    }

    async fn set_idle_since_now(&self, session_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.set_idle_since_now(session_id).await,
            Self::Sqlite(db) => db.set_idle_since_now(session_id).await,
        }
    }

    async fn clear_idle_since(&self, session_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.clear_idle_since(session_id).await,
            Self::Sqlite(db) => db.clear_idle_since(session_id).await,
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
        }
    }

    async fn get_event_by_id(&self, event_id: i64) -> anyhow::Result<Option<Event>> {
        match self {
            Self::Postgres(db) => db.get_event_by_id(event_id).await,
            Self::Sqlite(db) => db.get_event_by_id(event_id).await,
        }
    }

    async fn get_events_for_session(&self, session_id: Uuid) -> anyhow::Result<Vec<Event>> {
        match self {
            Self::Postgres(db) => db.get_events_for_session(session_id).await,
            Self::Sqlite(db) => db.get_events_for_session(session_id).await,
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
        }
    }

    async fn get_last_context_boundary(&self, session_id: Uuid) -> anyhow::Result<i64> {
        match self {
            Self::Postgres(db) => db.get_last_context_boundary(session_id).await,
            Self::Sqlite(db) => db.get_last_context_boundary(session_id).await,
        }
    }

    async fn upsert_repo(&self, session_id: Uuid, name: &str, git_url: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.upsert_repo(session_id, name, git_url).await,
            Self::Sqlite(db) => db.upsert_repo(session_id, name, git_url).await,
        }
    }

    async fn delete_repo(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.delete_repo(session_id, name).await,
            Self::Sqlite(db) => db.delete_repo(session_id, name).await,
        }
    }

    async fn list_repos(&self, session_id: Uuid) -> anyhow::Result<Vec<(String, String)>> {
        match self {
            Self::Postgres(db) => db.list_repos(session_id).await,
            Self::Sqlite(db) => db.list_repos(session_id).await,
        }
    }

    async fn list_repo_names(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        match self {
            Self::Postgres(db) => db.list_repo_names(session_id).await,
            Self::Sqlite(db) => db.list_repo_names(session_id).await,
        }
    }

    async fn insert_run(&self, session_id: Uuid, context_mode: &str) -> anyhow::Result<i64> {
        match self {
            Self::Postgres(db) => db.insert_run(session_id, context_mode).await,
            Self::Sqlite(db) => db.insert_run(session_id, context_mode).await,
        }
    }

    async fn finish_run(&self, run_id: i64, status: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.finish_run(run_id, status).await,
            Self::Sqlite(db) => db.finish_run(run_id, status).await,
        }
    }

    async fn is_run_in_flight(&self, session_id: Uuid) -> anyhow::Result<bool> {
        match self {
            Self::Postgres(db) => db.is_run_in_flight(session_id).await,
            Self::Sqlite(db) => db.is_run_in_flight(session_id).await,
        }
    }

    async fn list_global_allowlist(&self) -> anyhow::Result<Vec<(String, String)>> {
        match self {
            Self::Postgres(db) => db.list_global_allowlist().await,
            Self::Sqlite(db) => db.list_global_allowlist().await,
        }
    }

    async fn list_session_allowlist(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        match self {
            Self::Postgres(db) => db.list_session_allowlist(session_id).await,
            Self::Sqlite(db) => db.list_session_allowlist(session_id).await,
        }
    }

    async fn add_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.add_session_allowlist(session_id, domain).await,
            Self::Sqlite(db) => db.add_session_allowlist(session_id, domain).await,
        }
    }

    async fn remove_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.remove_session_allowlist(session_id, domain).await,
            Self::Sqlite(db) => db.remove_session_allowlist(session_id, domain).await,
        }
    }

    async fn is_domain_blocked(&self, domain: &str) -> anyhow::Result<bool> {
        match self {
            Self::Postgres(db) => db.is_domain_blocked(domain).await,
            Self::Sqlite(db) => db.is_domain_blocked(domain).await,
        }
    }

    async fn is_domain_global_allowed(&self, domain: &str) -> anyhow::Result<bool> {
        match self {
            Self::Postgres(db) => db.is_domain_global_allowed(domain).await,
            Self::Sqlite(db) => db.is_domain_global_allowed(domain).await,
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
        }
    }

    async fn reset_tokens_if_needed(&self, session_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.reset_tokens_if_needed(session_id).await,
            Self::Sqlite(db) => db.reset_tokens_if_needed(session_id).await,
        }
    }

    async fn get_session_usage(&self, session_id: Uuid) -> anyhow::Result<(i64, i64)> {
        match self {
            Self::Postgres(db) => db.get_session_usage(session_id).await,
            Self::Sqlite(db) => db.get_session_usage(session_id).await,
        }
    }

    async fn get_global_usage(&self) -> anyhow::Result<i64> {
        match self {
            Self::Postgres(db) => db.get_global_usage().await,
            Self::Sqlite(db) => db.get_global_usage().await,
        }
    }

    async fn add_usage(&self, session_id: Uuid, tokens: i64) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.add_usage(session_id, tokens).await,
            Self::Sqlite(db) => db.add_usage(session_id, tokens).await,
        }
    }

    async fn get_idle_sessions(&self, idle_timeout_secs: u64) -> anyhow::Result<Vec<Uuid>> {
        match self {
            Self::Postgres(db) => db.get_idle_sessions(idle_timeout_secs).await,
            Self::Sqlite(db) => db.get_idle_sessions(idle_timeout_secs).await,
        }
    }

    async fn clear_idle_since_for(&self, session_id: Uuid) -> anyhow::Result<()> {
        match self {
            Self::Postgres(db) => db.clear_idle_since_for(session_id).await,
            Self::Sqlite(db) => db.clear_idle_since_for(session_id).await,
        }
    }

    async fn subscribe_notifications(&self) -> anyhow::Result<NotificationRx> {
        match self {
            Self::Postgres(db) => db.subscribe_notifications().await,
            Self::Sqlite(db) => db.subscribe_notifications().await,
        }
    }
}
