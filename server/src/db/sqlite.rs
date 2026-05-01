use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use remora_common::Event;
use serde_json::Value;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::str::FromStr;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::{Database, NotificationRx};

pub struct SqliteDb {
    pool: SqlitePool,
    /// In-process notification bus (replaces Postgres LISTEN/NOTIFY).
    notify_tx: broadcast::Sender<i64>,
}

impl SqliteDb {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let opts = sqlx::sqlite::SqliteConnectOptions::from_str(url)?.create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;

        // Enable WAL mode for better concurrent access
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await?;

        let (notify_tx, _) = broadcast::channel(1024);
        Ok(Self { pool, notify_tx })
    }

    /// Emit a notification for a new event id (in-process replacement for pg_notify).
    fn notify(&self, event_id: i64) {
        let _ = self.notify_tx.send(event_id);
    }
}

#[async_trait]
impl Database for SqliteDb {
    async fn ping(&self) -> anyhow::Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    async fn run_migrations(&self) -> anyhow::Result<()> {
        sqlx::migrate!("../migrations/sqlite")
            .run(&self.pool)
            .await?;
        Ok(())
    }

    // -- sessions --

    async fn create_session(
        &self,
        description: &str,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let id_str = id.to_string();
        let now_str = now.to_rfc3339();
        let today = Utc::now().date_naive().to_string();

        sqlx::query(
            "INSERT INTO sessions (id, description, created_at, updated_at, \
             daily_token_cap, tokens_used_today, tokens_reset_date) \
             VALUES (?, ?, ?, ?, 999999999, 0, ?)",
        )
        .bind(&id_str)
        .bind(description)
        .bind(&now_str)
        .bind(&now_str)
        .bind(&today)
        .execute(&self.pool)
        .await?;

        Ok((id, description.to_string(), now))
    }

    async fn list_sessions(&self) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>)>> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, description, created_at FROM sessions ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|(id, desc, ts)| {
                let uuid = id.parse::<Uuid>()?;
                let dt = DateTime::parse_from_rfc3339(&ts)
                    .map(|d| d.with_timezone(&Utc))
                    .or_else(|_| ts.parse::<NaiveDateTime>().map(|nd| nd.and_utc()))?;
                Ok((uuid, desc, dt))
            })
            .collect()
    }

    async fn delete_session(&self, session_id: Uuid) -> anyhow::Result<u64> {
        let id_str = session_id.to_string();
        let result = sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(&id_str)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    async fn session_exists(&self, session_id: Uuid) -> anyhow::Result<bool> {
        let id_str = session_id.to_string();
        let row: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM sessions WHERE id = ?")
            .bind(&id_str)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0 > 0)
    }

    async fn get_session_status(&self, session_id: Uuid) -> anyhow::Result<Option<String>> {
        let id_str = session_id.to_string();
        let row: Option<(String,)> = sqlx::query_as("SELECT status FROM sessions WHERE id = ?")
            .bind(&id_str)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(s,)| s))
    }

    async fn set_session_expired(&self, session_id: Uuid) -> anyhow::Result<()> {
        let id_str = session_id.to_string();
        sqlx::query("UPDATE sessions SET status = 'expired' WHERE id = ?")
            .bind(&id_str)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn count_sessions(&self) -> anyhow::Result<i64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    async fn get_session_info(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<Option<(String, DateTime<Utc>, i64, i64)>> {
        let id_str = session_id.to_string();
        let row = sqlx::query_as::<_, (String, String, i64, i64)>(
            "SELECT description, created_at, \
             COALESCE(tokens_used_today, 0), COALESCE(daily_token_cap, 1000000) \
             FROM sessions WHERE id = ?",
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((desc, ts, used, cap)) => {
                let dt = DateTime::parse_from_rfc3339(&ts)
                    .map(|d| d.with_timezone(&Utc))
                    .or_else(|_| ts.parse::<NaiveDateTime>().map(|nd| nd.and_utc()))?;
                Ok(Some((desc, dt, used, cap)))
            }
            None => Ok(None),
        }
    }

    async fn set_idle_since_now(&self, session_id: Uuid) -> anyhow::Result<()> {
        let id_str = session_id.to_string();
        let now_str = Utc::now().to_rfc3339();
        sqlx::query("UPDATE sessions SET idle_since = ? WHERE id = ?")
            .bind(&now_str)
            .bind(&id_str)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn clear_idle_since(&self, session_id: Uuid) -> anyhow::Result<()> {
        let id_str = session_id.to_string();
        sqlx::query("UPDATE sessions SET idle_since = NULL WHERE id = ?")
            .bind(&id_str)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- events --

    async fn insert_event(
        &self,
        session_id: Uuid,
        author: &str,
        kind: &str,
        payload: Value,
    ) -> anyhow::Result<i64> {
        let sid_str = session_id.to_string();
        let now_str = Utc::now().to_rfc3339();
        let payload_str = serde_json::to_string(&payload)?;

        let id: (i64,) = sqlx::query_as(
            "INSERT INTO events (session_id, timestamp, author, kind, payload) \
             VALUES (?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(&sid_str)
        .bind(&now_str)
        .bind(author)
        .bind(kind)
        .bind(&payload_str)
        .fetch_one(&self.pool)
        .await?;

        self.notify(id.0);
        Ok(id.0)
    }

    async fn get_event_by_id(&self, event_id: i64) -> anyhow::Result<Option<Event>> {
        let row = sqlx::query_as::<_, (i64, String, String, Option<String>, String, String)>(
            "SELECT id, session_id, timestamp, author, kind, payload FROM events WHERE id = ?",
        )
        .bind(event_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((id, sid, ts, author, kind, payload_str)) => {
                let session_id = sid.parse::<Uuid>()?;
                let timestamp = DateTime::parse_from_rfc3339(&ts)
                    .map(|d| d.with_timezone(&Utc))
                    .or_else(|_| ts.parse::<NaiveDateTime>().map(|nd| nd.and_utc()))?;
                let payload: Value = serde_json::from_str(&payload_str)?;
                Ok(Some(Event {
                    id,
                    session_id,
                    timestamp,
                    author,
                    kind,
                    payload,
                }))
            }
            None => Ok(None),
        }
    }

    async fn get_events_for_session(&self, session_id: Uuid) -> anyhow::Result<Vec<Event>> {
        let sid_str = session_id.to_string();
        let rows = sqlx::query_as::<_, (i64, String, String, Option<String>, String, String)>(
            "SELECT id, session_id, timestamp, author, kind, payload \
             FROM events WHERE session_id = ? ORDER BY id",
        )
        .bind(&sid_str)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|(id, sid, ts, author, kind, payload_str)| {
                let session_id = sid.parse::<Uuid>()?;
                let timestamp = DateTime::parse_from_rfc3339(&ts)
                    .map(|d| d.with_timezone(&Utc))
                    .or_else(|_| ts.parse::<NaiveDateTime>().map(|nd| nd.and_utc()))?;
                let payload: Value = serde_json::from_str(&payload_str)?;
                Ok(Event {
                    id,
                    session_id,
                    timestamp,
                    author,
                    kind,
                    payload,
                })
            })
            .collect()
    }

    async fn get_recent_events_for_session(
        &self,
        session_id: Uuid,
        limit: i64,
    ) -> anyhow::Result<Vec<Event>> {
        let sid_str = session_id.to_string();
        let rows = sqlx::query_as::<_, (i64, String, String, Option<String>, String, String)>(
            "SELECT id, session_id, timestamp, author, kind, payload \
             FROM (SELECT id, session_id, timestamp, author, kind, payload \
                   FROM events WHERE session_id = ? ORDER BY id DESC LIMIT ?) \
             ORDER BY id",
        )
        .bind(&sid_str)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|(id, sid, ts, author, kind, payload_str)| {
                let session_id = sid.parse::<Uuid>()?;
                let timestamp = DateTime::parse_from_rfc3339(&ts)
                    .map(|d| d.with_timezone(&Utc))
                    .or_else(|_| ts.parse::<NaiveDateTime>().map(|nd| nd.and_utc()))?;
                let payload: Value = serde_json::from_str(&payload_str)?;
                Ok(Event {
                    id,
                    session_id,
                    timestamp,
                    author,
                    kind,
                    payload,
                })
            })
            .collect()
    }

    async fn get_events_since(
        &self,
        session_id: Uuid,
        since_id: i64,
    ) -> anyhow::Result<Vec<(i64, Option<String>, String, Value)>> {
        let sid_str = session_id.to_string();
        let rows = sqlx::query_as::<_, (i64, Option<String>, String, String)>(
            "SELECT id, author, kind, payload FROM events \
             WHERE session_id = ? AND id > ? ORDER BY id",
        )
        .bind(&sid_str)
        .bind(since_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|(id, author, kind, payload_str)| {
                let payload: Value = serde_json::from_str(&payload_str)?;
                Ok((id, author, kind, payload))
            })
            .collect()
    }

    async fn get_last_context_boundary(&self, session_id: Uuid) -> anyhow::Result<i64> {
        let sid_str = session_id.to_string();
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(
                (SELECT MAX(id) FROM events
                 WHERE session_id = ? AND kind IN ('claude_response', 'clear_marker')),
                0
            )",
        )
        .bind(&sid_str)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    // -- repos --

    async fn upsert_repo(&self, session_id: Uuid, name: &str, git_url: &str) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        sqlx::query(
            "INSERT INTO session_repos (session_id, name, git_url) VALUES (?, ?, ?) \
             ON CONFLICT (session_id, name) DO UPDATE SET git_url = excluded.git_url",
        )
        .bind(&sid_str)
        .bind(name)
        .bind(git_url)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_repo(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        sqlx::query("DELETE FROM session_repos WHERE session_id = ? AND name = ?")
            .bind(&sid_str)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_repos(&self, session_id: Uuid) -> anyhow::Result<Vec<(String, String)>> {
        let sid_str = session_id.to_string();
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT name, git_url FROM session_repos WHERE session_id = ? ORDER BY name",
        )
        .bind(&sid_str)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn list_repo_names(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        let sid_str = session_id.to_string();
        let rows =
            sqlx::query_as::<_, (String,)>("SELECT name FROM session_repos WHERE session_id = ?")
                .bind(&sid_str)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }

    // -- runs --

    async fn insert_run(&self, session_id: Uuid, context_mode: &str) -> anyhow::Result<i64> {
        let sid_str = session_id.to_string();
        let now_str = Utc::now().to_rfc3339();
        let row: Option<(i64,)> = sqlx::query_as(
            "INSERT INTO session_runs (session_id, started_at, status, heartbeat, context_mode) \
             SELECT ?, ?, 'running', ?, ? \
             WHERE NOT EXISTS (SELECT 1 FROM session_runs WHERE session_id = ? AND status = 'running') \
             RETURNING id",
        )
        .bind(&sid_str)
        .bind(&now_str)
        .bind(&now_str)
        .bind(context_mode)
        .bind(&sid_str)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(id,)| id)
            .ok_or_else(|| anyhow::anyhow!("A run is already in progress for this session"))
    }

    async fn finish_run(&self, run_id: i64, status: &str) -> anyhow::Result<()> {
        let now_str = Utc::now().to_rfc3339();
        sqlx::query("UPDATE session_runs SET status = ?, finished_at = ? WHERE id = ?")
            .bind(status)
            .bind(&now_str)
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn is_run_in_flight(&self, session_id: Uuid) -> anyhow::Result<bool> {
        let sid_str = session_id.to_string();
        let row: (i32,) = sqlx::query_as(
            "SELECT COUNT(*) FROM session_runs WHERE session_id = ? AND status = 'running'",
        )
        .bind(&sid_str)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }

    // -- allowlists --

    async fn list_global_allowlist(&self) -> anyhow::Result<Vec<(String, String)>> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT domain, kind FROM global_allowlist ORDER BY domain",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn list_session_allowlist(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        let sid_str = session_id.to_string();
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT domain FROM session_allowlist WHERE session_id = ? ORDER BY domain",
        )
        .bind(&sid_str)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(d,)| d).collect())
    }

    async fn add_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        let now_str = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO session_allowlist (session_id, domain, approved_at) VALUES (?, ?, ?) \
             ON CONFLICT DO NOTHING",
        )
        .bind(&sid_str)
        .bind(domain)
        .bind(&now_str)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn remove_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        sqlx::query("DELETE FROM session_allowlist WHERE session_id = ? AND domain = ?")
            .bind(&sid_str)
            .bind(domain)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn is_domain_blocked(&self, domain: &str) -> anyhow::Result<bool> {
        let row: (i32,) = sqlx::query_as(
            "SELECT COUNT(*) FROM global_allowlist WHERE domain = ? AND kind = 'block'",
        )
        .bind(domain)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }

    async fn is_domain_global_allowed(&self, domain: &str) -> anyhow::Result<bool> {
        let row: (i32,) = sqlx::query_as(
            "SELECT COUNT(*) FROM global_allowlist WHERE domain = ? AND kind = 'allow'",
        )
        .bind(domain)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }

    async fn is_domain_session_allowed(
        &self,
        session_id: Uuid,
        domain: &str,
    ) -> anyhow::Result<bool> {
        let sid_str = session_id.to_string();
        let row: (i32,) = sqlx::query_as(
            "SELECT COUNT(*) FROM session_allowlist WHERE session_id = ? AND domain = ?",
        )
        .bind(&sid_str)
        .bind(domain)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }

    // -- pending approvals --

    async fn create_pending_approval(
        &self,
        session_id: Uuid,
        domain: &str,
        url: &str,
        requested_by: &str,
    ) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        let now_str = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO pending_approvals (session_id, domain, url, requested_by, requested_at, resolved, approved) \
             VALUES (?, ?, ?, ?, ?, 0, NULL)",
        )
        .bind(&sid_str)
        .bind(domain)
        .bind(url)
        .bind(requested_by)
        .bind(&now_str)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn resolve_approval(
        &self,
        session_id: Uuid,
        domain: &str,
        approved: bool,
    ) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        sqlx::query(
            "UPDATE pending_approvals SET resolved = 1, approved = ? \
             WHERE session_id = ? AND domain = ? AND resolved = 0",
        )
        .bind(approved)
        .bind(&sid_str)
        .bind(domain)
        .execute(&self.pool)
        .await?;

        if approved {
            let now_str = Utc::now().to_rfc3339();
            sqlx::query(
                "INSERT INTO session_allowlist (session_id, domain, approved_at) VALUES (?, ?, ?) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(&sid_str)
            .bind(domain)
            .bind(&now_str)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn get_approved_pending(
        &self,
        session_id: Uuid,
        domain: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        let sid_str = session_id.to_string();
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT url, requested_by FROM pending_approvals \
             WHERE session_id = ? AND domain = ? AND approved = 1",
        )
        .bind(&sid_str)
        .bind(domain)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // -- quotas --

    async fn reset_tokens_if_needed(&self, session_id: Uuid) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        let today = Utc::now().date_naive().to_string();
        sqlx::query(
            "UPDATE sessions SET tokens_used_today = 0, tokens_reset_date = ? \
             WHERE id = ? AND tokens_reset_date < ?",
        )
        .bind(&today)
        .bind(&sid_str)
        .bind(&today)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_session_usage(&self, session_id: Uuid) -> anyhow::Result<(i64, i64)> {
        let sid_str = session_id.to_string();
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT COALESCE(tokens_used_today, 0), COALESCE(daily_token_cap, 1000000) \
             FROM sessions WHERE id = ?",
        )
        .bind(&sid_str)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn get_global_usage(&self) -> anyhow::Result<i64> {
        let today = Utc::now().date_naive().to_string();
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(tokens_used_today), 0) FROM sessions \
             WHERE tokens_reset_date = ?",
        )
        .bind(&today)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn add_usage(&self, session_id: Uuid, tokens: i64) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        sqlx::query("UPDATE sessions SET tokens_used_today = tokens_used_today + ? WHERE id = ?")
            .bind(tokens)
            .bind(&sid_str)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_idle_sessions(&self, idle_timeout_secs: u64) -> anyhow::Result<Vec<Uuid>> {
        let cutoff = Utc::now() - chrono::Duration::seconds(idle_timeout_secs as i64);
        let cutoff_str = cutoff.to_rfc3339();
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT id FROM sessions WHERE idle_since IS NOT NULL AND idle_since < ?",
        )
        .bind(&cutoff_str)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(id,)| Ok(id.parse::<Uuid>()?))
            .collect()
    }

    async fn clear_idle_since_for(&self, session_id: Uuid) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        sqlx::query("UPDATE sessions SET idle_since = NULL WHERE id = ?")
            .bind(&sid_str)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- owner key --

    async fn set_owner_key(&self, session_id: Uuid, key: &str) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        sqlx::query("UPDATE sessions SET owner_key = ? WHERE id = ?")
            .bind(key)
            .bind(&sid_str)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_owner_key(&self, session_id: Uuid) -> anyhow::Result<Option<String>> {
        let sid_str = session_id.to_string();
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT owner_key FROM sessions WHERE id = ?")
                .bind(&sid_str)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|(k,)| k))
    }

    // -- trusted participants --

    async fn trust_participant(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        let now_str = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO session_trusted (session_id, participant_name, added_at) VALUES (?, ?, ?) \
             ON CONFLICT DO NOTHING",
        )
        .bind(&sid_str)
        .bind(name)
        .bind(&now_str)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn untrust_participant(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        let sid_str = session_id.to_string();
        sqlx::query("DELETE FROM session_trusted WHERE session_id = ? AND participant_name = ?")
            .bind(&sid_str)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_trusted_participants(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        let sid_str = session_id.to_string();
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT participant_name FROM session_trusted \
             WHERE session_id = ? ORDER BY participant_name",
        )
        .bind(&sid_str)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }

    // -- notifications --

    async fn subscribe_notifications(&self) -> anyhow::Result<NotificationRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut broadcast_rx = self.notify_tx.subscribe();
        tokio::spawn(async move {
            loop {
                match broadcast_rx.recv().await {
                    Ok(event_id) => {
                        if tx.send(event_id).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("sqlite notification subscriber lagged by {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        Ok(rx)
    }
}
