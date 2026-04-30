use async_trait::async_trait;
use chrono::{DateTime, Utc};
use remora_common::Event;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

use super::{Database, NotificationRx};

pub struct PostgresDb {
    pool: PgPool,
}

impl PostgresDb {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(url)
            .await?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl Database for PostgresDb {
    async fn ping(&self) -> anyhow::Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    async fn run_migrations(&self) -> anyhow::Result<()> {
        sqlx::migrate!("../migrations/postgres")
            .run(&self.pool)
            .await?;
        Ok(())
    }

    // -- sessions --

    async fn create_session(
        &self,
        description: &str,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)> {
        let row = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>)>(
            "INSERT INTO sessions (description) VALUES ($1) RETURNING id, description, created_at",
        )
        .bind(description)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn list_sessions(&self) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>)>> {
        let rows = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>)>(
            "SELECT id, description, created_at FROM sessions ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn delete_session(&self, session_id: Uuid) -> anyhow::Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    async fn session_exists(&self, session_id: Uuid) -> anyhow::Result<bool> {
        let exists =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM sessions WHERE id = $1)")
                .bind(session_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(exists)
    }

    async fn get_session_info(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<Option<(String, DateTime<Utc>, i64, i64)>> {
        let row = sqlx::query_as::<_, (String, DateTime<Utc>, i64, i64)>(
            "SELECT description, created_at, \
             COALESCE(tokens_used_today, 0), COALESCE(daily_token_cap, 1000000) \
             FROM sessions WHERE id = $1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn set_idle_since_now(&self, session_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET idle_since = now() WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn clear_idle_since(&self, session_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET idle_since = NULL WHERE id = $1")
            .bind(session_id)
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
        let id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO events (session_id, author, kind, payload) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(session_id)
        .bind(author)
        .bind(kind)
        .bind(payload)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    async fn get_event_by_id(&self, event_id: i64) -> anyhow::Result<Option<Event>> {
        let row = sqlx::query_as::<_, (i64, Uuid, DateTime<Utc>, Option<String>, String, Value)>(
            "SELECT id, session_id, timestamp, author, kind, payload FROM events WHERE id = $1",
        )
        .bind(event_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(
            row.map(|(id, session_id, timestamp, author, kind, payload)| Event {
                id,
                session_id,
                timestamp,
                author,
                kind,
                payload,
            }),
        )
    }

    async fn get_events_for_session(&self, session_id: Uuid) -> anyhow::Result<Vec<Event>> {
        let rows = sqlx::query_as::<_, (i64, Uuid, DateTime<Utc>, Option<String>, String, Value)>(
            "SELECT id, session_id, timestamp, author, kind, payload \
             FROM events WHERE session_id = $1 ORDER BY id",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, session_id, timestamp, author, kind, payload)| Event {
                id,
                session_id,
                timestamp,
                author,
                kind,
                payload,
            })
            .collect())
    }

    async fn get_events_since(
        &self,
        session_id: Uuid,
        since_id: i64,
    ) -> anyhow::Result<Vec<(i64, Option<String>, String, Value)>> {
        let rows = sqlx::query_as::<_, (i64, Option<String>, String, Value)>(
            "SELECT id, author, kind, payload FROM events \
             WHERE session_id = $1 AND id > $2 ORDER BY id",
        )
        .bind(session_id)
        .bind(since_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_last_context_boundary(&self, session_id: Uuid) -> anyhow::Result<i64> {
        let row = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(
                (SELECT MAX(id) FROM events
                 WHERE session_id = $1 AND kind IN ('claude_response', 'clear_marker')),
                0
            )",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    // -- repos --

    async fn upsert_repo(&self, session_id: Uuid, name: &str, git_url: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO session_repos (session_id, name, git_url) VALUES ($1, $2, $3) \
             ON CONFLICT (session_id, name) DO UPDATE SET git_url = $3",
        )
        .bind(session_id)
        .bind(name)
        .bind(git_url)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_repo(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM session_repos WHERE session_id = $1 AND name = $2")
            .bind(session_id)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_repos(&self, session_id: Uuid) -> anyhow::Result<Vec<(String, String)>> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT name, git_url FROM session_repos WHERE session_id = $1 ORDER BY name",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn list_repo_names(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        let rows =
            sqlx::query_as::<_, (String,)>("SELECT name FROM session_repos WHERE session_id = $1")
                .bind(session_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }

    // -- runs --

    async fn insert_run(&self, session_id: Uuid, context_mode: &str) -> anyhow::Result<i64> {
        let row = sqlx::query_scalar::<_, i64>(
            "INSERT INTO session_runs (session_id, status, context_mode) \
             SELECT $1, 'running', $2 \
             WHERE NOT EXISTS (SELECT 1 FROM session_runs WHERE session_id = $1 AND status = 'running') \
             RETURNING id",
        )
        .bind(session_id)
        .bind(context_mode)
        .fetch_optional(&self.pool)
        .await?;
        row.ok_or_else(|| anyhow::anyhow!("A run is already in progress for this session"))
    }

    async fn finish_run(&self, run_id: i64, status: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE session_runs SET status = $1, finished_at = now() WHERE id = $2")
            .bind(status)
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn is_run_in_flight(&self, session_id: Uuid) -> anyhow::Result<bool> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM session_runs WHERE session_id = $1 AND status = 'running')",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
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
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT domain FROM session_allowlist WHERE session_id = $1 ORDER BY domain",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(d,)| d).collect())
    }

    async fn add_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO session_allowlist (session_id, domain) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(session_id)
        .bind(domain)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn remove_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM session_allowlist WHERE session_id = $1 AND domain = $2")
            .bind(session_id)
            .bind(domain)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn is_domain_blocked(&self, domain: &str) -> anyhow::Result<bool> {
        let blocked = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM global_allowlist WHERE domain = $1 AND kind = 'block')",
        )
        .bind(domain)
        .fetch_one(&self.pool)
        .await?;
        Ok(blocked)
    }

    async fn is_domain_global_allowed(&self, domain: &str) -> anyhow::Result<bool> {
        let allowed = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM global_allowlist WHERE domain = $1 AND kind = 'allow')",
        )
        .bind(domain)
        .fetch_one(&self.pool)
        .await?;
        Ok(allowed)
    }

    async fn is_domain_session_allowed(
        &self,
        session_id: Uuid,
        domain: &str,
    ) -> anyhow::Result<bool> {
        let allowed = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM session_allowlist WHERE session_id = $1 AND domain = $2)",
        )
        .bind(session_id)
        .bind(domain)
        .fetch_one(&self.pool)
        .await?;
        Ok(allowed)
    }

    // -- pending approvals --

    async fn create_pending_approval(
        &self,
        session_id: Uuid,
        domain: &str,
        url: &str,
        requested_by: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO pending_approvals (session_id, domain, url, requested_by) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(session_id)
        .bind(domain)
        .bind(url)
        .bind(requested_by)
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
        sqlx::query(
            "UPDATE pending_approvals SET resolved = true, approved = $1 \
             WHERE session_id = $2 AND domain = $3 AND resolved = false",
        )
        .bind(approved)
        .bind(session_id)
        .bind(domain)
        .execute(&self.pool)
        .await?;

        if approved {
            sqlx::query(
                "INSERT INTO session_allowlist (session_id, domain) VALUES ($1, $2) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(session_id)
            .bind(domain)
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
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT url, requested_by FROM pending_approvals \
             WHERE session_id = $1 AND domain = $2 AND approved = true",
        )
        .bind(session_id)
        .bind(domain)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // -- quotas --

    async fn reset_tokens_if_needed(&self, session_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE sessions SET tokens_used_today = 0, tokens_reset_date = CURRENT_DATE \
             WHERE id = $1 AND tokens_reset_date < CURRENT_DATE",
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_session_usage(&self, session_id: Uuid) -> anyhow::Result<(i64, i64)> {
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT COALESCE(tokens_used_today, 0), COALESCE(daily_token_cap, 1000000) \
             FROM sessions WHERE id = $1",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn get_global_usage(&self) -> anyhow::Result<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(tokens_used_today), 0)::BIGINT FROM sessions \
             WHERE tokens_reset_date = CURRENT_DATE",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn add_usage(&self, session_id: Uuid, tokens: i64) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET tokens_used_today = tokens_used_today + $1 WHERE id = $2")
            .bind(tokens)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_idle_sessions(&self, idle_timeout_secs: u64) -> anyhow::Result<Vec<Uuid>> {
        let cutoff = Utc::now() - chrono::Duration::seconds(idle_timeout_secs as i64);
        let rows = sqlx::query_as::<_, (Uuid,)>(
            "SELECT id FROM sessions WHERE idle_since IS NOT NULL AND idle_since < $1",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    async fn clear_idle_since_for(&self, session_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET idle_since = NULL WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- notifications --

    async fn subscribe_notifications(&self) -> anyhow::Result<NotificationRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let result: Result<(), sqlx::Error> = async {
                let mut listener = sqlx::postgres::PgListener::connect_with(&pool).await?;
                listener.listen("new_event").await?;
                tracing::info!("pg listener started on channel 'new_event'");
                loop {
                    let notification = listener.recv().await?;
                    let event_id: i64 = match notification.payload().parse() {
                        Ok(id) => id,
                        Err(e) => {
                            tracing::warn!("bad notify payload: {e}");
                            continue;
                        }
                    };
                    if tx.send(event_id).is_err() {
                        break;
                    }
                }
                Ok(())
            }
            .await;
            if let Err(e) = result {
                tracing::error!("pg listener died: {e}");
            }
        });
        Ok(rx)
    }
}
