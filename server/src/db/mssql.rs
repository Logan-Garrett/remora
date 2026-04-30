use async_trait::async_trait;
use bb8::Pool;
use bb8_tiberius::ConnectionManager;
use chrono::{DateTime, Utc};
use remora_common::Event;
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

    // Encryption: if TrustServerCertificate is set, we need encryption ON but trust the cert.
    // If encrypt is explicitly set, use that. Otherwise default based on trust_cert.
    if trust_cert {
        config.trust_cert();
        config.encryption(encrypt.unwrap_or(tiberius::EncryptionLevel::Required));
    } else {
        config.encryption(encrypt.unwrap_or(tiberius::EncryptionLevel::NotSupported));
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

    async fn list_sessions(&self) -> anyhow::Result<Vec<(Uuid, String, DateTime<Utc>)>> {
        let mut conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT id, description, created_at FROM sessions ORDER BY created_at DESC",
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
            result.push((id, desc, created));
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
                "INSERT INTO session_runs (session_id, status, context_mode) \
                 OUTPUT INSERTED.id \
                 VALUES (@P1, 'running', @P2)",
                &[&tib_id, &context_mode],
            )
            .await?
            .into_first_result()
            .await?;

        let id = rows
            .first()
            .map(|r| col_i64(r, 0))
            .ok_or_else(|| anyhow::anyhow!("insert_run returned no rows"))??;
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
