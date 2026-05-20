use async_trait::async_trait;
use chrono::{DateTime, Utc};
use remora_common::{ApiKeyInfo, Event, SessionToken, Team, TeamMember, User};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

use super::{
    AdminSessionInfo, AuditEvent, Database, GlobalUsage, MetricsData, NotificationRx, RunAnalytics,
    SessionRunCount, SessionUsage,
};

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

    async fn list_sessions(&self) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String)>> {
        let rows = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>, String)>(
            "SELECT id, description, created_at, status FROM sessions ORDER BY created_at DESC",
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

    async fn get_session_status(&self, session_id: Uuid) -> anyhow::Result<Option<String>> {
        let status = sqlx::query_scalar::<_, String>("SELECT status FROM sessions WHERE id = $1")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(status)
    }

    async fn set_session_expired(&self, session_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET status = 'expired' WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn reactivate_session(&self, session_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE sessions SET status = 'active', idle_since = NULL \
             WHERE id = $1 AND status = 'expired'",
        )
        .bind(session_id)
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

    async fn get_recent_events_for_session(
        &self,
        session_id: Uuid,
        limit: i64,
    ) -> anyhow::Result<Vec<Event>> {
        let rows = sqlx::query_as::<_, (i64, Uuid, DateTime<Utc>, Option<String>, String, Value)>(
            "SELECT id, session_id, timestamp, author, kind, payload \
             FROM (SELECT id, session_id, timestamp, author, kind, payload \
                   FROM events WHERE session_id = $1 ORDER BY id DESC LIMIT $2) sub \
             ORDER BY id",
        )
        .bind(session_id)
        .bind(limit)
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

    // -- owner key --

    async fn set_owner_key(&self, session_id: Uuid, key: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET owner_key = $1 WHERE id = $2")
            .bind(key)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_owner_key(&self, session_id: Uuid) -> anyhow::Result<Option<String>> {
        let row =
            sqlx::query_scalar::<_, Option<String>>("SELECT owner_key FROM sessions WHERE id = $1")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.flatten())
    }

    // -- session tokens --
    async fn create_session_token(&self, session_id: Uuid, label: &str) -> anyhow::Result<String> {
        let token = format!("rmr_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlx::query("INSERT INTO session_tokens (session_id, token, label) VALUES ($1, $2, $3)")
            .bind(session_id)
            .bind(&token)
            .bind(label)
            .execute(&self.pool)
            .await?;
        Ok(token)
    }
    async fn validate_session_token(&self, token: &str) -> anyhow::Result<Option<Uuid>> {
        let row = sqlx::query_scalar::<_, Uuid>(
            "SELECT session_id FROM session_tokens WHERE token = $1 AND revoked_at IS NULL",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }
    async fn revoke_session_token(&self, token: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE session_tokens SET revoked_at = now() WHERE token = $1")
            .bind(token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    async fn revoke_session_token_by_id(
        &self,
        session_id: Uuid,
        token_id: i64,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE session_tokens SET revoked_at = now() WHERE id = $1 AND session_id = $2 AND revoked_at IS NULL").bind(token_id).bind(session_id).execute(&self.pool).await?;
        Ok(())
    }
    async fn list_session_tokens(&self, session_id: Uuid) -> anyhow::Result<Vec<SessionToken>> {
        let rows = sqlx::query_as::<_, (i64, Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>)>("SELECT id, session_id, label, created_at, revoked_at FROM session_tokens WHERE session_id = $1 ORDER BY id").bind(session_id).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, session_id, label, created_at, revoked_at)| SessionToken {
                    id,
                    session_id,
                    label,
                    created_at,
                    revoked: revoked_at.is_some(),
                },
            )
            .collect())
    }

    // -- trusted participants --

    async fn trust_participant(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO session_trusted (session_id, participant_name) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(session_id)
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn untrust_participant(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM session_trusted WHERE session_id = $1 AND participant_name = $2")
            .bind(session_id)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_trusted_participants(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT participant_name FROM session_trusted \
             WHERE session_id = $1 ORDER BY participant_name",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }

    // -- users --

    async fn create_user(
        &self,
        email: &str,
        display_name: &str,
        password_hash: Option<&str>,
        role: &str,
    ) -> anyhow::Result<Uuid> {
        let id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (email, display_name, password_hash, role) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(email)
        .bind(display_name)
        .bind(password_hash)
        .bind(role)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    async fn get_user_by_email(&self, email: &str) -> anyhow::Result<Option<User>> {
        let row = sqlx::query_as::<_, (Uuid, String, String, String, DateTime<Utc>)>(
            "SELECT id, email, display_name, role, created_at FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, email, display_name, role, created_at)| User {
            id,
            email,
            display_name,
            role,
            created_at,
        }))
    }

    async fn get_user_by_id(&self, id: Uuid) -> anyhow::Result<Option<User>> {
        let row = sqlx::query_as::<_, (Uuid, String, String, String, DateTime<Utc>)>(
            "SELECT id, email, display_name, role, created_at FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, email, display_name, role, created_at)| User {
            id,
            email,
            display_name,
            role,
            created_at,
        }))
    }

    async fn get_password_hash(&self, email: &str) -> anyhow::Result<Option<String>> {
        let row = sqlx::query_scalar::<_, Option<String>>(
            "SELECT password_hash FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.flatten())
    }

    // -- refresh tokens --

    async fn store_refresh_token(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> anyhow::Result<Uuid> {
        let id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO refresh_tokens (user_id, token_hash, expires_at) \
             VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(user_id)
        .bind(token_hash)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    async fn validate_refresh_token(
        &self,
        token_hash: &str,
    ) -> anyhow::Result<Option<(Uuid, Uuid)>> {
        let row = sqlx::query_as::<_, (Uuid, Uuid)>(
            "SELECT id, user_id FROM refresh_tokens \
             WHERE token_hash = $1 AND expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn delete_refresh_token(&self, token_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM refresh_tokens WHERE id = $1")
            .bind(token_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn consume_refresh_token(&self, token_hash: &str) -> anyhow::Result<Option<Uuid>> {
        let row = sqlx::query_scalar::<_, Uuid>(
            "DELETE FROM refresh_tokens \
             WHERE token_hash = $1 AND expires_at > now() \
             RETURNING user_id",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    // -- oauth --

    async fn upsert_oauth_connection(
        &self,
        user_id: Uuid,
        provider: &str,
        provider_user_id: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO oauth_connections (user_id, provider, provider_user_id) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (provider, provider_user_id) DO UPDATE SET user_id = $1",
        )
        .bind(user_id)
        .bind(provider)
        .bind(provider_user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_user_by_oauth(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> anyhow::Result<Option<User>> {
        let row = sqlx::query_as::<_, (Uuid, String, String, String, DateTime<Utc>)>(
            "SELECT u.id, u.email, u.display_name, u.role, u.created_at \
             FROM users u JOIN oauth_connections o ON u.id = o.user_id \
             WHERE o.provider = $1 AND o.provider_user_id = $2",
        )
        .bind(provider)
        .bind(provider_user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, email, display_name, role, created_at)| User {
            id,
            email,
            display_name,
            role,
            created_at,
        }))
    }

    // -- api keys --

    async fn create_api_key(
        &self,
        user_id: Uuid,
        key_hash: &str,
        label: &str,
    ) -> anyhow::Result<Uuid> {
        let id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO api_keys (user_id, key_hash, label) \
             VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(user_id)
        .bind(key_hash)
        .bind(label)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    async fn validate_api_key(&self, key_hash: &str) -> anyhow::Result<Option<User>> {
        let row = sqlx::query_as::<_, (Uuid, String, String, String, DateTime<Utc>)>(
            "SELECT u.id, u.email, u.display_name, u.role, u.created_at \
             FROM users u JOIN api_keys k ON u.id = k.user_id \
             WHERE k.key_hash = $1 AND k.revoked_at IS NULL",
        )
        .bind(key_hash)
        .fetch_optional(&self.pool)
        .await?;
        // Update last_used_at on successful lookup
        if row.is_some() {
            let _ = sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE key_hash = $1")
                .bind(key_hash)
                .execute(&self.pool)
                .await;
        }
        Ok(row.map(|(id, email, display_name, role, created_at)| User {
            id,
            email,
            display_name,
            role,
            created_at,
        }))
    }

    async fn list_api_keys(&self, user_id: Uuid) -> anyhow::Result<Vec<ApiKeyInfo>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
                Option<DateTime<Utc>>,
            ),
        >(
            "SELECT id, label, created_at, last_used_at, revoked_at \
             FROM api_keys WHERE user_id = $1 ORDER BY created_at",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, label, created_at, last_used_at, revoked_at)| ApiKeyInfo {
                    id,
                    label,
                    created_at,
                    last_used_at,
                    revoked: revoked_at.is_some(),
                },
            )
            .collect())
    }

    async fn revoke_api_key(&self, key_id: Uuid, user_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE api_keys SET revoked_at = now() \
             WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(key_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // -- teams --

    async fn create_team(
        &self,
        name: &str,
        description: &str,
        created_by: Uuid,
    ) -> anyhow::Result<Uuid> {
        let id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO teams (name, description, created_by) \
             VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(name)
        .bind(description)
        .bind(created_by)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    async fn get_team(&self, team_id: Uuid) -> anyhow::Result<Option<Team>> {
        let row = sqlx::query_as::<_, (Uuid, String, String, i64, DateTime<Utc>)>(
            "SELECT id, name, description, daily_token_cap, created_at FROM teams WHERE id = $1",
        )
        .bind(team_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(id, name, description, daily_token_cap, created_at)| Team {
                id,
                name,
                description,
                daily_token_cap,
                created_at,
            },
        ))
    }

    async fn list_teams_for_user(&self, user_id: Uuid) -> anyhow::Result<Vec<Team>> {
        let rows = sqlx::query_as::<_, (Uuid, String, String, i64, DateTime<Utc>)>(
            "SELECT t.id, t.name, t.description, t.daily_token_cap, t.created_at \
             FROM teams t JOIN team_members tm ON t.id = tm.team_id \
             WHERE tm.user_id = $1 ORDER BY t.name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, name, description, daily_token_cap, created_at)| Team {
                    id,
                    name,
                    description,
                    daily_token_cap,
                    created_at,
                },
            )
            .collect())
    }

    async fn update_team(
        &self,
        team_id: Uuid,
        name: &str,
        description: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE teams SET name = $1, description = $2 WHERE id = $3")
            .bind(name)
            .bind(description)
            .bind(team_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete_team(&self, team_id: Uuid) -> anyhow::Result<()> {
        // Clear team_id on sessions before deleting team
        sqlx::query("UPDATE sessions SET team_id = NULL WHERE team_id = $1")
            .bind(team_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM teams WHERE id = $1")
            .bind(team_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- team members --

    async fn add_team_member(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO team_members (team_id, user_id, role) VALUES ($1, $2, $3) \
             ON CONFLICT (team_id, user_id) DO UPDATE SET role = $3",
        )
        .bind(team_id)
        .bind(user_id)
        .bind(role)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn remove_team_member(&self, team_id: Uuid, user_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
            .bind(team_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_team_members(&self, team_id: Uuid) -> anyhow::Result<Vec<TeamMember>> {
        let rows = sqlx::query_as::<_, (Uuid, String, String, String, DateTime<Utc>)>(
            "SELECT u.id, u.email, u.display_name, tm.role, tm.joined_at \
             FROM team_members tm JOIN users u ON tm.user_id = u.id \
             WHERE tm.team_id = $1 ORDER BY u.display_name",
        )
        .bind(team_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(user_id, email, display_name, role, joined_at)| TeamMember {
                    user_id,
                    email,
                    display_name,
                    role,
                    joined_at,
                },
            )
            .collect())
    }

    async fn get_team_member_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> anyhow::Result<Option<String>> {
        let row = sqlx::query_scalar::<_, String>(
            "SELECT role FROM team_members WHERE team_id = $1 AND user_id = $2",
        )
        .bind(team_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn update_team_member_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE team_members SET role = $1 WHERE team_id = $2 AND user_id = $3")
            .bind(role)
            .bind(team_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- team-scoped sessions --

    async fn create_session_for_team(
        &self,
        description: &str,
        team_id: Uuid,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)> {
        let row = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>)>(
            "INSERT INTO sessions (description, team_id) VALUES ($1, $2) \
             RETURNING id, description, created_at",
        )
        .bind(description)
        .bind(team_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn list_sessions_for_team(
        &self,
        team_id: Uuid,
    ) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String)>> {
        let rows = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>, String)>(
            "SELECT id, description, created_at, status FROM sessions \
             WHERE team_id = $1 ORDER BY created_at DESC",
        )
        .bind(team_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_session_team(&self, session_id: Uuid) -> anyhow::Result<Option<Uuid>> {
        let row =
            sqlx::query_scalar::<_, Option<Uuid>>("SELECT team_id FROM sessions WHERE id = $1")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.flatten())
    }

    // -- user dashboard --

    async fn list_sessions_for_user(
        &self,
        user_id: Uuid,
    ) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String, Option<String>)>> {
        let rows = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>, String, Option<String>)>(
            "SELECT DISTINCT s.id, s.description, s.created_at, s.status, t.name \
             FROM sessions s \
             LEFT JOIN teams t ON s.team_id = t.id \
             LEFT JOIN team_members tm ON s.team_id = tm.team_id \
             WHERE s.team_id IS NULL OR tm.user_id = $1 \
             ORDER BY s.created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // -- Phase 4: admin & observability --

    async fn get_usage_summary(&self) -> anyhow::Result<Vec<SessionUsage>> {
        let rows = sqlx::query_as::<_, (Uuid, String, i64, i64, String)>(
            "SELECT id, description, COALESCE(tokens_used_today, 0), \
             COALESCE(daily_token_cap, 1000000), \
             COALESCE(tokens_reset_date::TEXT, CURRENT_DATE::TEXT) \
             FROM sessions ORDER BY tokens_used_today DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    session_id,
                    description,
                    tokens_used_today,
                    daily_token_cap,
                    tokens_reset_date,
                )| {
                    SessionUsage {
                        session_id,
                        description,
                        tokens_used_today,
                        daily_token_cap,
                        tokens_reset_date,
                    }
                },
            )
            .collect())
    }

    async fn get_global_usage_summary(&self) -> anyhow::Result<GlobalUsage> {
        let row = sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT \
             (SELECT COALESCE(SUM(tokens_used_today), 0)::BIGINT FROM sessions \
              WHERE date(tokens_reset_date) = CURRENT_DATE), \
             (SELECT COUNT(*) FROM sessions), \
             (SELECT COUNT(*) FROM sessions WHERE status = 'active')",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(GlobalUsage {
            total_tokens_today: row.0,
            total_sessions: row.1,
            active_sessions: row.2,
        })
    }

    async fn get_run_analytics(&self) -> anyhow::Result<RunAnalytics> {
        let row = sqlx::query_as::<_, (i64, i64, i64, i64, f64)>(
            "SELECT \
             (SELECT COUNT(*) FROM session_runs), \
             (SELECT COUNT(*) FROM session_runs WHERE status = 'success'), \
             (SELECT COUNT(*) FROM session_runs WHERE status = 'failed'), \
             (SELECT COUNT(*) FROM session_runs WHERE status = 'timeout'), \
             (SELECT COALESCE(AVG(EXTRACT(EPOCH FROM (finished_at - started_at)))::FLOAT8, 0.0) \
              FROM session_runs WHERE finished_at IS NOT NULL)",
        )
        .fetch_one(&self.pool)
        .await?;

        let session_rows = sqlx::query_as::<_, (Uuid, i64)>(
            "SELECT session_id, COUNT(*) FROM session_runs \
             GROUP BY session_id ORDER BY COUNT(*) DESC LIMIT 20",
        )
        .fetch_all(&self.pool)
        .await?;

        let runs_by_session = session_rows
            .into_iter()
            .map(|(session_id, run_count)| SessionRunCount {
                session_id,
                run_count,
            })
            .collect();

        Ok(RunAnalytics {
            total_runs: row.0,
            successful: row.1,
            failed: row.2,
            timed_out: row.3,
            avg_duration_secs: row.4,
            runs_by_session,
        })
    }

    async fn list_all_sessions_admin(&self) -> anyhow::Result<Vec<AdminSessionInfo>> {
        let rows = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>, String, i64, i64)>(
            "SELECT id, description, created_at, status, \
             COALESCE(tokens_used_today, 0), COALESCE(daily_token_cap, 1000000) \
             FROM sessions ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, description, created_at, status, tokens_used_today, daily_token_cap)| {
                    AdminSessionInfo {
                        id,
                        description,
                        created_at,
                        status,
                        tokens_used_today,
                        daily_token_cap,
                    }
                },
            )
            .collect())
    }

    async fn update_session_quota(
        &self,
        session_id: Uuid,
        daily_token_cap: i64,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET daily_token_cap = $1 WHERE id = $2")
            .bind(daily_token_cap)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_all_users(&self) -> anyhow::Result<Vec<User>> {
        let rows = sqlx::query_as::<_, (Uuid, String, String, String, DateTime<Utc>)>(
            "SELECT id, email, display_name, role, created_at FROM users ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, email, display_name, role, created_at)| User {
                id,
                email,
                display_name,
                role,
                created_at,
            })
            .collect())
    }

    async fn update_user_role(&self, user_id: Uuid, role: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE users SET role = $1 WHERE id = $2")
            .bind(role)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
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
        let id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO audit_events (user_id, action, target_type, target_id, details, ip_address) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(user_id)
        .bind(action)
        .bind(target_type)
        .bind(target_id)
        .bind(details)
        .bind(ip_address)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    async fn list_audit_events(&self, limit: i64, offset: i64) -> anyhow::Result<Vec<AuditEvent>> {
        let rows = sqlx::query_as::<
            _,
            (
                i64,
                Option<Uuid>,
                String,
                String,
                Option<String>,
                Option<Value>,
                Option<String>,
                DateTime<Utc>,
            ),
        >(
            "SELECT id, user_id, action, target_type, target_id, details, ip_address, created_at \
             FROM audit_events ORDER BY id DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, user_id, action, target_type, target_id, details, ip_address, created_at)| {
                    AuditEvent {
                        id,
                        user_id,
                        action,
                        target_type,
                        target_id,
                        details,
                        ip_address,
                        created_at,
                    }
                },
            )
            .collect())
    }

    async fn get_metrics_data(&self) -> anyhow::Result<MetricsData> {
        let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64)>(
            "SELECT \
             (SELECT COUNT(*) FROM sessions), \
             (SELECT COUNT(*) FROM sessions WHERE status = 'active'), \
             (SELECT COALESCE(SUM(tokens_used_today), 0)::BIGINT FROM sessions \
              WHERE date(tokens_reset_date) = CURRENT_DATE), \
             (SELECT COUNT(*) FROM session_runs WHERE status = 'success'), \
             (SELECT COUNT(*) FROM session_runs WHERE status = 'failed'), \
             (SELECT COUNT(*) FROM session_runs WHERE status = 'timeout')",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(MetricsData {
            sessions_total: row.0,
            sessions_active: row.1,
            tokens_used_total: row.2,
            runs_successful: row.3,
            runs_failed: row.4,
            runs_timed_out: row.5,
        })
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
