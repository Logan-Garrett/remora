//! Test utilities for remora-server integration tests.
//!
//! Requires `DATABASE_URL` to point at a Postgres instance.
//! Tests that use `TestServer` are marked `#[ignore]` so they only run
//! when explicitly requested (or in CI where the env var is set).

use remora_server::db::{self, Database, DatabaseBackend};
use remora_server::state::{AppState, Config};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

/// A test token used across all integration tests.
pub const TEST_TOKEN: &str = "test-token-abc123";

/// A running instance of the remora server bound to a random port.
pub struct TestServer {
    pub addr: SocketAddr,
    pub db: Arc<DatabaseBackend>,
    _workspace_dir: tempfile::TempDir,
}

impl TestServer {
    /// Spin up a fresh server.
    ///
    /// Panics if `DATABASE_URL` is not set or the database is unreachable.
    pub async fn start() -> Self {
        let database_url = std::env::var("DATABASE_URL").expect(
            "DATABASE_URL must be set to run integration tests (e.g. \
             postgres://remora_test:test@localhost/remora_test)",
        );

        let provider = std::env::var("REMORA_DB_PROVIDER").unwrap_or_else(|_| "postgres".into());
        let backend = db::create_backend(&provider, &database_url)
            .await
            .expect("failed to connect to test database");
        let db_arc = Arc::new(backend);

        // Run migrations
        db_arc
            .run_migrations()
            .await
            .expect("failed to run migrations");

        // Clean slate: delete all sessions (cascades to events, repos, etc.)
        // We do this by listing and deleting each session via the trait.
        let sessions = db_arc.list_sessions().await.unwrap_or_default();
        for (id, _, _) in &sessions {
            let _ = db_arc.delete_session(*id).await;
        }

        let workspace_dir = tempfile::tempdir().expect("failed to create temp workspace dir");

        let config = Config {
            workspace_dir: workspace_dir.path().to_path_buf(),
            run_timeout_secs: 60,
            idle_timeout_secs: 1800,
            global_daily_cap: 10_000_000,
            claude_cmd: "echo".into(), // dummy -- tests never actually invoke Claude
            docker_image: "ubuntu:22.04".into(),
            skip_permissions: true,
            use_sandbox: false,
            permission_mode: String::new(),
            allowed_tools: vec![],
        };

        let state = AppState::new(db_arc.clone(), TEST_TOKEN.to_string(), config);
        let shared = Arc::new(state);

        // Start event notification listener
        let listener_state = Arc::clone(&shared);
        tokio::spawn(async move {
            let _ = remora_server::state::run_event_listener(listener_state).await;
        });

        let app = remora_server::build_router(shared);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind random port");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        TestServer {
            addr,
            db: db_arc,
            _workspace_dir: workspace_dir,
        }
    }

    /// Base HTTP URL, e.g. `http://127.0.0.1:12345`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Base WS URL, e.g. `ws://127.0.0.1:12345`.
    pub fn ws_base_url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    // -- Convenience helpers --

    /// Create a session via the REST API. Returns the session id.
    pub async fn create_session(&self, description: &str) -> uuid::Uuid {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/sessions", self.base_url()))
            .header("Authorization", format!("Bearer {TEST_TOKEN}"))
            .json(&serde_json::json!({ "description": description }))
            .send()
            .await
            .expect("create_session request failed");
        assert_eq!(resp.status(), 201, "expected 201 Created");
        let body: serde_json::Value = resp.json().await.unwrap();
        let id_str = body["id"].as_str().expect("response missing id");
        id_str.parse().expect("invalid uuid")
    }

    /// Open a WebSocket connection to the given session.
    pub async fn connect_ws(
        &self,
        session_id: uuid::Uuid,
        name: &str,
    ) -> (
        futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            tokio_tungstenite::tungstenite::Message,
        >,
        futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
    ) {
        use futures_util::StreamExt;
        let url = format!(
            "{}/sessions/{}?token={}&name={}",
            self.ws_base_url(),
            session_id,
            TEST_TOKEN,
            name,
        );
        let (ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("ws connect failed");
        ws.split()
    }

    /// Send a chat message over an existing WS connection.
    pub async fn send_chat(
        sink: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            tokio_tungstenite::tungstenite::Message,
        >,
        author: &str,
        text: &str,
    ) {
        use futures_util::SinkExt;
        let msg = serde_json::json!({
            "type": "chat",
            "author": author,
            "text": text,
        });
        sink.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&msg).unwrap(),
        ))
        .await
        .expect("ws send failed");
    }

    /// Read one ServerMsg from the WS stream, with a timeout.
    pub async fn wait_for_event(
        stream: &mut futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        timeout_ms: u64,
    ) -> Option<serde_json::Value> {
        use futures_util::StreamExt;
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), stream.next()).await;

        match result {
            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text)))) => {
                serde_json::from_str(&text).ok()
            }
            _ => None,
        }
    }

    /// Drain all pending events from a WS stream until timeout, returning them.
    pub async fn drain_events(
        stream: &mut futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        timeout_ms: u64,
    ) -> Vec<serde_json::Value> {
        let mut events = Vec::new();
        loop {
            match Self::wait_for_event(stream, timeout_ms).await {
                Some(ev) => events.push(ev),
                None => break,
            }
        }
        events
    }

    /// Send an arbitrary JSON message over an existing WS connection.
    pub async fn send_msg(
        sink: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            tokio_tungstenite::tungstenite::Message,
        >,
        msg: serde_json::Value,
    ) {
        use futures_util::SinkExt;
        sink.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&msg).unwrap(),
        ))
        .await
        .expect("ws send failed");
    }

    /// Wait for an event matching a predicate, with a timeout.
    /// Returns `Some(event)` if found, `None` on timeout.
    pub async fn wait_for_event_matching<F>(
        stream: &mut futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        predicate: F,
        timeout_ms: u64,
    ) -> Option<serde_json::Value>
    where
        F: Fn(&serde_json::Value) -> bool,
    {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match Self::wait_for_event(stream, remaining.as_millis() as u64).await {
                Some(ev) if predicate(&ev) => return Some(ev),
                Some(_) => continue,
                None => return None,
            }
        }
    }

    /// Expose a reference to the database backend.
    pub fn db(&self) -> &std::sync::Arc<remora_server::db::DatabaseBackend> {
        &self.db
    }

    /// Expose the workspace directory path.
    pub fn workspace_dir(&self) -> &std::path::Path {
        self._workspace_dir.path()
    }
}
