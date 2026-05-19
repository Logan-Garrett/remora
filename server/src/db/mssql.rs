use async_trait::async_trait;
use bb8::Pool;
use bb8_tiberius::ConnectionManager;
use chrono::{DateTime, Utc};
use remora_common::{ApiKeyInfo, Event, SessionToken, Team, TeamMember, User};
use serde_json::Value;
use tiberius::{AuthMethod, Config};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::{Database, NotificationRx};

pub struct MssqlDb {
    pool: Pool<ConnectionManager>,
    /// In-process notification bus (replaces Postgres LISTEN/NOTIFY).
    event_tx: broadcast::Sender<i64>,
}

/// Parse a tiberius connection string of the form
/// `server=host;database=db;user=u;password=p[;port=1433][;encrypt=false]`
/// into a `tiberius::Config`.
fn parse_connection_string(url: &str) -> anyhow::Result<Config> {
    let mut config = Config::new();
    config.port(1433);

    // First pass: collect all key-value pairs
    let mut host = String::new();
    let mut database = String::new();
    let mut user = String::new();
    let mut pass = String::new();
    let mut port: u16 = 1433;
    let mut encrypt = None;
    let mut trust_cert = false;

    for part in url.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, value) = match part.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        match key.trim().to_lowercase().as_str() {
            "server" | "host" => host = value.trim().to_string(),
            "database" | "db" => database = value.trim().to_string(),
            "user" | "uid" => user = value.trim().to_string(),
            "password" | "pwd" => pass = value.trim().to_string(),
            "port" => port = value.trim().parse().unwrap_or(1433),
            "encrypt" => {
                encrypt = Some(match value.trim().to_lowercase().as_str() {
                    "true" | "yes" | "required" => tiberius::EncryptionLevel::Required,
                    "false" | "no" => tiberius::EncryptionLevel::NotSupported,
                    _ => tiberius::EncryptionLevel::Off,
                });
            }
            "trustservercertificate" | "trust_cert" => {
                trust_cert = value.trim().to_lowercase() == "true";
            }
            _ => {}
        }
    }

    // Apply parsed values
    if !host.is_empty() {
        config.host(&host);
    }
    if !database.is_empty() {
        config.database(&database);
    }
    config.port(port);
    if !user.is_empty() {
        config.authentication(AuthMethod::sql_server(user, pass));
    }

    // Encryption defaults to NotSupported (plaintext). Users can override with encrypt=true.
    // TrustServerCertificate=true calls trust_cert() but doesn't force encryption level —
    // that's controlled separately via encrypt=.
    config.encryption(encrypt.unwrap_or(tiberius::EncryptionLevel::NotSupported));
    if trust_cert {
        config.trust_cert();
    }

    Ok(config)
}

impl MssqlDb {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let config = parse_connection_string(url)?;
        let mgr = ConnectionManager::new(config);
        let pool = Pool::builder().max_size(20).build(mgr).await?;

        let (event_tx, _) = broadcast::channel(1024);
        Ok(Self { pool, event_tx })
    }

    /// Emit a notification for a new event id (in-process, same as SQLite).
    fn notify(&self, event_id: i64) {
        let _ = self.event_tx.send(event_id);
    }

    /// Get a connection from the pool.
    async fn conn(&self) -> anyhow::Result<bb8::PooledConnection<'_, ConnectionManager>> {
        Ok(self.pool.get().await?)
    }
}

// ---------------------------------------------------------------------------
// Helpers to extract typed values from tiberius rows
// ---------------------------------------------------------------------------

/// Read a string column, handling both &str and NVarChar.
fn col_str(row: &tiberius::Row, idx: usize) -> anyhow::Result<String> {
    let val: &str = row
        .try_get::<&str, _>(idx)?
        .ok_or_else(|| anyhow::anyhow!("NULL in non-nullable string column {idx}"))?;
    Ok(val.to_string())
}

fn col_str_opt(row: &tiberius::Row, idx: usize) -> anyhow::Result<Option<String>> {
    Ok(row.try_get::<&str, _>(idx)?.map(|s| s.to_string()))
}

fn col_uuid(row: &tiberius::Row, idx: usize) -> anyhow::Result<Uuid> {
    let g: tiberius::Uuid = row
        .try_get::<tiberius::Uuid, _>(idx)?
        .ok_or_else(|| anyhow::anyhow!("NULL in non-nullable uuid column {idx}"))?;
    // tiberius::Uuid is a re-export of uuid::Uuid
    Ok(Uuid::from_bytes(*g.as_bytes()))
}

fn col_i64(row: &tiberius::Row, idx: usize) -> anyhow::Result<i64> {
    // BIGINT comes back as i64
    let v: i64 = row
        .try_get::<i64, _>(idx)?
        .ok_or_else(|| anyhow::anyhow!("NULL in non-nullable i64 column {idx}"))?;
    Ok(v)
}

fn col_datetime(row: &tiberius::Row, idx: usize) -> anyhow::Result<DateTime<Utc>> {
    let dto: chrono::DateTime<chrono::FixedOffset> = row
        .try_get::<chrono::DateTime<chrono::FixedOffset>, _>(idx)?
        .ok_or_else(|| anyhow::anyhow!("NULL in non-nullable datetime column {idx}"))?;
    Ok(dto.with_timezone(&Utc))
}

#[async_trait]
impl Database for MssqlDb {
    async fn ping(&self) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        tiberius::Query::new("SELECT 1").execute(&mut conn).await?;
        Ok(())
    }

    // -- migrations --

    async fn run_migrations(&self) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;

        // Ensure migration tracking table exists
        conn.simple_query(
            "IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = '_remora_migrations')
             BEGIN
                 CREATE TABLE _remora_migrations (
                     name NVARCHAR(512) PRIMARY KEY,
                     applied_at DATETIMEOFFSET NOT NULL DEFAULT SYSDATETIMEOFFSET()
                 );
             END",
        )
        .await?
        .into_results()
        .await?;

        // Read migration files from the embedded directory
        let migration_files: Vec<(&str, &str)> = vec![
            (
                "20260428000000_initial.sql",
                include_str!("../../../migrations/mssql/20260428000000_initial.sql"),
            ),
            (
                "20260428000001_full.sql",
                include_str!("../../../migrations/mssql/20260428000001_full.sql"),
            ),
            (
                "20260428000002_indexes.sql",
                include_str!("../../../migrations/mssql/20260428000002_indexes.sql"),
            ),
            (
                "20260501000000_session_status.sql",
                include_str!("../../../migrations/mssql/20260501000000_session_status.sql"),
            ),
            (
                "20260501000001_trusted_participants.sql",
                include_str!("../../../migrations/mssql/20260501000001_trusted_participants.sql"),
            ),
            (
                "20260501000002_owner_key.sql",
                include_str!("../../../migrations/mssql/20260501000002_owner_key.sql"),
            ),
            (
                "20260518000000_session_tokens.sql",
                include_str!("../../../migrations/mssql/20260518000000_session_tokens.sql"),
            ),
            (
                "20260518000001_users.sql",
                include_str!("../../../migrations/mssql/20260518000001_users.sql"),
            ),
            (
                "20260518000002_refresh_tokens.sql",
                include_str!("../../../migrations/mssql/20260518000002_refresh_tokens.sql"),
            ),
            (
                "20260518000003_oauth_connections.sql",
                include_str!("../../../migrations/mssql/20260518000003_oauth_connections.sql"),
            ),
            (
                "20260518000004_api_keys.sql",
                include_str!("../../../migrations/mssql/20260518000004_api_keys.sql"),
            ),
            (
                "20260519000000_teams.sql",
                include_str!("../../../migrations/mssql/20260519000000_teams.sql"),
            ),
            (
                "20260519000001_team_members.sql",
                include_str!("../../../migrations/mssql/20260519000001_team_members.sql"),
            ),
            (
                "20260519000002_sessions_team_id.sql",
                include_str!("../../../migrations/mssql/20260519000002_sessions_team_id.sql"),
            ),
        ];

        for (name, sql) in migration_files {
            // Check if already applied
            let result = conn
                .query(
                    "SELECT COUNT(*) FROM _remora_migrations WHERE name = @P1",
                    &[&name],
                )
                .await?
                .into_first_result()
                .await?;

            let count: i32 = result
                .first()
                .and_then(|r| r.try_get::<i32, _>(0).ok().flatten())
                .unwrap_or(0);

            if count > 0 {
                tracing::debug!("migration {name} already applied, skipping");
                continue;
            }

            tracing::info!("applying mssql migration: {name}");

            // Split on GO statements (T-SQL batch separator) and execute each batch.
            // Also split on lone semicolons for multi-statement files.
            let batches: Vec<&str> = sql
                .split("\nGO\n")
                .flat_map(|batch| batch.split("\nGO\r\n"))
                .collect();

            for batch in &batches {
                let trimmed = batch.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Execute each statement separately when separated by semicolons
                // at the top level. However, for MSSQL we generally run the whole
                // batch because ALTER TABLE statements etc. may appear together.
                conn.simple_query(trimmed).await?.into_results().await?;
            }

            // Record migration
            conn.execute(
                "INSERT INTO _remora_migrations (name) VALUES (@P1)",
                &[&name],
            )
            .await?;
        }

        Ok(())
    }

    // -- sessions --

    async fn create_session(
        &self,
        description: &str,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)> {
        let mut conn = self.conn().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        let tiberius_id = tiberius::Uuid::from_bytes(*id.as_bytes());
        let now_dto = chrono::DateTime::<chrono::FixedOffset>::from(now);

        conn.execute(
            "INSERT INTO sessions (id, description, created_at, updated_at, \
             daily_token_cap, tokens_used_today, tokens_reset_date) \
             VALUES (@P1, @P2, @P3, @P4, 999999999, 0, CAST(GETDATE() AS DATE))",
            &[&tiberius_id, &description, &now_dto, &now_dto],
        )
        .await?;

        Ok((id, description.to_string(), now))
    }

    async fn list_sessions(&self) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String)>> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT id, description, created_at, status FROM sessions ORDER BY created_at DESC",
                &[],
            )
            .await?
            .into_first_result()
            .await?;

        let mut result = Vec::new();
        for row in &rows {
            let id = col_uuid(row, 0)?;
            let desc = col_str(row, 1)?;
            let created = col_datetime(row, 2)?;
            let status = col_str(row, 3)?;
            result.push((id, desc, created, status));
        }
        Ok(result)
    }

    async fn delete_session(&self, session_id: Uuid) -> anyhow::Result<u64> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let result = conn
            .execute("DELETE FROM sessions WHERE id = @P1", &[&tib_id])
            .await?;
        Ok(result.total() as u64)
    }

    async fn session_exists(&self, session_id: Uuid) -> anyhow::Result<bool> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query("SELECT COUNT(*) FROM sessions WHERE id = @P1", &[&tib_id])
            .await?
            .into_first_result()
            .await?;
        let count: i32 = rows
            .first()
            .and_then(|r| r.try_get::<i32, _>(0).ok().flatten())
            .unwrap_or(0);
        Ok(count > 0)
    }

    async fn get_session_status(&self, session_id: Uuid) -> anyhow::Result<Option<String>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query("SELECT status FROM sessions WHERE id = @P1", &[&tib_id])
            .await?
            .into_first_result()
            .await?;
        let status = rows
            .first()
            .and_then(|r| r.try_get::<&str, _>(0).ok().flatten())
            .map(|s| s.to_string());
        Ok(status)
    }

    async fn set_session_expired(&self, session_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "UPDATE sessions SET status = 'expired' WHERE id = @P1",
            &[&tib_id],
        )
        .await?;
        Ok(())
    }

    async fn reactivate_session(&self, session_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "UPDATE sessions SET status = 'active', idle_since = NULL \
             WHERE id = @P1 AND status = 'expired'",
            &[&tib_id],
        )
        .await?;
        Ok(())
    }

    async fn count_sessions(&self) -> anyhow::Result<i64> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query("SELECT COUNT(*) FROM sessions", &[])
            .await?
            .into_first_result()
            .await?;
        let count = rows
            .first()
            .and_then(|r| r.try_get::<i64, _>(0).ok().flatten())
            .or_else(|| {
                rows.first()
                    .and_then(|r| r.try_get::<i32, _>(0).ok().flatten())
                    .map(|v| v as i64)
            })
            .unwrap_or(0);
        Ok(count)
    }

    async fn get_session_info(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<Option<(String, DateTime<Utc>, i64, i64)>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT description, created_at, \
                 COALESCE(tokens_used_today, 0), COALESCE(daily_token_cap, 1000000) \
                 FROM sessions WHERE id = @P1",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;

        match rows.first() {
            Some(row) => {
                let desc = col_str(row, 0)?;
                let created = col_datetime(row, 1)?;
                let used = col_i64(row, 2)?;
                let cap = col_i64(row, 3)?;
                Ok(Some((desc, created, used, cap)))
            }
            None => Ok(None),
        }
    }

    async fn set_idle_since_now(&self, session_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "UPDATE sessions SET idle_since = SYSDATETIMEOFFSET() WHERE id = @P1",
            &[&tib_id],
        )
        .await?;
        Ok(())
    }

    async fn clear_idle_since(&self, session_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "UPDATE sessions SET idle_since = NULL WHERE id = @P1",
            &[&tib_id],
        )
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
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let payload_str = serde_json::to_string(&payload)?;

        let rows = conn
            .query(
                "INSERT INTO events (session_id, [timestamp], author, kind, payload) \
                 OUTPUT INSERTED.id \
                 VALUES (@P1, SYSDATETIMEOFFSET(), @P2, @P3, @P4)",
                &[&tib_id, &author, &kind, &payload_str.as_str()],
            )
            .await?
            .into_first_result()
            .await?;

        let event_id = rows
            .first()
            .map(|r| col_i64(r, 0))
            .ok_or_else(|| anyhow::anyhow!("insert_event returned no rows"))??;

        self.notify(event_id);
        Ok(event_id)
    }

    async fn get_event_by_id(&self, event_id: i64) -> anyhow::Result<Option<Event>> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT id, session_id, [timestamp], author, kind, payload \
                 FROM events WHERE id = @P1",
                &[&event_id],
            )
            .await?
            .into_first_result()
            .await?;

        match rows.first() {
            Some(row) => {
                let id = col_i64(row, 0)?;
                let session_id = col_uuid(row, 1)?;
                let timestamp = col_datetime(row, 2)?;
                let author = col_str_opt(row, 3)?;
                let kind = col_str(row, 4)?;
                let payload_str = col_str(row, 5)?;
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
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT id, session_id, [timestamp], author, kind, payload \
                 FROM events WHERE session_id = @P1 ORDER BY id",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;

        let mut result = Vec::new();
        for row in &rows {
            let id = col_i64(row, 0)?;
            let sid = col_uuid(row, 1)?;
            let timestamp = col_datetime(row, 2)?;
            let author = col_str_opt(row, 3)?;
            let kind = col_str(row, 4)?;
            let payload_str = col_str(row, 5)?;
            let payload: Value = serde_json::from_str(&payload_str)?;
            result.push(Event {
                id,
                session_id: sid,
                timestamp,
                author,
                kind,
                payload,
            });
        }
        Ok(result)
    }

    async fn get_recent_events_for_session(
        &self,
        session_id: Uuid,
        limit: i64,
    ) -> anyhow::Result<Vec<Event>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT id, session_id, [timestamp], author, kind, payload \
                 FROM (SELECT TOP (@P2) id, session_id, [timestamp], author, kind, payload \
                       FROM events WHERE session_id = @P1 ORDER BY id DESC) sub \
                 ORDER BY id",
                &[&tib_id, &limit],
            )
            .await?
            .into_first_result()
            .await?;

        let mut result = Vec::new();
        for row in &rows {
            let id = col_i64(row, 0)?;
            let sid = col_uuid(row, 1)?;
            let timestamp = col_datetime(row, 2)?;
            let author = col_str_opt(row, 3)?;
            let kind = col_str(row, 4)?;
            let payload_str = col_str(row, 5)?;
            let payload: Value = serde_json::from_str(&payload_str)?;
            result.push(Event {
                id,
                session_id: sid,
                timestamp,
                author,
                kind,
                payload,
            });
        }
        Ok(result)
    }

    async fn get_events_since(
        &self,
        session_id: Uuid,
        since_id: i64,
    ) -> anyhow::Result<Vec<(i64, Option<String>, String, Value)>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT id, author, kind, payload FROM events \
                 WHERE session_id = @P1 AND id > @P2 ORDER BY id",
                &[&tib_id, &since_id],
            )
            .await?
            .into_first_result()
            .await?;

        let mut result = Vec::new();
        for row in &rows {
            let id = col_i64(row, 0)?;
            let author = col_str_opt(row, 1)?;
            let kind = col_str(row, 2)?;
            let payload_str = col_str(row, 3)?;
            let payload: Value = serde_json::from_str(&payload_str)?;
            result.push((id, author, kind, payload));
        }
        Ok(result)
    }

    async fn get_last_context_boundary(&self, session_id: Uuid) -> anyhow::Result<i64> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT COALESCE(
                    (SELECT MAX(id) FROM events
                     WHERE session_id = @P1 AND kind IN ('claude_response', 'clear_marker')),
                    0
                )",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;

        // COALESCE with 0 means we always get a row
        let val = rows
            .first()
            .and_then(|r| r.try_get::<i64, _>(0).ok().flatten())
            // Could come back as i32 if MAX returns INT
            .or_else(|| {
                rows.first()
                    .and_then(|r| r.try_get::<i32, _>(0).ok().flatten())
                    .map(|v| v as i64)
            })
            .unwrap_or(0);
        Ok(val)
    }

    // -- repos --

    async fn upsert_repo(&self, session_id: Uuid, name: &str, git_url: &str) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        // MERGE for upsert in T-SQL
        conn.execute(
            "MERGE INTO session_repos AS target \
             USING (SELECT @P1 AS session_id, @P2 AS name, @P3 AS git_url) AS source \
             ON target.session_id = source.session_id AND target.name = source.name \
             WHEN MATCHED THEN UPDATE SET git_url = source.git_url \
             WHEN NOT MATCHED THEN INSERT (session_id, name, git_url) \
                 VALUES (source.session_id, source.name, source.git_url);",
            &[&tib_id, &name, &git_url],
        )
        .await?;
        Ok(())
    }

    async fn delete_repo(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "DELETE FROM session_repos WHERE session_id = @P1 AND name = @P2",
            &[&tib_id, &name],
        )
        .await?;
        Ok(())
    }

    async fn list_repos(&self, session_id: Uuid) -> anyhow::Result<Vec<(String, String)>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT name, git_url FROM session_repos \
                 WHERE session_id = @P1 ORDER BY name",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;

        let mut result = Vec::new();
        for row in &rows {
            let name = col_str(row, 0)?;
            let url = col_str(row, 1)?;
            result.push((name, url));
        }
        Ok(result)
    }

    async fn list_repo_names(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT name FROM session_repos WHERE session_id = @P1",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;

        let mut result = Vec::new();
        for row in &rows {
            result.push(col_str(row, 0)?);
        }
        Ok(result)
    }

    // -- runs --

    async fn insert_run(&self, session_id: Uuid, context_mode: &str) -> anyhow::Result<i64> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "IF NOT EXISTS (SELECT 1 FROM session_runs WHERE session_id = @P1 AND status = 'running') \
                 BEGIN \
                     INSERT INTO session_runs (session_id, status, context_mode) \
                     OUTPUT INSERTED.id \
                     VALUES (@P1, 'running', @P2) \
                 END",
                &[&tib_id, &context_mode],
            )
            .await?
            .into_first_result()
            .await?;

        let id = rows
            .first()
            .map(|r| col_i64(r, 0))
            .ok_or_else(|| anyhow::anyhow!("A run is already in progress for this session"))??;
        Ok(id)
    }

    async fn finish_run(&self, run_id: i64, status: &str) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        conn.execute(
            "UPDATE session_runs SET status = @P1, finished_at = SYSDATETIMEOFFSET() \
             WHERE id = @P2",
            &[&status, &run_id],
        )
        .await?;
        Ok(())
    }

    async fn is_run_in_flight(&self, session_id: Uuid) -> anyhow::Result<bool> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT COUNT(*) FROM session_runs \
                 WHERE session_id = @P1 AND status = 'running'",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;

        let count: i32 = rows
            .first()
            .and_then(|r| r.try_get::<i32, _>(0).ok().flatten())
            .unwrap_or(0);
        Ok(count > 0)
    }

    // -- allowlists --

    async fn list_global_allowlist(&self) -> anyhow::Result<Vec<(String, String)>> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT domain, kind FROM global_allowlist ORDER BY domain",
                &[],
            )
            .await?
            .into_first_result()
            .await?;

        let mut result = Vec::new();
        for row in &rows {
            let domain = col_str(row, 0)?;
            let kind = col_str(row, 1)?;
            result.push((domain, kind));
        }
        Ok(result)
    }

    async fn list_session_allowlist(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT domain FROM session_allowlist \
                 WHERE session_id = @P1 ORDER BY domain",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;

        let mut result = Vec::new();
        for row in &rows {
            result.push(col_str(row, 0)?);
        }
        Ok(result)
    }

    async fn add_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        // Use MERGE to emulate ON CONFLICT DO NOTHING
        conn.execute(
            "MERGE INTO session_allowlist AS target \
             USING (SELECT @P1 AS session_id, @P2 AS domain) AS source \
             ON target.session_id = source.session_id AND target.domain = source.domain \
             WHEN NOT MATCHED THEN INSERT (session_id, domain) \
                 VALUES (source.session_id, source.domain);",
            &[&tib_id, &domain],
        )
        .await?;
        Ok(())
    }

    async fn remove_session_allowlist(&self, session_id: Uuid, domain: &str) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "DELETE FROM session_allowlist WHERE session_id = @P1 AND domain = @P2",
            &[&tib_id, &domain],
        )
        .await?;
        Ok(())
    }

    async fn is_domain_blocked(&self, domain: &str) -> anyhow::Result<bool> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT COUNT(*) FROM global_allowlist \
                 WHERE domain = @P1 AND kind = 'block'",
                &[&domain],
            )
            .await?
            .into_first_result()
            .await?;

        let count: i32 = rows
            .first()
            .and_then(|r| r.try_get::<i32, _>(0).ok().flatten())
            .unwrap_or(0);
        Ok(count > 0)
    }

    async fn is_domain_global_allowed(&self, domain: &str) -> anyhow::Result<bool> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT COUNT(*) FROM global_allowlist \
                 WHERE domain = @P1 AND kind = 'allow'",
                &[&domain],
            )
            .await?
            .into_first_result()
            .await?;

        let count: i32 = rows
            .first()
            .and_then(|r| r.try_get::<i32, _>(0).ok().flatten())
            .unwrap_or(0);
        Ok(count > 0)
    }

    async fn is_domain_session_allowed(
        &self,
        session_id: Uuid,
        domain: &str,
    ) -> anyhow::Result<bool> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT COUNT(*) FROM session_allowlist \
                 WHERE session_id = @P1 AND domain = @P2",
                &[&tib_id, &domain],
            )
            .await?
            .into_first_result()
            .await?;

        let count: i32 = rows
            .first()
            .and_then(|r| r.try_get::<i32, _>(0).ok().flatten())
            .unwrap_or(0);
        Ok(count > 0)
    }

    // -- pending approvals --

    async fn create_pending_approval(
        &self,
        session_id: Uuid,
        domain: &str,
        url: &str,
        requested_by: &str,
    ) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "INSERT INTO pending_approvals \
             (session_id, domain, url, requested_by, resolved, approved) \
             VALUES (@P1, @P2, @P3, @P4, 0, NULL)",
            &[&tib_id, &domain, &url, &requested_by],
        )
        .await?;
        Ok(())
    }

    async fn resolve_approval(
        &self,
        session_id: Uuid,
        domain: &str,
        approved: bool,
    ) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "UPDATE pending_approvals SET resolved = 1, approved = @P1 \
             WHERE session_id = @P2 AND domain = @P3 AND resolved = 0",
            &[&approved, &tib_id, &domain],
        )
        .await?;

        if approved {
            // Also add to session allowlist (MERGE for idempotency)
            conn.execute(
                "MERGE INTO session_allowlist AS target \
                 USING (SELECT @P1 AS session_id, @P2 AS domain) AS source \
                 ON target.session_id = source.session_id AND target.domain = source.domain \
                 WHEN NOT MATCHED THEN INSERT (session_id, domain) \
                     VALUES (source.session_id, source.domain);",
                &[&tib_id, &domain],
            )
            .await?;
        }
        Ok(())
    }

    async fn get_approved_pending(
        &self,
        session_id: Uuid,
        domain: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT url, requested_by FROM pending_approvals \
                 WHERE session_id = @P1 AND domain = @P2 AND approved = 1",
                &[&tib_id, &domain],
            )
            .await?
            .into_first_result()
            .await?;

        let mut result = Vec::new();
        for row in &rows {
            let url = col_str(row, 0)?;
            let requested_by = col_str(row, 1)?;
            result.push((url, requested_by));
        }
        Ok(result)
    }

    // -- quotas --

    async fn reset_tokens_if_needed(&self, session_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "UPDATE sessions SET tokens_used_today = 0, \
             tokens_reset_date = CAST(GETDATE() AS DATE) \
             WHERE id = @P1 AND tokens_reset_date < CAST(GETDATE() AS DATE)",
            &[&tib_id],
        )
        .await?;
        Ok(())
    }

    async fn get_session_usage(&self, session_id: Uuid) -> anyhow::Result<(i64, i64)> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT COALESCE(tokens_used_today, 0), COALESCE(daily_token_cap, 1000000) \
                 FROM sessions WHERE id = @P1",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;

        let row = rows
            .first()
            .ok_or_else(|| anyhow::anyhow!("session not found"))?;
        let used = col_i64(row, 0)?;
        let cap = col_i64(row, 1)?;
        Ok((used, cap))
    }

    async fn get_global_usage(&self) -> anyhow::Result<i64> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT COALESCE(SUM(tokens_used_today), 0) \
                 FROM sessions WHERE tokens_reset_date = CAST(GETDATE() AS DATE)",
                &[],
            )
            .await?
            .into_first_result()
            .await?;

        let val = rows
            .first()
            .and_then(|r| r.try_get::<i64, _>(0).ok().flatten())
            .unwrap_or(0);
        Ok(val)
    }

    async fn add_usage(&self, session_id: Uuid, tokens: i64) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "UPDATE sessions SET tokens_used_today = tokens_used_today + @P1 \
             WHERE id = @P2",
            &[&tokens, &tib_id],
        )
        .await?;
        Ok(())
    }

    async fn get_idle_sessions(&self, idle_timeout_secs: u64) -> anyhow::Result<Vec<Uuid>> {
        let mut conn = self.conn().await?;
        let cutoff = Utc::now() - chrono::Duration::seconds(idle_timeout_secs as i64);
        let cutoff_dto = chrono::DateTime::<chrono::FixedOffset>::from(cutoff);
        let rows = conn
            .query(
                "SELECT id FROM sessions \
                 WHERE idle_since IS NOT NULL AND idle_since < @P1",
                &[&cutoff_dto],
            )
            .await?
            .into_first_result()
            .await?;

        let mut result = Vec::new();
        for row in &rows {
            result.push(col_uuid(row, 0)?);
        }
        Ok(result)
    }

    async fn clear_idle_since_for(&self, session_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "UPDATE sessions SET idle_since = NULL WHERE id = @P1",
            &[&tib_id],
        )
        .await?;
        Ok(())
    }

    // -- owner key --

    async fn set_owner_key(&self, session_id: Uuid, key: &str) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "UPDATE sessions SET owner_key = @P1 WHERE id = @P2",
            &[&key, &tib_id],
        )
        .await?;
        Ok(())
    }

    async fn get_owner_key(&self, session_id: Uuid) -> anyhow::Result<Option<String>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query("SELECT owner_key FROM sessions WHERE id = @P1", &[&tib_id])
            .await?
            .into_first_result()
            .await?;
        let key = rows
            .first()
            .and_then(|r| r.try_get::<&str, _>(0).ok().flatten())
            .map(|s| s.to_string());
        Ok(key)
    }

    // -- session tokens --
    async fn create_session_token(&self, session_id: Uuid, label: &str) -> anyhow::Result<String> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let token = format!("rmr_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        conn.execute(
            "INSERT INTO session_tokens (session_id, token, label) VALUES (@P1, @P2, @P3)",
            &[&tib_id, &token.as_str(), &label],
        )
        .await?;
        Ok(token)
    }
    async fn validate_session_token(&self, token: &str) -> anyhow::Result<Option<Uuid>> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT session_id FROM session_tokens WHERE token = @P1 AND revoked_at IS NULL",
                &[&token],
            )
            .await?
            .into_first_result()
            .await?;
        match rows.first() {
            Some(row) => Ok(Some(col_uuid(row, 0)?)),
            None => Ok(None),
        }
    }
    async fn revoke_session_token(&self, token: &str) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        conn.execute(
            "UPDATE session_tokens SET revoked_at = SYSDATETIMEOFFSET() WHERE token = @P1",
            &[&token],
        )
        .await?;
        Ok(())
    }
    async fn revoke_session_token_by_id(
        &self,
        session_id: Uuid,
        token_id: i64,
    ) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute("UPDATE session_tokens SET revoked_at = SYSDATETIMEOFFSET() WHERE id = @P1 AND session_id = @P2 AND revoked_at IS NULL", &[&token_id, &tib_id]).await?;
        Ok(())
    }
    async fn list_session_tokens(&self, session_id: Uuid) -> anyhow::Result<Vec<SessionToken>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn.query("SELECT id, session_id, label, created_at, revoked_at FROM session_tokens WHERE session_id = @P1 ORDER BY id", &[&tib_id]).await?.into_first_result().await?;
        let mut result = Vec::new();
        for row in &rows {
            let id = col_i64(row, 0)?;
            let sid = col_uuid(row, 1)?;
            let label = col_str(row, 2)?;
            let created_at = col_datetime(row, 3)?;
            let revoked_at: Option<chrono::DateTime<chrono::FixedOffset>> =
                row.try_get::<chrono::DateTime<chrono::FixedOffset>, _>(4)?;
            result.push(SessionToken {
                id,
                session_id: sid,
                label,
                created_at,
                revoked: revoked_at.is_some(),
            });
        }
        Ok(result)
    }

    // -- trusted participants --

    async fn trust_participant(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "MERGE session_trusted AS t \
             USING (SELECT @P1 AS session_id, @P2 AS participant_name) AS s \
             ON t.session_id = s.session_id AND t.participant_name = s.participant_name \
             WHEN NOT MATCHED THEN INSERT (session_id, participant_name) \
             VALUES (s.session_id, s.participant_name);",
            &[&tib_id, &name],
        )
        .await?;
        Ok(())
    }

    async fn untrust_participant(&self, session_id: Uuid, name: &str) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        conn.execute(
            "DELETE FROM session_trusted WHERE session_id = @P1 AND participant_name = @P2",
            &[&tib_id, &name],
        )
        .await?;
        Ok(())
    }

    async fn list_trusted_participants(&self, session_id: Uuid) -> anyhow::Result<Vec<String>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query(
                "SELECT participant_name FROM session_trusted \
                 WHERE session_id = @P1 ORDER BY participant_name",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;
        let names = rows
            .iter()
            .filter_map(|r| r.try_get::<&str, _>(0).ok().flatten())
            .map(|s| s.to_string())
            .collect();
        Ok(names)
    }

    // -- users --

    async fn create_user(
        &self,
        email: &str,
        display_name: &str,
        password_hash: Option<&str>,
        role: &str,
    ) -> anyhow::Result<Uuid> {
        let mut conn = self.conn().await?;
        let id = Uuid::new_v4();
        let tib_id = tiberius::Uuid::from_bytes(*id.as_bytes());
        let pw: &str = password_hash.unwrap_or("");
        let has_pw = password_hash.is_some();
        if has_pw {
            conn.execute(
                "INSERT INTO users (id, email, display_name, password_hash, role) \
                 VALUES (@P1, @P2, @P3, @P4, @P5)",
                &[&tib_id, &email, &display_name, &pw, &role],
            )
            .await?;
        } else {
            conn.execute(
                "INSERT INTO users (id, email, display_name, password_hash, role) \
                 VALUES (@P1, @P2, @P3, NULL, @P4)",
                &[&tib_id, &email, &display_name, &role],
            )
            .await?;
        }
        Ok(id)
    }

    async fn get_user_by_email(&self, email: &str) -> anyhow::Result<Option<User>> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT id, email, display_name, role, created_at FROM users WHERE email = @P1",
                &[&email],
            )
            .await?
            .into_first_result()
            .await?;
        match rows.first() {
            Some(row) => {
                let id = col_uuid(row, 0)?;
                let email = col_str(row, 1)?;
                let display_name = col_str(row, 2)?;
                let role = col_str(row, 3)?;
                let created_at = col_datetime(row, 4)?;
                Ok(Some(User {
                    id,
                    email,
                    display_name,
                    role,
                    created_at,
                }))
            }
            None => Ok(None),
        }
    }

    async fn get_user_by_id(&self, id: Uuid) -> anyhow::Result<Option<User>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*id.as_bytes());
        let rows = conn
            .query(
                "SELECT id, email, display_name, role, created_at FROM users WHERE id = @P1",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;
        match rows.first() {
            Some(row) => {
                let uid = col_uuid(row, 0)?;
                let email = col_str(row, 1)?;
                let display_name = col_str(row, 2)?;
                let role = col_str(row, 3)?;
                let created_at = col_datetime(row, 4)?;
                Ok(Some(User {
                    id: uid,
                    email,
                    display_name,
                    role,
                    created_at,
                }))
            }
            None => Ok(None),
        }
    }

    async fn get_password_hash(&self, email: &str) -> anyhow::Result<Option<String>> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT password_hash FROM users WHERE email = @P1",
                &[&email],
            )
            .await?
            .into_first_result()
            .await?;
        Ok(rows
            .first()
            .and_then(|r| r.try_get::<&str, _>(0).ok().flatten())
            .map(|s| s.to_string()))
    }

    // -- refresh tokens --

    async fn store_refresh_token(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> anyhow::Result<Uuid> {
        let mut conn = self.conn().await?;
        let id = Uuid::new_v4();
        let tib_id = tiberius::Uuid::from_bytes(*id.as_bytes());
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        let exp_dto = chrono::DateTime::<chrono::FixedOffset>::from(expires_at);
        conn.execute(
            "INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at) \
             VALUES (@P1, @P2, @P3, @P4)",
            &[&tib_id, &tib_uid, &token_hash, &exp_dto],
        )
        .await?;
        Ok(id)
    }

    async fn validate_refresh_token(
        &self,
        token_hash: &str,
    ) -> anyhow::Result<Option<(Uuid, Uuid)>> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT id, user_id FROM refresh_tokens \
                 WHERE token_hash = @P1 AND expires_at > SYSDATETIMEOFFSET()",
                &[&token_hash],
            )
            .await?
            .into_first_result()
            .await?;
        match rows.first() {
            Some(row) => {
                let id = col_uuid(row, 0)?;
                let uid = col_uuid(row, 1)?;
                Ok(Some((id, uid)))
            }
            None => Ok(None),
        }
    }

    async fn delete_refresh_token(&self, token_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*token_id.as_bytes());
        conn.execute("DELETE FROM refresh_tokens WHERE id = @P1", &[&tib_id])
            .await?;
        Ok(())
    }

    async fn consume_refresh_token(&self, token_hash: &str) -> anyhow::Result<Option<Uuid>> {
        let mut conn = self.conn().await?;
        // MSSQL supports OUTPUT clause on DELETE for atomic consume
        let rows = conn
            .query(
                "DELETE FROM refresh_tokens \
                 OUTPUT deleted.user_id \
                 WHERE token_hash = @P1 AND expires_at > SYSDATETIMEOFFSET()",
                &[&token_hash],
            )
            .await?
            .into_first_result()
            .await?;
        match rows.first() {
            Some(row) => {
                let uid = col_uuid(row, 0)?;
                Ok(Some(uid))
            }
            None => Ok(None),
        }
    }

    // -- oauth --

    async fn upsert_oauth_connection(
        &self,
        user_id: Uuid,
        provider: &str,
        provider_user_id: &str,
    ) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        conn.execute(
            "MERGE INTO oauth_connections AS target \
             USING (SELECT @P1 AS user_id, @P2 AS provider, @P3 AS provider_user_id) AS source \
             ON target.provider = source.provider AND target.provider_user_id = source.provider_user_id \
             WHEN MATCHED THEN UPDATE SET user_id = source.user_id \
             WHEN NOT MATCHED THEN INSERT (user_id, provider, provider_user_id) \
                 VALUES (source.user_id, source.provider, source.provider_user_id);",
            &[&tib_uid, &provider, &provider_user_id],
        )
        .await?;
        Ok(())
    }

    async fn get_user_by_oauth(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> anyhow::Result<Option<User>> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT u.id, u.email, u.display_name, u.role, u.created_at \
                 FROM users u JOIN oauth_connections o ON u.id = o.user_id \
                 WHERE o.provider = @P1 AND o.provider_user_id = @P2",
                &[&provider, &provider_user_id],
            )
            .await?
            .into_first_result()
            .await?;
        match rows.first() {
            Some(row) => {
                let id = col_uuid(row, 0)?;
                let email = col_str(row, 1)?;
                let display_name = col_str(row, 2)?;
                let role = col_str(row, 3)?;
                let created_at = col_datetime(row, 4)?;
                Ok(Some(User {
                    id,
                    email,
                    display_name,
                    role,
                    created_at,
                }))
            }
            None => Ok(None),
        }
    }

    // -- api keys --

    async fn create_api_key(
        &self,
        user_id: Uuid,
        key_hash: &str,
        label: &str,
    ) -> anyhow::Result<Uuid> {
        let mut conn = self.conn().await?;
        let id = Uuid::new_v4();
        let tib_id = tiberius::Uuid::from_bytes(*id.as_bytes());
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        conn.execute(
            "INSERT INTO api_keys (id, user_id, key_hash, label) VALUES (@P1, @P2, @P3, @P4)",
            &[&tib_id, &tib_uid, &key_hash, &label],
        )
        .await?;
        Ok(id)
    }

    async fn validate_api_key(&self, key_hash: &str) -> anyhow::Result<Option<User>> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT u.id, u.email, u.display_name, u.role, u.created_at \
                 FROM users u JOIN api_keys k ON u.id = k.user_id \
                 WHERE k.key_hash = @P1 AND k.revoked_at IS NULL",
                &[&key_hash],
            )
            .await?
            .into_first_result()
            .await?;
        if let Some(row) = rows.first() {
            // Update last_used_at
            let _ = conn
                .execute(
                    "UPDATE api_keys SET last_used_at = SYSDATETIMEOFFSET() WHERE key_hash = @P1",
                    &[&key_hash],
                )
                .await;
            let id = col_uuid(row, 0)?;
            let email = col_str(row, 1)?;
            let display_name = col_str(row, 2)?;
            let role = col_str(row, 3)?;
            let created_at = col_datetime(row, 4)?;
            Ok(Some(User {
                id,
                email,
                display_name,
                role,
                created_at,
            }))
        } else {
            Ok(None)
        }
    }

    async fn list_api_keys(&self, user_id: Uuid) -> anyhow::Result<Vec<ApiKeyInfo>> {
        let mut conn = self.conn().await?;
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        let rows = conn
            .query(
                "SELECT id, label, created_at, last_used_at, revoked_at \
                 FROM api_keys WHERE user_id = @P1 ORDER BY created_at",
                &[&tib_uid],
            )
            .await?
            .into_first_result()
            .await?;
        let mut result = Vec::new();
        for row in &rows {
            let id = col_uuid(row, 0)?;
            let label = col_str(row, 1)?;
            let created_at = col_datetime(row, 2)?;
            let last_used_at: Option<chrono::DateTime<chrono::FixedOffset>> =
                row.try_get::<chrono::DateTime<chrono::FixedOffset>, _>(3)?;
            let revoked_at: Option<chrono::DateTime<chrono::FixedOffset>> =
                row.try_get::<chrono::DateTime<chrono::FixedOffset>, _>(4)?;
            result.push(ApiKeyInfo {
                id,
                label,
                created_at,
                last_used_at: last_used_at.map(|d| d.with_timezone(&Utc)),
                revoked: revoked_at.is_some(),
            });
        }
        Ok(result)
    }

    async fn revoke_api_key(&self, key_id: Uuid, user_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_kid = tiberius::Uuid::from_bytes(*key_id.as_bytes());
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        conn.execute(
            "UPDATE api_keys SET revoked_at = SYSDATETIMEOFFSET() \
             WHERE id = @P1 AND user_id = @P2 AND revoked_at IS NULL",
            &[&tib_kid, &tib_uid],
        )
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
        let mut conn = self.conn().await?;
        let id = Uuid::new_v4();
        let tib_id = tiberius::Uuid::from_bytes(*id.as_bytes());
        let tib_cb = tiberius::Uuid::from_bytes(*created_by.as_bytes());
        conn.execute(
            "INSERT INTO teams (id, name, description, created_by) VALUES (@P1, @P2, @P3, @P4)",
            &[&tib_id, &name, &description, &tib_cb],
        )
        .await?;
        Ok(id)
    }

    async fn get_team(&self, team_id: Uuid) -> anyhow::Result<Option<Team>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*team_id.as_bytes());
        let rows = conn
            .query(
                "SELECT id, name, description, daily_token_cap, created_at \
                 FROM teams WHERE id = @P1",
                &[&tib_id],
            )
            .await?
            .into_first_result()
            .await?;
        match rows.first() {
            Some(row) => {
                let id = col_uuid(row, 0)?;
                let name = col_str(row, 1)?;
                let description = col_str(row, 2)?;
                let daily_token_cap = col_i64(row, 3)?;
                let created_at = col_datetime(row, 4)?;
                Ok(Some(Team {
                    id,
                    name,
                    description,
                    daily_token_cap,
                    created_at,
                }))
            }
            None => Ok(None),
        }
    }

    async fn list_teams_for_user(&self, user_id: Uuid) -> anyhow::Result<Vec<Team>> {
        let mut conn = self.conn().await?;
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        let rows = conn
            .query(
                "SELECT t.id, t.name, t.description, t.daily_token_cap, t.created_at \
                 FROM teams t JOIN team_members tm ON t.id = tm.team_id \
                 WHERE tm.user_id = @P1 ORDER BY t.name",
                &[&tib_uid],
            )
            .await?
            .into_first_result()
            .await?;
        let mut result = Vec::new();
        for row in &rows {
            let id = col_uuid(row, 0)?;
            let name = col_str(row, 1)?;
            let description = col_str(row, 2)?;
            let daily_token_cap = col_i64(row, 3)?;
            let created_at = col_datetime(row, 4)?;
            result.push(Team {
                id,
                name,
                description,
                daily_token_cap,
                created_at,
            });
        }
        Ok(result)
    }

    async fn update_team(
        &self,
        team_id: Uuid,
        name: &str,
        description: &str,
    ) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*team_id.as_bytes());
        conn.execute(
            "UPDATE teams SET name = @P1, description = @P2 WHERE id = @P3",
            &[&name, &description, &tib_id],
        )
        .await?;
        Ok(())
    }

    async fn delete_team(&self, team_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*team_id.as_bytes());
        // Clear team_id on sessions before deleting team
        conn.execute(
            "UPDATE sessions SET team_id = NULL WHERE team_id = @P1",
            &[&tib_id],
        )
        .await?;
        conn.execute("DELETE FROM teams WHERE id = @P1", &[&tib_id])
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
        let mut conn = self.conn().await?;
        let tib_tid = tiberius::Uuid::from_bytes(*team_id.as_bytes());
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        conn.execute(
            "MERGE INTO team_members AS target \
             USING (SELECT @P1 AS team_id, @P2 AS user_id, @P3 AS role) AS source \
             ON target.team_id = source.team_id AND target.user_id = source.user_id \
             WHEN MATCHED THEN UPDATE SET role = source.role \
             WHEN NOT MATCHED THEN INSERT (team_id, user_id, role) \
                 VALUES (source.team_id, source.user_id, source.role);",
            &[&tib_tid, &tib_uid, &role],
        )
        .await?;
        Ok(())
    }

    async fn remove_team_member(&self, team_id: Uuid, user_id: Uuid) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_tid = tiberius::Uuid::from_bytes(*team_id.as_bytes());
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        conn.execute(
            "DELETE FROM team_members WHERE team_id = @P1 AND user_id = @P2",
            &[&tib_tid, &tib_uid],
        )
        .await?;
        Ok(())
    }

    async fn list_team_members(&self, team_id: Uuid) -> anyhow::Result<Vec<TeamMember>> {
        let mut conn = self.conn().await?;
        let tib_tid = tiberius::Uuid::from_bytes(*team_id.as_bytes());
        let rows = conn
            .query(
                "SELECT u.id, u.email, u.display_name, tm.role, tm.joined_at \
                 FROM team_members tm JOIN users u ON tm.user_id = u.id \
                 WHERE tm.team_id = @P1 ORDER BY u.display_name",
                &[&tib_tid],
            )
            .await?
            .into_first_result()
            .await?;
        let mut result = Vec::new();
        for row in &rows {
            let user_id = col_uuid(row, 0)?;
            let email = col_str(row, 1)?;
            let display_name = col_str(row, 2)?;
            let role = col_str(row, 3)?;
            let joined_at = col_datetime(row, 4)?;
            result.push(TeamMember {
                user_id,
                email,
                display_name,
                role,
                joined_at,
            });
        }
        Ok(result)
    }

    async fn get_team_member_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> anyhow::Result<Option<String>> {
        let mut conn = self.conn().await?;
        let tib_tid = tiberius::Uuid::from_bytes(*team_id.as_bytes());
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        let rows = conn
            .query(
                "SELECT role FROM team_members WHERE team_id = @P1 AND user_id = @P2",
                &[&tib_tid, &tib_uid],
            )
            .await?
            .into_first_result()
            .await?;
        match rows.first() {
            Some(row) => Ok(Some(col_str(row, 0)?)),
            None => Ok(None),
        }
    }

    async fn update_team_member_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        let tib_tid = tiberius::Uuid::from_bytes(*team_id.as_bytes());
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        conn.execute(
            "UPDATE team_members SET role = @P1 WHERE team_id = @P2 AND user_id = @P3",
            &[&role, &tib_tid, &tib_uid],
        )
        .await?;
        Ok(())
    }

    // -- team-scoped sessions --

    async fn create_session_for_team(
        &self,
        description: &str,
        team_id: Uuid,
    ) -> anyhow::Result<(Uuid, String, DateTime<Utc>)> {
        let mut conn = self.conn().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        let tib_id = tiberius::Uuid::from_bytes(*id.as_bytes());
        let tib_tid = tiberius::Uuid::from_bytes(*team_id.as_bytes());
        let now_dto = chrono::DateTime::<chrono::FixedOffset>::from(now);

        conn.execute(
            "INSERT INTO sessions (id, description, created_at, updated_at, \
             daily_token_cap, tokens_used_today, tokens_reset_date, team_id) \
             VALUES (@P1, @P2, @P3, @P4, 999999999, 0, CAST(GETDATE() AS DATE), @P5)",
            &[&tib_id, &description, &now_dto, &now_dto, &tib_tid],
        )
        .await?;

        Ok((id, description.to_string(), now))
    }

    async fn list_sessions_for_team(
        &self,
        team_id: Uuid,
    ) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String)>> {
        let mut conn = self.conn().await?;
        let tib_tid = tiberius::Uuid::from_bytes(*team_id.as_bytes());
        let rows = conn
            .query(
                "SELECT id, description, created_at, status FROM sessions \
                 WHERE team_id = @P1 ORDER BY created_at DESC",
                &[&tib_tid],
            )
            .await?
            .into_first_result()
            .await?;
        let mut result = Vec::new();
        for row in &rows {
            let id = col_uuid(row, 0)?;
            let desc = col_str(row, 1)?;
            let created = col_datetime(row, 2)?;
            let status = col_str(row, 3)?;
            result.push((id, desc, created, status));
        }
        Ok(result)
    }

    async fn get_session_team(&self, session_id: Uuid) -> anyhow::Result<Option<Uuid>> {
        let mut conn = self.conn().await?;
        let tib_id = tiberius::Uuid::from_bytes(*session_id.as_bytes());
        let rows = conn
            .query("SELECT team_id FROM sessions WHERE id = @P1", &[&tib_id])
            .await?
            .into_first_result()
            .await?;
        match rows.first() {
            Some(row) => {
                let tid: Option<tiberius::Uuid> = row.try_get::<tiberius::Uuid, _>(0)?;
                match tid {
                    Some(g) => Ok(Some(Uuid::from_bytes(*g.as_bytes()))),
                    None => Ok(None),
                }
            }
            None => Ok(None),
        }
    }

    // -- user dashboard --

    async fn list_sessions_for_user(
        &self,
        user_id: Uuid,
    ) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>, String, Option<String>)>> {
        let mut conn = self.conn().await?;
        let tib_uid = tiberius::Uuid::from_bytes(*user_id.as_bytes());
        let rows = conn
            .query(
                "SELECT DISTINCT s.id, s.description, s.created_at, s.status, t.name \
                 FROM sessions s \
                 LEFT JOIN teams t ON s.team_id = t.id \
                 LEFT JOIN team_members tm ON s.team_id = tm.team_id \
                 WHERE s.team_id IS NULL OR tm.user_id = @P1 \
                 ORDER BY s.created_at DESC",
                &[&tib_uid],
            )
            .await?
            .into_first_result()
            .await?;
        let mut result = Vec::new();
        for row in &rows {
            let id = col_uuid(row, 0)?;
            let desc = col_str(row, 1)?;
            let created = col_datetime(row, 2)?;
            let status = col_str(row, 3)?;
            let team_name = col_str_opt(row, 4)?;
            result.push((id, desc, created, status, team_name));
        }
        Ok(result)
    }

    // -- notifications --

    async fn subscribe_notifications(&self) -> anyhow::Result<NotificationRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut broadcast_rx = self.event_tx.subscribe();
        tokio::spawn(async move {
            loop {
                match broadcast_rx.recv().await {
                    Ok(event_id) => {
                        if tx.send(event_id).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("mssql notification subscriber lagged by {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        Ok(rx)
    }
}
